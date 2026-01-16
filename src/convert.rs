use std::iter;

#[cfg(feature = "f16")]
use half::f16;

use crate::{AlphaMode, PixelFormat};

#[derive(Debug)]
pub(crate) struct Input<'a> {
    data: &'a [u8],
    byte_stride: usize,
    alpha_mode: AlphaMode,
    format: PixelFormat,
}

#[derive(Debug)]
pub(crate) struct Output<'a> {
    data: &'a mut [FallbackPixel],
    stride: usize,
    alpha_mode: AlphaMode,
}

/// Convert and write pixel data from one row to another.
///
/// This is primarily meant as a fallback, so while it may be fairly efficient, that is not the
/// primary purpose. Users should instead.
///
/// # Strategy
///
/// Doing a generic conversion from data in one pixel format to another is difficult, so we allow
/// ourselves to assume that the output format is [`FALLBACK_FORMAT`].
///
/// If we didn't do this, we'd have to introduce a round-trip to a format like `Rgba32f` that is
/// (mostly) capable of storing all the other formats.
///
/// # Prior art
///
/// `SDL_ConvertPixels` + `SDL_PremultiplyAlpha` maybe?
pub(crate) fn convert_fallback(i: Input<'_>, o: Output<'_>) {
    // REMARK: We monomorphize a conversion implementation for each pixel format, this might not
    // be the most desirable?
    match i.format {
        // Unpacked formats.
        PixelFormat::Bgr8 => each_pixel::<u8, 3>(i, o, |[b, g, r]| [r, g, b, u8::ALPHA_OPAQUE]),
        PixelFormat::Rgb8 => each_pixel::<u8, 3>(i, o, |[r, g, b]| [r, g, b, u8::ALPHA_OPAQUE]),
        PixelFormat::Bgra8 => each_pixel::<u8, 4>(i, o, |[b, g, r, a]| [r, g, b, a]),
        PixelFormat::Rgba8 => each_pixel::<u8, 4>(i, o, |[r, g, b, a]| [r, g, b, a]),
        PixelFormat::Abgr8 => each_pixel::<u8, 4>(i, o, |[a, b, g, r]| [r, g, b, a]),
        PixelFormat::Argb8 => each_pixel::<u8, 4>(i, o, |[a, r, g, b]| [r, g, b, a]),
        PixelFormat::Bgr16 => each_pixel::<u16, 3>(i, o, |[b, g, r]| [r, g, b, u16::ALPHA_OPAQUE]),
        PixelFormat::Rgb16 => each_pixel::<u16, 3>(i, o, |[r, g, b]| [r, g, b, u16::ALPHA_OPAQUE]),
        PixelFormat::Bgra16 => each_pixel::<u16, 4>(i, o, |[b, g, r, a]| [r, g, b, a]),
        PixelFormat::Rgba16 => each_pixel::<u16, 4>(i, o, |[r, g, b, a]| [r, g, b, a]),
        PixelFormat::Abgr16 => each_pixel::<u16, 4>(i, o, |[a, b, g, r]| [r, g, b, a]),
        PixelFormat::Argb16 => each_pixel::<u16, 4>(i, o, |[a, r, g, b]| [r, g, b, a]),

        // Grayscale formats.
        PixelFormat::R1 => each_bitpacked_grayscale::<1>(i, o, |l| [l, l, l, u8::ALPHA_OPAQUE]),
        PixelFormat::R2 => each_bitpacked_grayscale::<2>(i, o, |l| [l, l, l, u8::ALPHA_OPAQUE]),
        PixelFormat::R4 => each_bitpacked_grayscale::<4>(i, o, |l| [l, l, l, u8::ALPHA_OPAQUE]),
        PixelFormat::R8 => each_pixel::<u8, 1>(i, o, |[l]| [l, l, l, u8::ALPHA_OPAQUE]),
        PixelFormat::R16 => each_pixel::<u16, 1>(i, o, |[l]| [l, l, l, u16::ALPHA_OPAQUE]),

        // Packed formats.
        PixelFormat::B2g3r3 => each_packed::<u8>(i, o, |pixel| {
            let b = pixel.extract_packed(6, 2);
            let g = pixel.extract_packed(3, 3);
            let r = pixel.extract_packed(0, 3);
            [r, g, b, u8::ALPHA_OPAQUE]
        }),
        PixelFormat::R3g3b2 => each_packed::<u8>(i, o, |pixel| {
            let r = pixel.extract_packed(5, 3);
            let g = pixel.extract_packed(2, 3);
            let b = pixel.extract_packed(0, 2);
            [r, g, b, u8::ALPHA_OPAQUE]
        }),

        PixelFormat::B5g6r5 => each_packed::<u16>(i, o, |pixel| {
            let b = pixel.extract_packed(11, 5);
            let g = pixel.extract_packed(5, 6);
            let r = pixel.extract_packed(0, 5);
            [r, g, b, u16::ALPHA_OPAQUE]
        }),
        PixelFormat::R5g6b5 => each_packed::<u16>(i, o, |pixel| {
            let r = pixel.extract_packed(11, 5);
            let g = pixel.extract_packed(5, 6);
            let b = pixel.extract_packed(0, 5);
            [r, g, b, u16::ALPHA_OPAQUE]
        }),

        PixelFormat::Bgra4 => each_packed::<u16>(i, o, |pixel| {
            let b = pixel.extract_packed(12, 4);
            let g = pixel.extract_packed(8, 4);
            let r = pixel.extract_packed(4, 4);
            let a = pixel.extract_packed(0, 4);
            [r, g, b, a]
        }),
        PixelFormat::Rgba4 => each_packed::<u16>(i, o, |pixel| {
            let r = pixel.extract_packed(12, 4);
            let g = pixel.extract_packed(8, 4);
            let b = pixel.extract_packed(4, 4);
            let a = pixel.extract_packed(0, 4);
            [r, g, b, a]
        }),
        PixelFormat::Abgr4 => each_packed::<u16>(i, o, |pixel| {
            let a = pixel.extract_packed(12, 4);
            let b = pixel.extract_packed(8, 4);
            let g = pixel.extract_packed(4, 4);
            let r = pixel.extract_packed(0, 4);
            [r, g, b, a]
        }),
        PixelFormat::Argb4 => each_packed::<u16>(i, o, |pixel| {
            let a = pixel.extract_packed(12, 4);
            let r = pixel.extract_packed(8, 4);
            let g = pixel.extract_packed(4, 4);
            let b = pixel.extract_packed(0, 4);
            [r, g, b, a]
        }),

        PixelFormat::Bgr5a1 => each_packed::<u16>(i, o, |pixel| {
            let b = pixel.extract_packed(11, 5);
            let g = pixel.extract_packed(6, 5);
            let r = pixel.extract_packed(1, 5);
            let a = pixel.extract_packed(0, 1);
            [r, g, b, a]
        }),
        PixelFormat::Rgb5a1 => each_packed::<u16>(i, o, |pixel| {
            let r = pixel.extract_packed(11, 5);
            let g = pixel.extract_packed(6, 5);
            let b = pixel.extract_packed(1, 5);
            let a = pixel.extract_packed(0, 1);
            [r, g, b, a]
        }),
        PixelFormat::A1bgr5 => each_packed::<u16>(i, o, |pixel| {
            let a = pixel.extract_packed(15, 1);
            let b = pixel.extract_packed(10, 5);
            let g = pixel.extract_packed(5, 5);
            let r = pixel.extract_packed(0, 5);
            [r, g, b, a]
        }),
        PixelFormat::A1rgb5 => each_packed::<u16>(i, o, |pixel| {
            let a = pixel.extract_packed(15, 1);
            let r = pixel.extract_packed(10, 5);
            let g = pixel.extract_packed(5, 5);
            let b = pixel.extract_packed(0, 5);
            [r, g, b, a]
        }),

        PixelFormat::Bgr10a2 => each_packed::<u32>(i, o, |pixel| {
            let b = pixel.extract_packed(22, 10);
            let g = pixel.extract_packed(12, 10);
            let r = pixel.extract_packed(2, 10);
            let a = pixel.extract_packed(0, 2);
            [r, g, b, a]
        }),
        PixelFormat::Rgb10a2 => each_packed::<u32>(i, o, |pixel| {
            let r = pixel.extract_packed(22, 10);
            let g = pixel.extract_packed(12, 10);
            let b = pixel.extract_packed(2, 10);
            let a = pixel.extract_packed(0, 2);
            [r, g, b, a]
        }),
        PixelFormat::A2bgr10 => each_packed::<u32>(i, o, |pixel| {
            let a = pixel.extract_packed(30, 2);
            let b = pixel.extract_packed(20, 10);
            let g = pixel.extract_packed(10, 10);
            let r = pixel.extract_packed(0, 10);
            [r, g, b, a]
        }),
        PixelFormat::A2rgb10 => each_packed::<u32>(i, o, |pixel| {
            let a = pixel.extract_packed(30, 2);
            let r = pixel.extract_packed(20, 10);
            let g = pixel.extract_packed(10, 10);
            let b = pixel.extract_packed(0, 10);
            [r, g, b, a]
        }),

        // Floating point formats.
        #[cfg(feature = "f16")]
        PixelFormat::Bgr16f => each_pixel::<f16, 3>(i, o, |[b, g, r]| [r, g, b, f16::ALPHA_OPAQUE]),
        #[cfg(feature = "f16")]
        PixelFormat::Rgb16f => each_pixel::<f16, 3>(i, o, |[r, g, b]| [r, g, b, f16::ALPHA_OPAQUE]),
        #[cfg(feature = "f16")]
        PixelFormat::Bgra16f => each_pixel::<f16, 4>(i, o, |[b, g, r, a]| [r, g, b, a]),
        #[cfg(feature = "f16")]
        PixelFormat::Rgba16f => each_pixel::<f16, 4>(i, o, |[r, g, b, a]| [r, g, b, a]),
        #[cfg(feature = "f16")]
        PixelFormat::Abgr16f => each_pixel::<f16, 4>(i, o, |[a, b, g, r]| [r, g, b, a]),
        #[cfg(feature = "f16")]
        PixelFormat::Argb16f => each_pixel::<f16, 4>(i, o, |[a, r, g, b]| [r, g, b, a]),
        PixelFormat::Bgr32f => each_pixel::<f32, 3>(i, o, |[b, g, r]| [r, g, b, f32::ALPHA_OPAQUE]),
        PixelFormat::Rgb32f => each_pixel::<f32, 3>(i, o, |[r, g, b]| [r, g, b, f32::ALPHA_OPAQUE]),
        PixelFormat::Bgra32f => each_pixel::<f32, 4>(i, o, |[b, g, r, a]| [r, g, b, a]),
        PixelFormat::Rgba32f => each_pixel::<f32, 4>(i, o, |[r, g, b, a]| [r, g, b, a]),
        PixelFormat::Abgr32f => each_pixel::<f32, 4>(i, o, |[a, b, g, r]| [r, g, b, a]),
        PixelFormat::Argb32f => each_pixel::<f32, 4>(i, o, |[a, r, g, b]| [r, g, b, a]),
    }
}

