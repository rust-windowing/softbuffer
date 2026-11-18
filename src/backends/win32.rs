//! Implementation of software buffering for Windows.
//!
//! Frames are presented through a DXGI swap chain. Where that isn't available, the buffer is a
//! bitmap that GDI blits to the window.

mod dxgi;

use crate::backend_interface::*;
use crate::{util, AlphaMode, Pixel, Rect, SoftBufferError};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::{NonZeroI32, NonZeroU32};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi;
use windows_sys::Win32::UI::ColorSystem::LCS_WINDOWS_COLOR_SPACE;

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
    pixels: NonNull<Pixel>,
    width: NonZeroI32,
    height: NonZeroI32,
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
    fn new(window_dc: Gdi::HDC, width: NonZeroI32, height: NonZeroI32, alpha: bool) -> Self {
        let dc = Allocator::get().allocate(window_dc);
        assert!(!dc.is_null());

        // Create a new bitmap info struct.
        let bitmap_info = BitmapInfo {
            bmi_header: Gdi::BITMAPV5HEADER {
                bV5Size: size_of::<Gdi::BITMAPV5HEADER>() as u32,
                bV5Width: width.get(),
                bV5Height: -height.get(),
                bV5Planes: 1,
                bV5BitCount: 32,
                bV5Compression: Gdi::BI_BITFIELDS,
                bV5SizeImage: 0,
                bV5XPelsPerMeter: 0,
                bV5YPelsPerMeter: 0,
                bV5ClrUsed: 0,
                bV5ClrImportant: 0,
                bV5RedMask: 0x00ff0000,
                bV5GreenMask: 0x0000ff00,
                bV5BlueMask: 0x000000ff,
                bV5AlphaMask: if alpha { 0xff000000 } else { 0x00000000 },
                bV5CSType: LCS_WINDOWS_COLOR_SPACE as u32,
                bV5Endpoints: Gdi::CIEXYZTRIPLE {
                    ciexyzRed: Gdi::CIEXYZ {
                        ciexyzX: 0,
                        ciexyzY: 0,
                        ciexyzZ: 0,
                    },
                    ciexyzGreen: Gdi::CIEXYZ {
                        ciexyzX: 0,
                        ciexyzY: 0,
                        ciexyzZ: 0,
                    },
                    ciexyzBlue: Gdi::CIEXYZ {
                        ciexyzX: 0,
                        ciexyzY: 0,
                        ciexyzZ: 0,
                    },
                },
                bV5GammaRed: 0,
                bV5GammaGreen: 0,
                bV5GammaBlue: 0,
                bV5Intent: 0,
                bV5ProfileData: 0,
                bV5ProfileSize: 0,
                bV5Reserved: 0,
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
        let mut pixels: *mut Pixel = ptr::null_mut();
        let bitmap = unsafe {
            Gdi::CreateDIBSection(
                dc,
                &bitmap_info as *const BitmapInfo as *const _,
                Gdi::DIB_RGB_COLORS,
                &mut pixels as *mut *mut Pixel as _,
                ptr::null_mut(),
                0,
            )
        };
        assert!(!bitmap.is_null());
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
            presented: false,
        }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        let num_bytes =
            byte_stride(self.width.get() as u32, 32) as usize * self.height.get() as usize;
        unsafe { slice::from_raw_parts_mut(self.pixels.as_ptr(), num_bytes / size_of::<Pixel>()) }
    }
}

/// The handle to a window for software buffering.
#[derive(Debug)]
pub struct Win32Impl<D: ?Sized, W> {
    /// The window handle.
    window: OnlyUsedFromOrigin<HWND>,

    /// What presents the frames.
    presenter: Presenter,

    /// The handle for the window.
    ///
    /// This should be kept alive in order to keep `window` valid.
    handle: W,

    /// The display handle.
    ///
    /// We don't use this, but other code might.
    _display: PhantomData<D>,
}

