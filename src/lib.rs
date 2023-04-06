#![doc = include_str!("../README.md")]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;
extern crate core;

#[cfg(target_os = "macos")]
mod cg;
#[cfg(target_os = "redox")]
mod orbital;
#[cfg(wayland_platform)]
mod wayland;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "windows")]
mod win32;
#[cfg(x11_platform)]
mod x11;

mod error;
mod util;

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops;
#[cfg(any(wayland_platform, x11_platform))]
use std::rc::Rc;

pub use error::SoftBufferError;

use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform.
pub struct Context {
    /// The inner static dispatch object.
    context_impl: ContextDispatch,
    _marker: PhantomData<*mut ()>,
}

/// A macro for creating the enum used to statically dispatch to the platform-specific implementation.
macro_rules! make_dispatch {
    (
        $(
            $(#[$attr:meta])*
            $name: ident ($context_inner: ty, $surface_inner: ty, $buffer_inner: ty),
        )*
    ) => {
        enum ContextDispatch {
            $(
                $(#[$attr])*
                $name($context_inner),
            )*
        }

        impl ContextDispatch {
            fn variant_name(&self) -> &'static str {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(_) => stringify!($name),
                    )*
                }
            }
        }

        enum SurfaceDispatch {
            $(
                $(#[$attr])*
                $name($surface_inner),
            )*
        }

        impl SurfaceDispatch {
            pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.resize(width, height),
                    )*
                }
            }

            pub fn buffer_mut(&mut self) -> Result<BufferDispatch, SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => Ok(BufferDispatch::$name(inner.buffer_mut()?)),
                    )*
                }
            }
        }

        enum BufferDispatch<'a> {
            $(
                $(#[$attr])*
                $name($buffer_inner),
            )*
        }

        impl<'a> BufferDispatch<'a> {
            #[inline]
            pub fn pixels(&self) -> &[u32] {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.pixels(),
                    )*
                }
            }

            #[inline]
            pub fn pixels_mut(&mut self) -> &mut [u32] {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.pixels_mut(),
                    )*
                }
            }

            pub fn present(self) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.present(),
                    )*
                }
            }
        }
    };
}

// XXX empty enum with generic bound is invalid?

