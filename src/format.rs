/// The pixel format of a surface and pixel buffer.
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
/// for row in buffer.rows_mut() {
///     row.as_chunks_mut::<3>().0.map(|[r, g, b]| {
///         *r = 0xff;
///         *g = 0x00;
///         *b = 0x00;
///     });
/// }
/// ```
///
/// When components are larger than one byte, such as in [`PixelFormat::Rgb32f`], the data for each
/// component is stored in the target platform's native endianess (in this case, on a little endian
/// system, it would be stored in memory as `R3,R2,R1,R0,G3,G2,G1,G0,B3,B2,B1,B0`).
///
/// The recommended way to work with these is to first transmute the data to the component type (in
/// this case `u32`), and then split rows in chunks by of the number of components:
///
/// ```no_run
/// // Fill a buffer that uses `PixelFormat::Rgb32` with red.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
/// for row in buffer.rows_u32_mut() {
///     row.as_chunks_mut::<3>().0.map(|[r, g, b]| {
///         *r = 0xffff;
///         *g = 0x0000;
///         *b = 0x0000;
///     });
/// }
/// ```
///
/// Packed formats such as [`PixelFormat::A1Bgr5`] are stored as an integer with same size, in the
/// target platform's native endianess (in this case, on a little endian system, it would be stored
/// in memory as `0bGGGBBBBB` in byte 0 and `0bARRRRGG` in byte 1).
///
/// The recommended way to work with these is to first transform the data to an integer that can
/// contain the entire pixel (in this case `u16`), and then work with the data as an integer:
///
/// ```
/// // Fill a buffer that uses `PixelFormat::A1Bgr5` with red.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
/// for row in buffer.rows_u16_mut() {
///     row.iter_mut().map(|pixel| {
///         *pixel = 0b0_11111_00000_00000;
///     });
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
/// ```
/// // Fill a buffer that uses `PixelFormat::R2` with gray.
/// # let buffer: softbuffer::Buffer<'_> = todo!();
/// for row in buffer.rows_mut() {
///     row.iter_mut().map(|pixels| {
///         *pixels = 0b00000000; // Clear
///         for i in 0..3 {
///             let pixel: u8 = 0b10;
///             *pixels |= pixel << i * 2;
///         }
///     });
/// }
/// ```
///
/// This is roughly the same naming scheme as what WebGPU uses.
///
/// See also the [Pixel Format Guide](https://afrantzis.com/pixel-format-guide/).
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum PixelFormat {
    //
    // Byte-aligned RGB formats
    //
    /// Red, green, blue, and alpha components. `u8` per channel. Laid out as `R0, G0, B0`. TODO.
    #[doc(alias = "Rgb888")]
    #[doc(alias = "VK_FORMAT_B8G8R8_UNORM")]
    Bgr8,
    #[doc(alias = "Bgr888")]
    #[doc(alias = "VK_FORMAT_R8G8B8_UNORM")]
    Rgb8,
    #[doc(alias = "Argb8888")]
    #[doc(alias = "Xrgb8888")]
    #[doc(alias = "VK_FORMAT_B8G8R8A8_UNORM")]
    Bgra8,
    #[doc(alias = "Abgr8888")]
    #[doc(alias = "Xbgr8888")]
    #[doc(alias = "VK_FORMAT_R8G8B8A8_UNORM")]
    Rgba8,
    #[doc(alias = "Rgba8888")]
    #[doc(alias = "Rgbx8888")]
    Abgr8,
    #[doc(alias = "Bgra8888")]
    #[doc(alias = "Bgrx8888")]
    Argb8,

    Bgr16,
    #[doc(alias = "VK_FORMAT_R16G16B16_UNORM")]
    Rgb16,
    #[doc(alias = "Argb16161616")]
    #[doc(alias = "Xrgb16161616")]
    Bgra16,
    #[doc(alias = "Abgr16161616")]
    #[doc(alias = "Xbgr16161616")]
    #[doc(alias = "VK_FORMAT_R16G16B16A16_UNORM")]
    Rgba16,
    #[doc(alias = "Rgba16161616")]
    #[doc(alias = "Rgbx16161616")]
    Abgr16,
    #[doc(alias = "Bgra16161616")]
    #[doc(alias = "Bgrx16161616")]
    Argb16,

    //
    // Grayscale formats
    //
    R1,
    R2,
    R4,
    #[doc(alias = "VK_FORMAT_R8_UNORM")]
    R8,
    #[doc(alias = "VK_FORMAT_R16_UNORM")]
    R16,
    // TODO: R16f,
    // TODO: R32f,

    //
    // Packed RGB formats
    //
    /// Packed into a `u8` as `0bBB_GGG_RR`.
    #[doc(alias = "Bgr233")]
    B2g3r3,
    /// Packed into a `u8` as `0bRRR_GGG_BB`.
    #[doc(alias = "Rgb332")]
    R3g3b2,

    /// Packed into a `u16` as `0bBBBBB_GGGGGG_RRRRR`.
    #[doc(alias = "Bgr565")]
    #[doc(alias = "VK_FORMAT_B5G6R5_UNORM_PACK16")]
    B5g6r5,
    /// Packed into a `u16` as `0bRRRRR_GGGGGG_BBBBB`.
    #[doc(alias = "Rgb565")]
    #[doc(alias = "VK_FORMAT_R5G6B5_UNORM_PACK16")]
    R5g6b5,

    /// Packed into a `u16` as `0bBBBB_GGGG_RRRR_AAAA`.
    #[doc(alias = "Bgra4444")]
    #[doc(alias = "Bgrx4444")]
    #[doc(alias = "VK_FORMAT_B4G4R4A4_UNORM_PACK16")]
    Bgra4,
    /// Packed into a `u16` as `0bRRRR_GGGG_BBBB_AAAA`.
    #[doc(alias = "Rgba4444")]
    #[doc(alias = "Rgbx4444")]
    #[doc(alias = "VK_FORMAT_R4G4B4A4_UNORM_PACK16")]
    Rgba4,
    /// Packed into a `u16` as `0bAAAA_BBBB_GGGG_RRRR`.
    #[doc(alias = "Abgr4444")]
    #[doc(alias = "Xbgr4444")]
    Abgr4,
    /// Packed into a `u16` as `0bAAAA_RRRR_GGGG_BBBB`.
    #[doc(alias = "Argb4444")]
    #[doc(alias = "Xrgb4444")]
    Argb4,

    /// Packed into a `u16` as `0bBBBBB_GGGGG_RRRRR_A`.
    #[doc(alias = "Bgrx5551")]
    #[doc(alias = "VK_FORMAT_B5G5R5A1_UNORM_PACK16")]
    Bgr5a1,
    /// Packed into a `u16` as `0bRRRRR_GGGGG_BBBBB_A`.
    #[doc(alias = "Rgbx5551")]
    #[doc(alias = "VK_FORMAT_R5G5B5A1_UNORM_PACK16")]
    Rgb5a1,
    /// Packed into a `u16` as `0bA_BBBBB_GGGGG_RRRRR`.
    #[doc(alias = "Xbgr1555")]
    A1bgr5,
    /// Packed into a `u16` as `0bA_RRRRR_GGGGG_BBBBB`.
    #[doc(alias = "Xrgb1555")]
    #[doc(alias = "VK_FORMAT_A1R5G5B5_UNORM_PACK16")]
    A1rgb5,

    /// Packed into a `u32` as `0bBBBBBBBBBB_GGGGGGGGGG_RRRRRRRRRR_AA`.
    #[doc(alias = "Bgra1010102")]
    #[doc(alias = "Bgrx1010102")]
    Bgr10a2,
    /// Packed into a `u32` as `0bRRRRRRRRRR_GGGGGGGGGG_BBBBBBBBBB_AA`.
    #[doc(alias = "Rgba1010102")]
    #[doc(alias = "Rgbx1010102")]
    Rgb10a2,
    /// Packed into a `u32` as `0bAA_BBBBBBBBBB_GGGGGGGGGG_RRRRRRRRRR`.
    #[doc(alias = "Abgr2101010")]
    #[doc(alias = "Xbgr2101010")]
    #[doc(alias = "VK_FORMAT_A2B10G10R10_UNORM_PACK32")]
    A2bgr10,
    /// Packed into a `u32` as `0bAA_RRRRRRRRRR_GGGGGGGGGG_BBBBBBBBBB`.
    #[doc(alias = "Argb2101010")]
    #[doc(alias = "Xrgb2101010")]
    #[doc(alias = "VK_FORMAT_A2R10G10B10_UNORM_PACK32")]
    A2rgb10,

    //
    // Floating point RGB formats.
    //
    #[cfg(feature = "f16")]
    Bgr16f,
    #[cfg(feature = "f16")]
    Rgb16f,
    #[doc(alias = "Argb16161616f")]
    #[doc(alias = "Xrgb16161616f")]
    #[cfg(feature = "f16")]
    Bgra16f,
    #[doc(alias = "Abgr16161616f")]
    #[doc(alias = "Xbgr16161616f")]
    #[cfg(feature = "f16")]
    Rgba16f,
    #[doc(alias = "Rgba16161616f")]
    #[doc(alias = "Rgbx16161616f")]
    #[cfg(feature = "f16")]
    Abgr16f,
    #[doc(alias = "Bgra16161616f")]
    #[doc(alias = "Bgrx16161616f")]
    #[cfg(feature = "f16")]
    Argb16f,

    Bgr32f,
    #[doc(alias = "VK_FORMAT_R32G32B32_SFLOAT")]
    Rgb32f,
    #[doc(alias = "Argb32323232f")]
    #[doc(alias = "Xrgb32323232f")]
    Bgra32f,
    #[doc(alias = "Abgr32323232f")]
    #[doc(alias = "Xbgr32323232f")]
    #[doc(alias = "VK_FORMAT_R32G32B32A32_SFLOAT")]
    Rgba32f,
    #[doc(alias = "Rgba32323232f")]
    #[doc(alias = "Rgbx32323232f")]
    Abgr32f,
    #[doc(alias = "Bgra32323232f")]
    #[doc(alias = "Bgrx32323232f")]
    Argb32f,
    // TODO: AYCbCr formats?
}

impl PixelFormat {
    /// The number of bits wide that a single pixel in a buffer would be.
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
            #[cfg(feature = "f16")]
            Self::Bgr16f | Self::Rgb16f => 48,
            #[cfg(feature = "f16")]
            Self::Bgra16f | Self::Rgba16f | Self::Abgr16f | Self::Argb16f => 64,
            Self::Bgr32f | Self::Rgb32f => 96,
            Self::Bgra32f | Self::Rgba32f | Self::Abgr32f | Self::Argb32f => 128,
        }
    }

    /// The number of components this format has.
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
            #[cfg(feature = "f16")]
            Self::Bgr16f | Self::Rgb16f => 3,
            #[cfg(feature = "f16")]
            Self::Bgra16f | Self::Rgba16f | Self::Abgr16f | Self::Argb16f => 4,
            Self::Bgr32f | Self::Rgb32f => 3,
            Self::Bgra32f | Self::Rgba32f | Self::Abgr32f | Self::Argb32f => 4,
        }
    }
}
