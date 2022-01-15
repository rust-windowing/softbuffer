use std::os::raw::{c_char, c_uint};
use raw_window_handle::XlibHandle;
use x11_dl::xlib::{Display, GC, Visual, Xlib, ZPixmap};
use crate::GraphicsContextImpl;

pub struct X11Impl{
    handle: XlibHandle,
    lib: Xlib,
    gc: GC,
    visual: *mut Visual,
    depth: i32
}

impl X11Impl {

    pub unsafe fn new(handle: XlibHandle) -> Self{
        let lib = Xlib::open().unwrap();
        let screen = (lib.XDefaultScreen)(handle.display as *mut Display);
        let gc = (lib.XDefaultGC)(handle.display as *mut Display, screen);
        let visual = (lib.XDefaultVisual)(handle.display as *mut Display, screen);
        let depth = (lib.XDefaultDepth)(handle.display as *mut Display, screen);

        Self{
            handle,
            lib,
            gc,
            visual,
            depth
        }
    }

}

impl GraphicsContextImpl for X11Impl {

    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        //create image
        let image = (self.lib.XCreateImage)(
            self.handle.display as *mut Display,
            self.visual,
            self.depth as u32,
            ZPixmap,
            0,
            (buffer.as_ptr()) as *mut c_char,
            width as u32,
            height as u32,
            32,
            (width*4) as i32
        );

        if image.is_null(){
            panic!("Image is null!");
        }

        //push image to window
        (self.lib.XPutImage)(
            self.handle.display as *mut Display,
            self.handle.window,
            self.gc,
            image,
            0,
            0,
            0,
            0,
            width as c_uint,
            height as c_uint
        );

        (*image).data = std::ptr::null_mut();
        (self.lib.XDestroyImage)(image);
    }

}