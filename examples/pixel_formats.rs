//! An example to test rendering with different pixel formats.
//!
//! Press `1`, `2`, `3` or `4` to change the alpha mode to `Opaque`, `Ignored`, `Premultiplied` and
//! `Postmultiplied` respectively.
//!
//! The expected output is the same as in [the `transparency` example](./transparency.rs), though
//! sometimes with less color fidelity (because the pixel mode doesn't allow it).
use half::f16;
use softbuffer::{AlphaMode, Context, PixelFormat, Surface};
use std::num::NonZeroU32;
use std::ops::{BitOr, Shl};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();

    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let mut format_index = 0;
    let mut alpha_mode = AlphaMode::default();

    let app = util::WinitAppBuilder::with_init(
        |elwt| util::make_window(elwt, |w| w),
        move |_elwt, window| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(move |window, surface, window_id, event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                let Some(surface) = surface else {
                    tracing::error!("Resized fired before Resumed or after Suspended");
                    return;
                };

                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    surface.resize(width, height).unwrap();
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    tracing::error!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                tracing::info!(pixel_format = ?surface.pixel_format(), "redraw");

                let mut buffer = surface.next_buffer().unwrap();

                let width = buffer.width().get() as usize;
                let pixel_format = buffer.pixel_format();
                let bpp = pixel_format.bits_per_pixel() as usize;

                let byte_stride = buffer.byte_stride().get() as usize;
                let pixels = buffer.pixels();
                // SAFETY: `Pixel` can be reinterpreted as 4 `u8`s.
                let data_u8: &mut [u8] = unsafe {
                    std::slice::from_raw_parts_mut(
                        pixels.as_mut_ptr().cast::<u8>(),
                        pixels.len() * 4,
                    )
                };

                let split = (width / 6) * bpp / 8;
                let required_a = if alpha_mode == AlphaMode::Opaque {
                    1.0
                } else {
                    0.0
                };
                for row in data_u8.chunks_mut(byte_stride) {
                    let (left, row) = row.split_at_mut(split);
                    fill(left, pixel_format, 1.0, 0.5, 0.0, 1.0f32.max(required_a));
                    let (left, row) = row.split_at_mut(split);
                    fill(left, pixel_format, 1.0, 1.0, 0.0, 0.5f32.max(required_a));
                    let (left, row) = row.split_at_mut(split);
                    fill(left, pixel_format, 0.5, 0.5, 0.0, 0.5f32.max(required_a));
                    let (left, row) = row.split_at_mut(split);
                    fill(left, pixel_format, 1.0, 0.5, 0.0, 0.5f32.max(required_a));
                    let (left, row) = row.split_at_mut(split);
                    fill(left, pixel_format, 1.0, 0.5, 0.0, 0.0f32.max(required_a));
                    fill(row, pixel_format, 0.0, 0.0, 0.0, 0.0f32.max(required_a));
                }

                buffer.present().unwrap();
            }
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        repeat: false,
                        ..
                    },
                ..
            } => {
                elwt.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        repeat: false,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let Some(surface) = surface else {
                    tracing::error!("KeyboardInput fired before Resumed or after Suspended");
                    return;
                };

                match logical_key.to_text() {
                    Some("1") => alpha_mode = AlphaMode::Opaque,
                    Some("2") => alpha_mode = AlphaMode::Ignored,
                    Some("3") => alpha_mode = AlphaMode::Premultiplied,
                    Some("4") => alpha_mode = AlphaMode::Postmultiplied,
                    Some("n") => format_index += 1,
                    Some("r") => format_index = 0,
                    _ => return,
                }

                if ALL_FORMATS.len() <= format_index {
                    format_index = 0;
                }
                let pixel_format = ALL_FORMATS[format_index];

                if !surface
                    .supported_pixel_formats(alpha_mode)
                    .contains(&pixel_format)
                {
                    tracing::warn!(?alpha_mode, ?pixel_format, "not supported by the backend");
                    return;
                }

                tracing::info!(?alpha_mode, ?pixel_format, "configure");
                let size = window.inner_size();
                let width = NonZeroU32::new(size.width).unwrap();
                let height = NonZeroU32::new(size.height).unwrap();
                surface
                    .configure(width, height, alpha_mode, pixel_format)
                    .unwrap();
                assert_eq!(surface.alpha_mode(), alpha_mode);
                assert_eq!(surface.pixel_format(), pixel_format);

                window.set_transparent(matches!(
                    alpha_mode,
                    AlphaMode::Premultiplied | AlphaMode::Postmultiplied
                ));

                window.request_redraw();
            }
            _ => {}
        }
    });

    util::run_app(event_loop, app);
}

