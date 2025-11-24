use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let context = softbuffer::Context::new(event_loop.owned_display_handle()).unwrap();

    let app = winit_app::WinitAppBuilder::with_init(
        |elwt| winit_app::make_window(elwt, |w| w),
        move |_elwt, window| {
            let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
            surface.resize(0, 0).unwrap();
            surface
        },
    )
    .with_event_handler(|window, surface, window_id, event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                let Some(surface) = surface else {
                    eprintln!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let width = size.width.saturating_sub(100);
                let height = size.height.saturating_sub(100);
                surface.resize(width, height).unwrap();
            }
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    eprintln!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let mut buffer = surface.buffer_mut().unwrap();
                for y in 0..buffer.height() {
                    for x in 0..buffer.width() {
                        let red = x % 255;
                        let green = y % 255;
                        let blue = (x * y) % 255;
                        let index = y * buffer.width() + x;
                        buffer[index as usize] = blue | (green << 8) | (red << 16);
                    }
                }

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
    });

    winit_app::run_app(event_loop, app);
}
