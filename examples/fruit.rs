use image::GenericImageView;
use std::num::NonZeroU32;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

fn main() {
    //see fruit.jpg.license for the license of fruit.jpg
    let fruit = image::load_from_memory(include_bytes!("fruit.jpg")).unwrap();
    let (width, height) = (fruit.width(), fruit.height());

    let event_loop = EventLoop::new().unwrap();
    let context = softbuffer::Context::new(event_loop.owned_display_handle()).unwrap();

    let app = winit_app::WinitAppBuilder::with_init(
        move |elwt| {
            winit_app::make_window(elwt, |w| {
                w.with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            })
        },
        move |_elwt, window| {
            let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
            // Intentionally only set the size of the surface once, at creation.
            // This is needed if the window chooses to ignore the size we passed in above, and for the
            // platforms softbuffer supports that don't yet extract the size from the window.
            surface
                .resize(
                    NonZeroU32::new(width).unwrap(),
                    NonZeroU32::new(height).unwrap(),
                )
                .unwrap();
            surface
        },
    )
    .with_event_handler(move |window, surface, window_id, event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    eprintln!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                let mut buffer = surface.buffer_mut().unwrap();
                let width = fruit.width() as usize;
                for (x, y, pixel) in fruit.pixels() {
                    let red = pixel.0[0] as u32;
                    let green = pixel.0[1] as u32;
                    let blue = pixel.0[2] as u32;

                    let color = blue | (green << 8) | (red << 16);
                    buffer[y as usize * width + x as usize] = color;
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