pub(crate) const FALLBACK_FORMAT: PixelFormat =
    if cfg!(any(target_family = "wasm", target_os = "android")) {
        PixelFormat::Rgba8
    } else {
        PixelFormat::Bgra8
    };

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(C, align(4))]
pub(crate) struct FallbackPixel {
    #[cfg(not(any(target_family = "wasm", target_os = "android")))]
    b: u8,
    #[cfg(any(target_family = "wasm", target_os = "android"))]
    r: u8,

    g: u8,

    #[cfg(any(target_family = "wasm", target_os = "android"))]
    b: u8,
    #[cfg(not(any(target_family = "wasm", target_os = "android")))]
    r: u8,

    a: u8,
}

/// Convert the input data to a number of components with the given type `T`, loop over each pixel,
/// and convert it to RGBA using the provided closure.
///
/// This routine cannot be used when multiple pixels are packed into a single byte (such as for
/// [`PixelFormat::R2`]).
fn each_pixel_raw<T: PixelComponent, const COMPONENTS: usize>(
    i: Input<'_>,
    o: Output<'_>,
    convert_rgba: impl Fn([T; COMPONENTS]) -> [T; 4],
) {
    let input_rows = i.data.chunks_exact(i.byte_stride);
    let output_rows = o.data.chunks_exact_mut(o.stride);
    for (input_row, output_row) in input_rows.zip(output_rows) {
        // TODO: Make sure that the input stride is always multiple of the input pixel format's
        // bytes per pixel, such that we can do this cast before the loop.
        let input_row = T::cast(input_row);

        // Intentionally ignore trailing bytes, we might be working with stride-aligned rows where
        // there can be a few extra bytes at the end that shouldn't be treated as pixel values.
        let (input_row, _rest) = input_row.as_chunks::<COMPONENTS>();

        // The input should never be larger than the output, though the output is allowed to have
        // more pixels (in cases where it's stride-aligned, there can be leftover pixels at the end).
        debug_assert!(input_row.len() <= output_row.len());

        for (input_pixel, output_pixel) in input_row.iter().zip(output_row) {
            // Extract the components and convert alpha.
            let [r, g, b, a] = convert_rgba(*input_pixel);

            // Scale each pixel down to the native `u8` format that we're using.
            *output_pixel = FallbackPixel {
                r: r.scale(),
                g: g.scale(),
                b: b.scale(),
                a: a.scale(),
            };
        }
    }
}