#[derive(Debug)]
enum Presenter {
    /// A DXGI flip model swap chain, which hands DWM whole frames.
    Dxgi(dxgi::Presenter),

    /// GDI, where DXGI isn't available.
    Gdi(GdiPresenter),
}

#[derive(Debug)]
struct GdiPresenter {
    /// The window that the device context belongs to.
    window: OnlyUsedFromOrigin<HWND>,

    /// The device context for the window.
    dc: OnlyUsedFromOrigin<Gdi::HDC>,

    /// The buffer used to hold the image.
    buffer: Option<Buffer>,
}

impl GdiPresenter {
    fn new(window: HWND) -> Result<Self, SoftBufferError> {
        // Get the handle to the device context.
        // SAFETY: We have confirmed that the window handle is valid.
        let dc = Allocator::get().get_dc(window);

        // GetDC returns null if there is a platform error.
        if dc.is_null() {
            return Err(SoftBufferError::PlatformError(
                Some("Device Context is null".into()),
                Some(Box::new(io::Error::last_os_error())),
            ));
        }

        Ok(Self {
            window: window.into(),
            dc: dc.into(),
            buffer: None,
        })
    }
}

impl Drop for GdiPresenter {
    fn drop(&mut self) {
        // Release our resources.
        Allocator::get().release(self.window.0, self.dc.0);
    }
}

/// The Win32-compatible bitmap information.
#[repr(C)]
struct BitmapInfo {
    bmi_header: Gdi::BITMAPV5HEADER,
    bmi_colors: [Gdi::RGBQUAD; 3],
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for Win32Impl<D, W> {
    type Context = D;
    type Buffer<'surface>
        = BufferImpl<'surface>
    where
        Self: 'surface;

    /// Create a new `Win32Impl` from a `Win32WindowHandle`.
    fn new(window: W, _display: &D) -> Result<Self, crate::error::InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::Win32(handle) = raw else {
            return Err(crate::InitError::Unsupported(window));
        };

        let hwnd = handle.hwnd.get() as HWND;
        let presenter = match dxgi::Presenter::new(hwnd) {
            Ok(presenter) => Presenter::Dxgi(presenter),
            Err(error) => {
                tracing::warn!("DXGI is not available, presenting with GDI: {error}");
                Presenter::Gdi(GdiPresenter::new(hwnd)?)
            }
        };

        Ok(Self {
            window: hwnd.into(),
            presenter,
            handle: window,
            _display: PhantomData,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.handle
    }

    #[inline]
    fn supports_alpha_mode(&self, alpha_mode: AlphaMode) -> bool {
        // TODO: Support transparency.
        matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Ignored)
    }

    fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        alpha_mode: AlphaMode,
    ) -> Result<(), SoftBufferError> {
        let (width, height) = (|| {
            let width = NonZeroI32::try_from(width).ok()?;
            let height = NonZeroI32::try_from(height).ok()?;
            Some((width, height))
        })()
        .ok_or(SoftBufferError::SizeOutOfRange { width, height })?;

        if let Presenter::Dxgi(presenter) = &mut self.presenter {
            match presenter.configure(width.try_into().unwrap(), height.try_into().unwrap()) {
                Ok(()) => return Ok(()),
                // GDI no longer draws into a window that a flip model swap chain presented to.
                Err(error) if presenter.presented() => return Err(error),
                Err(error) => {
                    tracing::warn!("DXGI failed, presenting with GDI: {error}");
                    self.presenter = Presenter::Gdi(GdiPresenter::new(self.window.0)?);
                }
            }
        }
        let Presenter::Gdi(gdi) = &mut self.presenter else {
            unreachable!("the presenter is GDI")
        };

        if let Some(buffer) = gdi.buffer.as_ref() {
            if buffer.width == width && buffer.height == height {
                return Ok(());
            }
        }

        let alpha = match alpha_mode {
            AlphaMode::Opaque | AlphaMode::Ignored => false,
            AlphaMode::Premultiplied => true,
            AlphaMode::Postmultiplied => unreachable!("unsupported alpha format"),
        };
        gdi.buffer = Some(Buffer::new(gdi.dc.0, width, height, alpha));

        Ok(())
    }

