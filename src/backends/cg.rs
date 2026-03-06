//! Softbuffer implementation using CoreGraphics.
use crate::error::InitError;
use crate::{backend_interface::*, AlphaMode, PixelFormat};
use crate::{util, Pixel, Rect, SoftBufferError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, Message};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey,
    NSKeyValueObservingOptions, NSNumber, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString, NSValue,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr::{self, slice_from_raw_parts_mut, NonNull};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SoftbufferObserver"]
    #[ivars = SendCALayer]
    #[derive(Debug)]
    struct Observer;

    /// NSKeyValueObserving
    impl Observer {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            self.update(key_path, change);
        }
    }
);

impl Observer {
    fn new(layer: &CALayer) -> Retained<Self> {
        let this = Self::alloc().set_ivars(SendCALayer(layer.retain()));
        unsafe { msg_send![super(this), init] }
    }

    fn update(
        &self,
        key_path: Option<&NSString>,
        change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
    ) {
        let layer = self.ivars();

        let change =
            change.expect("requested a change dictionary in `addObserver`, but none was provided");
        let new = change
            .objectForKey(unsafe { NSKeyValueChangeNewKey })
            .expect("requested change dictionary did not contain `NSKeyValueChangeNewKey`");

        // NOTE: Setting these values usually causes a quarter second animation to occur, which is
        // undesirable.
        //
        // However, since we're setting them inside an observer, there already is a transaction
        // ongoing, and as such we don't need to wrap this in a `CATransaction` ourselves.

        if key_path == Some(ns_string!("contentsScale")) {
            let new = new.downcast::<NSNumber>().unwrap();
            let scale_factor = new.as_cgfloat();

            // Set the scale factor of the layer to match the root layer when it changes (e.g. if
            // moved to a different monitor, or monitor settings changed).
            layer.setContentsScale(scale_factor);
        } else if key_path == Some(ns_string!("bounds")) {
            let new = new.downcast::<NSValue>().unwrap();
            let bounds = new.get_rect().expect("new bounds value was not CGRect");

            // Set `bounds` and `position` so that the new layer is inside the superlayer.
            //
            // This differs from just setting the `bounds`, as it also takes into account any
            // translation that the superlayer may have that we'd want to preserve.
            layer.setFrame(bounds);
        } else {
            panic!("unknown observed keypath {key_path:?}");
        }
    }
}

#[derive(Debug)]
pub struct CGImpl<D, W> {
    /// Our layer.
    layer: SendCALayer,
    /// The layer that our layer was created from.
    ///
    /// Can also be retrieved from `layer.superlayer()`.
    root_layer: SendCALayer,
    observer: Retained<Observer>,
    rgb_color_space: CFRetained<CGColorSpace>,
    gray_color_space: CFRetained<CGColorSpace>,
    /// The width of the underlying buffer.
    width: usize,
    /// The height of the underlying buffer.
    height: usize,
    window_handle: W,
    _display: PhantomData<D>,
}

