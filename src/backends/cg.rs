//! Softbuffer implementation using CoreGraphics.
use crate::backend_interface::*;
use crate::error::InitError;
use crate::{Pixel, Rect, SoftBufferError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, Message};
use objc2_core_foundation::{CFMutableDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint};
use objc2_core_graphics::CGColorSpace;
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey,
    NSKeyValueObservingOptions, NSNumber, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString, NSValue,
};
use objc2_io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceCacheMode, kIOSurfaceColorSpace, kIOSurfaceHeight,
    kIOSurfaceMapWriteCombineCache, kIOSurfacePixelFormat, kIOSurfaceWidth, IOSurfaceLockOptions,
    IOSurfaceRef,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::{size_of, ManuallyDrop};
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr;

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
    front2: Option<Buffer>,
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
    type Buffer<'a>
        = BufferImpl<'a>
    where
        Self: 'a;

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

        // The color space we're using.
        // TODO: Allow setting this to something else?
        let color_space = CGColorSpace::new_device_rgb().unwrap();

        // Grab initial width and height from the layer (whose properties have just been initialized
        // by the observer using `NSKeyValueObservingOptionInitial`).
        let size = layer.bounds().size;
        let scale_factor = layer.contentsScale();
        let width = (size.width * scale_factor) as usize;
        let height = (size.height * scale_factor) as usize;

        // FIXME(madsmtm): Allow setting this:
        // https://github.com/rust-windowing/softbuffer/pull/320
        let write_combine_cache = false;
        let properties = Buffer::properties(
            width,
            height,
            kCVPixelFormatType_32BGRA,
            4,
            &color_space,
            write_combine_cache,
        );

        Ok(Self {
            layer: SendCALayer(layer),
            root_layer: SendCALayer(root_layer),
            observer,
            color_space,
            front: Buffer::new(&properties),
            // TODO: Allow configuring amount of buffers?
            front2: Some(Buffer::new(&properties)),
            back: Buffer::new(&properties),
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let width = width.get() as usize;
        let height = height.get() as usize;

        // TODO: Is this check desirable?
        if self.front.surface.width() == width && self.front.surface.height() == height {
            return Ok(());
        }

        // Recreate buffers. It's fine to release the old ones, `CALayer.contents` and/or the
        // compositor is going to keep a reference if they're still in use.
        let properties = Buffer::properties(
            width,
            height,
            kCVPixelFormatType_32BGRA,
            4,
            &self.color_space,
            false, // write_combine_cache
        );
        self.back = Buffer::new(&properties);
        // Keep a second buffer if it was there before.
        if self.front2.is_some() {
            self.front2 = Some(Buffer::new(&properties));
        }
        self.front = Buffer::new(&properties);

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_>, SoftBufferError> {
        // Block until back buffer is no longer being used by the compositor.
        //
        // TODO: Allow configuring this: https://github.com/rust-windowing/softbuffer/issues/29
        // TODO: Is this actually the check we want to do? It seems like the compositor doesn't
        // properly set the usage state when the application loses focus, even if you continue
        // rendering there?
        //
        // Should we instead set up a `CVDisplayLink`, and only allow using the back buffer once a
        // certain number of frames have passed since it was presented? Would be better though not
        // perfect, `CVDisplayLink` isn't guaranteed to actually match the display's refresh rate:
        // https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/CoreVideo/CVProg_Concepts/CVProg_Concepts.html#//apple_ref/doc/uid/TP40001536-CH202-DontLinkElementID_2
        //
        // Another option would be to keep a boundless queue as described in:
        // https://github.com/commercial-emacs/commercial-emacs/blob/68f5a28a316ea0c553d4274ce86e95fc4a5a701a/src/nsterm.m#L10552-L10571
        while self.back.surface.is_in_use() {
            std::thread::yield_now();
        }

        // Lock the back buffer to allow writing to it.
        //
        // Either unlocked in `BufferImpl`s `Drop` or `present_with_damage`.
        self.back.lock();

        Ok(BufferImpl {
            front: &mut self.front,
            front2: &mut self.front2,
            back: &mut self.back,
            layer: &mut self.layer,
        })
    }
}

/// The implementation used for presenting the back buffer to the surface.
///
/// This is triple-buffered because that's what QuartzCore / the compositor seems to require:
/// - The front buffer is what's currently assigned to `CALayer.contents`, and was submitted to the
///   compositor in the previous iteration of the run loop.
/// - The front2 / middle buffer is what the compositor is currently drawing from.
/// - The back buffer is what we'll be drawing into.
#[derive(Debug)]
pub struct BufferImpl<'a> {
    front: &'a mut Buffer,
    front2: &'a mut Option<Buffer>,
    back: &'a mut Buffer,
    layer: &'a mut SendCALayer,
}