/// Extends [`each_pixel_raw`] with alpha conversion.
fn each_pixel<T: PixelComponent, const COMPONENTS: usize>(
    i: Input<'_>,
    o: Output<'_>,
    convert_rgba: impl Fn([T; COMPONENTS]) -> [T; 4],
) {
    // Convert alpha.
    //
    // We do this before scaling, to make sure that components with higher resolution
    // than the target `u8` get scaled more precisely.
    //
    // NOTE: We monomorphize `each_pixel_raw` here depending on the required alpha conversion. This
    // is nice because it allows the compiler to autovectorize the inner conversion loop (probably),
    // though it does also cost a bit in terms of compile times and code size.
    //
    // TODO: Consider doing the alpha conversion in a separate pass afterwards, and then not worry
    // about the multiplication precision? That would reduce the code-size a fair bit.
    match (i.alpha_mode, o.alpha_mode) {
        // No conversion.
        (AlphaMode::Opaque, AlphaMode::Opaque)
        | (AlphaMode::PostMultiplied, AlphaMode::PostMultiplied)
        | (AlphaMode::PreMultiplied, AlphaMode::PreMultiplied) => {
            each_pixel_raw(i, o, convert_rgba)
        }
        // Convert opaque -> transparent (i.e. ignore alpha / x component).
        (AlphaMode::Opaque, AlphaMode::PostMultiplied | AlphaMode::PreMultiplied) => {
            each_pixel_raw(i, o, |pixel| {
                let [r, g, b, _a] = convert_rgba(pixel);
                [r, g, b, T::ALPHA_OPAQUE]
            });
        }
        (AlphaMode::PostMultiplied, AlphaMode::PreMultiplied) => {
            each_pixel_raw(i, o, |pixel| {
                let [r, g, b, a] = convert_rgba(pixel);
                [r.premultiply(a), g.premultiply(a), b.premultiply(a), a]
            });
        }
        (AlphaMode::PreMultiplied, AlphaMode::PostMultiplied) => {
            each_pixel_raw(i, o, |pixel| {
                let [r, g, b, a] = convert_rgba(pixel);
                [
                    r.unpremultiply(a),
                    g.unpremultiply(a),
                    b.unpremultiply(a),
                    a,
                ]
            });
        }
        (AlphaMode::PostMultiplied | AlphaMode::PreMultiplied, AlphaMode::Opaque) => {
            unreachable!("cannot convert alpha to opaque") // TODO
        }
    }
}

