use crate::{GraphicsContextImpl, SoftBufferError};
use raw_window_handle::{HasRawWindowHandle, AppKitHandle};
use objc::runtime::Object;
use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst, kCGRenderingIntentDefault};
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::geometry::{CGPoint, CGSize, CGRect};
use core_graphics::image::CGImage;
use core_graphics::sys;

pub struct CGImpl;

impl CGImpl {
    pub unsafe fn new<W: HasRawWindowHandle>(handle: AppKitHandle) -> Result<Self, SoftBufferError<W>> {
        let window = handle.ns_window as *mut Object;
        let cls = class!(NSGraphicsContext);
        let graphics_context: *mut Object = msg_send![cls, graphicsContextWithWindow:window];
        if graphics_context.is_null() {
            return Err(SoftBufferError::PlatformError(Some("Graphics context is null".into()), None));
        }
        let _: () = msg_send![cls, setCurrentContext:graphics_context];
        Ok(Self)
    }
}

impl GraphicsContextImpl for CGImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let cls = class!(NSGraphicsContext);
        let graphics_context: *mut Object = msg_send![cls, currentContext];
        let context_ptr: *mut sys::CGContext = msg_send![graphics_context, CGContext];
        let context = CGContext::from_existing_context_ptr(context_ptr);
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
        let origin = CGPoint { x: 0f64, y: 0f64 };
        let size = CGSize { width: width as f64, height: height as f64 };
        let rect = CGRect { origin, size };
        context.draw_image(rect, &image);
    }
}
