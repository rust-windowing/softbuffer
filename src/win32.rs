//! Implementation of software buffering for Windows.
//!
//! This module converts the input buffer into a bitmap and then stretches it to the window.

use crate::SoftBufferError;
use raw_window_handle::Win32WindowHandle;

use std::io;
use std::mem;
use std::ptr::{self, NonNull};
use std::slice;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi;

const ZERO_QUAD: Gdi::RGBQUAD = Gdi::RGBQUAD {
    rgbBlue: 0,
    rgbGreen: 0,
    rgbRed: 0,
    rgbReserved: 0,
};

struct Buffer {
    dc: Gdi::HDC,
    bitmap: Gdi::HBITMAP,
    pixels: NonNull<u32>,
    width: i32,
    height: i32,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            Gdi::DeleteDC(self.dc);
            Gdi::DeleteObject(self.bitmap);
        }
    }
}

impl Buffer {
    fn new(window_dc: Gdi::HDC, width: i32, height: i32) -> Self {
        let dc = unsafe { Gdi::CreateCompatibleDC(window_dc) };
        assert!(dc != 0);

        // Create a new bitmap info struct.
        let bitmap_info = BitmapInfo {
            bmi_header: Gdi::BITMAPINFOHEADER {
                biSize: mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: Gdi::BI_BITFIELDS,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmi_colors: [
                Gdi::RGBQUAD {
                    rgbRed: 0xff,
                    ..ZERO_QUAD
                },
                Gdi::RGBQUAD {
                    rgbGreen: 0xff,
                    ..ZERO_QUAD
                },
                Gdi::RGBQUAD {
                    rgbBlue: 0xff,
                    ..ZERO_QUAD
                },
            ],
        };

        // XXX alignment?
        // XXX better to use CreateFileMapping, and pass hSection?
        // XXX test return value?
        let mut pixels: *mut u32 = ptr::null_mut();
        let bitmap = unsafe {
            Gdi::CreateDIBSection(
                dc,
                &bitmap_info as *const BitmapInfo as *const _,
                Gdi::DIB_RGB_COLORS,
                &mut pixels as *mut *mut u32 as _,
                0,
                0,
            )
        };
        assert!(bitmap != 0);
        let pixels = NonNull::new(pixels).unwrap();

        unsafe {
            Gdi::SelectObject(dc, bitmap);
        }

        Self {
            dc,
            bitmap,
            width,
            height,
            pixels,
        }
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        unsafe {
            slice::from_raw_parts_mut(
                self.pixels.as_ptr(),
                self.width as usize * self.height as usize,
            )
        }
    }
}

/// The handle to a window for software buffering.
pub struct Win32Impl {
    /// The window handle.
    window: HWND,

    /// The device context for the window.
    dc: Gdi::HDC,

    buffer: Option<Buffer>,
}

/// The Win32-compatible bitmap information.
#[repr(C)]
struct BitmapInfo {
    bmi_header: Gdi::BITMAPINFOHEADER,
    bmi_colors: [Gdi::RGBQUAD; 3],
}

impl Win32Impl {
    /// Create a new `Win32Impl` from a `Win32WindowHandle`.
    ///
    /// # Safety
    ///
    /// The `Win32WindowHandle` must be a valid window handle.
    pub unsafe fn new(handle: &Win32WindowHandle) -> Result<Self, crate::SoftBufferError> {
        // It is valid for the window handle to be null here. Error out if it is.
        if handle.hwnd.is_null() {
            return Err(SoftBufferError::IncompleteWindowHandle);
        }

        // Get the handle to the device context.
        // SAFETY: We have confirmed that the window handle is valid.
        let hwnd = handle.hwnd as HWND;
        let dc = unsafe { Gdi::GetDC(hwnd) };

        // GetDC returns null if there is a platform error.
        if dc == 0 {
            return Err(SoftBufferError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            ));
        }

        Ok(Self {
            dc,
            window: hwnd,
            buffer: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(buffer) = self.buffer.as_ref() {
            if buffer.width == width as i32 && buffer.height == height as i32 {
                return;
            }
        }

        self.buffer = if width != 0 && height != 0 {
            Some(Buffer::new(self.dc, width as i32, height as i32))
        } else {
            None
        }
    }

    pub fn buffer_mut(&mut self) -> &mut [u32] {
        self.buffer.as_mut().map_or(&mut [], Buffer::pixels_mut)
    }

    pub fn present(&mut self) -> Result<(), SoftBufferError> {
        if let Some(buffer) = &self.buffer {
            unsafe {
                Gdi::BitBlt(
                    self.dc,
                    0,
                    0,
                    buffer.width,
                    buffer.height,
                    buffer.dc,
                    0,
                    0,
                    Gdi::SRCCOPY,
                );

                // Validate the window.
                Gdi::ValidateRect(self.window, ptr::null_mut());
            }
        }

        Ok(())
    }
}
