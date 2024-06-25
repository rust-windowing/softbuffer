use std::num::NonZeroU32;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let app = winit_app::WinitAppBuilder::with_init(|elwt| {
        let window = winit_app::make_window(elwt, |w| w);

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        (window, surface)
    })
    .with_event_handler(|state, event, elwt| {
        let (window, surface) = state;
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent {
                window_id,
                event: WindowEvent::RedrawRequested,
            } if window_id == window.id() => {
                if let (Some(width), Some(height)) = {
                    let size = window.inner_size();
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                } {
                    surface.resize(width, height).unwrap();

                    let mut buffer = surface.buffer_mut().unwrap();
                    for y in 0..height.get() {
                        for x in 0..width.get() {
                            let red = x % 255;
                            let green = y % 255;
                            let blue = (x * y) % 255;
                            let index = y as usize * width.get() as usize + x as usize;
                            buffer[index] = blue | (green << 8) | (red << 16);
                        }
                    }

                    buffer.present().unwrap();
                }
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
