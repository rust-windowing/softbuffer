use crate::SoftBufferError;
use core_graphics::base::{
    kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst, kCGRenderingIntentDefault,
};
use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::image::CGImage;
use raw_window_handle::AppKitWindowHandle;

use cocoa::appkit::{NSView, NSViewHeightSizable, NSViewWidthSizable, NSWindow};
use cocoa::base::{id, nil};
use cocoa::quartzcore::{transaction, CALayer, ContentsGravity};
use foreign_types::ForeignType;

use std::sync::Arc;

struct Buffer(Vec<u32>);

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        bytemuck::cast_slice(&self.0)
    }
}

pub struct CGImpl {
    layer: CALayer,
    color_space: CGColorSpace,
    buffer: Option<Vec<u32>>,
    width: u32,
    height: u32,
}

impl CGImpl {
    pub unsafe fn new(handle: AppKitWindowHandle) -> Result<Self, SoftBufferError> {
        let window = handle.ns_window as id;
        let view = handle.ns_view as id;
        let layer = CALayer::new();
        unsafe {
            let subview: id = NSView::alloc(nil).initWithFrame_(NSView::frame(view));
            layer.set_contents_gravity(ContentsGravity::TopLeft);
            layer.set_needs_display_on_bounds_change(false);
            layer.set_contents_scale(window.backingScaleFactor());
            subview.setLayer(layer.id());
            subview.setAutoresizingMask_(NSViewWidthSizable | NSViewHeightSizable);

            view.addSubview_(subview); // retains subview (+1) = 2
            let _: () = msg_send![subview, release]; // releases subview (-1) = 1
        }
        let color_space = CGColorSpace::create_device_rgb();
        Ok(Self {
            layer,
            color_space,
            width: 0,
            height: 0,
            buffer: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    pub fn buffer_mut(&mut self) -> &mut [u32] {
        if self.buffer.is_none() {
            self.buffer = Some(Vec::new());
        }
        let buffer = self.buffer.as_mut().unwrap();
        buffer.resize(self.width as usize * self.height as usize * 4, 0);
        buffer.as_mut()
    }

    pub fn present(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            let data_provider = CGDataProvider::from_buffer(Arc::new(Buffer(buffer)));
            let image = CGImage::new(
                self.width as usize,
                self.height as usize,
                8,
                32,
                (self.width * 4) as usize,
                &self.color_space,
                kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst,
                &data_provider,
                false,
                kCGRenderingIntentDefault,
            );

            // The CALayer has a default action associated with a change in the layer contents, causing
            // a quarter second fade transition to happen every time a new buffer is applied. This can
            // be mitigated by wrapping the operation in a transaction and disabling all actions.
            transaction::begin();
            transaction::set_disable_actions(true);

            unsafe { self.layer.set_contents(image.as_ptr() as id) };

            transaction::commit();
        }
    }

    pub(crate) unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        self.resize(width.into(), height.into());
        self.buffer_mut().copy_from_slice(buffer);
        self.present();
    }
}
