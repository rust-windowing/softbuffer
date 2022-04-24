use crate::{GraphicsContextImpl, SoftBufferError};
use raw_window_handle::{HasRawWindowHandle, AppKitHandle};
use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst, kCGRenderingIntentDefault};
use core_graphics::color_space::CGColorSpace;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::geometry::CGSize;
use core_graphics::image::CGImage;

use cocoa::base::id;
use cocoa::quartzcore::CALayer;
use foreign_types::ForeignType;

pub struct CGImpl {
    layer: CALayer,
}

impl CGImpl {
    pub unsafe fn new<W: HasRawWindowHandle>(handle: AppKitHandle) -> Result<Self, SoftBufferError<W>> {
        let view = handle.ns_view as id;
        let layer = CALayer::new();
        let _: () = msg_send![view, setLayer:layer.clone()];
        Ok(Self{layer})
    }
}

impl GraphicsContextImpl for CGImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let color_space = CGColorSpace::create_device_rgb();
        let slice = std::slice::from_raw_parts(
            buffer.as_ptr() as *const u8,
            buffer.len() * 4);
        let data_provider = CGDataProvider::from_slice(slice);
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

        let size = CGSize::new(width as f64, height as f64);
        let rep: id = msg_send![class!(NSCGImageRep), alloc];
        let rep: id = msg_send![rep, initWithCGImage:image.as_ptr() size:size];

        let nsimage: id = msg_send![class!(NSImage), alloc];
        let nsimage: id = msg_send![nsimage, initWithSize:size];
        let _: () = msg_send![nsimage, addRepresentation:rep];
        let _: () = msg_send![rep, release];

        self.layer.set_contents(nsimage);
        let _: () = msg_send![nsimage, release];
    }
}
