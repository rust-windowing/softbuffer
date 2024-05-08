//! Implementation of software buffering for Windows.
//!
//! This module converts the input buffer into a bitmap and then stretches it to the window.

use crate::backend_interface::*;
use crate::{Rect, SoftBufferError};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::io;
use std::marker::PhantomData;
use std::mem;
use std::num::{NonZeroI32, NonZeroU32};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi;
use windows_sys::Win32::UI::{Shell, WindowsAndMessaging as Win};

const ZERO_QUAD: Gdi::RGBQUAD = Gdi::RGBQUAD {
    rgbBlue: 0,
    rgbGreen: 0,
    rgbRed: 0,
    rgbReserved: 0,
};
const SUBCLASS_ID: usize = 0xDEADBEEF;

struct Buffer {
    // Invariant: This window has our custom softbuffer subclass.
    owner_window: HWND,
    dc: Gdi::HDC,
    bitmap: Gdi::HBITMAP,
    pixels: NonNull<u32>,
    width: NonZeroI32,
    height: NonZeroI32,
    presented: bool,
}

// SAFETY: We allow no interior mutability here and we drop the HDC on its origin thread.
unsafe impl Send for Buffer {}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            // Delete the DC by posting a message that calls DeleteDC on the origin thread.
            Win::PostMessageW(
                self.owner_window,
                destroy_dc_and_bitmap(),
                self.dc as usize,
                self.bitmap,
            );
        }
    }
}

impl Buffer {
    fn new(owner: HWND, window_dc: Gdi::HDC, width: NonZeroI32, height: NonZeroI32) -> Self {
        let dc = unsafe { Win::SendMessageW(owner, get_compatible_dc(), 0, window_dc) };
        assert!(dc != 0);

        // Create a new bitmap info struct.
        let bitmap_info = BitmapInfo {
            bmi_header: Gdi::BITMAPINFOHEADER {
                biSize: mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: width.get(),
                biHeight: -height.get(),
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
        // Note: Bitmaps are apparently threadsafe.
        // https://devblogs.microsoft.com/oldnewthing/20051013-11/?p=33783
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
            owner_window: owner,
            dc,
            bitmap,
            width,
            height,
            pixels,
            presented: false,
        }
    }

    #[inline]
    fn pixels(&self) -> &[u32] {
        unsafe {
            slice::from_raw_parts(
                self.pixels.as_ptr(),
                i32::from(self.width) as usize * i32::from(self.height) as usize,
            )
        }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        unsafe {
            slice::from_raw_parts_mut(
                self.pixels.as_ptr(),
                i32::from(self.width) as usize * i32::from(self.height) as usize,
            )
        }
    }
}

/// The handle to a window for software buffering.
pub struct Win32Impl<D: ?Sized, W> {
    /// The window handle. We do not own this.
    window: HWND,

    /// The device context for the window.
    dc: Gdi::HDC,

    /// The buffer used to hold the image.
    buffer: Option<Buffer>,

    /// The handle for the window.
    ///
    /// This should be kept alive in order to keep `window` valid.
    handle: W,

