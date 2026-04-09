//! Softbuffer implementation using CoreGraphics.
use crate::error::InitError;
use crate::{backend_interface::*, AlphaMode};
use crate::{util, Pixel, Rect, SoftBufferError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, Message};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider,
    CGDataProviderDirectCallbacks, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey,
    NSKeyValueObservingOptions, NSNull, NSNumber, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString, NSValue,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
use tracing::{trace, warn};

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::{size_of, ManuallyDrop};
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
    /// The buffer that we will render into.
    ///
    /// We use single-buffering because QuartzCore copies internally before sending the buffer to
    /// the compositor (so we wouldn't gain anything by double-buffering).
    buffer: Buffer,
    /// The width of the buffer.
    width: u32,
    /// The height of the buffer.
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

        // Remove the layer we created from the root layer.
        self.layer.removeFromSuperlayer();
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
        //
        // This layer is removed from the root layer when the surface is `Drop`ped.
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

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied.
        //
        // We avoid this by setting the action for the "contents" key to NULL.
        //
        // TODO(madsmtm): Do we want to do the same for bounds/contentsScale for smoother resizing?
        layer.setActions(Some(&NSDictionary::from_slices(
            &[ns_string!("contents")],
            &[ProtocolObject::from_ref(&*NSNull::null())],
        )));

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
            buffer: Buffer::new(width, height),
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

        // Recreate buffer. It's fine to release the old one, `CALayer.contents` is going to keep
        // a reference to it around as long as it's still in use.
        self.buffer = Buffer::new(width, height);
        self.width = width;
        self.height = height;

        Ok(())
    }

    fn next_buffer(&mut self, alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        // Unlocked in `present_with_damage` or the buffer's `Drop`.
        self.buffer.info().lock();

        Ok(BufferImpl {
            buffer: &mut self.buffer,
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

/// The implementation used for presenting the buffer to the surface.
#[derive(Debug)]
pub struct BufferImpl<'surface> {
    buffer: &'surface mut Buffer,
    width: u32,
    height: u32,
    color_space: &'surface CGColorSpace,
    alpha_info: CGImageAlphaInfo,
    layer: &'surface mut SendCALayer,
}

impl Drop for BufferImpl<'_> {
    fn drop(&mut self) {
        self.buffer.info().unlock();
    }
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
        let info = self.buffer.info();
        // SAFETY: The data is locked in `next_buffer`, so we know it's not being used elsewhere.
        unsafe { &mut *info.data.get() }
    }

    fn age(&self) -> u8 {
        self.buffer.age
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        // Unlock the buffer now (and not in `Drop`).
        let Self {
            buffer,
            width,
            height,
            color_space,
            alpha_info,
            layer,
        } = &mut *ManuallyDrop::new(self);
        buffer.info().unlock();

        // The buffer's contents have now been set by the user.
        buffer.age = 1;

        // `CGBitmapInfo` consists of a combination of `CGImageAlphaInfo`, `CGImageComponentInfo`
        // `CGImageByteOrderInfo` and `CGImagePixelFormatInfo` (see e.g. `CGBitmapInfoMake`).
        //
        // TODO: Use `CGBitmapInfo::new` once the next version of objc2-core-graphics is released.
        let bitmap_info = CGBitmapInfo(
            alpha_info.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::Order32Host.0
                | CGImagePixelFormatInfo::Packed.0,
        );

        // CGImage is (intended to be) immutable, so we re-create it on each present.
        // SAFETY: The `decode` pointer is NULL.
        let image = unsafe {
            CGImage::new(
                *width as usize,
                *height as usize,
                8,
                32,
                util::byte_stride(*width) as usize,
                Some(color_space),
                bitmap_info,
                Some(&buffer.data_provider),
                ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .unwrap();

        // Wrap layer modifications in a transaction. Unclear if we should keep doing this, see
        // <https://github.com/rust-windowing/softbuffer/pull/275> for discussion about this.
        CATransaction::begin();

        // SAFETY: The contents is `CGImage`, which is a valid class for `contents`.
        unsafe { layer.setContents(Some(image.as_ref())) };

        CATransaction::commit();

        Ok(())
    }
}

/// A single buffer.
#[derive(Debug)]
struct Buffer {
    data_provider: CFRetained<CGDataProvider>,
    age: u8,
}

// SAFETY: We only mutate the `CGDataProvider`'s info when we know it's not referenced by anything
// else (which we know by locking), and only then behind `&mut`.
unsafe impl Send for Buffer {}
// SAFETY: Same as above.
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(width: u32, height: u32) -> Self {
        trace!("Buffer::new");
        let num_bytes = util::byte_stride(width) as usize * (height as usize);
        let data = vec![Pixel::INIT; num_bytes / size_of::<Pixel>()].into_boxed_slice();

        unsafe extern "C-unwind" fn get_byte_pointer(info: *mut c_void) -> *const c_void {
            trace!("get_byte_pointer");
            // SAFETY: The `info` pointer was set to `BufferInfo` on creation.
            let info: &BufferInfo = unsafe { &*info.cast() };
            // CG is about to use the pointer, so lock it.
            info.lock();
            // SAFETY: The buffer is not being accessed elsewhere (we just acquired the lock).
            let buffer = unsafe { &*info.data.get() };
            buffer.as_ptr().cast()
        }

        unsafe extern "C-unwind" fn release_byte_pointer(
            info: *mut c_void,
            _data_ptr: NonNull<c_void>,
        ) {
            trace!("release_byte_pointer");
            // SAFETY: The `info` pointer was set to `BufferInfo` on creation.
            let info: &BufferInfo = unsafe { &*info.cast() };
            // CG will no longer access the pointer, so we can safely unlock it.
            info.unlock();
        }

        unsafe extern "C-unwind" fn release_info(info: *mut c_void) {
            trace!("release_info");
            // SAFETY: This is the same pointer that we passed to `Box::into_raw` on creation.
            drop(unsafe { Box::from_raw(info.cast::<BufferInfo>()) });
        }

        // Wrap `BufferInfo` in a pointer to allow passing it to `CGDataProvider`.
        let info = Box::new(BufferInfo {
            data: UnsafeCell::new(data),
            locked: AtomicBool::new(false),
        });
        let callbacks = CGDataProviderDirectCallbacks {
            version: 0,
            getBytePointer: Some(get_byte_pointer),
            releaseBytePointer: Some(release_byte_pointer),
            // We could provide this instead of `getBytePointer`/`releaseBytePointer`, but those two
            // are likely to be more performant.
            getBytesAtPosition: None,
            releaseInfo: Some(release_info),
        };

        // SAFETY: The `info` pointer is valid, and our callbacks are correctly implemented.
        let data_provider = unsafe {
            CGDataProvider::new_direct(
                // Pass ownership of the `info` pointer. This will be released in `release_info`.
                Box::into_raw(info).cast(),
                num_bytes as libc::off_t,
                &callbacks,
            )
        }
        .unwrap();

        Self {
            data_provider,
            age: 0,
        }
    }

    fn info(&self) -> &BufferInfo {
        let ptr = CGDataProvider::info(Some(&self.data_provider));
        // SAFETY: The buffer info was passed to our data provider on creation, and the provider is
        // valid for at least `'self`.
        unsafe { &*ptr.cast::<BufferInfo>() }
    }
}

/// Data contained in the `CGDataProvider`.
struct BufferInfo {
    /// The buffer contents.
    ///
    /// This may either be in use by the data provider, or it may be in use by us. Neither
    /// CoreGraphics nor QuartzCore provide any guarantees (that I could find) on when the
    /// `CALayer.contents`/`CGImage` is read, which means we must be prepared for:
    /// 1. It being read when the `CGImage` is created.
    /// 2. It being read when `layer.setContents()` is called.
    /// 3. It being read when `CATransaction::commit()` is called.
    /// 4. It being read when the transaction is actually committed, which usually happens
    ///    implicitly at the end of the thread's run loop.
    ///
    /// In practice, option 4 seems to be what happens (when rendering off-thread, usually you'll
    /// see option 3, because most off-thread rendering doesn't have a runloop running, so the
    /// `CATransaction::commit()` will do the actual commit), which means we need to lock the data
    /// somehow, see below.
    data: UnsafeCell<Box<[Pixel]>>,

    /// Whether the data above is currently locked.
    ///
    /// Needs to be thread-safe because the user may:
    /// - Render on thread 1 with a runloop (schedules the buffer to be read at the end, see above).
    /// - Move `Surface` to thread 2 and continue rendering there.
    ///
    /// The release of the buffer would then happen on thread 1, which we'd like to wait for on the
    /// new thread.
    ///
    /// We _could_ use a mutex here to ensure thread priority inversion happens, but it's a bit
    /// harder to work with those since the Rust standard library doesn't really make it possible to
    /// lock a mutex in one function and unlock it in another (as needed by `get_byte_pointer` /
    /// `release_byte_pointer`). In practice, it's very unlikely to be an issue, since rendering
    /// generally only happens on one thread (it's very rare for it to move between threads as
    /// described above), and the main thread is heavily prioritized already (even if you were to
    /// move rendering, you'd usually be moving to/from the main thread).
    locked: AtomicBool,
}

/// See <https://mara.nl/atomics/building-spinlock.html> for details on the atomic operations.
impl BufferInfo {
    /// Lock the buffer.
    fn lock(&self) {
        if self.locked.swap(true, Ordering::Acquire) {
            // Failing to lock the buffer should only happen in exceptional cases.
            //
            // If it keeps failing for > 100ms, it's very likely that the user accidentally leaked
            // the buffer (and that this will deadlock forever).
            let now = Instant::now();
            let mut has_warned = false;
            while self.locked.swap(true, Ordering::Acquire) {
                if !has_warned && Duration::from_millis(100) < now.elapsed() {
                    warn!("probable deadlock: waiting on lock for more than 100ms");
                    has_warned = true;
                }
                std::thread::yield_now();
            }
        }
        // Successfully locked the buffer
    }

    /// Unlock the buffer.
    fn unlock(&self) {
        debug_assert!(
            self.locked.load(Ordering::Relaxed),
            "unlocking buffer that wasn't locked"
        );
        self.locked.store(false, Ordering::Release);
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
