//! Softbuffer implementation using CoreGraphics.
use crate::error::InitError;
use crate::{backend_interface::*, AlphaMode};
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
use std::mem::{self, size_of};
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr::{self, slice_from_raw_parts_mut, NonNull};
use std::slice;

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
    color_space: CFRetained<CGColorSpace>,
    front: Buffer,
    middle: Option<Buffer>,
    back: Buffer,
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

        // The color space we're using. Initialize it here to reduce work later on.
        // TODO: Allow setting this to something else?
        let color_space = CGColorSpace::new_device_rgb().unwrap();

        // Grab initial width and height from the layer (whose properties have just been initialized
        // by the observer using `NSKeyValueObservingOptionInitial`).
        let size = layer.bounds().size;
        let scale_factor = layer.contentsScale();
        let width = (size.width * scale_factor) as u32;
        let height = (size.height * scale_factor) as u32;

        Ok(Self {
            layer: SendCALayer(layer),
            root_layer: SendCALayer(root_layer),
            observer,
            color_space,
            front: Buffer::new(width, height),
            middle: Some(Buffer::new(width, height)),
            back: Buffer::new(width, height),
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    #[inline]
    fn supports_alpha_mode(&self, _alpha_mode: AlphaMode) -> bool {
        true
    }

    fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        alpha_mode: AlphaMode,
    ) -> Result<(), SoftBufferError> {
        let opaque = matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Ignored);
        self.layer.setOpaque(opaque);
        // TODO: Set opaque-ness on root layer too? Is that our responsibility, or Winit's?
        // self.root_layer.setOpaque(opaque);

        let width = width.get();
        let height = height.get();

        // TODO: Is this check desirable?
        if self.front.width == width && self.front.height == height {
            return Ok(());
        }

        // Recreate buffers. It's fine to release the old ones, `CALayer.contents` is going to keep
        // a reference if they're still in use.
        self.front = Buffer::new(width, height);
        self.back = Buffer::new(width, height);

        Ok(())
    }

    fn next_buffer(&mut self, alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        // Block until back buffer is no longer being used by the compositor.
        //
        // TODO: Allow configuring this? https://github.com/rust-windowing/softbuffer/issues/29
        // TODO: Is this actually the check we want to do? It feels overly restrictive.
        // TODO: Should we instead keep a boundless queue, and use the latest available buffer?
        tracing::warn!("next_buffer");
        while self.back.is_in_use() {
            tracing::warn!("in use");
            std::thread::yield_now();
        }

        Ok(BufferImpl {
            front: &mut self.front,
            middle: &mut self.middle,
            back: &mut self.back,
            color_space: &self.color_space,
            alpha_info: match (alpha_mode, cfg!(target_endian = "little")) {
                (AlphaMode::Opaque | AlphaMode::Ignored, true) => CGImageAlphaInfo::NoneSkipFirst,
                (AlphaMode::Opaque | AlphaMode::Ignored, false) => CGImageAlphaInfo::NoneSkipLast,
                (AlphaMode::Premultiplied, true) => CGImageAlphaInfo::PremultipliedFirst,
                (AlphaMode::Premultiplied, false) => CGImageAlphaInfo::PremultipliedLast,
                (AlphaMode::Postmultiplied, true) => CGImageAlphaInfo::First,
                (AlphaMode::Postmultiplied, false) => CGImageAlphaInfo::Last,
            },
            layer: &mut self.layer,
        })
    }
}

