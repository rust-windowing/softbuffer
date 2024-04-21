use crate::backend_interface::*;
use crate::error::InitError;
use crate::{Rect, SoftBufferError};
use core_graphics::base::{
    kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst, kCGRenderingIntentDefault,
};
use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::image::CGImage;
use objc2::runtime::AnyObject;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use foreign_types::ForeignType;
use objc2::msg_send;
use objc2::rc::Id;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow};
use objc2_foundation::MainThreadMarker;
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;

struct Buffer(Vec<u32>);

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        bytemuck::cast_slice(&self.0)
    }
}

pub struct CGImpl<D, W> {
    layer: Id<CALayer>,
    window: Id<NSWindow>,
    color_space: CGColorSpace,
    size: Option<(NonZeroU32, NonZeroU32)>,
    window_handle: W,
    _display: PhantomData<D>,
}

// TODO(madsmtm): Expose this in `objc2_app_kit`.
fn set_layer(view: &NSView, layer: &CALayer) {
    unsafe { msg_send![view, setLayer: layer] }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for CGImpl<D, W> {
    type Context = D;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    fn new(window_src: W, _display: &D) -> Result<Self, InitError<W>> {
        let raw = window_src.window_handle()?.as_raw();
        let handle = match raw {
            RawWindowHandle::AppKit(handle) => handle,
            _ => return Err(InitError::Unsupported(window_src)),
        };

        let mtm =
            MainThreadMarker::new().expect("can only access AppKit handles on the main thread");
        let view = handle.ns_view.as_ptr();
        // SAFETY: The pointer came from `WindowHandle`, which ensures that
        // the `AppKitWindowHandle` contains a valid pointer to an `NSView`.
        // Unwrap is fine, since the pointer came from `NonNull`.
        let view: Id<NSView> = unsafe { Id::retain(view.cast()) }.unwrap();
        let layer = CALayer::new();
        let subview = unsafe { NSView::initWithFrame(mtm.alloc(), view.frame()) };
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });
        layer.setNeedsDisplayOnBoundsChange(false);
        set_layer(&subview, &layer);
        unsafe {
            subview.setAutoresizingMask(NSAutoresizingMaskOptions(
                NSAutoresizingMaskOptions::NSViewWidthSizable.0
                    | NSAutoresizingMaskOptions::NSViewHeightSizable.0,
            ))
        };

        unsafe { view.addSubview(&subview) };
        let color_space = CGColorSpace::create_device_rgb();
        Ok(Self {
            layer,
            window: view.window().expect("view to be inside a window"),
            color_space,
            size: None,
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        self.size = Some((width, height));
        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let (width, height) = self
            .size
            .expect("Must set size of surface before calling `buffer_mut()`");

        Ok(BufferImpl {
            buffer: vec![0; width.get() as usize * height.get() as usize],
            imp: self,
        })
    }
}

pub struct BufferImpl<'a, D, W> {
    imp: &'a mut CGImpl<D, W>,
    buffer: Vec<u32>,
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'a, D, W> {
    #[inline]
    fn pixels(&self) -> &[u32] {
        &self.buffer
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }

    fn age(&self) -> u8 {
        0
    }

    fn present(self) -> Result<(), SoftBufferError> {
        let data_provider = CGDataProvider::from_buffer(Arc::new(Buffer(self.buffer)));
        let (width, height) = self.imp.size.unwrap();
        let image = CGImage::new(
            width.get() as usize,
            height.get() as usize,
            8,
            32,
            (width.get() * 4) as usize,
            &self.imp.color_space,
            kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst,
            &data_provider,
            false,
            kCGRenderingIntentDefault,
        );

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be mitigated by wrapping the operation in a transaction and disabling all actions.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        self.imp
            .layer
            .setContentsScale(self.imp.window.backingScaleFactor());

        unsafe {
            self.imp
                .layer
                .setContents((image.as_ptr() as *mut AnyObject).as_ref());
        };

        CATransaction::commit();

        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.present()
    }
}
