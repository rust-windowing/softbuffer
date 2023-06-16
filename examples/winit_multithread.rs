//! `Surface` implements `Send`. This makes sure that multithreading can work here.

#[cfg(not(target_family = "wasm"))]
#[path = "utils/winit_app.rs"]
mod winit_app;

#[cfg(not(target_family = "wasm"))]
mod ex {
    use std::num::NonZeroU32;
    use std::sync::{mpsc, Arc, Mutex};
    use winit::event::{Event, KeyEvent, WindowEvent};
    use winit::event_loop::{ControlFlow, EventLoop};
    use winit::keyboard::{Key, NamedKey};
    use winit::window::Window;

    use super::winit_app;

    type Surface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

    fn render_thread(
        window: Arc<Window>,
        surface: Arc<Mutex<Surface>>,
        do_render: mpsc::Receiver<()>,
        done: mpsc::Sender<()>,
    ) {
        loop {
            println!("waiting for render...");
            if do_render.recv().is_err() {
                println!("surface state destroyed");
                break;
            }

            // Perform the rendering.
            let mut surface = surface.lock().unwrap();
            if let (Some(width), Some(height)) = {
                let size = window.inner_size();
                println!("got size: {size:?}");
                (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            } {
                println!("resizing...");
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

                println!("presenting...");
                buffer.present().unwrap();
            }

            // We're done, tell the main thread to keep going.
            done.send(()).ok();
        }
    }

    pub(super) fn entry() {
        let event_loop = EventLoop::new().unwrap();

        let app = winit_app::WinitAppBuilder::with_init(
            |elwt| {
                let attributes = Window::default_attributes();
                #[cfg(target_arch = "wasm32")]
                let attributes =
                    winit::platform::web::WindowAttributesExtWebSys::with_append(attributes, true);
                let window = Arc::new(elwt.create_window(attributes).unwrap());

                let context = softbuffer::Context::new(window.clone()).unwrap();

                (window, context)
            },
            |_elwt, (window, context)| {
                let surface = {
                    println!("making surface...");
                    let surface = softbuffer::Surface::new(context, window.clone()).unwrap();
                    Arc::new(Mutex::new(surface))
                };

                // Spawn a thread to handle rendering for this specific surface. The channels will
                // be closed and the thread will be stopped whenever this surface (the returned
                // context below) is dropped, so that it can all be recreated again (on Android)
                // when a new surface is created.
                let (start_render, do_render) = mpsc::channel();
                let (render_done, finish_render) = mpsc::channel();
                println!("starting thread...");
                std::thread::spawn({
                    let window = window.clone();
                    let surface = surface.clone();
                    move || render_thread(window, surface, do_render, render_done)
                });

                (surface, start_render, finish_render)
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
                    let Some((_surface, start_render, finish_render)) = surface else {
                        eprintln!("RedrawRequested fired before Resumed or after Suspended");
                        return;
                    };
                    // Start the render and then finish it.
                    start_render.send(()).unwrap();
                    finish_render.recv().unwrap();
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
}

#[cfg(target_family = "wasm")]
mod ex {
    pub(crate) fn entry() {
        eprintln!("winit_multithreaded doesn't work on WASM");
    }
}

fn main() {
    ex::entry();
}
