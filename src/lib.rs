#![doc = include_str!("../README.md")]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;
extern crate core;

#[cfg(target_os = "macos")]
mod cg;
#[cfg(kms_platform)]
mod kms;
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
#[cfg(any(wayland_platform, x11_platform, kms_platform))]
use std::rc::Rc;

use error::InitError;
pub use error::SoftBufferError;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

#[cfg(target_arch = "wasm32")]
pub use self::web::SurfaceExtWeb;

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform.
pub struct Context<D> {
    _marker: PhantomData<*mut ()>,

    /// The inner static dispatch object.
    context_impl: ContextDispatch<D>,
}

/// A macro for creating the enum used to statically dispatch to the platform-specific implementation.
macro_rules! make_dispatch {
    (
        <$dgen: ident, $wgen: ident> =>
        $(
            $(#[$attr:meta])*
            $name: ident
            ($context_inner: ty, $surface_inner: ty, $buffer_inner: ty),
        )*
    ) => {
        enum ContextDispatch<$dgen> {
            $(
                $(#[$attr])*
                $name($context_inner),
            )*
        }

        impl<D: HasDisplayHandle> ContextDispatch<D> {
            fn variant_name(&self) -> &'static str {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(_) => stringify!($name),
                    )*
                }
            }
        }

        #[allow(clippy::large_enum_variant)] // it's boxed anyways
        enum SurfaceDispatch<$dgen, $wgen> {
            $(
                $(#[$attr])*
                $name($surface_inner),
            )*
        }

        impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceDispatch<D, W> {
            pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.resize(width, height),
                    )*
                }
            }

            pub fn buffer_mut(&mut self) -> Result<BufferDispatch<'_, D, W>, SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => Ok(BufferDispatch::$name(inner.buffer_mut()?)),
                    )*
                }
            }

            pub fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.fetch(),
                    )*
                }
            }
        }

        enum BufferDispatch<'a, $dgen, $wgen> {
            $(
                $(#[$attr])*
                $name($buffer_inner),
            )*
        }

        impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferDispatch<'a, D, W> {
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

            pub fn age(&self) -> u8 {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.age(),
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

            pub fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.present_with_damage(damage),
                    )*
                }
            }
        }
    };
}

// XXX empty enum with generic bound is invalid?

