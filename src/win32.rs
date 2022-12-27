//! Implementation of software buffering for Windows.
//!
//! This module converts the input buffer into a bitmap and then stretches it to the window.

use crate::SoftBufferError;
use raw_window_handle::Win32WindowHandle;

use std::io;
use std::mem;
use std::os::raw::c_int;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    GetDC, StretchDIBits, ValidateRect, BITMAPINFOHEADER, BI_BITFIELDS, DIB_RGB_COLORS, HDC,
    RGBQUAD, SRCCOPY,
};

/// The handle to a window for software buffering.
pub struct Win32Impl {
    /// The window handle.
    window: HWND,

    /// The device context for the window.
    dc: HDC,
}

/// The Win32-compatible bitmap information.
#[repr(C)]
struct BitmapInfo {
    pub bmi_header: BITMAPINFOHEADER,
    pub bmi_colors: [RGBQUAD; 3],
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
        let dc = unsafe { GetDC(hwnd) };

        // GetDC returns null if there is a platform error.
        if dc == 0 {
            return Err(SoftBufferError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            ));
        }

        Ok(Self { dc, window: hwnd })
    }

    pub(crate) unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        // Create a new bitmap info struct.
        let bmi_header = BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_BITFIELDS,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let zero_quad = RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        };
        let bmi_colors = [
            RGBQUAD {
                rgbRed: 0xff,
                ..zero_quad
            },
            RGBQUAD {
                rgbGreen: 0xff,
                ..zero_quad
            },
            RGBQUAD {
                rgbBlue: 0xff,
                ..zero_quad
            },
        ];
        let bitmap_info = BitmapInfo {
            bmi_header,
            bmi_colors,
        };

        // Stretch the bitmap onto the window.
        // SAFETY:
        //  - The bitmap information is valid.
        //  - The buffer is a valid pointer to image data.
        unsafe {
            StretchDIBits(
                self.dc,
                0,
                0,
                width as c_int,
                height as c_int,
                0,
                0,
                width as c_int,
                height as c_int,
                buffer.as_ptr().cast(),
                &bitmap_info as *const BitmapInfo as *const _,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };

        // Validate the window.
        unsafe { ValidateRect(self.window, std::ptr::null_mut()) };
    }
}
