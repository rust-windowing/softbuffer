/// The pixel format of a surface and pixel buffer.
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
///
/// # Byte order
///
/// Non-packed formats (also called array formats) such as [`PixelFormat::Rgb8`] are stored with
/// each component in byte order (in this case R in byte 0, G in byte 1 and B in byte 2).
///
/// The recommended way to work with these is to split rows in chunks of the number of components:
///
/// ```no_run
/// // Fill a buffer that uses `PixelFormat::Rgb8` with red.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
///
/// let byte_stride = buffer.byte_stride().get() as usize;
/// let pixels = buffer.pixels();
/// // SAFETY: `Pixel` can be reinterpreted as 4 `u8`s.
/// let data_u8 = unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u8>(), pixels.len() * 4) };
///
/// for row in data_u8.chunks_mut(byte_stride) {
///     // Split and ignore remaining bytes in each row used to align rows to CPU cache lines.
///     let (row, _remaining) = row.as_chunks_mut::<3>();
///     for [r, g, b] in row {
///         *r = 0xff;
///         *g = 0x00;
///         *b = 0x00;
///     }
/// }
/// ```
///
/// When components are larger than one byte, such as in [`PixelFormat::Rgba16`], the data for each
/// component is stored in the target platform's native endianess, but components are still ordered
/// in byte-order (so in this case, on a little endian system, it would be stored in memory as
/// `R1,R0,G1,G0,B1,B0,A1,A0`).
///
/// The recommended way to work with these is to first transmute the data to the component type (in
/// this case `u16`), and then split rows in chunks by of the number of components:
///
/// ```no_run
/// // Fill a buffer that uses `PixelFormat::Rgba16` with red.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
///
/// let byte_stride = buffer.byte_stride().get() as usize;
/// let pixels = buffer.pixels();
/// // SAFETY: `Pixel` can be reinterpreted as 2 `u16`s.
/// let data_u16 = unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u16>(), pixels.len() * 2) };
///
/// for row in data_u16.chunks_mut(byte_stride) {
///     // Split and ignore remaining bytes in each row used to align rows to CPU cache lines.
///     let (row, _remaining) = row.as_chunks_mut::<4>();
///     for [r, g, b, a] in row {
///         *r = 0xffff;
///         *g = 0x0000;
///         *b = 0x0000;
///         *a = 0x0000;
///     }
/// }
/// ```
///
/// Packed formats such as [`PixelFormat::A1bgr5`] are stored as an integer with same size, in the
/// target platform's native endianess (in this case, on a little endian system, it would be stored
/// in memory as `0bGGGBBBBB` in byte 0 and `0bARRRRGG` in byte 1).
///
/// The recommended way to work with these is to first transform the data to an integer that can
/// contain the entire pixel (in this case `u16`), and then work with the data as that integer:
///
/// ```no_run
/// // Fill a buffer that uses `PixelFormat::A1bgr5` with red.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
///
/// let byte_stride = buffer.byte_stride().get() as usize;
/// let pixels = buffer.pixels();
/// // SAFETY: `Pixel` can be reinterpreted as 2 `u16`s.
/// let data_u16 = unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u16>(), pixels.len() * 2) };
///
/// for row in data_u16.chunks_mut(byte_stride) {
///     for pixel in row {
///         *pixel = 0b0_11111_00000_00000;
///     }
/// }
/// ```
///
/// Finally, some formats such as [`PixelFormat::R2`] have multiple pixels packed into a single
/// byte. These are stored with the first pixel in the most significant bits (so in this case
/// `0b00112233`). TODO: Verify this!
///
/// The recommended way to work with these is to iterate over the data as an `u8`, and then use
/// bitwise operators to write each pixel (or write them in bulk if you can):
///
/// ```no_run
/// // Fill a buffer that uses `PixelFormat::R2` with gray.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
///
/// let byte_stride = buffer.byte_stride().get() as usize;
/// let pixels = buffer.pixels();
/// // SAFETY: `Pixel` can be reinterpreted as 4 `u8`s.
/// let data_u8 = unsafe { std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u8>(), pixels.len() * 4) };
///
/// for row in data_u8.chunks_mut(byte_stride) {
///     for pixels in row {
///         *pixels = 0b00000000; // Clear
///         for i in 0..3 {
///             let pixel: u8 = 0b10;
///             *pixels |= pixel << i * 2;
///         }
///     }
/// }
/// ```
///
/// See also the [Pixel Format Guide](https://afrantzis.com/pixel-format-guide/).
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum PixelFormat {
    // This uses roughly the same naming scheme as WebGPU does.

    //
    // Byte-aligned RGB formats
    //
    /// 24-bit BGR, `u8` per channel. Laid out in memory as `B,G,R`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgb888")]
    #[doc(alias = "VK_FORMAT_B8G8R8_UNORM")]
    Bgr8,

    /// 24-bit RGB, `u8` per channel. Laid out in memory as `R,G,B`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgr888")]
    #[doc(alias = "VK_FORMAT_R8G8B8_UNORM")]
    Rgb8,

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

    /// 32-bit ABGR, `u8` per channel. Laid out in memory as `A,B,G,R`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba8888")]
    #[doc(alias = "Rgbx8888")]
    Abgr8,

    /// 32-bit ARGB, `u8` per channel. Laid out in memory as `A,R,G,B`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra8888")]
    #[doc(alias = "Bgrx8888")]
    Argb8,

    /// 48-bit BGR, `u16` per channel. Laid out in memory as `B0,B1,G0,G1,R0,R1` (big endian) or
    /// `B1,B0,G1,G0,R1,R0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    Bgr16,

    /// 48-bit RGB, `u16` per channel. Laid out in memory as `R0,R1,G0,G1,B0,B1` (big endian) or
    /// `R1,R0,G1,G0,B1,B0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "VK_FORMAT_R16G16B16_UNORM")]
    Rgb16,

    /// 64-bit BGRA, `u16` per channel. Laid out in memory as `B0,B1,G0,G1,R0,R1,A0,A1` (big endian)
    /// or `B1,B0,G1,G0,R1,R0,A1,A0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Argb16161616")]
    #[doc(alias = "Xrgb16161616")]
    Bgra16,

    /// 64-bit RGBA, `u16` per channel. Laid out in memory as `R0,R1,G0,G1,B0,B1,A0,A1` (big endian)
    /// or `R1,R0,G1,G0,B1,B0,A1,A0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Abgr16161616")]
    #[doc(alias = "Xbgr16161616")]
    #[doc(alias = "VK_FORMAT_R16G16B16A16_UNORM")]
    Rgba16,

    /// 64-bit ABGR, `u16` per channel. Laid out in memory as `A0,A1,B0,B1,G0,G1,R0,R1` (big endian)
    /// or `A1,A0,B1,B0,G1,G0,R1,R0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba16161616")]
    #[doc(alias = "Rgbx16161616")]
    Abgr16,

    /// 64-bit ARGB, `u16` per channel. Laid out in memory as `A1,A0,R0,R1,G0,G1,B0,B1` (big endian)
    /// or `A1,A0,R1,R0,G1,G0,B1,B0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra16161616")]
    #[doc(alias = "Bgrx16161616")]
    Argb16,

    //
    // Grayscale formats
    //
    /// 1-bit grayscale. 8 pixels are packed into a single byte as `0b0_1_2_3_4_5_6_7`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    R1,

    /// 2-bit grayscale. 4 pixels are packed into a single byte as `0b00_11_22_33`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    R2,

    /// 4-bit grayscale. 2 pixels are packed into a single byte as `0b0000_1111`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    R4,

    /// 8-bit grayscale, `u8` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "VK_FORMAT_R8_UNORM")]
    R8,

    /// 16-bit grayscale, `u16` per channel. Laid out in memory as `R0,R1` (big endian) or
    /// `R1,R0` (little endian).
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "VK_FORMAT_R16_UNORM")]
    R16,

    // TODO: R16f?
    // TODO: R32f?

    //
    // Packed RGB formats
    //
    /// 8-bit BGR. Packed into a `u8` as `0bBB_GGG_RR`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgr233")]
    B2g3r3,

    /// 8-bit RGB. Packed into a `u8` as `0bRRR_GGG_BB`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgb332")]
    R3g3b2,

    /// 16-bit BGR. Packed into a `u16` as `0bBBBBB_GGGGGG_RRRRR`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgr565")]
    #[doc(alias = "VK_FORMAT_B5G6R5_UNORM_PACK16")]
    B5g6r5,

    /// 16-bit RGB. Packed into a `u16` as `0bRRRRR_GGGGGG_BBBBB`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgb565")]
    #[doc(alias = "VK_FORMAT_R5G6B5_UNORM_PACK16")]
    R5g6b5,

    /// 16-bit BGRA. Packed into a `u16` as `0bBBBB_GGGG_RRRR_AAAA`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra4444")]
    #[doc(alias = "Bgrx4444")]
    #[doc(alias = "VK_FORMAT_B4G4R4A4_UNORM_PACK16")]
    Bgra4,

    /// 16-bit RGBA. Packed into a `u16` as `0bRRRR_GGGG_BBBB_AAAA`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba4444")]
    #[doc(alias = "Rgbx4444")]
    #[doc(alias = "VK_FORMAT_R4G4B4A4_UNORM_PACK16")]
    Rgba4,

    /// 16-bit ABGR. Packed into a `u16` as `0bAAAA_BBBB_GGGG_RRRR`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Abgr4444")]
    #[doc(alias = "Xbgr4444")]
    Abgr4,

    /// 16-bit ARGB. Packed into a `u16` as `0bAAAA_RRRR_GGGG_BBBB`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Argb4444")]
    #[doc(alias = "Xrgb4444")]
    Argb4,

    /// 16-bit BGRA. Packed into a `u16` as `0bBBBBB_GGGGG_RRRRR_A`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgrx5551")]
    #[doc(alias = "VK_FORMAT_B5G5R5A1_UNORM_PACK16")]
    Bgr5a1,

    /// 16-bit RGBA. Packed into a `u16` as `0bRRRRR_GGGGG_BBBBB_A`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgbx5551")]
    #[doc(alias = "VK_FORMAT_R5G5B5A1_UNORM_PACK16")]
    Rgb5a1,

    /// 16-bit ABGR. Packed into a `u16` as `0bA_BBBBB_GGGGG_RRRRR`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Xbgr1555")]
    A1bgr5,

    /// 16-bit ARGB. Packed into a `u16` as `0bA_RRRRR_GGGGG_BBBBB`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Xrgb1555")]
    #[doc(alias = "VK_FORMAT_A1R5G5B5_UNORM_PACK16")]
    A1rgb5,

    /// 32-bit BGRA. Packed into a `u32` as `0bBBBBBBBBBB_GGGGGGGGGG_RRRRRRRRRR_AA`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra1010102")]
    #[doc(alias = "Bgrx1010102")]
    Bgr10a2,

    /// 32-bit RGBA. Packed into a `u32` as `0bRRRRRRRRRR_GGGGGGGGGG_BBBBBBBBBB_AA`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba1010102")]
    #[doc(alias = "Rgbx1010102")]
    Rgb10a2,

    /// 32-bit ABGR. Packed into a `u32` as `0bAA_BBBBBBBBBB_GGGGGGGGGG_RRRRRRRRRR`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Abgr2101010")]
    #[doc(alias = "Xbgr2101010")]
    #[doc(alias = "VK_FORMAT_A2B10G10R10_UNORM_PACK32")]
    A2bgr10,

    /// 32-bit ARGB. Packed into a `u32` as `0bAA_RRRRRRRRRR_GGGGGGGGGG_BBBBBBBBBB`.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Argb2101010")]
    #[doc(alias = "Xrgb2101010")]
    #[doc(alias = "VK_FORMAT_A2R10G10B10_UNORM_PACK32")]
    A2rgb10,

    //
    // Floating point RGB formats.
    //
    /// 48-bit floating-point BGR, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    Bgr16f,

    /// 48-bit floating-point RGB, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    Rgb16f,

    /// 64-bit floating-point BGRA, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Argb16161616f")]
    #[doc(alias = "Xrgb16161616f")]
    Bgra16f,

    /// 64-bit floating-point RGBA, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Abgr16161616f")]
    #[doc(alias = "Xbgr16161616f")]
    Rgba16f,

    /// 64-bit floating-point ABGR, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba16161616f")]
    #[doc(alias = "Rgbx16161616f")]
    Abgr16f,

    /// 64-bit floating-point ARGB, `f16` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra16161616f")]
    #[doc(alias = "Bgrx16161616f")]
    Argb16f,

    /// 96-bit floating-point BGR, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    Bgr32f,

    /// 96-bit floating-point RGB, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "VK_FORMAT_R32G32B32_SFLOAT")]
    Rgb32f,

    /// 128-bit floating-point BGRA, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Argb32323232f")]
    #[doc(alias = "Xrgb32323232f")]
    Bgra32f,

    /// 128-bit floating-point RGBA, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Abgr32323232f")]
    #[doc(alias = "Xbgr32323232f")]
    #[doc(alias = "VK_FORMAT_R32G32B32A32_SFLOAT")]
    Rgba32f,

    /// 128-bit floating-point ABGR, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Rgba32323232f")]
    #[doc(alias = "Rgbx32323232f")]
    Abgr32f,

    /// 128-bit floating-point ARGB, `f32` per channel.
    ///
    /// ## Platform Support
    ///
    /// Not yet supported by any backend.
    #[doc(alias = "Bgra32323232f")]
    #[doc(alias = "Bgrx32323232f")]
    Argb32f,
    // TODO: AYCbCr formats?
}

