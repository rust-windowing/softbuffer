//! Implements `buffer_interface::*` traits for enums dispatching to backends

use crate::{backend_interface::*, backends, InitError, Rect, SoftBufferError, BufferReturn, WithAlpha, WithoutAlpha};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use duplicate::duplicate_item;
use std::num::NonZeroU32;
#[cfg(any(wayland_platform, x11_platform, kms_platform))]
use std::sync::Arc;

/// A macro for creating the enum used to statically dispatch to the platform-specific implementation.
macro_rules! make_enum {
    (
        <$dgen: ident, $wgen: ident, $alpha: ident> =>
        $(
            $(#[$attr:meta])*
            $name: ident
            ($context_inner: ty, $surface_inner: ty, $buffer_inner: ty),
        )*
    ) => {
        pub(crate) enum ContextDispatch<$dgen> {
            $(
                $(#[$attr])*
                $name($context_inner),
            )*
        }

        #[allow(clippy::large_enum_variant)] // it's boxed anyways
        pub(crate) enum SurfaceDispatch<$dgen, $wgen, $alpha> {
            $(
                $(#[$attr])*
                $name($surface_inner),
            )*
        }

        pub(crate) enum BufferDispatch<'a, $dgen, $wgen, $alpha> {
            $(
                $(#[$attr])*
                $name($buffer_inner),
            )*
        }

        impl<D: HasDisplayHandle> ContextDispatch<D> {
            pub fn variant_name(&self) -> &'static str {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(_) => stringify!($name),
                    )*
                }
            }
        }

        impl<D: HasDisplayHandle> ContextInterface<D> for ContextDispatch<D> {
            fn new(mut display: D) -> Result<Self, InitError<D>>
            where
                D: Sized,
            {
                $(
                    $(#[$attr])*
                    match <$context_inner as ContextInterface<D>>::new(display) {
                        Ok(x) => {
                            return Ok(Self::$name(x));
                        }
                        Err(InitError::Unsupported(d)) => display = d,
                        Err(InitError::Failure(f)) => return Err(InitError::Failure(f)),
                    }
                )*

                Err(InitError::Unsupported(display))
            }
        }
    };
}

macro_rules! make_dispatch {
    (
        <$dgen: ident, $wgen: ident, $alpha: ident> =>
        $(
            $(#[$attr:meta])*
            $name: ident
            ($context_inner: ty, $surface_inner: ty, $buffer_inner: ty),
        )*
    ) => {
        impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W, $alpha> for SurfaceDispatch<D, W, $alpha>{
            type Context = ContextDispatch<D>;
            type Buffer<'a> = BufferDispatch<'a, D, W, $alpha> where Self: 'a;

            fn new(window: W, display: &Self::Context) -> Result<Self, InitError<W>>
            where
                W: Sized,
            Self: Sized {
                match display {
                    $(
                        $(#[$attr])*
                        ContextDispatch::$name(inner) => Ok(Self::$name(<$surface_inner>::new(window, inner)?)),
                    )*
                }
            }

            fn new_with_alpha(window: W, display: &Self::Context) -> Result<Self, InitError<W>>
            where
                W: Sized,
            Self: Sized {
                match display {
                    $(
                        $(#[$attr])*
                        ContextDispatch::$name(inner) => Ok(Self::$name(<$surface_inner>::new_with_alpha(window, inner)?)),
                    )*
                }
            }

            fn window(&self) -> &W {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.window(),
                    )*
                }
            }

            fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.resize(width, height),
                    )*
                }
            }

            fn buffer_mut(&mut self) -> Result<BufferDispatch<'_, D, W, $alpha>, SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => Ok(BufferDispatch::$name(inner.buffer_mut()?)),
                    )*
                }
            }

            fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.fetch(),
                    )*
                }
            }
        }

        
        impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface<$alpha> for BufferDispatch<'a, D, W, $alpha> {
            #[inline]
            fn pixels(&self) -> &[u32] {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.pixels(),
                    )*
                }
            }

            #[inline]
            fn pixels_mut(&mut self) -> &mut [u32] {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.pixels_mut(),
                    )*
                }
            }

            fn pixels_rgb_mut(&mut self) -> &mut[<$alpha as BufferReturn>::Output]{
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.pixels_rgb_mut(),
                    )*
                }
            }

            fn age(&self) -> u8 {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.age(),
                    )*
                }
            }

            fn present(self) -> Result<(), SoftBufferError> {
                match self {
                    $(
                        $(#[$attr])*
                        Self::$name(inner) => inner.present(),
                    )*
                }
            }

            fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
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

make_enum!{
    <D, W, A> =>
    #[cfg(x11_platform)]
    X11(Arc<backends::x11::X11DisplayImpl<D>>, backends::x11::X11Impl<D, W>, backends::x11::BufferImpl<'a, D, W>),
    #[cfg(wayland_platform)]
    Wayland(Arc<backends::wayland::WaylandDisplayImpl<D>>, backends::wayland::WaylandImpl<D, W>, backends::wayland::BufferImpl<'a, D, W>),
    #[cfg(kms_platform)]
    Kms(Arc<backends::kms::KmsDisplayImpl<D>>, backends::kms::KmsImpl<D, W>, backends::kms::BufferImpl<'a, D, W>),
    #[cfg(target_os = "windows")]
    Win32(D, backends::win32::Win32Impl<D, W, A>, backends::win32::BufferImpl<'a, D, W, A>),
    #[cfg(target_vendor = "apple")]
    CoreGraphics(D, backends::cg::CGImpl<D, W, A>, backends::cg::BufferImpl<'a, D, W, A>),
    #[cfg(target_arch = "wasm32")]
    Web(backends::web::WebDisplayImpl<D>, backends::web::WebImpl<D, W>, backends::web::BufferImpl<'a, D, W>),
    #[cfg(target_os = "redox")]
    Orbital(D, backends::orbital::OrbitalImpl<D, W>, backends::orbital::BufferImpl<'a, D, W>),
}

#[duplicate_item(
    TY;
    [ WithAlpha ];
    [ WithoutAlpha ];
  )]
make_dispatch! {
    <D, W, TY> =>
    #[cfg(x11_platform)]
    X11(Arc<backends::x11::X11DisplayImpl<D>>, backends::x11::X11Impl<D, W>, backends::x11::BufferImpl<'a, D, W>),
    #[cfg(wayland_platform)]
    Wayland(Arc<backends::wayland::WaylandDisplayImpl<D>>, backends::wayland::WaylandImpl<D, W>, backends::wayland::BufferImpl<'a, D, W>),
    #[cfg(kms_platform)]
    Kms(Arc<backends::kms::KmsDisplayImpl<D>>, backends::kms::KmsImpl<D, W>, backends::kms::BufferImpl<'a, D, W>),
    #[cfg(target_os = "windows")]
    Win32(D, backends::win32::Win32Impl<D, W, TY>, backends::win32::BufferImpl<'a, D, W, TY>),
    #[cfg(target_vendor = "apple")]
    CoreGraphics(D, backends::cg::CGImpl<D, W, TY>, backends::cg::BufferImpl<'a, D, W, TY>),
    #[cfg(target_arch = "wasm32")]
    Web(backends::web::WebDisplayImpl<D>, backends::web::WebImpl<D, W>, backends::web::BufferImpl<'a, D, W>),
    #[cfg(target_os = "redox")]
    Orbital(D, backends::orbital::OrbitalImpl<D, W>, backends::orbital::BufferImpl<'a, D, W>),
}