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
use std::mem::size_of;
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
    /// The buffers that we may render into.
    ///
    /// This contains an unbounded number of buffers, since we don't get any feedback from
    /// QuartzCore about when it's done using the buffer, other than the retain count of the data
    /// provider (which is a weak signal). It shouldn't be needed (QuartzCore seems to copy the data
    /// from `CGImage` once), though theoretically there might be cases where it would have a
    /// multi-stage pipeline where it processes the image once, retains it, and sends it onwards to
    /// be processed again later (and such things might change depending on OS version), so we do
    /// this to be safe.
    ///
    /// Anecdotally, if the user renders 3 times within a single frame (which they probably
    /// shouldn't do, but would be safe), we need 4 buffers according to the retain counts.
    ///
    /// Note that having more buffers here shouldn't add any presentation delay, since we still go
    /// directly from drawing to the back buffer and presenting the front buffer.
    buffers: Vec<Buffer>,
    /// The width of the current buffers.
    width: u32,
    /// The height of the current buffers.
    height: u32,
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
            // We'll usually do double-buffering, but might end up needing more buffers if the user
            // renders multiple times per frame.
            buffers: vec![Buffer::new(width, height), Buffer::new(width, height)],
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
        if self.width == width && self.height == height {
            return Ok(());
        }

        // Recreate buffers. It's fine to release the old ones, `CALayer.contents` is going to keep
        // a reference if they're still in use.
        self.buffers = vec![Buffer::new(width, height), Buffer::new(width, height)];
        self.width = width;
        self.height = height;

        Ok(())
    }

    fn next_buffer(&mut self, alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        // If the backmost buffer might be in use, allocate a new buffer.
        //
        // TODO: Add an `unsafe` option to disable this, and always assume 2 buffers?
        if self.buffers.last().unwrap().might_be_in_use() {
            self.buffers.push(Buffer::new(self.width, self.height));
            // This should have no effect on latency, but it will affect the `buffer.age()` that
            // users see, and unbounded allocation is undesirable too, so we should try to avoid it.

            if self.buffers.len() <= 3 {
                // Winit currently might emit redraw events twice in a single frame, so we need an
                // extra buffer there, see https://github.com/rust-windowing/winit/issues/2640.
                // TODO(madsmtm): Always issue a warning once the Winit issue is fixed.
                tracing::debug!("had to allocate extra buffer in `next_buffer`, this is probably a bug in Winit's RedrawRequested");
            } else {
                tracing::warn!("had to allocate extra buffer in `next_buffer`, you might be rendering faster than the event loop can handle?");
            }
        }

        Ok(BufferImpl {
            buffers: &mut self.buffers,
            width: self.width,
            height: self.height,
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
#[derive(Debug)]
pub struct BufferImpl<'surface> {
    buffers: &'surface mut Vec<Buffer>,
    width: u32,
    height: u32,
    color_space: &'surface CGColorSpace,
    alpha_info: CGImageAlphaInfo,
    layer: &'surface mut SendCALayer,
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(util::byte_stride(self.width)).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.height).unwrap()
    }

    fn pixels_mut(&mut self) -> &mut [Pixel] {
        let back = self.buffers.last_mut().unwrap();

        // Should've been verified by `next_buffer` above.
        debug_assert!(!back.might_be_in_use());

        let num_bytes = util::byte_stride(self.width) as usize * (self.height as usize);
        // SAFETY: The pointer is valid, and we know that we're the only owners of the back buffer's
        // data provider. This, combined with taking `&mut self` in this function, means that we can
        // safely write to the buffer.
        unsafe { slice::from_raw_parts_mut(back.data_ptr, num_bytes / size_of::<Pixel>()) }
    }

    fn age(&self) -> u8 {
        let back = self.buffers.last().unwrap();
        back.age
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        // Rotate buffers such that the back buffer is now the front buffer.
        self.buffers.rotate_right(1);

        let (front, rest) = self.buffers.split_first_mut().unwrap();
        front.age = 1; // This buffer (previously the back buffer) was just rendered into.

        // Bump the age of the other buffers.
        for buffer in rest {
            if buffer.age != 0 {
                buffer.age += 1;
            }
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

        // CGImage is (intended to be) immutable, so we re-create it on each present.
        // SAFETY: The `decode` pointer is NULL.
        let image = unsafe {
            CGImage::new(
                self.width as usize,
                self.height as usize,
                8,
                32,
                util::byte_stride(self.width) as usize,
                Some(self.color_space),
                bitmap_info,
                Some(&front.data_provider),
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
    age: u8,
}

// SAFETY: We only mutate the `CGDataProvider` when we know it's not referenced by anything else,
// and only then behind `&mut`.
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
            age: 0,
        }
    }

    /// Whether the buffer might currently be in use.
    ///
    /// Might return `false` even if the buffer is unused (such as if it ended up in an autorelease
    /// pool), but if this returns `true`, the provider is definitely not in use.
    fn might_be_in_use(&self) -> bool {
        self.data_provider.retain_count() != 1
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
