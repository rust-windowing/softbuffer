/// A pixel format that Softbuffer may use.
///
/// # Alpha
///
/// These pixel formats all include the alpha channel in their name, but formats that ignore the
/// alpha channel are supported if you set [`AlphaMode::Ignored`]. This will make `RGBA` mean `RGBX`
/// and `BGRA` mean `BGRX`.
///
/// [`AlphaMode::Ignored`]: crate::AlphaMode::Ignored
///
/// # Default
///
/// The [`Default::default`] implementation returns the pixel format that Softbuffer uses for the
/// current target platform.
///
/// Currently, this is [`BGRA`][Self::Bgra] on all platforms except WebAssembly and Android, where
/// it is [`RGBA`][Self::Rgba], since the API on these platforms does not support BGRA.
///
/// The format for a given platform may change in a non-breaking release if found to be more
/// performant.
///
/// This distinction should only be relevant if you're bitcasting `Pixel` to/from a `u32`, to e.g.
/// avoid unnecessary copying, see the documentation for [`Pixel`][crate::Pixel] for examples.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum PixelFormat {
    /// The pixel format is `RGBA` (red, green, blue, alpha).
    ///
    /// This is currently the default on macOS/iOS, KMS/DRM, Orbital, Wayland, Windows and X11.
    #[cfg_attr(not(any(target_family = "wasm", target_os = "android")), default)]
    Bgra,
    /// The pixel format is `BGRA` (blue, green, red, alpha).
    ///
    /// This is currently the default on Android and Web.
    #[cfg_attr(any(target_family = "wasm", target_os = "android"), default)]
    Rgba,
    // Intentionally exhaustive for now.
}

impl PixelFormat {
    /// Check whether the given pixel format is the default format that Softbuffer uses.
    #[inline]
    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}
