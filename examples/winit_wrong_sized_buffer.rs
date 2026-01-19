//! A window with a surface that is 200 pixels less wide and 400 pixels less tall.
//!
//! This is useful for testing that zero-sized buffers work, as well as testing buffer vs. window
//! size discrepancies in general.
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let context = softbuffer::Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| util::make_window(elwt, |w| w),
        move |_elwt, window| {
            let size = window.inner_size();

            let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

            let width = size.width.saturating_sub(200);
            let height = size.height.saturating_sub(400);
            tracing::info!("size initially at: {width}/{height}");
            surface.resize(width, height).unwrap();
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
                    tracing::warn!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let width = size.width.saturating_sub(200);
                let height = size.height.saturating_sub(400);
                tracing::info!("resized to: {width}/{height}");
                surface.resize(width, height).unwrap();
            }
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    tracing::warn!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let mut buffer = surface.buffer_mut().unwrap();
                for (x, y, pixel) in buffer.pixels_iter() {
                    let red = x % 255;
                    let green = y % 255;
                    let blue = (x * y) % 255;
                    *pixel = blue | (green << 8) | (red << 16);
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
