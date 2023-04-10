use core_foundation::{
    base::TCFType, dictionary::CFDictionary, number::CFNumber, string::CFString,
};
use io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth, IOSurface, IOSurfaceRef,
};
use std::{ffi::c_int, slice};

#[link(name = "IOSurface", kind = "framework")]
extern "C" {
    fn IOSurfaceGetBaseAddress(buffer: IOSurfaceRef) -> *mut u8;
    fn IOSurfaceGetBytesPerRow(buffer: IOSurfaceRef) -> usize;
    fn IOSurfaceLock(buffer: IOSurfaceRef, options: u32, seed: *mut u32) -> c_int;
    fn IOSurfaceUnlock(buffer: IOSurfaceRef, options: u32, seed: *mut u32) -> c_int;
}

pub struct Buffer {
    io_surface: IOSurface,
    ptr: *mut u8,
    pixels: usize,
}

impl Buffer {
    pub fn new(width: i32, height: i32) -> Self {
        let properties = unsafe {
            CFDictionary::from_CFType_pairs(&[
                (
                    CFString::wrap_under_get_rule(kIOSurfaceWidth),
                    CFNumber::from(width).as_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kIOSurfaceHeight),
                    CFNumber::from(height).as_CFType(),
                ),
                (
                    CFString::wrap_under_get_rule(kIOSurfaceBytesPerElement),
                    CFNumber::from(4).as_CFType(),
                ),
                // TODO: Can we always use stride = width? Is it efficient?
                /*
                (
                    CFString::wrap_under_get_rule(kIOSurfaceBytesPerRow),
                    CFNumber::from(width).as_CFType(),
                ),
                */
                (
                    CFString::wrap_under_get_rule(kIOSurfacePixelFormat),
                    CFNumber::from(i32::from_be_bytes([b'B', b'G', b'R', b'A'])).as_CFType(),
                ),
            ])
        };
        let io_surface = io_surface::new(&properties);
        let ptr = unsafe { IOSurfaceGetBaseAddress(io_surface.obj) };
        dbg!(width);
        dbg!(unsafe { IOSurfaceGetBytesPerRow(io_surface.obj) } / 4);
        let pixels = width as usize * height as usize;
        Self {
            io_surface,
            ptr,
            pixels,
        }
    }

    pub fn as_ptr(&self) -> IOSurfaceRef {
        self.io_surface.obj
    }

    pub unsafe fn lock(&mut self) {
        let mut seed = 0;
        unsafe {
            IOSurfaceLock(self.io_surface.obj, 0, &mut seed);
        }
    }

    pub unsafe fn unlock(&mut self) {
        let mut seed = 0;
        unsafe {
            IOSurfaceUnlock(self.io_surface.obj, 0, &mut seed);
        }
    }

    // TODO: We can assume alignment, right?
    #[inline]
    pub unsafe fn pixels_ref(&self) -> &[u32] {
        unsafe { slice::from_raw_parts(self.ptr as *mut u32, self.pixels) }
    }

    #[inline]
    pub unsafe fn pixels_mut(&self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u32, self.pixels) }
    }
}
