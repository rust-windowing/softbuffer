use crate::{GraphicsContextImpl, SwBufError};
use raw_window_handle::AppKitWindowHandle;
use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst, kCGRenderingIntentDefault};
use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::image::CGImage;

use cocoa::base::{id, nil};
use cocoa::appkit::{NSView, NSViewWidthSizable, NSViewHeightSizable};
use cocoa::quartzcore::{CALayer, ContentsGravity};
use foreign_types::ForeignType;

use std::sync::Arc;

pub struct CGImpl {
    layer: CALayer,
}

impl CGImpl {
    pub unsafe fn new(handle: AppKitWindowHandle) -> Result<Self, SwBufError> {
        let view = handle.ns_view as id;
        let layer = CALayer::new();
        let subview: id = NSView::alloc(nil).initWithFrame_(view.frame());
        layer.set_contents_gravity(ContentsGravity::TopLeft);
        layer.set_needs_display_on_bounds_change(false);
        subview.setLayer(layer.id());
        subview.setAutoresizingMask_(NSViewWidthSizable | NSViewHeightSizable);

        view.addSubview_(subview); // retains subview (+1) = 2
        let _: () = msg_send![subview, release]; // releases subview (-1) = 1
        Ok(Self{layer})
    }
}

impl GraphicsContextImpl for CGImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let color_space = CGColorSpace::create_device_rgb();
        let data = std::slice::from_raw_parts(
            buffer.as_ptr() as *const u8,
            buffer.len() * 4).to_vec();
        let data_provider = CGDataProvider::from_buffer(Arc::new(data));
        let image = CGImage::new(
            width as usize,
            height as usize,
            8,
            32,
            (width * 4) as usize,
            &color_space,
            kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst,
            &data_provider,
            false,
            kCGRenderingIntentDefault,
        );
        self.layer.set_contents(image.as_ptr() as id);
    }
}
