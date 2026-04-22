/// The pixel format of a surface and buffer.
///
/// # Alpha
///
/// These pixel formats all include the alpha channel in their name, but formats that ignore the
/// alpha channel are supported if you use [`AlphaMode::Ignored`]. This will make `Rgba8` mean
/// `Rgbx8`, for example.
///
/// [`AlphaMode::Ignored`]: crate::AlphaMode::Ignored
///
/// # Default
///
/// The [`Default::default`] implementation returns the pixel format that Softbuffer uses for the
/// current target platform.
///
/// Currently, this is [`Bgra8`][Self::Bgra8] on all platforms except WebAssembly and Android, where
/// it is [`Rgba8`][Self::Rgba8], since the API on these platforms does not support BGRA.
///
/// The format for a given platform may change in a non-breaking release if found to be more
/// performant.
///
/// This distinction should only be relevant if you're bitcasting `Pixel` to/from a `u32`, to e.g.
/// avoid unnecessary copying, see the documentation for [`Pixel`][crate::Pixel] for examples.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum PixelFormat {
    // This uses roughly the same naming scheme as WebGPU does.
    /// 32-bit BGRA, `u8` per channel. Laid out in memory as `B,G,R,A`.
    ///
    /// **This is currently the default on macOS/iOS, KMS/DRM, Orbital, Wayland, Windows and X11**.
    ///
    /// ## Platform Support
    ///
    /// - macOS/iOS, KMS/DRM, Orbital, Wayland, Windows and X11: Supported.
    /// - Android and Web: Not yet supported.
    #[cfg_attr(not(any(target_family = "wasm", target_os = "android")), default)]
    #[doc(alias = "Argb8888")]
    #[doc(alias = "Xrgb8888")]
    #[doc(alias = "VK_FORMAT_B8G8R8A8_UNORM")]
    Bgra8,

    /// 32-bit RGBA, `u8` per channel. Laid out in memory as `R,G,B,A`.
    ///
    /// **This is currently the default on Android and Web**.
    ///
    /// ## Platform Support
    ///
    /// - Android and Web: Supported.
    /// - macOS/iOS, KMS/DRM, Orbital, Wayland, Windows and X11: Not yet supported.
    #[cfg_attr(any(target_family = "wasm", target_os = "android"), default)]
    #[doc(alias = "Abgr8888")]
    #[doc(alias = "Xbgr8888")]
    #[doc(alias = "VK_FORMAT_R8G8B8A8_UNORM")]
    Rgba8,
}

impl PixelFormat {
    /// Check whether the given pixel format is the default format that Softbuffer uses.
    #[inline]
    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}