make_dispatch! {
    <D, W> =>
    #[cfg(x11_platform)]
    X11(Rc<x11::X11DisplayImpl<D>>, x11::X11Impl<D, W>, x11::BufferImpl<'a, D, W>),
    #[cfg(wayland_platform)]
    Wayland(Rc<wayland::WaylandDisplayImpl<D>>, wayland::WaylandImpl<D, W>, wayland::BufferImpl<'a, D, W>),
    #[cfg(kms_platform)]
    Kms(Rc<kms::KmsDisplayImpl<D>>, kms::KmsImpl<D, W>, kms::BufferImpl<'a, D, W>),
    #[cfg(target_os = "windows")]
    Win32(D, win32::Win32Impl<D, W>, win32::BufferImpl<'a, D, W>),
    #[cfg(target_os = "macos")]
    CG(D, cg::CGImpl<D, W>, cg::BufferImpl<'a, D, W>),
    #[cfg(target_arch = "wasm32")]
    Web(web::WebDisplayImpl<D>, web::WebImpl<D, W>, web::BufferImpl<'a, D, W>),
    #[cfg(target_os = "redox")]
    Orbital(D, orbital::OrbitalImpl<D, W>, orbital::BufferImpl<'a, D, W>),
}

impl<D: HasDisplayHandle> Context<D> {
    /// Creates a new instance of this struct, using the provided display.
    pub fn new(mut dpy: D) -> Result<Self, SoftBufferError> {
        macro_rules! try_init {
            ($imp:ident, $x:ident => $make_it:expr) => {{
                let $x = dpy;
                match { $make_it } {
                    Ok(x) => {
                        return Ok(Self {
                            context_impl: ContextDispatch::$imp(x),
                            _marker: PhantomData,
                        })
                    }
                    Err(InitError::Unsupported(d)) => dpy = d,
                    Err(InitError::Failure(f)) => return Err(f),
                }
            }};
        }

        #[cfg(x11_platform)]
        try_init!(X11, display => x11::X11DisplayImpl::new(display).map(Rc::new));
        #[cfg(wayland_platform)]
        try_init!(Wayland, display => wayland::WaylandDisplayImpl::new(display).map(Rc::new));
        #[cfg(kms_platform)]
        try_init!(Kms, display => kms::KmsDisplayImpl::new(display).map(Rc::new));
        #[cfg(target_os = "windows")]
        try_init!(Win32, display => Ok(display));
        #[cfg(target_os = "macos")]
        try_init!(CG, display => Ok(display));
        #[cfg(target_arch = "wasm32")]
        try_init!(Web, display => web::WebDisplayImpl::new(display));
        #[cfg(target_os = "redox")]
        try_init!(Orbital, display => Ok(display));

        let raw = dpy.display_handle()?.as_raw();
        Err(SoftBufferError::UnsupportedDisplayPlatform {
            human_readable_display_platform_name: display_handle_type_name(&raw),
            display_handle: raw,
        })
    }
}

/// A rectangular region of the buffer coordinate space.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    /// x coordinate of top left corner
    pub x: u32,
    /// y coordinate of top left corner
    pub y: u32,
    /// width
    pub width: NonZeroU32,
    /// height
    pub height: NonZeroU32,
}

/// A surface for drawing to a window with software buffers.
pub struct Surface<D, W> {
    /// This is boxed so that `Surface` is the same size on every platform.
    surface_impl: Box<SurfaceDispatch<D, W>>,
    _marker: PhantomData<*mut ()>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Surface<D, W> {
    /// Creates a new surface for the context for the provided window.
    pub fn new(context: &Context<D>, window: W) -> Result<Self, SoftBufferError> {
        macro_rules! leap {
            ($e:expr) => {{
                match ($e) {
                    Ok(x) => x,
                    Err(InitError::Unsupported(window)) => {
                        let raw = window.window_handle()?.as_raw();
                        return Err(SoftBufferError::UnsupportedWindowPlatform {
                            human_readable_window_platform_name: window_handle_type_name(&raw),
                            human_readable_display_platform_name: context
                                .context_impl
                                .variant_name(),
                            window_handle: raw,
                        });
                    }
                    Err(InitError::Failure(f)) => return Err(f),
                }
            }};
        }

        let imple = match &context.context_impl {
            #[cfg(x11_platform)]
            ContextDispatch::X11(xcb_display_handle) => {
                SurfaceDispatch::X11(leap!(x11::X11Impl::new(window, xcb_display_handle.clone())))
            }
            #[cfg(wayland_platform)]
            ContextDispatch::Wayland(wayland_display_impl) => SurfaceDispatch::Wayland(leap!(
                wayland::WaylandImpl::new(window, wayland_display_impl.clone())
            )),
            #[cfg(kms_platform)]
            ContextDispatch::Kms(kms_display_impl) => {
                SurfaceDispatch::Kms(leap!(kms::KmsImpl::new(window, kms_display_impl.clone())))
            }
            #[cfg(target_os = "windows")]
            ContextDispatch::Win32(_) => {
                SurfaceDispatch::Win32(leap!(win32::Win32Impl::new(window)))
            }
            #[cfg(target_os = "macos")]
            ContextDispatch::CG(_) => SurfaceDispatch::CG(leap!(cg::CGImpl::new(window))),
            #[cfg(target_arch = "wasm32")]
            ContextDispatch::Web(web_display_impl) => {
                SurfaceDispatch::Web(leap!(web::WebImpl::new(web_display_impl, window)))
            }
            #[cfg(target_os = "redox")]
            ContextDispatch::Orbital(_) => {
                SurfaceDispatch::Orbital(leap!(orbital::OrbitalImpl::new(window)))
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

    /// Copies the window contents into a buffer.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On X11, the window must be visible.
    /// - On macOS, Redox and Wayland, this function is unimplemented.
    /// - On Web, this will fail if the content was supplied by
    ///   a different origin depending on the sites CORS rules.
    pub fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        self.surface_impl.fetch()
    }

    /// Return a [`Buffer`] that the next frame should be rendered into. The size must
    /// be set with [`Surface::resize`] first. The initial contents of the buffer may be zeroed, or
    /// may contain a previous frame. Call [`Buffer::age`] to determine this.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On DRM/KMS, there is no reliable and sound way to wait for the page flip to happen from within
    ///   `softbuffer`. Therefore it is the responsibility of the user to wait for the page flip before
    ///   sending another frame.
    pub fn buffer_mut(&mut self) -> Result<Buffer<'_, D, W>, SoftBufferError> {
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
///
/// Currently [`Buffer::present`] must block copying image data on:
/// - Web
/// - macOS
pub struct Buffer<'a, D, W> {
    buffer_impl: BufferDispatch<'a, D, W>,
    _marker: PhantomData<*mut ()>,
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> Buffer<'a, D, W> {
    /// Is age is the number of frames ago this buffer was last presented. So if the value is
    /// `1`, it is the same as the last frame, and if it is `2`, it is the same as the frame
    /// before that (for backends using double buffering). If the value is `0`, it is a new
    /// buffer that has unspecified contents.
    ///
    /// This can be used to update only a portion of the buffer.
    pub fn age(&self) -> u8 {
        self.buffer_impl.age()
    }

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

    /// Presents buffer to the window, with damage regions.
    ///
    /// # Platform dependent behavior
    ///
    /// Supported on:
    /// - Wayland
    /// - X, when XShm is available
    /// - Win32
    /// - Web
    ///
    /// Otherwise this is equivalent to [`Self::present`].
    pub fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.buffer_impl.present_with_damage(damage)
    }
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> ops::Deref for Buffer<'a, D, W> {
    type Target = [u32];

    #[inline]
    fn deref(&self) -> &[u32] {
        self.buffer_impl.pixels()
    }
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> ops::DerefMut for Buffer<'a, D, W> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u32] {
        self.buffer_impl.pixels_mut()
    }
}

/// There is no display handle.
#[derive(Debug)]
pub struct NoDisplayHandle(core::convert::Infallible);

impl HasDisplayHandle for NoDisplayHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        match self.0 {}
    }
}

/// There is no window handle.
#[derive(Debug)]
pub struct NoWindowHandle(());

impl HasWindowHandle for NoWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
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
