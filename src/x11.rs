use crate::{GraphicsContextImpl, SwBufError};
use raw_window_handle::{XlibDisplayHandle, XlibWindowHandle};
use std::os::raw::{c_char, c_uint};
use x11_dl::xlib::{Display, Visual, Xlib, ZPixmap, GC};

pub struct X11Impl {
    window_handle: XlibWindowHandle,
    display_handle: XlibDisplayHandle,
    lib: Xlib,
    gc: GC,
    visual: *mut Visual,
    depth: i32,
}

impl X11Impl {
    pub unsafe fn new(window_handle: XlibWindowHandle, display_handle: XlibDisplayHandle) -> Result<Self, SwBufError> {
        let lib = match Xlib::open() {
            Ok(lib) => lib,
            Err(e) => return Err(SwBufError::PlatformError(Some("Failed to open Xlib".into()), Some(Box::new(e))))
        };
        let screen = (lib.XDefaultScreen)(display_handle.display as *mut Display);
        let gc = (lib.XDefaultGC)(display_handle.display as *mut Display, screen);
        let visual = (lib.XDefaultVisual)(display_handle.display as *mut Display, screen);
        let depth = (lib.XDefaultDepth)(display_handle.display as *mut Display, screen);

        Ok(
            Self {
                window_handle,
                display_handle,
                lib,
                gc,
                visual,
                depth,
            }
        )
    }
}

impl GraphicsContextImpl for X11Impl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        //create image
        let image = (self.lib.XCreateImage)(
            self.display_handle.display as *mut Display,
            self.visual,
            self.depth as u32,
            ZPixmap,
            0,
            (buffer.as_ptr()) as *mut c_char,
            width as u32,
            height as u32,
            32,
            (width * 4) as i32,
        );

        //push image to window
        (self.lib.XPutImage)(
            self.display_handle.display as *mut Display,
            self.window_handle.window,
            self.gc,
            image,
            0,
            0,
            0,
            0,
            width as c_uint,
            height as c_uint,
        );

        (*image).data = std::ptr::null_mut();
        (self.lib.XDestroyImage)(image);
    }
}