/// Helper on top of [`each_pixel`] for when working with packed formats.
fn each_packed<T: PixelComponent>(i: Input<'_>, o: Output<'_>, convert: impl Fn(T) -> [T; 4]) {
    each_pixel::<T, 1>(i, o, |[pixel]| convert(pixel));
}

/// Convert multiple grayscale pixels packed into a single byte.
///
/// Kind of a special case, multiple pixels are packed into a single byte.
fn each_bitpacked_grayscale<const BPP: u8>(
    i: Input<'_>,
    o: Output<'_>,
    convert: impl Fn(u8) -> [u8; 4],
) {
    // Assume pixels have a stride of at least 8 bits.
    let input_rows = i.data.chunks_exact(i.byte_stride);
    let output_rows = o.data.chunks_exact_mut(o.stride);

    for (input_row, output_row) in input_rows.zip(output_rows) {
        let input_row = input_row
            .iter()
            // Split multiple pixels packed into a single byte up into several bytes.
            .flat_map(|pixels| iter::repeat_n(pixels, 8 / BPP as usize).enumerate())
            // Extract the pixel from the byte.
            .map(|(i, v)| v.extract_packed(((8 / BPP - 1) - i as u8) * BPP, BPP));

        for (input_pixel, output_pixel) in input_row.zip(output_row) {
            let [r, g, b, a] = convert(input_pixel);
            *output_pixel = FallbackPixel { r, g, b, a };
        }
    }
}

/// A trait for representing the different kinds of data.
trait PixelComponent: Sized + Copy + std::fmt::Debug {
    const ALPHA_OPAQUE: Self;

    fn cast(data: &[u8]) -> &[Self];

    fn scale(self) -> u8;

    fn premultiply(self, alpha: Self) -> Self;

    fn unpremultiply(self, alpha: Self) -> Self;

    /// Extract part
    fn extract_packed(self, position: u8, size: u8) -> Self;
}

