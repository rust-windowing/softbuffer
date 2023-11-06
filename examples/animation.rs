use instant::Instant;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use std::f64::consts::PI;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowBuilder;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new().build(&event_loop).unwrap());

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;

        web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .body()
            .unwrap()
            .append_child(&window.canvas().unwrap())
            .unwrap();
    }

    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    let mut old_size = (0, 0);
    let mut frames = pre_render_frames(0, 0);

    let start = Instant::now();
    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::RedrawRequested,
                } if window_id == window.id() => {
                    if let (Some(width), Some(height)) = {
                        let size = window.inner_size();
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    } {
                        let elapsed = start.elapsed().as_secs_f64() % 1.0;

                        if (width.get(), height.get()) != old_size {
                            old_size = (width.get(), height.get());
                            frames = pre_render_frames(width.get() as usize, height.get() as usize);
                        };

                        let frame = &frames[((elapsed * 60.0).round() as usize).clamp(0, 59)];

                        surface.resize(width, height).unwrap();
                        let mut buffer = surface.buffer_mut().unwrap();
                        buffer.copy_from_slice(frame);
                        buffer.present().unwrap();
                    }
                }
                Event::AboutToWait => {
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    logical_key: Key::Named(NamedKey::Escape),
                                    ..
                                },
                            ..
                        },
                    window_id,
                } if window_id == window.id() => {
                    elwt.exit();
                }
                _ => {}
            }
        })
        .unwrap();
}

fn pre_render_frames(width: usize, height: usize) -> Vec<Vec<u32>> {
    let render = |frame_id| {
        let elapsed = ((frame_id as f64) / (60.0)) * 2.0 * PI;

        let coords = (0..height).flat_map(|x| (0..width).map(move |y| (x, y)));
        coords
            .map(|(x, y)| {
                let y = (y as f64) / (height as f64);
                let x = (x as f64) / (width as f64);
                let red =
                    ((((y + elapsed).sin() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);
                let green =
                    ((((x + elapsed).sin() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);
                let blue =
                    ((((y - elapsed).cos() * 0.5 + 0.5) * 255.0).round() as u32).clamp(0, 255);

                blue | (green << 8) | (red << 16)
            })
            .collect::<Vec<_>>()
    };

    #[cfg(target_arch = "wasm32")]
    return (0..60).map(render).collect();

    #[cfg(not(target_arch = "wasm32"))]
    (0..60).into_par_iter().map(render).collect()
}
