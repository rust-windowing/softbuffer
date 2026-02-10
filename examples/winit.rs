use softbuffer::{Context, Pixel, Surface};
use std::num::NonZeroU32;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

#[cfg(not(target_os = "android"))]
fn main() {
    util::setup();

    entry(EventLoop::new().unwrap())
}

pub(crate) fn entry(event_loop: EventLoop<()>) {
    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| util::make_window(elwt, |w| w),
        move |_elwt, window| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(|window, surface, window_id, event, elwt| {
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
