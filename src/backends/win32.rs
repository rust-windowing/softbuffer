//! Implementation of software buffering for Windows.
//!
//! This module converts the input buffer into a bitmap and then stretches it to the window.

use crate::backend_interface::*;
use crate::{util, Rect, SoftBufferError};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::io;
use std::marker::PhantomData;
use std::mem;
use std::ptr::{self, NonNull};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi;

const ZERO_QUAD: Gdi::RGBQUAD = Gdi::RGBQUAD {
    rgbBlue: 0,
    rgbGreen: 0,
    rgbRed: 0,
    rgbReserved: 0,
};

#[derive(Debug)]
struct Buffer {
    dc: Gdi::HDC,
    bitmap: Gdi::HBITMAP,
    pixels: NonNull<[u32]>,
    presented: bool,
}

unsafe impl Send for Buffer {}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            Gdi::DeleteObject(self.bitmap);
        }

        Allocator::get().deallocate(self.dc);
    }
}

impl Buffer {
    fn new(window_dc: Gdi::HDC, width: i32, height: i32) -> Self {
        let dc = Allocator::get().allocate(window_dc);
        assert!(!dc.is_null());

        // Create a new bitmap info struct.
        let bitmap_info = BitmapInfo {
            bmi_header: Gdi::BITMAPINFOHEADER {
                biSize: mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height -> origin is the upper-left corner.
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
                ptr::null_mut(),
                0,
            )
        };
        assert!(!bitmap.is_null());
        let pixels = NonNull::new(pixels).unwrap();
        let pixels = NonNull::slice_from_raw_parts(pixels, width as usize * height as usize);

        unsafe {
            Gdi::SelectObject(dc, bitmap);
        }

        Self {
            dc,
            bitmap,
            pixels,
            presented: false,
        }
    }
}

/// The handle to a window for software buffering.
#[derive(Debug)]
pub struct Win32Impl<D: ?Sized, W> {
    /// The window handle.
    window: OnlyUsedFromOrigin<HWND>,

    /// The device context for the window.
    dc: OnlyUsedFromOrigin<Gdi::HDC>,

    /// The buffer used to hold the image.
    ///
    /// No buffer -> width or height is zero.
    buffer: Option<Buffer>,

    /// The width of the buffer.
    width: u32,

    /// The height of the buffer.
    height: u32,

    /// The handle for the window.
    ///
    /// This should be kept alive in order to keep `window` valid.
    handle: W,

    /// The display handle.
    ///
    /// We don't use this, but other code might.
    _display: PhantomData<D>,
}

impl<D: ?Sized, W> Drop for Win32Impl<D, W> {
    fn drop(&mut self) {
        // Release our resources.
        Allocator::get().release(self.window.0, self.dc.0);
    }
}

/// The Win32-compatible bitmap information.
#[repr(C)]
struct BitmapInfo {
    bmi_header: Gdi::BITMAPINFOHEADER,
    bmi_colors: [Gdi::RGBQUAD; 3],
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for Win32Impl<D, W> {
    type Context = D;
    type Buffer<'a>
        = BufferImpl<'a>
    where
        Self: 'a;

    /// Create a new `Win32Impl` from a `Win32WindowHandle`.
    fn new(window: W, _display: &D) -> Result<Self, crate::error::InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::Win32(handle) = raw else {
            return Err(crate::InitError::Unsupported(window));
        };

        // Get the handle to the device context.
        // SAFETY: We have confirmed that the window handle is valid.
        let hwnd = handle.hwnd.get() as HWND;
        let dc = Allocator::get().get_dc(hwnd);

        // GetDC returns null if there is a platform error.
        if dc.is_null() {
            return Err(SoftBufferError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            )
            .into());
        }

