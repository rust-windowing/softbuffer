//! `Surface` implements `Send`. This makes sure that multithreading can work here.

mod util;

#[cfg(not(target_family = "wasm"))]
pub mod ex {
    use softbuffer::{Context, Pixel};
    use std::num::NonZeroU32;
    use std::sync::{mpsc, Arc, Mutex};
    use winit::event::{KeyEvent, WindowEvent};
    use winit::event_loop::{ControlFlow, EventLoop, OwnedDisplayHandle};
    use winit::keyboard::{Key, NamedKey};
    use winit::window::Window;

    use super::util;

    type Surface = softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>;

    fn render_thread(
        do_render: mpsc::Receiver<(Arc<Mutex<Surface>>, NonZeroU32, NonZeroU32)>,
        done: mpsc::Sender<()>,
    ) {
        loop {
            tracing::info!("waiting for render...");
            let Ok((surface, width, height)) = do_render.recv() else {
                tracing::info!("main thread destroyed");
                break;
            };

            // Perform the rendering.
            let mut surface = surface.lock().unwrap();
            tracing::info!("resizing...");
            surface.resize(width, height).unwrap();

            let mut buffer = surface.buffer_mut().unwrap();
            for (x, y, pixel) in buffer.pixels_iter() {
                let red = x % 255;
                let green = y % 255;
                let blue = (x * y) % 255;
                *pixel = Pixel::new_rgb(red as u8, green as u8, blue as u8);
            }

            tracing::info!("presenting...");
            buffer.present().unwrap();

            // We're done, tell the main thread to keep going.
            done.send(()).ok();
        }
    }

    pub fn entry(event_loop: EventLoop<()>) {
        let context = Context::new(event_loop.owned_display_handle()).unwrap();

        let app = util::WinitAppBuilder::with_init(
            |elwt| {
                let attributes = Window::default_attributes();
                #[cfg(target_family = "wasm")]
                let attributes =
                    winit::platform::web::WindowAttributesExtWebSys::with_append(attributes, true);
                let window = Arc::new(elwt.create_window(attributes).unwrap());

                // Spawn a thread to handle rendering for this specific surface. The channels will
                // be closed and the thread will be stopped whenever this surface (the returned
                // context below) is dropped, so that it can all be recreated again (on Android)
                // when a new surface is created.
                let (start_render, do_render) = mpsc::channel();
                let (render_done, finish_render) = mpsc::channel();
                tracing::info!("starting thread...");
                std::thread::spawn(move || render_thread(do_render, render_done));

                (window, start_render, finish_render)
            },
            move |_elwt, (window, _start_render, _finish_render)| {
                tracing::info!("making surface...");
                Arc::new(Mutex::new(Surface::new(&context, window.clone()).unwrap()))
            },
        )
        .with_event_handler(|state, surface, window_id, event, elwt| {
            let (window, start_render, finish_render) = state;
            elwt.set_control_flow(ControlFlow::Wait);

            if window_id != window.id() {
                return;
            }

            match event {
                WindowEvent::RedrawRequested => {
                    let Some(surface) = surface else {
                        tracing::error!("RedrawRequested fired before Resumed or after Suspended");
                        return;
                    };

                    let size = window.inner_size();
                    tracing::info!("got size: {size:?}");
                    if let (Some(width), Some(height)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    {
                        // Start the render and then finish it.
                        start_render.send((surface.clone(), width, height)).unwrap();
                        finish_render.recv().unwrap();
                    }
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
}

#[cfg(target_family = "wasm")]
mod ex {
    use winit::event_loop::EventLoop;
    pub(crate) fn entry(_event_loop: EventLoop<()>) {
        panic!("winit_multithreaded doesn't work on WASM")
    }
}

#[cfg(not(target_os = "android"))]
fn main() {
    util::setup();

    use winit::event_loop::EventLoop;
    ex::entry(EventLoop::new().unwrap())
}
