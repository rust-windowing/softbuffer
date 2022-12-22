//! Implementation of software buffering for Windows.
//! 
//! This module converts the input buffer into a bitmap and then stretches it to the window.

use crate::{GraphicsContextImpl, SwBufError};
use raw_window_handle::Win32WindowHandle;

use std::mem;
use std::io;
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
    pub unsafe fn new(handle: &Win32WindowHandle) -> Result<Self, crate::SwBufError> {
        // It is valid for the window handle to be null here. Error out if it is.
        if handle.hwnd.is_null() {
            return Err(SwBufError::IncompleteWindowHandle);
        }

        // Get the handle to the device context.
        // SAFETY: We have confirmed that the window handle is valid.
        let hwnd = handle.hwnd as HWND;
        let dc = GetDC(hwnd);

        // GetDC returns null if there is a platform error.
        if dc == 0 {
            return Err(SwBufError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            ));
        }

        Ok(Self {
            dc,
            window: hwnd,
        })
    }
}

impl GraphicsContextImpl for Win32Impl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        // Create a new bitmap info struct.
        let mut bitmap_info: BitmapInfo =mem::zeroed();

        bitmap_info.bmi_header.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmi_header.biPlanes = 1;
        bitmap_info.bmi_header.biBitCount = 32;
        bitmap_info.bmi_header.biCompression = BI_BITFIELDS;
        bitmap_info.bmi_header.biWidth = width as i32;
        bitmap_info.bmi_header.biHeight = -(height as i32);
        bitmap_info.bmi_colors[0].rgbRed = 0xff;
        bitmap_info.bmi_colors[1].rgbGreen = 0xff;
        bitmap_info.bmi_colors[2].rgbBlue = 0xff;

        // Stretch the bitmap onto the window.
        // SAFETY:
        //  - The bitmap information is valid.
        //  - The buffer is a valid pointer to image data.
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
        );

        // Validate the window.
        ValidateRect(self.window, std::ptr::null_mut());
    }
}
