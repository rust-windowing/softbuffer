#[cfg(not(target_family = "wasm"))]
use rayon::prelude::*;
use softbuffer::{Context, Pixel, Surface};
use std::f64::consts::PI;
use std::num::NonZeroU32;
use web_time::Instant;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let start = Instant::now();

    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |event_loop| {
            let window = util::make_window(event_loop, |w| w);

            let old_size = (0, 0);
            let frames = pre_render_frames(0, 0, 0);

            (window, old_size, frames)
        },
        move |_elwft, (window, _old_size, _frames)| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(move |state, surface, window_id, event, elwt| {
        let (window, old_size, frames) = state;

        elwt.set_control_flow(ControlFlow::Poll);

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

                let elapsed = start.elapsed().as_secs_f64() % 1.0;

                let mut buffer = surface.next_buffer().unwrap();

                let size = (buffer.width().get(), buffer.height().get());
                if size != *old_size {
                    *old_size = size;
                    *frames = pre_render_frames(buffer.byte_stride().get() / 4, size.0, size.1);
                }

                let frame = &frames[((elapsed * 60.0).round() as usize).clamp(0, 59)];

                buffer.pixels().copy_from_slice(frame);
                buffer.present().unwrap();
            }
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        ..
                    },
                ..
            } => {
                elwt.exit();
            }
            _ => {}
        }
    })
    .with_about_to_wait_handler(|state, _, _| {
        let (window, _, _) = state;
        window.request_redraw();
    });

    util::run_app(event_loop, app);
}

fn pre_render_frames(stride: u32, width: u32, height: u32) -> Vec<Vec<Pixel>> {
    let render = |frame_id| {
        let elapsed = ((frame_id as f64) / (60.0)) * 2.0 * PI;

        let coords = (0..height).flat_map(|x| (0..stride).map(move |y| (x, y)));
        coords
            .map(|(x, y)| {
                let y = (y as f64) / (height as f64);
                let x = (x as f64) / (width as f64);
                let r = ((((y + elapsed).sin() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);
                let g = ((((x + elapsed).sin() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);
                let b = ((((y - elapsed).cos() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);

                Pixel::new_rgb(r as u8, g as u8, b as u8)
            })
            .collect::<Vec<_>>()
    };

    #[cfg(target_family = "wasm")]
    return (0..60).map(render).collect();

    #[cfg(not(target_family = "wasm"))]
    (0..60).into_par_iter().map(render).collect()
}