impl PixelComponent for u8 {
    const ALPHA_OPAQUE: Self = 0xff;

    fn cast(data: &[u8]) -> &[u8] {
        data
    }

    fn scale(self) -> u8 {
        self
    }

    fn premultiply(self, alpha: u8) -> Self {
        // TODO: Do we need to optimize this using bit shifts or similar?
        ((self as u16 * alpha as u16) / 0xff) as u8
    }

    fn unpremultiply(self, alpha: u8) -> Self {
        // TODO: Can we find a cleaner / more efficient way to implement this?
        (self as u16 * u8::MAX as u16)
            .checked_div(alpha as u16)
            .unwrap_or(0)
            .min(u8::MAX as u16) as u8
    }

    fn extract_packed(self, position: u8, size: u8) -> Self {
        // The maximum value of the component.
        //
        // size=1 => 0b1
        // size=2 => 0b11
        // size=4 => 0b1111
        let max = (1 << size) - 1;

        // Extract the value of the component.
        //
        // self=0b11001011, position=0, max=0b11 => 0b11
        // self=0b11001011, position=2, max=0b11 => 0b10
        // self=0b11001011, position=4, max=0b00 => 0b00
        let value = (self >> position) & max;

        // Expand the value to fill the entire u8.
        (value as u16 * u8::MAX as u16 / max as u16) as u8
    }
}

impl PixelComponent for u16 {
    const ALPHA_OPAQUE: Self = 0xffff;

    fn cast(data: &[u8]) -> &[u16] {
        // SAFETY: `u8`s can be safely reinterpreted as a `u16`.
        let ([], data, []) = (unsafe { data.align_to::<u16>() }) else {
            unreachable!("data was not properly aligned");
        };
        data
    }

    fn scale(self) -> u8 {
        // Grab the high bytes of the value.
        //
        // NOTE: This truncates instead of rounding, which is a bit imprecise, but it's probably
        // fine, we're going to be displaying the image right after.
        (self >> 8) as u8
    }

    fn premultiply(self, alpha: u16) -> Self {
        ((self as u32 * alpha as u32) / Self::MAX as u32) as u16
    }

    fn unpremultiply(self, alpha: u16) -> Self {
        (self as u32 * u16::MAX as u32)
            .checked_div(alpha as u32)
            .unwrap_or(0)
            .min(u16::MAX as u32) as u16
    }

    fn extract_packed(self, position: u8, size: u8) -> Self {
        let max = (1 << size) - 1;
        let value = (self >> position) & max;
        (value as u32 * u16::MAX as u32 / max as u32) as u16
    }
}

impl PixelComponent for u32 {
    const ALPHA_OPAQUE: Self = 0xffffffff;

    fn cast(data: &[u8]) -> &[u32] {
        // SAFETY: `u8`s can be safely reinterpreted as a `u32`.
        let ([], data, []) = (unsafe { data.align_to::<u32>() }) else {
            unreachable!("data was not properly aligned");
        };
        data
    }

    fn scale(self) -> u8 {
        (self >> 24) as u8
    }

    fn premultiply(self, alpha: u32) -> Self {
        ((self as u64 * alpha as u64) / Self::MAX as u64) as u32
    }

    fn unpremultiply(self, alpha: u32) -> Self {
        (self as u64 * u32::MAX as u64)
            .checked_div(alpha as u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    }

    fn extract_packed(self, position: u8, size: u8) -> Self {
        let max = (1 << size) - 1;
        let value = (self >> position) & max;
        (value as u64 * u32::MAX as u64 / max as u64) as u32
    }
}

#[cfg(feature = "f16")]
impl PixelComponent for f16 {
    const ALPHA_OPAQUE: Self = f16::from_f32_const(1.0);

    fn cast(data: &[u8]) -> &[f16] {
        // SAFETY: `u8`s can be safely reinterpreted as a `f16`.
        let ([], data, []) = (unsafe { data.align_to::<f16>() }) else {
            unreachable!("data was not properly aligned");
        };
        data
    }

    fn scale(self) -> u8 {
        let this: f32 = self.into();
        this.scale()
    }

    fn premultiply(self, alpha: f16) -> Self {
        self * alpha
    }

    fn unpremultiply(self, alpha: f16) -> Self {
        self / alpha
    }

    fn extract_packed(self, _position: u8, _size: u8) -> Self {
        unreachable!()
    }
}

impl PixelComponent for f32 {
    const ALPHA_OPAQUE: Self = 1.0;