make_dispatch! {
    #[cfg(x11_platform)]
    X11(Rc<x11::X11DisplayImpl>, x11::X11Impl, x11::BufferImpl<'a>),
    #[cfg(wayland_platform)]
    Wayland(Rc<wayland::WaylandDisplayImpl>, wayland::WaylandImpl, wayland::BufferImpl<'a>),
    #[cfg(target_os = "windows")]
    Win32((), win32::Win32Impl, win32::BufferImpl<'a>),
    #[cfg(target_os = "macos")]
    CG((), cg::CGImpl, cg::BufferImpl<'a>),
    #[cfg(target_arch = "wasm32")]
    Web(web::WebDisplayImpl, web::WebImpl, web::BufferImpl<'a>),
    #[cfg(target_os = "redox")]
    Orbital((), orbital::OrbitalImpl, orbital::BufferImpl<'a>),
}

impl Context {
    /// Creates a new instance of this struct, using the provided display.
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided object is valid for the lifetime of the Context
    pub unsafe fn new<D: HasRawDisplayHandle>(display: &D) -> Result<Self, SoftBufferError> {
        unsafe { Self::from_raw(display.raw_display_handle()) }
    }

    /// Creates a new instance of this struct, using the provided display handles
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided handle is valid for the lifetime of the Context
    pub unsafe fn from_raw(raw_display_handle: RawDisplayHandle) -> Result<Self, SoftBufferError> {
        let imple: ContextDispatch = match raw_display_handle {
            #[cfg(x11_platform)]
            RawDisplayHandle::Xlib(xlib_handle) => unsafe {
                ContextDispatch::X11(Rc::new(x11::X11DisplayImpl::from_xlib(xlib_handle)?))
            },
            #[cfg(x11_platform)]
            RawDisplayHandle::Xcb(xcb_handle) => unsafe {
                ContextDispatch::X11(Rc::new(x11::X11DisplayImpl::from_xcb(xcb_handle)?))
            },
            #[cfg(wayland_platform)]
            RawDisplayHandle::Wayland(wayland_handle) => unsafe {
                ContextDispatch::Wayland(Rc::new(wayland::WaylandDisplayImpl::new(wayland_handle)?))
            },
            #[cfg(target_os = "windows")]
            RawDisplayHandle::Windows(_) => ContextDispatch::Win32(()),
            #[cfg(target_os = "macos")]
            RawDisplayHandle::AppKit(_) => ContextDispatch::CG(()),
            #[cfg(target_arch = "wasm32")]
            RawDisplayHandle::Web(_) => ContextDispatch::Web(web::WebDisplayImpl::new()?),
            #[cfg(target_os = "redox")]
            RawDisplayHandle::Orbital(_) => ContextDispatch::Orbital(()),
            unimplemented_display_handle => {
                return Err(SoftBufferError::UnsupportedDisplayPlatform {
                    human_readable_display_platform_name: display_handle_type_name(
                        &unimplemented_display_handle,
                    ),
                    display_handle: unimplemented_display_handle,
                })
            }
        };

        Ok(Self {
            context_impl: imple,
            _marker: PhantomData,
        })
    }
}

/// A surface for drawing to a window with software buffers.
pub struct Surface {
    /// This is boxed so that `Surface` is the same size on every platform.
    surface_impl: Box<SurfaceDispatch>,
    _marker: PhantomData<*mut ()>,
}

impl Surface {
    /// Creates a new surface for the context for the provided window.
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided objects are valid to draw a 2D buffer to, and are valid for the
    ///    lifetime of the Context
    pub unsafe fn new<W: HasRawWindowHandle>(
        context: &Context,
        window: &W,
    ) -> Result<Self, SoftBufferError> {
        unsafe { Self::from_raw(context, window.raw_window_handle()) }
    }

    /// Creates a new surface for the context for the provided raw window handle.
    ///
    /// # Safety
    ///
    ///  - Ensure that the provided handles are valid to draw a 2D buffer to, and are valid for the
    ///    lifetime of the Context
    pub unsafe fn from_raw(
        context: &Context,
        raw_window_handle: RawWindowHandle,
    ) -> Result<Self, SoftBufferError> {
        let imple: SurfaceDispatch = match (&context.context_impl, raw_window_handle) {
            #[cfg(x11_platform)]
            (
                ContextDispatch::X11(xcb_display_handle),
                RawWindowHandle::Xlib(xlib_window_handle),
            ) => SurfaceDispatch::X11(unsafe {
                x11::X11Impl::from_xlib(xlib_window_handle, xcb_display_handle.clone())?
            }),
            #[cfg(x11_platform)]
            (ContextDispatch::X11(xcb_display_handle), RawWindowHandle::Xcb(xcb_window_handle)) => {
                SurfaceDispatch::X11(unsafe {
                    x11::X11Impl::from_xcb(xcb_window_handle, xcb_display_handle.clone())?
                })
            }
            #[cfg(wayland_platform)]
            (
                ContextDispatch::Wayland(wayland_display_impl),
                RawWindowHandle::Wayland(wayland_window_handle),
            ) => SurfaceDispatch::Wayland(unsafe {
                wayland::WaylandImpl::new(wayland_window_handle, wayland_display_impl.clone())?
            }),
            #[cfg(target_os = "windows")]
            (ContextDispatch::Win32(()), RawWindowHandle::Win32(win32_handle)) => {
                SurfaceDispatch::Win32(unsafe { win32::Win32Impl::new(&win32_handle)? })
            }
            #[cfg(target_os = "macos")]
            (ContextDispatch::CG(()), RawWindowHandle::AppKit(appkit_handle)) => {
                SurfaceDispatch::CG(unsafe { cg::CGImpl::new(appkit_handle)? })
            }
            #[cfg(target_arch = "wasm32")]
            (ContextDispatch::Web(context), RawWindowHandle::Web(web_handle)) => {
                SurfaceDispatch::Web(web::WebImpl::new(context, web_handle)?)
            }
            #[cfg(target_os = "redox")]
            (ContextDispatch::Orbital(()), RawWindowHandle::Orbital(orbital_handle)) => {
                SurfaceDispatch::Orbital(orbital::OrbitalImpl::new(orbital_handle)?)
            }
            (unsupported_display_impl, unimplemented_window_handle) => {
                return Err(SoftBufferError::UnsupportedWindowPlatform {
                    human_readable_window_platform_name: window_handle_type_name(
                        &unimplemented_window_handle,
                    ),
                    human_readable_display_platform_name: unsupported_display_impl.variant_name(),
                    window_handle: unimplemented_window_handle,
                })
            }
        };

        Ok(Self {
            surface_impl: Box::new(imple),
            _marker: PhantomData,
        })
    }

    /// Set the size of the buffer that will be returned by [`Surface::buffer_mut`].
    ///
    /// If the size of the buffer does not match the size of the window, the buffer is drawn
    /// in the upper-left corner of the window. It is recommended in most production use cases
    /// to have the buffer fill the entire window. Use your windowing library to find the size
    /// of the window.
    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        self.surface_impl.resize(width, height)
    }

    /// Return a [`Buffer`] that the next frame should be rendered into. The size must
    /// be set with [`Surface::resize`] first. The initial contents of the buffer may be zeroed, or
    /// may contain a previous frame.
    pub fn buffer_mut(&mut self) -> Result<Buffer, SoftBufferError> {
        Ok(Buffer {
            buffer_impl: self.surface_impl.buffer_mut()?,
            _marker: PhantomData,
        })
    }
}

/// A buffer that can be written to by the CPU and presented to the window.
///
/// This derefs to a `[u32]`, which depending on the backend may be a mapping into shared memory
/// accessible to the display server, so presentation doesn't require any (client-side) copying.
///
/// This trusts the display server not to mutate the buffer, which could otherwise be unsound.
///
/// # Data representation
///
/// The format of the buffer is as follows. There is one `u32` in the buffer for each pixel in
/// the area to draw. The first entry is the upper-left most pixel. The second is one to the right
/// etc. (Row-major top to bottom left to right one `u32` per pixel). Within each `u32` the highest
/// order 8 bits are to be set to 0. The next highest order 8 bits are the red channel, then the
/// green channel, and then the blue channel in the lowest-order 8 bits. See the examples for
/// one way to build this format using bitwise operations.
///
/// --------
///
/// Pixel format (`u32`):
///
/// 00000000RRRRRRRRGGGGGGGGBBBBBBBB
///
/// 0: Bit is 0
/// R: Red channel
/// G: Green channel
/// B: Blue channel
///
/// # Platform dependent behavior
/// No-copy presentation is currently supported on:
/// - Wayland
/// - X, when XShm is available
/// - Win32
/// - Orbital, when buffer size matches window size
/// Currently [`Buffer::present`] must block copying image data on:
/// - Web
/// - macOS
pub struct Buffer<'a> {
    buffer_impl: BufferDispatch<'a>,
    _marker: PhantomData<*mut ()>,
}

impl<'a> Buffer<'a> {
    /// Presents buffer to the window.
    ///
    /// # Platform dependent behavior
    ///
    /// ## Wayland
    ///
    /// On Wayland, calling this function may send requests to the underlying `wl_surface`. The
    /// graphics context may issue `wl_surface.attach`, `wl_surface.damage`, `wl_surface.damage_buffer`
    /// and `wl_surface.commit` requests when presenting the buffer.
    ///
    /// If the caller wishes to synchronize other surface/window changes, such requests must be sent to the
    /// Wayland compositor before calling this function.
    pub fn present(self) -> Result<(), SoftBufferError> {
        self.buffer_impl.present()
    }
}

impl<'a> ops::Deref for Buffer<'a> {
    type Target = [u32];

    #[inline]
    fn deref(&self) -> &[u32] {
        self.buffer_impl.pixels()
    }
}

impl<'a> ops::DerefMut for Buffer<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u32] {
        self.buffer_impl.pixels_mut()
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
