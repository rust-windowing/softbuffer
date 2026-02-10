use softbuffer::{Context, Pixel, Surface};
use std::num::NonZeroU32;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| util::make_window(elwt, |w| w),
        move |_elwt, window| {
            let mut surface = Surface::new(&context, window.clone()).unwrap();
            // Intentionally set the size of the surface to something different than the size of the window.
            surface
                .resize(NonZeroU32::new(256).unwrap(), NonZeroU32::new(128).unwrap())
                .unwrap();
            surface
        },
    )
    .with_event_handler(|window, surface, window_id, event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    tracing::warn!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let mut buffer = surface.next_buffer().unwrap();
                for (x, y, pixel) in buffer.pixels_iter() {
                    let red = x % 255;
                    let green = y % 255;
                    let blue = (x * y) % 255;
                    *pixel = Pixel::new_rgb(red as u8, green as u8, blue as u8);
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

    util::run_app(event_loop, app);
}