impl Drop for BufferImpl<'_> {
    fn drop(&mut self) {
        // Unlock the buffer we locked above.
        self.back.unlock();
    }
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        // A multiple of the cache line size, which is `64` on x86_64 and `128` on Aarch64.
        // Check with `sysctl hw.cachelinesize`.
        NonZeroU32::new(self.back.surface.bytes_per_row() as u32).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.back.surface.width() as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.back.surface.height() as u32).unwrap()
    }

    fn pixels_mut(&mut self) -> &mut [Pixel] {
        let num_pixels =
            self.back.surface.bytes_per_row() * self.back.surface.height() / size_of::<Pixel>();
        let ptr = self.back.surface.base_address().cast::<Pixel>();

        // SAFETY: `IOSurface` is a kernel-managed buffer, which means it's page-aligned, which is
        // plenty for the 4 byte alignment required here.
        //
        // Additionally, buffer is owned by us, and we're the only ones that are going to write to
        // it. Since we re-use buffers, the buffer _might_ be read by the compositor while we write
        // to it - this is still sound on our side, though it might cause tearing, depending on when
        // the memory is flushed by the kernel.
        unsafe { std::slice::from_raw_parts_mut(ptr.as_ptr(), num_pixels) }
    }

    #[inline]
    fn age(&self) -> u8 {
        self.back.age
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        // Unlock the buffer now, and avoid the `unlock` in `Drop`.
        // Would be prettier with https://github.com/rust-lang/rfcs/pull/3466.
        let this = &mut *ManuallyDrop::new(self);
        let front = &mut *this.front;
        let front2 = &mut this.front2;
        let back = &mut *this.back;
        let layer = &mut *this.layer;
        // Note that unlocking effectively flushes the changes, without this, the contents might not
        // be visible to the compositor.
        back.unlock();

        back.age = 1;
        if let Some(front2) = front2 {
            if front2.age != 0 {
                front2.age += 1;
            }
        }
        if front.age != 0 {
            front.age += 1;
        }

        // Rotate buffers such that the back buffer is now the front buffer.
        if let Some(front2) = front2 {
            std::mem::swap(back, front2);
            std::mem::swap(front2, front);
        } else {
            std::mem::swap(back, front);
        }

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be avoided by wrapping the operation in a transaction and disabling all actions.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        // SAFETY: We set `CALayer.contents` to an `IOSurface`, which is an undocumented option, but
        // it's done in browsers and GDK:
        // https://gitlab.gnome.org/GNOME/gtk/-/blob/4266c3c7b15299736df16c9dec57cd8ec7c7ebde/gdk/macos/GdkMacosTile.c#L44
        // And tested to work at least as far back as macOS 10.12.
        unsafe { layer.setContents(Some(front.surface.as_ref())) };

        CATransaction::commit();
        Ok(())
    }
}

