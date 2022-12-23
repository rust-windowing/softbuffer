#![doc = include_str!("../README.md")]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;
extern crate core;

#[cfg(target_os = "macos")]
mod cg;
#[cfg(target_os = "redox")]
mod orbital;
#[cfg(all(feature = "wayland", any(target_os = "linux", target_os = "freebsd")))]
mod wayland;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "windows")]
mod win32;
#[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
mod x11;

mod error;

pub use error::SwBufError;

use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform.
pub struct GraphicsContext {
    /// The inner static dispatch object.
    ///
    /// This is boxed so that `GraphicsContext` is the same size on every platform, which should
    /// hopefully prevent surprises.
    graphics_context_impl: Box<Dispatch>,
}

/// A macro for creating the enum used to statically dispatch to the platform-specific implementation.
macro_rules! make_dispatch {
    (
        $(
            $(#[$attr:meta])*
            $name: ident ($inner_ty: ty),
        )*
    ) => {
        enum Dispatch {
            $(
                $(#[$attr])*
                $name($inner_ty),
            )*
        }

        impl Dispatch {
            unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => unsafe { inner.set_buffer(buffer, width, height) },
                    )*
                }
            }
        }
    };
}

make_dispatch! {
    #[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
    X11(x11::X11Impl),
    #[cfg(all(feature = "wayland", any(target_os = "linux", target_os = "freebsd")))]
    Wayland(wayland::WaylandImpl),
    #[cfg(target_os = "windows")]
    Win32(win32::Win32Impl),
    #[cfg(target_os = "macos")]
    CG(cg::CGImpl),
    #[cfg(target_arch = "wasm32")]
    Web(web::WebImpl),
    #[cfg(target_os = "redox")]
    Orbital(orbital::OrbitalImpl),
}

impl GraphicsContext {
    /// Creates a new instance of this struct, using the provided window and display.
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided objects are valid to draw a 2D buffer to, and are valid for the
    ///    lifetime of the GraphicsContext
    pub unsafe fn new<W: HasRawWindowHandle, D: HasRawDisplayHandle>(
        window: &W,
        display: &D,
    ) -> Result<Self, SwBufError> {
        unsafe { Self::from_raw(window.raw_window_handle(), display.raw_display_handle()) }
    }

    /// Creates a new instance of this struct, using the provided raw window and display handles
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided handles are valid to draw a 2D buffer to, and are valid for the
    ///    lifetime of the GraphicsContext
    pub unsafe fn from_raw(
        raw_window_handle: RawWindowHandle,
        raw_display_handle: RawDisplayHandle,
    ) -> Result<Self, SwBufError> {
        let imple: Dispatch = match (raw_window_handle, raw_display_handle) {
            #[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
            (
                RawWindowHandle::Xlib(xlib_window_handle),
                RawDisplayHandle::Xlib(xlib_display_handle),
            ) => Dispatch::X11(unsafe {
                x11::X11Impl::from_xlib(xlib_window_handle, xlib_display_handle)?
            }),
            #[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
            (
                RawWindowHandle::Xcb(xcb_window_handle),
                RawDisplayHandle::Xcb(xcb_display_handle),
            ) => Dispatch::X11(unsafe {
                x11::X11Impl::from_xcb(xcb_window_handle, xcb_display_handle)?
            }),
            #[cfg(all(feature = "wayland", any(target_os = "linux", target_os = "freebsd")))]
            (
                RawWindowHandle::Wayland(wayland_window_handle),
                RawDisplayHandle::Wayland(wayland_display_handle),
            ) => Dispatch::Wayland(unsafe {
                wayland::WaylandImpl::new(wayland_window_handle, wayland_display_handle)?
            }),
            #[cfg(target_os = "windows")]
            (RawWindowHandle::Win32(win32_handle), _) => {
                Dispatch::Win32(unsafe { win32::Win32Impl::new(&win32_handle)? })
            }
            #[cfg(target_os = "macos")]
            (RawWindowHandle::AppKit(appkit_handle), _) => {
                Dispatch::CG(unsafe { cg::CGImpl::new(appkit_handle)? })
            }
            #[cfg(target_arch = "wasm32")]
            (RawWindowHandle::Web(web_handle), _) => Dispatch::Web(web::WebImpl::new(web_handle)?),
            #[cfg(target_os = "redox")]
            (RawWindowHandle::Orbital(orbital_handle), _) => {
                Dispatch::Orbital(orbital::OrbitalImpl::new(orbital_handle)?)
            }
            (unimplemented_window_handle, unimplemented_display_handle) => {
                return Err(SwBufError::UnsupportedPlatform {
                    human_readable_window_platform_name: window_handle_type_name(
                        &unimplemented_window_handle,
                    ),
                    human_readable_display_platform_name: display_handle_type_name(
                        &unimplemented_display_handle,
                    ),
                    window_handle: unimplemented_window_handle,
                    display_handle: unimplemented_display_handle,
                })
            }
        };

        Ok(Self {
            graphics_context_impl: Box::new(imple),
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