/// Fill a single row of data with the given red, green and blue pixel, in the given format.
fn fill(row: &mut [u8], format: PixelFormat, r: f32, g: f32, b: f32, a: f32) {
    let l = (r + g + b) / 3.0; // Very simple lightness calculation
    match format {
        PixelFormat::Bgr8 => each_pixel::<u8>(row, &[b, g, r]),
        PixelFormat::Rgb8 => each_pixel::<u8>(row, &[r, g, b]),
        PixelFormat::Bgra8 => each_pixel::<u8>(row, &[b, g, r, a]),
        PixelFormat::Rgba8 => each_pixel::<u8>(row, &[r, g, b, a]),
        PixelFormat::Abgr8 => each_pixel::<u8>(row, &[a, b, g, r]),
        PixelFormat::Argb8 => each_pixel::<u8>(row, &[a, r, g, b]),
        PixelFormat::Bgr16 => each_pixel::<u16>(row, &[b, g, r]),
        PixelFormat::Rgb16 => each_pixel::<u16>(row, &[r, g, b]),
        PixelFormat::Bgra16 => each_pixel::<u16>(row, &[b, g, r, a]),
        PixelFormat::Rgba16 => each_pixel::<u16>(row, &[r, g, b, a]),
        PixelFormat::Abgr16 => each_pixel::<u16>(row, &[a, b, g, r]),
        PixelFormat::Argb16 => each_pixel::<u16>(row, &[a, r, g, b]),

        // Grayscale formats.
        PixelFormat::R1 => each_bitpacked_grayscale::<1>(row, l),
        PixelFormat::R2 => each_bitpacked_grayscale::<2>(row, l),
        PixelFormat::R4 => each_bitpacked_grayscale::<4>(row, l),
        PixelFormat::R8 => each_pixel::<u8>(row, &[l]),
        PixelFormat::R16 => each_pixel::<u16>(row, &[l]),

        // Packed formats.
        PixelFormat::B2g3r3 => each_packed::<u8>(row, &[(b, 2), (g, 3), (r, 3)]),
        PixelFormat::R3g3b2 => each_packed::<u8>(row, &[(r, 3), (g, 3), (b, 2)]),

        PixelFormat::B5g6r5 => each_packed::<u16>(row, &[(b, 5), (g, 6), (r, 5)]),
        PixelFormat::R5g6b5 => each_packed::<u16>(row, &[(r, 5), (g, 6), (b, 5)]),

        PixelFormat::Bgra4 => each_packed::<u16>(row, &[(b, 4), (g, 4), (r, 4), (a, 4)]),
        PixelFormat::Rgba4 => each_packed::<u16>(row, &[(r, 4), (g, 4), (b, 4), (a, 4)]),
        PixelFormat::Abgr4 => each_packed::<u16>(row, &[(a, 4), (b, 4), (g, 4), (r, 4)]),
        PixelFormat::Argb4 => each_packed::<u16>(row, &[(a, 4), (r, 4), (g, 4), (b, 4)]),

        PixelFormat::Bgr5a1 => each_packed::<u16>(row, &[(b, 5), (g, 5), (r, 5), (a, 1)]),
        PixelFormat::Rgb5a1 => each_packed::<u16>(row, &[(r, 5), (g, 5), (b, 5), (a, 1)]),
        PixelFormat::A1bgr5 => each_packed::<u16>(row, &[(a, 1), (b, 5), (g, 5), (r, 5)]),
        PixelFormat::A1rgb5 => each_packed::<u16>(row, &[(a, 1), (r, 5), (g, 5), (b, 5)]),

        PixelFormat::Bgr10a2 => each_packed::<u32>(row, &[(b, 10), (g, 10), (r, 10), (a, 2)]),
        PixelFormat::Rgb10a2 => each_packed::<u32>(row, &[(r, 10), (g, 10), (b, 10), (a, 2)]),
        PixelFormat::A2bgr10 => each_packed::<u32>(row, &[(a, 2), (b, 10), (g, 10), (r, 10)]),
        PixelFormat::A2rgb10 => each_packed::<u32>(row, &[(a, 2), (r, 10), (g, 10), (b, 10)]),

        // Floating point formats.
        PixelFormat::Bgr16f => each_pixel::<f16>(row, &[b, g, r]),
        PixelFormat::Rgb16f => each_pixel::<f16>(row, &[r, g, b]),
        PixelFormat::Bgra16f => each_pixel::<f16>(row, &[b, g, r, a]),
        PixelFormat::Rgba16f => each_pixel::<f16>(row, &[r, g, b, a]),
        PixelFormat::Abgr16f => each_pixel::<f16>(row, &[a, b, g, r]),
        PixelFormat::Argb16f => each_pixel::<f16>(row, &[a, r, g, b]),
        PixelFormat::Bgr32f => each_pixel::<f32>(row, &[b, g, r]),
        PixelFormat::Rgb32f => each_pixel::<f32>(row, &[r, g, b]),
        PixelFormat::Bgra32f => each_pixel::<f32>(row, &[b, g, r, a]),
        PixelFormat::Rgba32f => each_pixel::<f32>(row, &[r, g, b, a]),
        PixelFormat::Abgr32f => each_pixel::<f32>(row, &[a, b, g, r]),
        PixelFormat::Argb32f => each_pixel::<f32>(row, &[a, r, g, b]),

        _ => unimplemented!(),
    }
}

