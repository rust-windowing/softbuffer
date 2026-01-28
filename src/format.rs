/// A pixel format that Softbuffer may use.
///
/// # Default
///
/// The [`Default::default`] implementation returns the pixel format that Softbuffer uses for the
/// current target platform.
///
/// Currently, this is [`BGRX`][Self::Bgrx] on all platforms except WebAssembly and Android, where
/// it is [`RGBX`][Self::Rgbx], since the API on these platforms does not support BGRX.
///
/// The format for a given platform may change in a non-breaking release if found to be more
/// performant.
///
/// This distinction should only be relevant if you're bitcasting `Pixel` to/from a `u32`, to e.g.
/// avoid unnecessary copying, see the documentation for [`Pixel`][crate::Pixel] for examples.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum PixelFormat {
    /// The pixel format is `RGBX` (red, green, blue, unset).
    ///
    /// This is currently the default on macOS/iOS, KMS/DRM, Orbital, Wayland, Windows and X11.
    #[cfg_attr(not(any(target_family = "wasm", target_os = "android")), default)]
    Bgrx,
    /// The pixel format is `BGRX` (blue, green, red, unset).
    ///
    /// This is currently the default on Android and Web.
    #[cfg_attr(any(target_family = "wasm", target_os = "android"), default)]
    Rgbx,
    // Intentionally exhaustive for now.
}

impl PixelFormat {
    /// Check whether the given pixel format is the default format that Softbuffer uses.
    #[inline]
    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}
