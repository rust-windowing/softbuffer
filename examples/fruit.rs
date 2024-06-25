use image::GenericImageView;
use std::num::NonZeroU32;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

fn main() {
    //see fruit.jpg.license for the license of fruit.jpg
    let fruit = image::load_from_memory(include_bytes!("fruit.jpg")).unwrap();
    let (width, height) = (fruit.width(), fruit.height());

    let event_loop = EventLoop::new().unwrap();

    let app = winit_app::WinitAppBuilder::with_init(move |elwt| {
        let window = winit_app::make_window(elwt, |w| {
            w.with_inner_size(winit::dpi::PhysicalSize::new(width, height))
        });

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        (window, surface)
    })
    .with_event_handler(move |state, event, elwt| {
        let (window, surface) = state;
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent {
                window_id,
                event: WindowEvent::RedrawRequested,
            } if window_id == window.id() => {
                surface
                    .resize(
                        NonZeroU32::new(fruit.width()).unwrap(),
                        NonZeroU32::new(fruit.height()).unwrap(),
                    )
                    .unwrap();

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