impl PixelFormat {
    /// Check whether the given pixel format is the default format that Softbuffer uses.
    #[inline]
    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    /// The number of bits wide that a single pixel in a buffer would be.
    #[doc(alias = "bpp")]
    #[inline]
    pub const fn bits_per_pixel(self) -> u8 {
        match self {
            Self::Rgb8 | Self::Bgr8 => 24,
            Self::Bgra8 | Self::Rgba8 | Self::Abgr8 | Self::Argb8 => 32,
            Self::Rgb16 | Self::Bgr16 => 48,
            Self::Bgra16 | Self::Rgba16 | Self::Abgr16 | Self::Argb16 => 64,
            Self::R1 => 1,
            Self::R2 => 2,
            Self::R4 => 4,
            Self::R8 => 8,
            Self::R16 => 16,
            Self::B2g3r3 | Self::R3g3b2 => 8,
            Self::B5g6r5 | Self::R5g6b5 => 16,
            Self::Bgra4 | Self::Rgba4 | Self::Abgr4 | Self::Argb4 => 16,
            Self::Bgr5a1 | Self::Rgb5a1 | Self::A1bgr5 | Self::A1rgb5 => 16,
            Self::Bgr10a2 | Self::Rgb10a2 | Self::A2bgr10 | Self::A2rgb10 => 32,
            Self::Bgr16f | Self::Rgb16f => 48,
            Self::Bgra16f | Self::Rgba16f | Self::Abgr16f | Self::Argb16f => 64,
            Self::Bgr32f | Self::Rgb32f => 96,
            Self::Bgra32f | Self::Rgba32f | Self::Abgr32f | Self::Argb32f => 128,
        }
    }