    fn cast(data: &[u8]) -> &[f32] {
        // SAFETY: `u8`s can be safely reinterpreted as a `f32`.
        let ([], data, []) = (unsafe { data.align_to::<f32>() }) else {
            unreachable!("data was not properly aligned");
        };
        data
    }

    fn scale(self) -> u8 {
        (self * 255.0) as u8
    }

    fn premultiply(self, alpha: f32) -> Self {
        self * alpha
    }

    fn unpremultiply(self, alpha: f32) -> Self {
        self / alpha
    }

    fn extract_packed(self, _position: u8, _size: u8) -> Self {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_converts_to<T: PixelComponent, const N: usize>(
        input: &[[T; N]],
        expected: &[[u8; 4]],
        input_format: PixelFormat,
        input_alpha_mode: AlphaMode,
        output_alpha_mode: AlphaMode,
    ) {
        // SAFETY: All pixel components can be converted to `u8`.
        let ([], input_row, []) = (unsafe { input.align_to::<u8>() }) else {
            unreachable!()
        };
        let mut output_row = vec![
            FallbackPixel::default();
            input_row.len() * 8 / input_format.bits_per_pixel() as usize
        ];
        let input = Input {
            data: input_row,
            byte_stride: input_row.len(),
            alpha_mode: input_alpha_mode,
            format: input_format,
        };
        let output_stride = output_row.len();
        let output = Output {
            data: &mut output_row,
            stride: output_stride,
            alpha_mode: output_alpha_mode,
        };
        convert_fallback(input, output);

        let expected: Vec<FallbackPixel> = expected
            .iter()
            .cloned()
            .map(|[r, g, b, a]| FallbackPixel { r, g, b, a })
            .collect();
        assert_eq!(output_row, expected);
    }

    #[test]
    fn rgb_alpha() {
        let pixel_data: &[[u8; 3]] = &[[0x00, 0x11, 0x22], [0x33, 0x44, 0x55]];
        let expected = &[[0x00, 0x11, 0x22, 0xff], [0x33, 0x44, 0x55, 0xff]];
        let input_format = PixelFormat::Rgb8;

        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::Opaque,
            AlphaMode::Opaque,
        );

        // The alpha mode doesn't matter for opaque formats such as `Format::Rgb8`.
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::Opaque,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::PostMultiplied,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::PreMultiplied,
            AlphaMode::PostMultiplied,
        );
    }

    #[test]
    fn rgba_alpha() {
        // See the following link for expected values:
        // https://html.spec.whatwg.org/multipage/canvas.html#premultiplied-alpha-and-the-2d-rendering-context
        let pixel_data: &[[u8; 4]] = &[
            [0xff, 0x7f, 0x00, 0xff],
            [0xff, 0x7f, 0x00, 0x7f],
            [0xff, 0x7f, 0x00, 0x00],
        ];
        let input_format = PixelFormat::Rgba8;

        assert_converts_to(
            pixel_data,
            pixel_data,
            input_format,
            AlphaMode::Opaque,
            AlphaMode::Opaque,
        );
        assert_converts_to(
            pixel_data,
            // Opaque -> alpha means make alpha channel opaque.
            &[
                [0xff, 0x7f, 0x00, 0xff],
                [0xff, 0x7f, 0x00, 0xff],
                [0xff, 0x7f, 0x00, 0xff],
            ],
            input_format,
            AlphaMode::Opaque,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            &[
                [0xff, 0x7f, 0x00, 0xff], // Opaque -> Values aren't changed.
                [0x7f, 0x3f, 0x00, 0x7f], // Semi-transparent -> Values are scaled according to alpha.
                [0x00, 0x00, 0x00, 0x00], // Fully transparent -> Fully transparent black.
            ],
            input_format,
            AlphaMode::PostMultiplied,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            &[
                [0xff, 0x7f, 0x00, 0xff], // Opaque -> Values aren't changed.
                [0xff, 0xff, 0x00, 0x7f], // Semi-transparent -> Pixels whose value are larger than the alpha are unrepresentable.
                [0x00, 0x00, 0x00, 0x00], // Fully transparent -> Non-black pixels are unrepresentable.
            ],
            input_format,
            AlphaMode::PreMultiplied,
            AlphaMode::PostMultiplied,
        );
    }