/// A single buffer in Softbuffer.
///
/// Buffers are backed by an `IOSurface`, which is a shared memory buffer that can be passed to the
/// compositor without copying. The best official documentation I've found for how this works is
/// probably this keynote:
/// <https://nonstrict.eu/wwdcindex/wwdc2010/422/>
///
/// The first ~10mins of this keynote is also pretty good, it describes CA and the render server:
/// <https://nonstrict.eu/wwdcindex/wwdc2014/419/>
/// <https://wwdcnotes.com/documentation/wwdcnotes/wwdc14-419-advanced-graphics-and-animations-for-ios-apps/>
///
/// See also these links:
/// - <https://developer.apple.com/library/archive/documentation/Performance/Conceptual/OpenCL_MacProgGuide/SynchronizingIOSurfacesAcrossProcessors/SynchronizingIOSurfacesAcrossProcessors.html>
/// - <http://russbishop.net/cross-process-rendering>
/// - <https://www.chromium.org/developers/design-documents/iosurface-meeting-notes/>
/// - <https://github.com/gpuweb/gpuweb/issues/2535>
/// - <https://github.com/Me1000/out-of-process-calayer-rendering>
#[derive(Debug)]
struct Buffer {
    surface: CFRetained<IOSurfaceRef>,
    age: u8,
}

// SAFETY: `IOSurface` is marked `NS_SWIFT_SENDABLE`.
unsafe impl Send for Buffer {}
// SAFETY: Same as above.
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(properties: &CFMutableDictionary<CFString, CFType>) -> Self {
        let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }.unwrap();
        Self { surface, age: 0 }
    }

    fn properties(
        width: usize,
        height: usize,
        pixel_format: u32,
        bytes_per_pixel: u32,
        color_space: &CGColorSpace,
        write_combine_cache: bool,
    ) -> CFRetained<CFMutableDictionary<CFString, CFType>> {
        let properties = CFMutableDictionary::<CFString, CFType>::empty();

        // Set properties of the surface.
        properties.add(
            unsafe { kIOSurfaceWidth },
            &CFNumber::new_isize(width as isize),
        );
        properties.add(
            unsafe { kIOSurfaceHeight },
            &CFNumber::new_isize(height as isize),
        );
        // NOTE: If an unsupported pixel format is provided, the compositor usually won't render
        // anything (which means it'll render whatever was there before, very glitchy).
        //
        // The list of formats is hardware- and OS-dependent, see e.g. the following link:
        // https://developer.apple.com/forums/thread/673868
        //
        // Basically only `kCVPixelFormatType_32BGRA` is guaranteed to work, though from testing,
        // there's a few more that we might be able to use; see the following repository:
        // https://github.com/madsmtm/iosurface-calayer-formats
        properties.add(
            unsafe { kIOSurfacePixelFormat },
            &CFNumber::new_i32(pixel_format as i32),
        );
        properties.add(
            unsafe { kIOSurfaceBytesPerElement },
            &CFNumber::new_i32(bytes_per_pixel as i32),
        );

        // TODO: kIOSurfaceICCProfile instead? Or in addition to this?
        properties.add(
            unsafe { kIOSurfaceColorSpace },
            &*color_space.property_list().unwrap(),
        );

        // Be a bit more strict about usage of the surface in debug mode.
        #[cfg(debug_assertions)]
        properties.add(
            unsafe { objc2_io_surface::kIOSurfacePixelSizeCastingAllowed },
            &**objc2_core_foundation::CFBoolean::new(false),
        );

        if write_combine_cache {
            properties.add(
                unsafe { kIOSurfaceCacheMode },
                &**CFNumber::new_i32(kIOSurfaceMapWriteCombineCache as _),
            );
        }

        properties
    }

    // The compositor shouldn't be writing to our surface, let's ensure that with this flag.
    const LOCK_OPTIONS: IOSurfaceLockOptions = IOSurfaceLockOptions::AvoidSync;

    #[track_caller]
    fn lock(&self) {
        let ret = unsafe { self.surface.lock(Self::LOCK_OPTIONS, ptr::null_mut()) };
        if ret != 0 {
            panic!("failed locking buffer: {ret}");
        }
    }

    #[track_caller]
    fn unlock(&self) {
        let ret = unsafe { self.surface.unlock(Self::LOCK_OPTIONS, ptr::null_mut()) };
        if ret != 0 {
            panic!("failed unlocking buffer: {ret}");
        }
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

// Grabbed from `objc2-core-video` to avoid having to depend on that (for now at least).
#[allow(non_upper_case_globals)]
const kCVPixelFormatType_32BGRA: u32 = 0x42475241;