fn each_pixel<T: Component>(row: &mut [u8], components: &[f32]) {
    // SAFETY: `u8`s can be re-interpreted as `Component`.
    let ([], row, []) = (unsafe { row.align_to_mut::<T>() }) else {
        unreachable!("row was not properly aligned");
    };

    for pixel in row.chunks_mut(components.len()) {
        for (out_component, component) in pixel.iter_mut().zip(components) {
            *out_component = T::from_f32(*component);
        }
    }
}

/// Fill packed format data.
fn each_packed<T: Component + Shl<u8, Output = T> + BitOr<T, Output = T>>(
    row: &mut [u8],
    packed_components: &[(f32, u8)],
) {
    // Pack components into a single pixel of type `T`.
    let mut pixel = T::ZERO;
    let mut shift = 0;
    for (component, size) in packed_components.iter().rev() {
        let q = T::quantize(*component, *size);
        pixel = pixel | q << shift;
        shift += size;
    }

    // SAFETY: `u8`s can be re-interpreted as `Component`.
    // We allow there to be leftover data at the end.
    let ([], row, _) = (unsafe { row.align_to_mut::<T>() }) else {
        unreachable!("row was not properly aligned");
    };

    row.fill(pixel);
}

/// Convert multiple grayscale pixels packed into a single byte. Kind of a special case.
fn each_bitpacked_grayscale<const BPP: u8>(row: &mut [u8], component: f32) {
    let mut pixel = 0;
    for shift in (0..8).step_by(BPP as usize) {
        let q = u8::quantize(component, BPP);
        pixel |= q << shift;
    }

    row.fill(pixel);
}

/// A trait for representing the different kinds of pixel data components.
///
/// # Safety
///
/// Must be able to be reinterpreted from `u8`s.
unsafe trait Component: Sized + Copy + std::fmt::Debug + PartialEq {
    const ZERO: Self;

    fn from_f32(component: f32) -> Self;

    fn quantize(component: f32, size: u8) -> Self;
}

unsafe impl Component for u8 {
    const ZERO: Self = 0;

    fn from_f32(component: f32) -> Self {
        (component * u8::MAX as f32) as u8
    }

    fn quantize(component: f32, size: u8) -> Self {
        let max = (1 << size) - 1;
        (component * max as f32).round() as Self
    }
}

unsafe impl Component for u16 {
    const ZERO: Self = 0;

    fn from_f32(component: f32) -> Self {
        (component * u16::MAX as f32) as u16
    }

    fn quantize(component: f32, size: u8) -> Self {
        let max = (1 << size) - 1;
        (component * max as f32).round() as Self
    }
}

unsafe impl Component for u32 {
    const ZERO: Self = 0;

    fn from_f32(component: f32) -> Self {
        (component * u32::MAX as f32) as u32
    }

    fn quantize(component: f32, size: u8) -> Self {
        let max = (1 << size) - 1;
        (component * max as f32).round() as Self
    }
}

unsafe impl Component for f16 {
    const ZERO: Self = f16::from_f32_const(0.0);

    fn from_f32(component: f32) -> Self {
        f16::from_f32(component)
    }

    fn quantize(_component: f32, _size: u8) -> Self {
        unimplemented!()
    }
}

unsafe impl Component for f32 {
    const ZERO: Self = 0.0;

    fn from_f32(component: f32) -> Self {
        component
    }

    fn quantize(_component: f32, _size: u8) -> Self {
        unimplemented!()
    }
}

const ALL_FORMATS: &[PixelFormat] = &[
    PixelFormat::Bgr8,
    PixelFormat::Rgb8,
    PixelFormat::Bgra8,
    PixelFormat::Rgba8,
    PixelFormat::Abgr8,
    PixelFormat::Argb8,
    PixelFormat::Bgr16,
    PixelFormat::Rgb16,
    PixelFormat::Bgra16,
    PixelFormat::Rgba16,
    PixelFormat::Abgr16,
    PixelFormat::Argb16,
    // Grayscale formats.
    PixelFormat::R1,
    PixelFormat::R2,
    PixelFormat::R4,
    PixelFormat::R8,
    PixelFormat::R16,
    // Packed formats.
    PixelFormat::B2g3r3,
    PixelFormat::R3g3b2,
    PixelFormat::B5g6r5,
    PixelFormat::R5g6b5,
    PixelFormat::Bgra4,
    PixelFormat::Rgba4,
    PixelFormat::Abgr4,
    PixelFormat::Argb4,
    PixelFormat::Bgr5a1,
    PixelFormat::Rgb5a1,
    PixelFormat::A1bgr5,
    PixelFormat::A1rgb5,
    PixelFormat::Bgr10a2,
    PixelFormat::Rgb10a2,
    PixelFormat::A2bgr10,
    PixelFormat::A2rgb10,
    // Floating point formats.
    PixelFormat::Bgr16f,
    PixelFormat::Rgb16f,
    PixelFormat::Bgra16f,
    PixelFormat::Rgba16f,
    PixelFormat::Abgr16f,
    PixelFormat::Argb16f,
    PixelFormat::Bgr32f,
    PixelFormat::Rgb32f,
    PixelFormat::Bgra32f,
    PixelFormat::Rgba32f,
    PixelFormat::Abgr32f,
    PixelFormat::Argb32f,
];