    #[test]
    fn grayscale_alpha() {
        let pixel_data: &[[u8; 1]] = &[[0b11_00_11_00], [0b00_01_10_11]];
        let expected = &[
            [0xff, 0xff, 0xff, 0xff],
            [0x00, 0x00, 0x00, 0xff],
            [0xff, 0xff, 0xff, 0xff],
            [0x00, 0x00, 0x00, 0xff],
            [0x00, 0x00, 0x00, 0xff],
            [0x55, 0x55, 0x55, 0xff],
            [0xaa, 0xaa, 0xaa, 0xff],
            [0xff, 0xff, 0xff, 0xff],
        ];
        let input_format = PixelFormat::R2;

        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::Opaque,
            AlphaMode::Opaque,
        );

        // The alpha mode doesn't matter for grayscale formats.
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::Opaque,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::PostMultiplied,
            AlphaMode::PreMultiplied,
        );
        assert_converts_to(
            pixel_data,
            expected,
            input_format,
            AlphaMode::PreMultiplied,
            AlphaMode::PostMultiplied,
        );
    }

    /// Test a conversion in each format.
    #[test]
    fn all_formats() {
        #[track_caller]
        fn assert_convert<T: PixelComponent, const N: usize>(
            input: [T; N],
            expected: &[[u8; 4]],
            input_format: PixelFormat,
        ) {
            assert_converts_to(
                &[input],
                expected,
                input_format,
                // Same alpha mode.
                AlphaMode::PreMultiplied,
                AlphaMode::PreMultiplied,
            );
        }

        let expected = &[[0x22, 0x44, 0x66, 0xff]];
        assert_convert::<u8, _>([0x66, 0x44, 0x22], expected, PixelFormat::Bgr8);
        assert_convert::<u8, _>([0x22, 0x44, 0x66], expected, PixelFormat::Rgb8);
        assert_convert::<u8, _>([0x66, 0x44, 0x22, 0xff], expected, PixelFormat::Bgra8);
        assert_convert::<u8, _>([0x22, 0x44, 0x66, 0xff], expected, PixelFormat::Rgba8);
        assert_convert::<u8, _>([0xff, 0x66, 0x44, 0x22], expected, PixelFormat::Abgr8);
        assert_convert::<u8, _>([0xff, 0x22, 0x44, 0x66], expected, PixelFormat::Argb8);
        assert_convert::<u16, _>([0x6655, 0x4433, 0x2211], expected, PixelFormat::Bgr16);
        assert_convert::<u16, _>([0x2211, 0x4433, 0x6655], expected, PixelFormat::Rgb16);
        assert_convert::<u16, _>(
            [0x6655, 0x4433, 0x2211, 0xffee],
            expected,
            PixelFormat::Bgra16,
        );
        assert_convert::<u16, _>(
            [0x2211, 0x4433, 0x6655, 0xffee],
            expected,
            PixelFormat::Rgba16,
        );
        assert_convert::<u16, _>(
            [0xffee, 0x6655, 0x4433, 0x2211],
            expected,
            PixelFormat::Abgr16,
        );
        assert_convert::<u16, _>(
            [0xffee, 0x2211, 0x4433, 0x6655],
            expected,
            PixelFormat::Argb16,
        );

        assert_convert::<u8, 1>(
            [0b1_0_1_1_0_0_0_1],
            &[
                [0xff, 0xff, 0xff, 0xff],
                [0x00, 0x00, 0x00, 0xff],
                [0xff, 0xff, 0xff, 0xff],
                [0xff, 0xff, 0xff, 0xff],
                [0x00, 0x00, 0x00, 0xff],
                [0x00, 0x00, 0x00, 0xff],
                [0x00, 0x00, 0x00, 0xff],
                [0xff, 0xff, 0xff, 0xff],
            ],
            PixelFormat::R1,
        );
        assert_convert::<u8, 1>(
            [0b10_11_00_01],
            &[
                [0xaa, 0xaa, 0xaa, 0xff],
                [0xff, 0xff, 0xff, 0xff],
                [0x00, 0x00, 0x00, 0xff],
                [0x55, 0x55, 0x55, 0xff],
            ],
            PixelFormat::R2,
        );
        assert_convert::<u8, 1>(
            [0b1011_0001],
            &[[0xbb, 0xbb, 0xbb, 0xff], [0x11, 0x11, 0x11, 0xff]],
            PixelFormat::R4,
        );
        assert_convert::<u8, 1>([0x11], &[[0x11, 0x11, 0x11, 0xff]], PixelFormat::R8);
        assert_convert::<u16, 1>([0x11_22], &[[0x11, 0x11, 0x11, 0xff]], PixelFormat::R16);

        let expected = &[[0xb6, 0x48, 0xaa, 0xff]];
        assert_convert::<u8, 1>([0b10_010_101], expected, PixelFormat::B2g3r3);
        assert_convert::<u8, 1>([0b101_010_10], expected, PixelFormat::R3g3b2);
        let expected = &[[0xbd, 0xc7, 0xa5, 0xff]];
        assert_convert::<u16, 1>([0b10100_110001_10111], expected, PixelFormat::B5g6r5);
        assert_convert::<u16, 1>([0b10111_110001_10100], expected, PixelFormat::R5g6b5);
        let expected = &[[0x11, 0x22, 0x44, 0x88]];
        assert_convert::<u16, 1>([0b0100_0010_0001_1000], expected, PixelFormat::Bgra4);
        assert_convert::<u16, 1>([0b0001_0010_0100_1000], expected, PixelFormat::Rgba4);
        assert_convert::<u16, 1>([0b1000_0100_0010_0001], expected, PixelFormat::Abgr4);
        assert_convert::<u16, 1>([0b1000_0001_0010_0100], expected, PixelFormat::Argb4);
        let expected = &[[0xe7, 0x5a, 0xAd, 0xff]];
        assert_convert::<u16, 1>([0b10101_01011_11100_1], expected, PixelFormat::Bgr5a1);
        assert_convert::<u16, 1>([0b11100_01011_10101_1], expected, PixelFormat::Rgb5a1);
        assert_convert::<u16, 1>([0b1_10101_01011_11100], expected, PixelFormat::A1bgr5);
        assert_convert::<u16, 1>([0b1_11100_01011_10101], expected, PixelFormat::A1rgb5);
        let expected = &[[0xe3, 0x59, 0xAe, 0xaa]];
        assert_convert::<u32, 1>(
            [0b1010111011_0101100101_1110001111_10],
            expected,
            PixelFormat::Bgr10a2,
        );
        assert_convert::<u32, 1>(
            [0b1110001111_0101100101_1010111011_10],
            expected,
            PixelFormat::Rgb10a2,
        );
        assert_convert::<u32, 1>(
            [0b10_1010111011_0101100101_1110001111],
            expected,
            PixelFormat::A2bgr10,
        );
        assert_convert::<u32, 1>(
            [0b10_1110001111_0101100101_1010111011],
            expected,
            PixelFormat::A2rgb10,
        );

        // [0.3, 0.5, 0.7, 1.0]
        let expected = &[[0x4c, 0x7f, 0xb2, 0xff]];
        #[cfg(feature = "f16")]
        assert_convert::<f16, 3>(
            [0.7, 0.5, 0.3].map(f16::from_f32),
            expected,
            PixelFormat::Bgr16f,
        );
        #[cfg(feature = "f16")]
        assert_convert::<f16, 3>(
            [0.3, 0.5, 0.7].map(f16::from_f32),
            expected,
            PixelFormat::Rgb16f,
        );
        #[cfg(feature = "f16")]
        assert_convert::<f16, 4>(
            [0.7, 0.5, 0.3, 1.0].map(f16::from_f32),
            expected,
            PixelFormat::Bgra16f,
        );
        #[cfg(feature = "f16")]
        assert_convert::<f16, 4>(
            [0.3, 0.5, 0.7, 1.0].map(f16::from_f32),
            expected,
            PixelFormat::Rgba16f,
        );
        #[cfg(feature = "f16")]
        assert_convert::<f16, 4>(
            [1.0, 0.7, 0.5, 0.3].map(f16::from_f32),
            expected,
            PixelFormat::Abgr16f,
        );
        #[cfg(feature = "f16")]
        assert_convert::<f16, 4>(
            [1.0, 0.3, 0.5, 0.7].map(f16::from_f32),
            expected,
            PixelFormat::Argb16f,
        );
        assert_convert::<f32, 3>([0.7, 0.5, 0.3], expected, PixelFormat::Bgr32f);
        assert_convert::<f32, 3>([0.3, 0.5, 0.7], expected, PixelFormat::Rgb32f);
        assert_convert::<f32, 4>([0.7, 0.5, 0.3, 1.0], expected, PixelFormat::Bgra32f);
        assert_convert::<f32, 4>([0.3, 0.5, 0.7, 1.0], expected, PixelFormat::Rgba32f);
        assert_convert::<f32, 4>([1.0, 0.7, 0.5, 0.3], expected, PixelFormat::Abgr32f);
        assert_convert::<f32, 4>([1.0, 0.3, 0.5, 0.7], expected, PixelFormat::Argb32f);
    }

    #[test]
    fn stride() {}
}