    /// The number of components this format has.
    #[inline]
    pub const fn components(self) -> u8 {
        match self {
            Self::Rgb8 | Self::Bgr8 => 3,
            Self::Bgra8 | Self::Rgba8 | Self::Abgr8 | Self::Argb8 => 4,
            Self::Rgb16 | Self::Bgr16 => 3,
            Self::Bgra16 | Self::Rgba16 | Self::Abgr16 | Self::Argb16 => 4,
            Self::R1 | Self::R2 | Self::R4 | Self::R8 | Self::R16 => 1,
            Self::B2g3r3 | Self::R3g3b2 => 3,
            Self::B5g6r5 | Self::R5g6b5 => 3,
            Self::Bgra4 | Self::Rgba4 | Self::Abgr4 | Self::Argb4 => 4,
            Self::Bgr5a1 | Self::Rgb5a1 | Self::A1bgr5 | Self::A1rgb5 => 4,
            Self::Bgr10a2 | Self::Rgb10a2 | Self::A2bgr10 | Self::A2rgb10 => 4,
            Self::Bgr16f | Self::Rgb16f => 3,
            Self::Bgra16f | Self::Rgba16f | Self::Abgr16f | Self::Argb16f => 4,
            Self::Bgr32f | Self::Rgb32f => 3,
            Self::Bgra32f | Self::Rgba32f | Self::Abgr32f | Self::Argb32f => 4,
        }
    }
}
