use std::num::NonZeroU32;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let app = winit_app::WinitAppBuilder::with_init(
        |elwt| {
            let window = winit_app::make_window(elwt, |w| w);

            let context = softbuffer::Context::new(window.clone()).unwrap();

            (window, context)
        },
        |_elwt, (window, context)| {
            let mut surface = softbuffer::Surface::new(context, window.clone()).unwrap();
            // Intentionally set the size of the surface to something different than the size of the window.
            surface
                .resize(NonZeroU32::new(256).unwrap(), NonZeroU32::new(128).unwrap())
                .unwrap();
            surface
        },
    )
    .with_event_handler(|state, surface, event, elwt| {
        let (window, _context) = state;
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent {
                window_id,
                event: WindowEvent::RedrawRequested,
            } if window_id == window.id() => {
                let Some(surface) = surface else {
                    eprintln!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let mut buffer = surface.buffer_mut().unwrap();
                let width = buffer.width();
                for y in 0..buffer.height() {
                    for x in 0..width {
                        let red = x as u32 % 255;
                        let green = y as u32 % 255;
                        let blue = (x as u32 * y as u32) % 255;

                        let color = blue | (green << 8) | (red << 16);
                        buffer[y * width + x] = color;
                    }
                }
                buffer.present().unwrap();
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
    });

    winit_app::run_app(event_loop, app);
}