        Ok(Self {
            dc: dc.into(),
            window: hwnd.into(),
            buffer: None,
            width: 0,
            height: 0,
            handle: window,
            _display: PhantomData,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.handle
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), SoftBufferError> {
        let (width_i32, height_i32) = util::convert_size::<i32>(width, height)
            .map_err(|_| SoftBufferError::SizeOutOfRange { width, height })?;

        if self.width == width && self.height == height {
            return Ok(());
        }

        // Attempting to create a zero-sized Gdi::HBITMAP returns NULL, so we handle this case
        // ourselves.
        self.buffer = if width_i32 != 0 && height_i32 != 0 {
            Some(Buffer::new(self.dc.0, width_i32, height_i32))
        } else {
            None
        };
        self.width = width;
        self.height = height;

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_>, SoftBufferError> {
        Ok(BufferImpl {
            window: &self.window,
            dc: &self.dc,
            buffer: &mut self.buffer,
            width: self.width,
            height: self.height,
        })
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

#[derive(Debug)]
pub struct BufferImpl<'a> {
    window: &'a OnlyUsedFromOrigin<HWND>,
    dc: &'a OnlyUsedFromOrigin<Gdi::HDC>,
    buffer: &'a mut Option<Buffer>,
    width: u32,
    height: u32,
}

impl BufferInterface for BufferImpl<'_> {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    fn pixels(&self) -> &[u32] {
        if let Some(buffer) = &self.buffer {
            unsafe { buffer.pixels.as_ref() }
        } else {
            &[]
        }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        if let Some(buffer) = &mut self.buffer {
            unsafe { buffer.pixels.as_mut() }
        } else {
            &mut []
        }
    }

    fn age(&self) -> u8 {
        match self.buffer.as_ref() {
            Some(buffer) if buffer.presented => 1,
            _ => 0,
        }
    }

    fn present(self) -> Result<(), SoftBufferError> {
        let rect = Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        self.present_with_damage(&[rect])
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        if let Some(buffer) = self.buffer {
            for rect in damage.iter().copied() {
                let (x, y, width, height) = (|| {
                    Some((
                        i32::try_from(rect.x).ok()?,
                        i32::try_from(rect.y).ok()?,
                        i32::try_from(rect.width).ok()?,
                        i32::try_from(rect.height).ok()?,
                    ))
                })()
                .ok_or(SoftBufferError::DamageOutOfRange { rect })?;

                unsafe {
                    Gdi::BitBlt(
                        self.dc.0,
                        x,
                        y,
                        width,
                        height,
                        self.dc.0,
                        x,
                        y,
                        Gdi::SRCCOPY,
                    )
                };
            }

            buffer.presented = true;
        } else {
            // No buffer -> don't draw anything, this is consistent with having a zero-sized buffer.
            //
            // Once we implement <https://github.com/rust-windowing/softbuffer/issues/177> though,
            // we'll probably want to clear the window here instead.
        }

        // Validate the window.
        unsafe { Gdi::ValidateRect(self.window.0, ptr::null_mut()) };

        Ok(())
    }
}

/// Allocator for device contexts.
///
/// Device contexts can only be allocated or freed on the thread that originated them.
/// So we spawn a thread specifically for allocating and freeing device contexts.
/// This is the interface to that thread.
struct Allocator {
    /// The channel for sending commands.
    sender: Mutex<mpsc::Sender<Command>>,
}

impl Allocator {
    /// Get the global instance of the allocator.
    fn get() -> &'static Allocator {
        static ALLOCATOR: OnceLock<Allocator> = OnceLock::new();
        ALLOCATOR.get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Command>();

            // Create a thread responsible for DC handling.
            thread::Builder::new()
                .name(concat!("softbuffer_", env!("CARGO_PKG_VERSION"), "_dc_allocator").into())
                .spawn(move || {
                    while let Ok(command) = receiver.recv() {
                        command.handle();
                    }
                })
                .expect("failed to spawn the DC allocator thread");

            Allocator {
                sender: Mutex::new(sender),
            }
        })
    }

    /// Send a command to the allocator thread.
    fn send_command(&self, cmd: Command) {
        self.sender.lock().unwrap().send(cmd).unwrap();
    }

    /// Get the device context for a window.
    fn get_dc(&self, window: HWND) -> Gdi::HDC {
        let (callback, waiter) = mpsc::sync_channel(1);

        // Send command to the allocator.
        self.send_command(Command::GetDc { window, callback });

        // Wait for the response back.
        waiter.recv().unwrap()
    }

    /// Allocate a new device context.
    fn allocate(&self, dc: Gdi::HDC) -> Gdi::HDC {
        let (callback, waiter) = mpsc::sync_channel(1);

        // Send command to the allocator.
        self.send_command(Command::Allocate { dc, callback });

        // Wait for the response back.
        waiter.recv().unwrap()
    }

    /// Deallocate a device context.
    fn deallocate(&self, dc: Gdi::HDC) {
        self.send_command(Command::Deallocate(dc));
    }

    /// Release a device context.
    fn release(&self, owner: HWND, dc: Gdi::HDC) {
        self.send_command(Command::Release { dc, owner });
    }
}

/// Commands to be sent to the allocator.
enum Command {
    /// Call `GetDc` to get the device context for the provided window.
    GetDc {
        /// The window to provide a device context for.
        window: HWND,

        /// Send back the device context.
        callback: mpsc::SyncSender<Gdi::HDC>,
    },

    /// Allocate a new device context using `GetCompatibleDc`.
    Allocate {
        /// The DC to be compatible with.
        dc: Gdi::HDC,

        /// Send back the device context.
        callback: mpsc::SyncSender<Gdi::HDC>,
    },

    /// Deallocate a device context.
    Deallocate(Gdi::HDC),

    /// Release a window-associated device context.
    Release {
        /// The device context to release.
        dc: Gdi::HDC,

        /// The window that owns this device context.
        owner: HWND,
    },
}

unsafe impl Send for Command {}

impl Command {
    /// Handle this command.
    ///
    /// This should be called on the allocator thread.
    fn handle(self) {
        match self {
            Self::GetDc { window, callback } => {
                // Get the DC and send it back.
                let dc = unsafe { Gdi::GetDC(window) };
                callback.send(dc).ok();
            }

            Self::Allocate { dc, callback } => {
                // Allocate a DC and send it back.
                let dc = unsafe { Gdi::CreateCompatibleDC(dc) };
                callback.send(dc).ok();
            }

            Self::Deallocate(dc) => {
                // Deallocate this DC.
                unsafe {
                    Gdi::DeleteDC(dc);
                }
            }

            Self::Release { dc, owner } => {
                // Release this DC.
                unsafe {
                    Gdi::ReleaseDC(owner, dc);
                }
            }
        }
    }
}

#[derive(Debug)]
struct OnlyUsedFromOrigin<T>(T);
unsafe impl<T> Send for OnlyUsedFromOrigin<T> {}
unsafe impl<T> Sync for OnlyUsedFromOrigin<T> {}

impl<T> From<T> for OnlyUsedFromOrigin<T> {
    fn from(t: T) -> Self {
        Self(t)
    }
}