    fn next_buffer(&mut self, _alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        let target = match &mut self.presenter {
            Presenter::Dxgi(presenter) => {
                presenter.map()?;
                Target::Dxgi(presenter)
            }
            Presenter::Gdi(gdi) => Target::Gdi {
                dc: &gdi.dc,
                buffer: gdi
                    .buffer
                    .as_mut()
                    .expect("Must set size of surface before calling `next_buffer()`"),
            },
        };

        Ok(BufferImpl {
            window: &self.window,
            target,
        })
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<Pixel>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

#[derive(Debug)]
pub struct BufferImpl<'surface> {
    window: &'surface OnlyUsedFromOrigin<HWND>,
    target: Target<'surface>,
}

#[derive(Debug)]
enum Target<'surface> {
    Dxgi(&'surface mut dxgi::Presenter),
    Gdi {
        dc: &'surface OnlyUsedFromOrigin<Gdi::HDC>,
        buffer: &'surface mut Buffer,
    },
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        match &self.target {
            Target::Dxgi(presenter) => presenter.byte_stride(),
            Target::Gdi { buffer, .. } => {
                let width = buffer.width.get() as u32;
                NonZeroU32::new(byte_stride(width, 32)).unwrap()
            }
        }
    }

    fn width(&self) -> NonZeroU32 {
        match &self.target {
            Target::Dxgi(presenter) => presenter.width(),
            Target::Gdi { buffer, .. } => buffer.width.try_into().unwrap(),
        }
    }

    fn height(&self) -> NonZeroU32 {
        match &self.target {
            Target::Dxgi(presenter) => presenter.height(),
            Target::Gdi { buffer, .. } => buffer.height.try_into().unwrap(),
        }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        match &mut self.target {
            Target::Dxgi(presenter) => presenter.pixels_mut(),
            Target::Gdi { buffer, .. } => buffer.pixels_mut(),
        }
    }

    fn age(&self) -> u8 {
        match &self.target {
            Target::Dxgi(presenter) => presenter.age(),
            Target::Gdi { buffer, .. } => {
                if buffer.presented {
                    1
                } else {
                    0
                }
            }
        }
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        match self.target {
            Target::Dxgi(presenter) => presenter.present(damage)?,
            Target::Gdi { dc, buffer } => {
                for rect in damage {
                    let rect = util::clamp_rect(
                        *rect,
                        buffer.width.try_into().unwrap(),
                        buffer.height.try_into().unwrap(),
                    );
                    let x = util::to_i32_saturating(rect.x);
                    let y = util::to_i32_saturating(rect.y);
                    let width = util::to_i32_saturating(rect.width.get());
                    let height = util::to_i32_saturating(rect.height.get());

                    // TODO: Draw with something else to make transparency work.
                    unsafe {
                        Gdi::BitBlt(dc.0, x, y, width, height, buffer.dc, x, y, Gdi::SRCCOPY)
                    };
                }
                buffer.presented = true;
            }
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

#[cfg(test)]
mod tests;

#[derive(Debug)]
struct OnlyUsedFromOrigin<T>(T);
unsafe impl<T> Send for OnlyUsedFromOrigin<T> {}
unsafe impl<T> Sync for OnlyUsedFromOrigin<T> {}

impl<T> From<T> for OnlyUsedFromOrigin<T> {
    fn from(t: T) -> Self {
        Self(t)
    }
}

#[inline]
fn byte_stride(width: u32, bit_count: u32) -> u32 {
    // <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader#calculating-surface-stride>
    // When `bit_count == 32`, this is always just equal to `width * 4`.
    ((width * bit_count + 31) & !31) >> 3
}
