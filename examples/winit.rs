use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

use winit::window::WindowBuilder;

#[cfg(not(target_os = "android"))]
fn main() {
    run(EventLoop::new().unwrap())
}

pub(crate) fn run(event_loop: EventLoop<()>) {
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

    let mut state = None;

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);

            match event {
                Event::Resumed => {
                    let context = softbuffer::Context::new(window.clone()).unwrap();
                    let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
                    state = Some((context, surface));
                }
                Event::Suspended => {
                    state = None;
                }
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::RedrawRequested,
                } if window_id == window.id() => {
                    let Some((_, surface)) = state.as_mut() else {
                        eprintln!("RedrawRequested fired before Resumed or after Suspended");
                        return;
                    };
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
                                let index = y as usize * buffer.stride() as usize + x as usize;
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
        })
        .unwrap();
}