/// The implementation used for presenting the back buffer to the surface.
///
/// This is triple-buffered because that's what QuartzCore / the compositor seems to require:
/// - The front buffer is what's currently assigned to `CALayer.contents`, and was submitted to the
///   compositor in the previous iteration of the run loop.
/// - The middle buffer is what the compositor is currently drawing from (assuming a 1 frame delay).
/// - The back buffer is what we'll be drawing into.
///
/// This is especially important because `softbuffer::Surface` may be used from different threads.
#[derive(Debug)]
pub struct BufferImpl<'surface> {
    front: &'surface mut Buffer,
    middle: &'surface mut Option<Buffer>,
    back: &'surface mut Buffer,
    color_space: &'surface CGColorSpace,
    alpha_info: CGImageAlphaInfo,
    layer: &'surface mut SendCALayer,
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(util::byte_stride(self.back.width)).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.back.width).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.back.height).unwrap()
    }

    fn pixels_mut(&mut self) -> &mut [Pixel] {
        // SAFETY: Called on the back buffer.
        unsafe { self.back.data() }
    }

    fn age(&self) -> u8 {
        self.back.age
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.back.age = 1;
        if let Some(middle) = self.middle {
            if middle.age != 0 {
                middle.age += 1;
            }
        }
        if self.front.age != 0 {
            self.front.age += 1;
        }

        // Rotate buffers such that the back buffer is now the front buffer.
        if let Some(middle) = self.middle {
            mem::swap(self.back, middle);
            mem::swap(middle, self.front);
        } else {
            mem::swap(self.back, self.front);
        }

        // `CGBitmapInfo` consists of a combination of `CGImageAlphaInfo`, `CGImageComponentInfo`
        // `CGImageByteOrderInfo` and `CGImagePixelFormatInfo` (see e.g. `CGBitmapInfoMake`).
        //
        // TODO: Use `CGBitmapInfo::new` once the next version of objc2-core-graphics is released.
        let bitmap_info = CGBitmapInfo(
            self.alpha_info.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::Order32Host.0
                | CGImagePixelFormatInfo::Packed.0,
        );

        // CGImage is immutable, so we re-create it
        let image = unsafe {
            CGImage::new(
                self.front.width as usize,
                self.front.height as usize,
                8,
                32,
                util::byte_stride(self.front.width) as usize,
                Some(self.color_space),
                bitmap_info,
                Some(&self.front.data_provider),
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

/// A single buffer in Softbuffer.
#[derive(Debug)]
struct Buffer {
    data_provider: CFRetained<CGDataProvider>,
    data_ptr: *mut Pixel,
    width: u32,
    height: u32,
    age: u8,
}

// SAFETY: We only mutate the `CGDataProvider` when we know it's the back buffer, and only then
// behind `&mut`.
unsafe impl Send for Buffer {}
// SAFETY: Same as above.
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(width: u32, height: u32) -> Self {
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

        let num_bytes = util::byte_stride(width) as usize * (height as usize);

        let buffer = vec![Pixel::default(); num_bytes / size_of::<Pixel>()].into_boxed_slice();
        let data_ptr = Box::into_raw(buffer).cast::<c_void>();

        // SAFETY: The data pointer and length are valid.
        // The info pointer can safely be NULL, we don't use it in the `release` callback.
        let data_provider = unsafe {
            CGDataProvider::with_data(ptr::null_mut(), data_ptr, num_bytes, Some(release))
        }
        .unwrap();

        Self {
            data_provider,
            data_ptr: data_ptr.cast(),
            width,
            height,
            age: 0,
        }
    }

    /// Hint for whether the data provider is currently being used.
    ///
    /// Might return `false`, but if this returns `true`, the provider is definitely not in use.
    fn is_in_use(&self) -> bool {
        self.data_provider.retain_count() != 1
    }

    /// # Safety
    ///
    /// Must only be called on the back buffer.
    unsafe fn data(&mut self) -> &mut [Pixel] {
        // Check that nobody else is using the data provider.
        debug_assert!(!self.is_in_use());

        let num_bytes = util::byte_stride(self.width) as usize * (self.height as usize);
        // SAFETY: The pointer is valid, and ownership rules are upheld by caller.
        unsafe { slice::from_raw_parts_mut(self.data_ptr, num_bytes / size_of::<Pixel>()) }
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
