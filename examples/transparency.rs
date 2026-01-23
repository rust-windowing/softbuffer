//! An example to test transparent rendering.
//!
//! Press `o`, `i`, `m` or `t` to change the alpha mode to `Opaque`, `Ignored`, `Premultiplied` and
//! `Postmultiplied` respectively.
//!
//! This should render 6 rectangular areas. For details on the terminology, see:
//! <https://html.spec.whatwg.org/multipage/canvas.html#premultiplied-alpha-and-the-2d-rendering-context>
//!
//! (255, 127, 0, 255):
//! - Opaque/Ignored: Completely-opaque orange.
//! - Postmultiplied: Completely-opaque orange.
//! - Premultiplied:  Completely-opaque orange.
//!
//! (255, 255, 0, 127):
//! - Opaque/Ignored: Completely-opaque yellow.
//! - Postmultiplied: Halfway-opaque yellow.
//! - Premultiplied:  Additive halfway-opaque yellow.
//!
//! (127, 127, 0, 127):
//! - Opaque/Ignored: Completely-opaque dark yellow.
//! - Postmultiplied: Halfway-opaque dark yellow.
//! - Premultiplied:  Halfway-opaque yellow.
//!
//! (255, 127, 0, 127):
//! - Opaque/Ignored: Completely-opaque orange.
//! - Postmultiplied: Halfway-opaque orange.
//! - Premultiplied:  Additive halfway-opaque orange.
//!
//! (255, 127, 0, 0):
//! - Opaque/Ignored: Completely-opaque orange.
//! - Postmultiplied: Fully-transparent orange.
//! - Premultiplied:  Additive fully-transparent orange.
//!
//! (0, 0, 0, 0):
//! - Opaque/Ignored: Completely-opaque black.
//! - Postmultiplied: Fully-transparent.
//! - Premultiplied:  Fully-transparent.
use softbuffer::{AlphaMode, Context, Pixel, Surface};
use std::num::NonZeroU32;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();

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

                tracing::info!(alpha_mode = ?surface.alpha_mode(), "redraw");

                let alpha_mode = surface.alpha_mode();
                let mut buffer = surface.buffer_mut().unwrap();
                let width = buffer.width().get();
                for (x, _, pixel) in buffer.pixels_iter() {
                    let rectangle_number = (x * 6) / width;
                    *pixel = match rectangle_number {
                        0 => Pixel::new_rgba(255, 127, 0, 255),
                        1 => Pixel::new_rgba(255, 255, 0, 127),
                        2 => Pixel::new_rgba(127, 127, 0, 127),
                        3 => Pixel::new_rgba(255, 127, 0, 127),
                        4 => Pixel::new_rgba(255, 127, 0, 0),
                        _ => Pixel::new_rgba(0, 0, 0, 0),
                    };

                    // Convert `AlphaMode::Opaque` -> `AlphaMode::Ignored`.
                    if alpha_mode == AlphaMode::Opaque {
                        pixel.a = 255;
                    };
                }

                buffer.present().unwrap();
            }
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        repeat: false,
                        ..
                    },
                ..
            } => {
                elwt.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        repeat: false,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let Some(surface) = surface else {
                    tracing::error!("KeyboardInput fired before Resumed or after Suspended");
                    return;
                };

                let alpha_mode = match logical_key.to_text() {
                    Some("o") => AlphaMode::Opaque,
                    Some("i") => AlphaMode::Ignored,
                    Some("m") => AlphaMode::Premultiplied,
                    Some("t") => AlphaMode::Postmultiplied,
                    _ => return,
                };

                if !surface.supports_alpha_mode(alpha_mode) {
                    tracing::warn!(?alpha_mode, "not supported by the backend");
                    return;
                }

                tracing::info!(?alpha_mode, "set alpha");
                let size = window.inner_size();
                let width = NonZeroU32::new(size.width).unwrap();
                let height = NonZeroU32::new(size.height).unwrap();
                surface.configure(width, height, alpha_mode).unwrap();
                assert_eq!(surface.alpha_mode(), alpha_mode);

                window.set_transparent(matches!(
                    alpha_mode,
                    AlphaMode::Premultiplied | AlphaMode::Postmultiplied
                ));

                window.request_redraw();
            }
            _ => {}
        }
    });

    util::run_app(event_loop, app);
}
