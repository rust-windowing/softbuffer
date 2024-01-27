//! Implements `buffer_interface::*` traits for enums dispatching to backends

use crate::{backend_interface::*, Rect, SoftBufferError};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::num::NonZeroU32;
#[cfg(any(wayland_platform, x11_platform, kms_platform))]
use std::rc::Rc;

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
        pub(crate) enum ContextDispatch<$dgen> {
            $(
                $(#[$attr])*
                $name($context_inner),
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

        #[allow(clippy::large_enum_variant)] // it's boxed anyways
        pub(crate) enum SurfaceDispatch<$dgen, $wgen> {
            $(
                $(#[$attr])*
                $name($surface_inner),
            )*
        }

        impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<W> for SurfaceDispatch<D, W> {
            type Buffer<'a> = BufferDispatch<'a, D, W> where Self: 'a;

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

            fn buffer_mut(&mut self) -> Result<BufferDispatch<'_, D, W>, SoftBufferError> {
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

        pub(crate) enum BufferDispatch<'a, $dgen, $wgen> {
            $(
                $(#[$attr])*
                $name($buffer_inner),
            )*
        }

        impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferDispatch<'a, D, W> {
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

make_dispatch! {
    <D, W> =>
    #[cfg(x11_platform)]
    X11(Rc<crate::x11::X11DisplayImpl<D>>, crate::x11::X11Impl<D, W>, crate::x11::BufferImpl<'a, D, W>),
    #[cfg(wayland_platform)]
    Wayland(Rc<crate::wayland::WaylandDisplayImpl<D>>, crate::wayland::WaylandImpl<D, W>, crate::wayland::BufferImpl<'a, D, W>),
    #[cfg(kms_platform)]
    Kms(Rc<crate::kms::KmsDisplayImpl<D>>, crate::kms::KmsImpl<D, W>, crate::kms::BufferImpl<'a, D, W>),
    #[cfg(target_os = "windows")]
    Win32(D, crate::win32::Win32Impl<D, W>, crate::win32::BufferImpl<'a, D, W>),
    #[cfg(target_os = "macos")]
    CG(D, crate::cg::CGImpl<D, W>, crate::cg::BufferImpl<'a, D, W>),
    #[cfg(target_arch = "wasm32")]
    Web(crate::web::WebDisplayImpl<D>, crate::web::WebImpl<D, W>, crate::web::BufferImpl<'a, D, W>),
    #[cfg(target_os = "redox")]
    Orbital(D, crate::orbital::OrbitalImpl<D, W>, crate::orbital::BufferImpl<'a, D, W>),
}
