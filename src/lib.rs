#![doc = include_str!("../README.md")]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;
extern crate core;

#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "macos")]
mod cg;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod x11;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod wayland;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "redox")]
mod orbital;

mod error;

pub use error::SwBufError;

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle};

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform. This struct owns the window that this data corresponds to
/// to ensure safety, as that data must be destroyed before the window itself is destroyed. You may
/// access the underlying window via [`window`](Self::window) and [`window_mut`](Self::window_mut).
pub struct GraphicsContext {
    graphics_context_impl: Box<dyn GraphicsContextImpl>,
}

impl GraphicsContext {
    /// Creates a new instance of this struct, using the provided window.
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided window is valid to draw a 2D buffer to, and is valid for the
    ///    lifetime of the GraphicsContext
    pub unsafe fn new<W: HasRawWindowHandle + HasRawDisplayHandle>(window: &W) -> Result<Self, SwBufError> {
        Self::from_raw(window.raw_window_handle(), window.raw_display_handle())
    }

    /// Creates a new instance of this struct, using the provided raw handles
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided handles are valid to draw a 2D buffer to, and are valid for the
    ///    lifetime of the GraphicsContext
    pub unsafe fn from_raw(raw_window_handle: RawWindowHandle, raw_display_handle: RawDisplayHandle) -> Result<Self, SwBufError> {
        let imple: Box<dyn GraphicsContextImpl> = match (raw_window_handle, raw_display_handle) {
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            (RawWindowHandle::Xlib(xlib_window_handle), RawDisplayHandle::Xlib(xlib_display_handle)) => Box::new(x11::X11Impl::new(xlib_window_handle, xlib_display_handle)?),
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            (RawWindowHandle::Wayland(wayland_window_handle), RawDisplayHandle::Wayland(wayland_display_handle)) => Box::new(wayland::WaylandImpl::new(wayland_window_handle, wayland_display_handle)?),
            #[cfg(target_os = "windows")]
            (RawWindowHandle::Win32(win32_handle), _) => Box::new(win32::Win32Impl::new(&win32_handle)?),
            #[cfg(target_os = "macos")]
            (RawWindowHandle::AppKit(appkit_handle), _) => Box::new(cg::CGImpl::new(appkit_handle)?),
            #[cfg(target_arch = "wasm32")]
            (RawWindowHandle::Web(web_handle), _) => Box::new(web::WebImpl::new(web_handle)?),
            #[cfg(target_os = "redox")]
            (RawWindowHandle::Orbital(orbital_handle), _) => Box::new(orbital::OrbitalImpl::new(orbital_handle)?),
            (unimplemented_window_handle, unimplemented_display_handle) => return Err(SwBufError::UnsupportedPlatform {
                human_readable_window_platform_name: window_handle_type_name(&unimplemented_window_handle),
                human_readable_display_platform_name: display_handle_type_name(&unimplemented_display_handle),
                window_handle: unimplemented_window_handle,
                display_handle: unimplemented_display_handle
            }),
        };

        Ok(Self {
            graphics_context_impl: imple,
        })
    }

    /// Shows the given buffer with the given width and height on the window corresponding to this
    /// graphics context. Panics if buffer.len() ≠ width*height. If the size of the buffer does
    /// not match the size of the window, the buffer is drawn in the upper-left corner of the window.
    /// It is recommended in most production use cases to have the buffer fill the entire window.
    /// Use your windowing library to find the size of the window.
    ///
    /// The format of the buffer is as follows. There is one u32 in the buffer for each pixel in
    /// the area to draw. The first entry is the upper-left most pixel. The second is one to the right
    /// etc. (Row-major top to bottom left to right one u32 per pixel). Within each u32 the highest
    /// order 8 bits are to be set to 0. The next highest order 8 bits are the red channel, then the
    /// green channel, and then the blue channel in the lowest-order 8 bits. See the examples for
    /// one way to build this format using bitwise operations.
    ///
    /// --------
    ///
    /// Pixel format (u32):
    ///
    /// 00000000RRRRRRRRGGGGGGGGBBBBBBBB
    ///
    /// 0: Bit is 0
    /// R: Red channel
    /// G: Green channel
    /// B: Blue channel
    #[inline]
    pub fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        if (width as usize) * (height as usize) != buffer.len() {
            panic!("The size of the passed buffer is not the correct size. Its length must be exactly width*height.");
        }

        unsafe {
            self.graphics_context_impl.set_buffer(buffer, width, height);
        }
    }
}

trait GraphicsContextImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16);
}

fn window_handle_type_name(handle: &RawWindowHandle) -> &'static str {
    match handle {
        RawWindowHandle::Xlib(_) => "Xlib",
        RawWindowHandle::Win32(_) => "Win32",
        RawWindowHandle::WinRt(_) => "WinRt",
        RawWindowHandle::Web(_) => "Web",
        RawWindowHandle::Wayland(_) => "Wayland",
        RawWindowHandle::AndroidNdk(_) => "AndroidNdk",
        RawWindowHandle::AppKit(_) => "AppKit",
        RawWindowHandle::Orbital(_) => "Orbital",
        RawWindowHandle::UiKit(_) => "UiKit",
        RawWindowHandle::Xcb(_) => "XCB",
        RawWindowHandle::Drm(_) => "DRM",
        RawWindowHandle::Gbm(_) => "GBM",
        RawWindowHandle::Haiku(_) => "Haiku",
        _ => "Unknown Name", //don't completely fail to compile if there is a new raw window handle type that's added at some point
    }
}

fn display_handle_type_name(handle: &RawDisplayHandle) -> &'static str {
    match handle {
        RawDisplayHandle::Xlib(_) => "Xlib",
        RawDisplayHandle::Web(_) => "Web",
        RawDisplayHandle::Wayland(_) => "Wayland",
        RawDisplayHandle::AppKit(_) => "AppKit",
        RawDisplayHandle::Orbital(_) => "Orbital",
        RawDisplayHandle::UiKit(_) => "UiKit",
        RawDisplayHandle::Xcb(_) => "XCB",
        RawDisplayHandle::Drm(_) => "DRM",
        RawDisplayHandle::Gbm(_) => "GBM",
        RawDisplayHandle::Haiku(_) => "Haiku",
        RawDisplayHandle::Windows(_) => "Windows",
        RawDisplayHandle::Android(_) => "Android",
        _ => "Unknown Name", //don't completely fail to compile if there is a new raw window handle type that's added at some point
    }
}