    /// The display handle.
    ///
    /// We don't use this, but other code might.
    _display: PhantomData<D>,
}

/// The Win32-compatible bitmap information.
#[repr(C)]
struct BitmapInfo {
    bmi_header: Gdi::BITMAPINFOHEADER,
    bmi_colors: [Gdi::RGBQUAD; 3],
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Win32Impl<D, W> {
    fn present_with_damage(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let buffer = self.buffer.as_mut().unwrap();
        unsafe {
            for rect in damage.iter().copied() {
                let (x, y, width, height) = (|| {
                    Some((
                        i32::try_from(rect.x).ok()?,
                        i32::try_from(rect.y).ok()?,
                        i32::try_from(rect.width.get()).ok()?,
                        i32::try_from(rect.height.get()).ok()?,
                    ))
                })()
                .ok_or(SoftBufferError::DamageOutOfRange { rect })?;
                Gdi::BitBlt(self.dc, x, y, width, height, buffer.dc, x, y, Gdi::SRCCOPY);
            }

            // Validate the window.
            Gdi::ValidateRect(self.window, ptr::null_mut());
        }
        buffer.presented = true;

        Ok(())
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for Win32Impl<D, W> {
    type Context = D;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    /// Create a new `Win32Impl` from a `Win32WindowHandle`.
    fn new(window: W, _display: &D) -> Result<Self, crate::error::InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let handle = match raw {
            RawWindowHandle::Win32(handle) => handle,
            _ => return Err(crate::InitError::Unsupported(window)),
        };

        // Get the handle to the device context.
        // SAFETY: We have confirmed that the window handle is valid.
        // SAFETY: By the safety conditions for window_handle, this window is valid for
        // this thread.
        let hwnd = handle.hwnd.get() as HWND;

        // Add a subclass to this window that lets us perform some Windows operations
        // on its original thread.
        let result =
            unsafe { Shell::SetWindowSubclass(hwnd, Some(handle_dc_subclass), SUBCLASS_ID, 0) };
        if result == 0 {
            return Err(SoftBufferError::PlatformError(
                Some("Unable to set window subclass".into()),
                Some(Box::new(io::Error::last_os_error())),
            )
            .into());
        }

        let dc = unsafe { Gdi::GetDC(hwnd) };

        // GetDC returns null if there is a platform error.
        if dc == 0 {
            return Err(SoftBufferError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            )
            .into());
        }

        Ok(Self {
            dc,
            window: hwnd,
            buffer: None,
            handle: window,
            _display: PhantomData,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let (width, height) = (|| {
            let width = NonZeroI32::new(i32::try_from(width.get()).ok()?)?;
            let height = NonZeroI32::new(i32::try_from(height.get()).ok()?)?;
            Some((width, height))
        })()
        .ok_or(SoftBufferError::SizeOutOfRange { width, height })?;

        if let Some(buffer) = self.buffer.as_ref() {
            if buffer.width == width && buffer.height == height {
                return Ok(());
            }
        }

        self.buffer = Some(Buffer::new(self.window, self.dc, width, height));

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        if self.buffer.is_none() {
            panic!("Must set size of surface before calling `buffer_mut()`");
        }

        Ok(BufferImpl(self))
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

impl<D: ?Sized, W> Drop for Win32Impl<D, W> {
    fn drop(&mut self) {
        // Remove the subclass from the window on its origin thread.
        unsafe {
            Win::PostMessageW(self.window, remove_subclass(), 0, self.dc);
        }
    }
}

pub struct BufferImpl<'a, D, W>(&'a mut Win32Impl<D, W>);

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'a, D, W> {
    #[inline]
    fn pixels(&self) -> &[u32] {
        self.0.buffer.as_ref().unwrap().pixels()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        self.0.buffer.as_mut().unwrap().pixels_mut()
    }

    fn age(&self) -> u8 {
        match self.0.buffer.as_ref() {
            Some(buffer) if buffer.presented => 1,
            _ => 0,
        }
    }

    fn present(self) -> Result<(), SoftBufferError> {
        let imp = self.0;
        let buffer = imp.buffer.as_ref().unwrap();
        imp.present_with_damage(&[Rect {
            x: 0,
            y: 0,
            // We know width/height will be non-negative
            width: buffer.width.try_into().unwrap(),
            height: buffer.height.try_into().unwrap(),
        }])
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let imp = self.0;
        imp.present_with_damage(damage)
    }
}

/// Window procedure for our custom subclass.
unsafe extern "system" fn handle_dc_subclass(
    hwnd: HWND,
    umsg: u32,
    wparam: usize,
    lparam: isize,
    _subclass_id: usize,
    _subclass_data: usize,
) -> isize {
    abort_on_panic(move || {
        if umsg == get_compatible_dc() {
            // Get a DC.
            unsafe { Gdi::CreateCompatibleDC(lparam) }
        } else if umsg == destroy_dc_and_bitmap() {
            // Destroy the DC and bitmap for this window.
            unsafe {
                Gdi::DeleteDC(wparam as isize);
                Gdi::DeleteObject(lparam);
            }

            0
        } else if umsg == remove_subclass() {
            // Release the existing DC.
            unsafe {
                Gdi::ReleaseDC(hwnd, lparam);
            }

            // Remove the subclass from the window.
            unsafe {
                Shell::RemoveWindowSubclass(hwnd, Some(handle_dc_subclass), SUBCLASS_ID);
            }

            0
        } else {
            // This isn't one of our custom messages. Forward to the underlying class.
            unsafe { Shell::DefSubclassProc(hwnd, umsg, wparam, lparam) }
        }
    })
}

macro_rules! static_message {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        fn $name() -> u32 {
            static MESSAGE: AtomicU32 = AtomicU32::new(0);
            const MESSAGE_NAME: &str = concat!(
                "softbuffer_", env!("CARGO_PKG_VERSION"), "_", stringify!($name)
            );

            let mut result = MESSAGE.load(Ordering::Relaxed);

            if result == 0 {
                // Register the window message, then store it.
                let message = MESSAGE_NAME.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                result = unsafe {
                    Win::RegisterWindowMessageW(message.as_ptr())
                };
                MESSAGE.store(result, Ordering::SeqCst);
            }

            result
        }
    }
}

static_message! {
    /// Get the compatible DC for an existing DC.
    ///
    /// lparam is the DC. The returned value is the DC.
    get_compatible_dc
}

static_message! {
    /// Destroy a DC and a bitmap for this window.
    ///
    /// wparam is the DC, lparam is the bitmap.
    destroy_dc_and_bitmap
}

static_message! {
    /// Remove our subclass and release our DC.
    ///
    /// lparam is the DC.
    remove_subclass
}

fn abort_on_panic<R>(f: impl FnOnce() -> R) -> R {
    struct AbortOnDrop;

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            std::process::abort();
        }
    }

    let bomb = AbortOnDrop;
    let data = f();
    core::mem::forget(bomb);
    data
}
