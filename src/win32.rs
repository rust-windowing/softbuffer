use crate::{GraphicsContextImpl, SwBufError};
use raw_window_handle::{HasRawWindowHandle, Win32WindowHandle};
use std::os::raw::c_int;
use winapi::shared::windef::{HDC, HWND};
use winapi::um::wingdi::{StretchDIBits, BITMAPINFOHEADER, BI_BITFIELDS, RGBQUAD};
use winapi::um::winuser::{GetDC, ValidateRect};

pub struct Win32Impl {
    window: HWND,
    dc: HDC,
}

// Wrap this so we can have a proper number of bmiColors to write in
// From minifb
#[repr(C)]
struct BitmapInfo {
    pub bmi_header: BITMAPINFOHEADER,
    pub bmi_colors: [RGBQUAD; 3],
}

impl Win32Impl {
    pub unsafe fn new<W: HasRawWindowHandle>(handle: &Win32WindowHandle) -> Result<Self, crate::SwBufError<W>> {
        let dc = GetDC(handle.hwnd as HWND);

        if dc.is_null(){
            return Err(SwBufError::PlatformError(Some("Device Context is null".into()), None));
        }

        Ok(
            Self {
                dc,
                window: handle.hwnd as HWND,
            }
        )
    }
}

impl GraphicsContextImpl for Win32Impl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let mut bitmap_info: BitmapInfo = std::mem::zeroed();

        bitmap_info.bmi_header.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmi_header.biPlanes = 1;
        bitmap_info.bmi_header.biBitCount = 32;
        bitmap_info.bmi_header.biCompression = BI_BITFIELDS;
        bitmap_info.bmi_header.biWidth = width as i32;
        bitmap_info.bmi_header.biHeight = -(height as i32);
        bitmap_info.bmi_colors[0].rgbRed = 0xff;
        bitmap_info.bmi_colors[1].rgbGreen = 0xff;
        bitmap_info.bmi_colors[2].rgbBlue = 0xff;

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
            std::mem::transmute(buffer.as_ptr()),
            std::mem::transmute(&bitmap_info),
            winapi::um::wingdi::DIB_RGB_COLORS,
            winapi::um::wingdi::SRCCOPY,
        );

        ValidateRect(self.window, std::ptr::null_mut());
    }
}