impl<D, W> Drop for CGImpl<D, W> {
    fn drop(&mut self) {
        // SAFETY: Registered in `new`, must be removed before the observer is deallocated.
        unsafe {
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("contentsScale"));
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("bounds"));
        }
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for CGImpl<D, W> {
    type Context = D;
    type Buffer<'surface>
        = BufferImpl<'surface>
    where
        Self: 'surface;

    fn new(window_src: W, _display: &D) -> Result<Self, InitError<W>> {
        // `NSView`/`UIView` can only be accessed from the main thread.
        let _mtm = MainThreadMarker::new().ok_or(SoftBufferError::PlatformError(
            Some("can only access Core Graphics handles from the main thread".to_string()),
            None,
        ))?;

        let root_layer = match window_src.window_handle()?.as_raw() {
            RawWindowHandle::AppKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `AppKitWindowHandle` contains a valid pointer to an `NSView`.
                //
                // We use `NSObject` here to avoid importing `objc2-app-kit`.
                let view: &NSObject = unsafe { handle.ns_view.cast().as_ref() };

                // Force the view to become layer backed
                let _: () = unsafe { msg_send![view, setWantsLayer: Bool::YES] };

                // SAFETY: `-[NSView layer]` returns an optional `CALayer`
                let layer: Option<Retained<CALayer>> = unsafe { msg_send![view, layer] };
                layer.expect("failed making the view layer-backed")
            }
            RawWindowHandle::UiKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `UiKitWindowHandle` contains a valid pointer to an `UIView`.
                //
                // We use `NSObject` here to avoid importing `objc2-ui-kit`.
                let view: &NSObject = unsafe { handle.ui_view.cast().as_ref() };

                // SAFETY: `-[UIView layer]` returns `CALayer`
                let layer: Retained<CALayer> = unsafe { msg_send![view, layer] };
                layer
            }
            _ => return Err(InitError::Unsupported(window_src)),
        };

        // Add a sublayer, to avoid interfering with the root layer, since setting the contents of
        // e.g. a view-controlled layer is brittle.
        let layer = CALayer::new();
        root_layer.addSublayer(&layer);

        // Set the anchor point and geometry. Softbuffer's uses a coordinate system with the origin
        // in the top-left corner.
        //
        // NOTE: This doesn't really matter unless we start modifying the `position` of our layer
        // ourselves, but it's nice to have in place.
        layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        layer.setGeometryFlipped(true);

        // Do not use auto-resizing mask.
        //
        // This is done to work around a bug in macOS 14 and above, where views using auto layout
        // may end up setting fractional values as the bounds, and that in turn doesn't propagate
        // properly through the auto-resizing mask and with contents gravity.
        //
        // Instead, we keep the bounds of the layer in sync with the root layer using an observer,
        // see below.
        //
        // layer.setAutoresizingMask(kCALayerHeightSizable | kCALayerWidthSizable);

        let observer = Observer::new(&layer);
        // Observe changes to the root layer's bounds and scale factor, and apply them to our layer.
        //
        // The previous implementation updated the scale factor inside `resize`, but this works
        // poorly with transactions, and is generally inefficient. Instead, we update the scale
        // factor only when needed because the super layer's scale factor changed.
        //
        // Note that inherent in this is an explicit design decision: We control the `bounds` and
        // `contentsScale` of the layer directly, and instead let the `resize` call that the user
        // controls only be the size of the underlying buffer.
        //
        // SAFETY: Observer deregistered in `Drop` before the observer object is deallocated.
        unsafe {
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("contentsScale"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("bounds"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
        }

        // Set the content so that it is placed in the top-left corner if it does not have the same
        // size as the surface itself.
        //
        // TODO(madsmtm): Consider changing this to `kCAGravityResize` to stretch the content if
        // resized to something that doesn't fit, see #177.
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });

        // Default alpha mode is opaque.
        layer.setOpaque(true);

        // Initialize color space here, to reduce work later on.
        let rgb_color_space = CGColorSpace::new_device_rgb().unwrap();
        let gray_color_space = CGColorSpace::new_device_gray().unwrap();

        // Grab initial width and height from the layer (whose properties have just been initialized
        // by the observer using `NSKeyValueObservingOptionInitial`).
        let size = layer.bounds().size;
        let scale_factor = layer.contentsScale();
        let width = (size.width * scale_factor) as usize;
        let height = (size.height * scale_factor) as usize;

        Ok(Self {
            layer: SendCALayer(layer),
            root_layer: SendCALayer(root_layer),
            observer,
            rgb_color_space,
            gray_color_space,
            width,
            height,
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    #[inline]
    fn supported_pixel_formats(&self, alpha_mode: AlphaMode) -> &[PixelFormat] {
        // All alpha modes supported (ish).
        //
        // <https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/drawingwithquartz2d/dq_context/dq_context.html#//apple_ref/doc/uid/TP30001066-CH203-BCIBHHBB>
        // <https://developer.apple.com/library/archive/qa/qa1501/_index.html#//apple_ref/doc/uid/DTS10004198>
        match alpha_mode {
            AlphaMode::Ignored | AlphaMode::Opaque | AlphaMode::Postmultiplied => {
                &[
                    // Some of these probably depend on host endianess?
                    PixelFormat::Rgb8,
                    PixelFormat::Bgra8,
                    PixelFormat::Rgba8,
                    PixelFormat::Abgr8,
                    PixelFormat::Argb8,
                    PixelFormat::Rgb16,
                    PixelFormat::Rgba16,
                    PixelFormat::Argb16,
                    // Grayscale formats are supported.
                    PixelFormat::R1,
                    PixelFormat::R2,
                    PixelFormat::R4,
                    PixelFormat::R8,
                    PixelFormat::R16,
                    // Packed formats only support RGB, and not `R3g3b2`. `Bgra4` etc. also seems to instead
                    // support the ordering `[G, B, A, R]`, `[B, A, R, G]` etc., which we can't use.
                    PixelFormat::R5g6b5,
                    PixelFormat::Rgb5a1, // TODO: Doesn't support premul alpha?
                    PixelFormat::A1rgb5, // TODO: Doesn't support premul alpha?
                    PixelFormat::Rgb10a2,
                    PixelFormat::A2rgb10,
                    // *BGR* versions of floats just produce black?
                    PixelFormat::Rgb16f,
                    PixelFormat::Rgba16f,
                    PixelFormat::Argb16f,
                    PixelFormat::Rgb32f,
                    PixelFormat::Rgba32f,
                    PixelFormat::Argb32f,
                ]
            }
            AlphaMode::Premultiplied => {
                // Same table as above, except for `PixelFormat::Rgb5a1` and `PixelFormat::A1rgb5`.
                &[
                    PixelFormat::Rgb8,
                    PixelFormat::Bgra8,
                    PixelFormat::Rgba8,
                    PixelFormat::Abgr8,
                    PixelFormat::Argb8,
                    PixelFormat::Rgb16,
                    PixelFormat::Rgba16,
                    PixelFormat::Argb16,
                    PixelFormat::R1,
                    PixelFormat::R2,
                    PixelFormat::R4,
                    PixelFormat::R8,
                    PixelFormat::R16,
                    PixelFormat::R5g6b5,
                    PixelFormat::Rgb10a2,
                    PixelFormat::A2rgb10,
                    PixelFormat::Rgb16f,
                    PixelFormat::Rgba16f,
                    PixelFormat::Argb16f,
                    PixelFormat::Rgb32f,
                    PixelFormat::Rgba32f,
                    PixelFormat::Argb32f,
                ]
            }
        }
    }

    fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        alpha_mode: AlphaMode,
        _pixel_format: PixelFormat,
    ) -> Result<(), SoftBufferError> {
        let opaque = matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Ignored);
        self.layer.setOpaque(opaque);
        // TODO: Set opaque-ness on root layer too? Is that our responsibility, or Winit's?
        // self.root_layer.setOpaque(opaque);

        self.width = width.get() as usize;
        self.height = height.get() as usize;
        Ok(())
    }

    fn next_buffer(
        &mut self,
        alpha_mode: AlphaMode,
        pixel_format: PixelFormat,
    ) -> Result<BufferImpl<'_>, SoftBufferError> {
        let num_bytes = util::byte_stride(self.width as u32, pixel_format.bits_per_pixel())
            as usize
            * self.height;
        // `CGBitmapInfo` consists of a combination of `CGImageAlphaInfo`, `CGImageComponentInfo`
        // `CGImageByteOrderInfo` and `CGImagePixelFormatInfo`, see `CGBitmapInfoMake`.
        //
        // TODO: Use `CGBitmapInfo::new` once the next version of objc2-core-graphics is released.
        let bitmap_info = CGBitmapInfo(
            alpha_info(alpha_mode, pixel_format).0
                | component_info(pixel_format).0
                | byte_order_info(pixel_format).0
                | pixel_format_info(pixel_format).0,
        );
        // Required that we use a different color space when using grayscale colors.
        let color_space = if matches!(
            pixel_format,
            PixelFormat::R1
                | PixelFormat::R2
                | PixelFormat::R4
                | PixelFormat::R8
                | PixelFormat::R16
        ) {
            &self.gray_color_space
        } else {
            &self.rgb_color_space
        };
        Ok(BufferImpl {
            buffer: util::PixelBuffer(vec![Pixel::default(); num_bytes / size_of::<Pixel>()]),
            width: self.width,
            height: self.height,
            color_space,
            bitmap_info,
            bits_per_component: bits_per_component(pixel_format),
            bits_per_pixel: pixel_format.bits_per_pixel(),
            layer: &mut self.layer,
        })
    }
}

