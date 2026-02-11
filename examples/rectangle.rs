use softbuffer::{Buffer, Context, Pixel, Surface};
use std::num::NonZeroU32;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn redraw(buffer: &mut Buffer<'_>, flag: bool) {
    let width = buffer.width().get();
    let height = buffer.height().get();
    for (x, y, pixel) in buffer.pixels_iter() {
        *pixel = if flag && x >= 100 && x < width - 100 && y >= 100 && y < height - 100 {
            Pixel::new_rgb(0xff, 0xff, 0xff)
        } else {
            let red = (x & 0xff) ^ (y & 0xff);
            let green = (x & 0x7f) ^ (y & 0x7f);
            let blue = (x & 0x3f) ^ (y & 0x3f);
            Pixel::new_rgb(red as u8, green as u8, blue as u8)
        };
    }
}

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| {
            let window = util::make_window(elwt, |w| {
                w.with_title("Press space to show/hide a rectangle")
            });

            let flag = false;

            (window, flag)
        },
        move |_elwt, (window, _flag)| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(|state, surface, window_id, event, elwt| {
        let (window, flag) = state;

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
                    // Resize surface
                    surface.resize(width, height).unwrap();
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    tracing::error!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };
                // Draw something in the window
                let mut buffer = surface.next_buffer().unwrap();
                redraw(&mut buffer, *flag);
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

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: Key::Named(NamedKey::Space),
                        ..
                    },
                ..
            } => {
                // Flip the rectangle flag and request a redraw to show the changed image
                *flag = !*flag;
                window.request_redraw();
            }

            _ => {}
        }
    });

    util::run_app(event_loop, app);
}
