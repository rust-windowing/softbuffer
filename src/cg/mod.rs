use crate::SoftBufferError;
use raw_window_handle::AppKitWindowHandle;

use cocoa::appkit::{NSView, NSViewHeightSizable, NSViewWidthSizable, NSWindow};
use cocoa::base::{id, nil};
use cocoa::quartzcore::{transaction, CALayer, ContentsGravity};

use std::num::NonZeroU32;
use std::ptr;

mod buffer;
use buffer::Buffer;

pub struct CGImpl {
    layer: CALayer,
    window: id,
    width: u32,
    height: u32,
    data: Vec<u32>,
    buffer: Option<Buffer>,
}

impl CGImpl {
    pub unsafe fn new(handle: AppKitWindowHandle) -> Result<Self, SoftBufferError> {
        let window = handle.ns_window as id;
        let window: id = msg_send![window, retain];
        let view = handle.ns_view as id;
        let layer = CALayer::new();
        unsafe {
            let subview: id = NSView::alloc(nil).initWithFrame_(NSView::frame(view));
            layer.set_contents_gravity(ContentsGravity::TopLeft);
            layer.set_needs_display_on_bounds_change(false);
            subview.setLayer(layer.id());
            subview.setAutoresizingMask_(NSViewWidthSizable | NSViewHeightSizable);

            view.addSubview_(subview); // retains subview (+1) = 2
            let _: () = msg_send![subview, release]; // releases subview (-1) = 1
        }
        Ok(Self {
            layer,
            window,
            width: 0,
            height: 0,
            data: Vec::new(),
            buffer: None,
        })
    }

    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let width = width.get();
        let height = height.get();
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.data.resize(width as usize * height as usize, 0);
            self.buffer = Some(Buffer::new(width, height));
        }
        Ok(())
    }

    pub fn buffer_mut(&mut self) -> Result<BufferImpl, SoftBufferError> {
        if self.buffer.is_none() {
            panic!("Must set size of surface before calling `buffer_mut()`");
        }

        Ok(BufferImpl { imp: self })
    }
}

pub struct BufferImpl<'a> {
    imp: &'a mut CGImpl,
}

impl<'a> BufferImpl<'a> {
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        &self.imp.data
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.imp.data
    }

    pub fn present(self) -> Result<(), SoftBufferError> {
        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be mitigated by wrapping the operation in a transaction and disabling all actions.
        transaction::begin();
        transaction::set_disable_actions(true);

        let buffer = self.imp.buffer.as_mut().unwrap();
        unsafe {
            // Copy pixels into `IOSurface` buffer, with right stride and
            // alpha
            buffer.lock();
            let stride = buffer.stride();
            let pixels = buffer.pixels_mut();
            let width = self.imp.width as usize;
            for y in 0..self.imp.height as usize {
                for x in 0..width {
                    // Set alpha to 255
                    let value = self.imp.data[y * width + x] | (255 << 24);
                    pixels[y * stride + x] = value;
                }
            }
            buffer.unlock();

            self.imp
                .layer
                .set_contents_scale(self.imp.window.backingScaleFactor());
            self.imp.layer.set_contents(ptr::null_mut());
            self.imp.layer.set_contents(buffer.as_ptr() as id);
        };

        transaction::commit();

        Ok(())
    }
}

impl Drop for CGImpl {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.window, release];
        }
    }
}