#[derive(Debug)]
pub struct BufferImpl<'surface> {
    width: usize,
    height: usize,
    color_space: &'surface CGColorSpace,
    buffer: util::PixelBuffer,
    bitmap_info: CGBitmapInfo,
    bits_per_component: u8,
    bits_per_pixel: u8,
    layer: &'surface mut SendCALayer,
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(util::byte_stride(self.width as u32, self.bits_per_pixel)).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.height as u32).unwrap()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        &mut self.buffer
    }

    fn age(&self) -> u8 {
        0
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        unsafe extern "C-unwind" fn release(
            _info: *mut c_void,
            data: NonNull<c_void>,
            size: usize,
        ) {
            let data = data.cast::<Pixel>();
            let slice = slice_from_raw_parts_mut(data.as_ptr(), size / size_of::<Pixel>());
            // SAFETY: This is the same slice that we passed to `Box::into_raw` below.
            drop(unsafe { Box::from_raw(slice) })
        }

        let data_provider = {
            let len = self.buffer.len() * size_of::<Pixel>();
            let buffer: *mut [Pixel] = Box::into_raw(self.buffer.0.into_boxed_slice());
            // Convert slice pointer to thin pointer.
            let data_ptr = buffer.cast::<c_void>();

            // SAFETY: The data pointer and length are valid.
            // The info pointer can safely be NULL, we don't use it in the `release` callback.
            unsafe {
                CGDataProvider::with_data(ptr::null_mut(), data_ptr, len, Some(release)).unwrap()
            }
        };

        let image = unsafe {
            CGImage::new(
                self.width,
                self.height,
                self.bits_per_component as usize,
                self.bits_per_pixel as usize,
                util::byte_stride(self.width as u32, self.bits_per_pixel) as usize,
                Some(self.color_space),
                self.bitmap_info,
                Some(&data_provider),
                ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .unwrap();

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be avoided by wrapping the operation in a transaction and disabling all actions.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        // SAFETY: The contents is `CGImage`, which is a valid class for `contents`.
        unsafe { self.layer.setContents(Some(image.as_ref())) };

        CATransaction::commit();
        Ok(())
    }
}

#[derive(Debug)]
struct SendCALayer(Retained<CALayer>);

// SAFETY: CALayer is dubiously thread safe, like most things in Core Animation.
// But since we make sure to do our changes within a CATransaction, it is
// _probably_ fine for us to use CALayer from different threads.
//
// See also:
// https://developer.apple.com/documentation/quartzcore/catransaction/1448267-lock?language=objc
// https://stackoverflow.com/questions/76250226/how-to-render-content-of-calayer-on-a-background-thread
unsafe impl Send for SendCALayer {}
// SAFETY: Same as above.
unsafe impl Sync for SendCALayer {}

impl Deref for SendCALayer {
    type Target = CALayer;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn alpha_info(alpha_mode: AlphaMode, pixel_format: PixelFormat) -> CGImageAlphaInfo {
    let first = match alpha_mode {
        AlphaMode::Opaque | AlphaMode::Ignored => CGImageAlphaInfo::NoneSkipFirst,
        AlphaMode::Premultiplied => CGImageAlphaInfo::PremultipliedFirst,
        AlphaMode::Postmultiplied => CGImageAlphaInfo::First,
    };
    let last = match alpha_mode {
        AlphaMode::Opaque | AlphaMode::Ignored => CGImageAlphaInfo::NoneSkipLast,
        AlphaMode::Premultiplied => CGImageAlphaInfo::PremultipliedLast,
        AlphaMode::Postmultiplied => CGImageAlphaInfo::Last,
    };

    match pixel_format {
        // Byte-aligned RGB formats.
        PixelFormat::Bgr8 | PixelFormat::Bgr16 => CGImageAlphaInfo::None,
        PixelFormat::Rgb8 | PixelFormat::Rgb16 => CGImageAlphaInfo::None,
        PixelFormat::Bgra8 | PixelFormat::Bgra16 => first,
        PixelFormat::Rgba8 | PixelFormat::Rgba16 => last,
        PixelFormat::Abgr8 | PixelFormat::Abgr16 => last,
        PixelFormat::Argb8 | PixelFormat::Argb16 => first,
        // Grayscale formats.
        PixelFormat::R1
        | PixelFormat::R2
        | PixelFormat::R4
        | PixelFormat::R8
        | PixelFormat::R16 => CGImageAlphaInfo::None,
        // Packed formats.
        PixelFormat::B2g3r3 | PixelFormat::R3g3b2 => CGImageAlphaInfo::None,
        PixelFormat::B5g6r5 | PixelFormat::R5g6b5 => CGImageAlphaInfo::None,
        PixelFormat::Bgra4 | PixelFormat::Bgr5a1 | PixelFormat::Bgr10a2 => first,
        PixelFormat::Rgba4 | PixelFormat::Rgb5a1 | PixelFormat::Rgb10a2 => last,
        PixelFormat::Abgr4 | PixelFormat::A1bgr5 | PixelFormat::A2bgr10 => last,
        PixelFormat::Argb4 | PixelFormat::A1rgb5 | PixelFormat::A2rgb10 => first,
        // Floating point formats.
        PixelFormat::Bgr16f | PixelFormat::Bgr32f => CGImageAlphaInfo::None,
        PixelFormat::Rgb16f | PixelFormat::Rgb32f => CGImageAlphaInfo::None,
        PixelFormat::Bgra16f | PixelFormat::Bgra32f => first,
        PixelFormat::Rgba16f | PixelFormat::Rgba32f => last,
        PixelFormat::Abgr16f | PixelFormat::Abgr32f => last,
        PixelFormat::Argb16f | PixelFormat::Argb32f => first,
    }
}

fn component_info(pixel_format: PixelFormat) -> CGImageComponentInfo {
    if matches!(
        pixel_format,
        PixelFormat::Bgr16f
            | PixelFormat::Rgb16f
            | PixelFormat::Bgra16f
            | PixelFormat::Rgba16f
            | PixelFormat::Abgr16f
            | PixelFormat::Argb16f
            | PixelFormat::Bgr32f
            | PixelFormat::Rgb32f
            | PixelFormat::Bgra32f
            | PixelFormat::Rgba32f
            | PixelFormat::Abgr32f
            | PixelFormat::Argb32f
    ) {
        CGImageComponentInfo::Float
    } else {
        CGImageComponentInfo::Integer
    }
}

fn byte_order_info(pixel_format: PixelFormat) -> CGImageByteOrderInfo {
    match pixel_format {
        // Byte-aligned RGB formats.
        PixelFormat::Bgr8 => unimplemented!(),
        PixelFormat::Rgb8 => CGImageByteOrderInfo::OrderDefault,
        PixelFormat::Bgra8 | PixelFormat::Abgr8 => CGImageByteOrderInfo::Order32Little,
        PixelFormat::Rgba8 | PixelFormat::Argb8 => CGImageByteOrderInfo::Order32Big,
        PixelFormat::Bgr16 | PixelFormat::Bgra16 | PixelFormat::Abgr16 => {
            CGImageByteOrderInfo::Order16Big
        }
        PixelFormat::Rgb16 | PixelFormat::Rgba16 | PixelFormat::Argb16 => {
            CGImageByteOrderInfo::Order16Little
        }

        // Grayscale formats.
        PixelFormat::R1 | PixelFormat::R2 | PixelFormat::R4 | PixelFormat::R8 => {
            CGImageByteOrderInfo::OrderDefault
        }
        PixelFormat::R16 => CGImageByteOrderInfo::Order16Little,

        // Packed formats.
        PixelFormat::B2g3r3 | PixelFormat::R3g3b2 => CGImageByteOrderInfo::OrderDefault,
        PixelFormat::B5g6r5 => CGImageByteOrderInfo::Order16Big,
        PixelFormat::R5g6b5 => CGImageByteOrderInfo::Order16Little,
        PixelFormat::Bgra4 | PixelFormat::Abgr4 | PixelFormat::Bgr5a1 | PixelFormat::A1bgr5 => {
            CGImageByteOrderInfo::Order16Big
        }
        PixelFormat::Rgba4 | PixelFormat::Argb4 | PixelFormat::Rgb5a1 | PixelFormat::A1rgb5 => {
            CGImageByteOrderInfo::Order16Little
        }
        PixelFormat::Bgr10a2 | PixelFormat::A2bgr10 => CGImageByteOrderInfo::Order32Big,
        PixelFormat::Rgb10a2 | PixelFormat::A2rgb10 => CGImageByteOrderInfo::Order32Little,

        // Floating point formats.
        PixelFormat::Bgr16f | PixelFormat::Bgra16f | PixelFormat::Abgr16f => {
            CGImageByteOrderInfo::Order16Big
        }
        PixelFormat::Rgb16f | PixelFormat::Rgba16f | PixelFormat::Argb16f => {
            CGImageByteOrderInfo::Order16Little
        }
        PixelFormat::Bgr32f | PixelFormat::Bgra32f | PixelFormat::Abgr32f => {
            CGImageByteOrderInfo::Order32Big
        }
        PixelFormat::Rgb32f | PixelFormat::Rgba32f | PixelFormat::Argb32f => {
            CGImageByteOrderInfo::Order32Little
        }
    }
}

fn pixel_format_info(pixel_format: PixelFormat) -> CGImagePixelFormatInfo {
    match pixel_format {
        PixelFormat::R5g6b5 => CGImagePixelFormatInfo::RGB565,
        PixelFormat::Rgb5a1 | PixelFormat::A1rgb5 => CGImagePixelFormatInfo::RGB555,
        PixelFormat::Rgb10a2 | PixelFormat::A2rgb10 => CGImagePixelFormatInfo::RGB101010,
        // Probably not correct for some formats, but it's the best we can do.
        _ => CGImagePixelFormatInfo::Packed,
    }
}

fn bits_per_component(pixel_format: PixelFormat) -> u8 {
    match pixel_format {
        // Byte-aligned RGB formats.
        PixelFormat::Bgr8
        | PixelFormat::Rgb8
        | PixelFormat::Bgra8
        | PixelFormat::Rgba8
        | PixelFormat::Abgr8
        | PixelFormat::Argb8 => 8,
        PixelFormat::Bgr16
        | PixelFormat::Rgb16
        | PixelFormat::Bgra16
        | PixelFormat::Rgba16
        | PixelFormat::Abgr16
        | PixelFormat::Argb16 => 16,

        // Grayscale formats.
        PixelFormat::R1 => 1,
        PixelFormat::R2 => 2,
        PixelFormat::R4 => 4,
        PixelFormat::R8 => 8,
        PixelFormat::R16 => 16,

        // Packed formats.
        PixelFormat::B2g3r3 | PixelFormat::R3g3b2 => 3,
        PixelFormat::B5g6r5 | PixelFormat::R5g6b5 => 5,
        PixelFormat::Bgra4 | PixelFormat::Rgba4 | PixelFormat::Abgr4 | PixelFormat::Argb4 => 4,
        PixelFormat::Bgr5a1 | PixelFormat::Rgb5a1 | PixelFormat::A1bgr5 | PixelFormat::A1rgb5 => 5,
        PixelFormat::Bgr10a2
        | PixelFormat::Rgb10a2
        | PixelFormat::A2bgr10
        | PixelFormat::A2rgb10 => 10,

        // Floating point formats.
        PixelFormat::Bgr16f
        | PixelFormat::Rgb16f
        | PixelFormat::Bgra16f
        | PixelFormat::Rgba16f
        | PixelFormat::Abgr16f
        | PixelFormat::Argb16f => 16,
        PixelFormat::Bgr32f
        | PixelFormat::Rgb32f
        | PixelFormat::Bgra32f
        | PixelFormat::Rgba32f
        | PixelFormat::Abgr32f
        | PixelFormat::Argb32f => 32,
    }
}

#[cfg(target_endian = "big")]
compile_error!("softbuffer's Apple implementation has not been tested on big endian");
