//! Animates a small square and presents only the area that changed each frame.
//!
//! The rest of the window must stay intact even when the system discards what the window shows,
//! for example when it is minimized and restored, or moved to a screen with another scale factor.

use softbuffer::{Buffer, Context, Pixel, Rect, Surface};
use std::num::NonZeroU32;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

mod util;

const SQUARE_SIZE: u32 = 40;
const SQUARE_STEP: u32 = 4;
const SQUARE_COLOR: Pixel = Pixel::new_rgb(0xff, 0xff, 0xff);

fn background(x: u32, y: u32) -> Pixel {
    Pixel::new_rgb(0x40 + (x % 0xc0) as u8, 0x60, 0x40 + (y % 0xc0) as u8)
}

fn square_at(frame: u32, width: NonZeroU32, height: NonZeroU32) -> Option<Rect> {
    let size = SQUARE_SIZE.min(width.get()).min(height.get());
    let travel = width.get() - size + 1;
    Some(Rect {
        x: frame.wrapping_mul(SQUARE_STEP) % travel,
        y: (height.get() - size) / 2,
        width: NonZeroU32::new(size)?,
        height: NonZeroU32::new(size)?,
    })
}

fn fill(buffer: &mut Buffer<'_>, rect: Rect, pixel: impl Fn(u32, u32) -> Pixel) {
    let columns = rect.x..rect.x + rect.width.get();
    for (y, row) in buffer
        .pixel_rows()
        .enumerate()
        .skip(rect.y as usize)
        .take(rect.height.get() as usize)
    {
        for x in columns.clone() {
            row[x as usize] = pixel(x, y as u32);
        }
    }
}

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| {
            let window = util::make_window(elwt, |w| w.with_title("Partial presentation"));
            let frame = 0u32;
            let previous_square: Option<Rect> = None;
            (window, frame, previous_square)
        },
        move |_elwt, (window, _, _)| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(|state, surface, window_id, event, elwt| {
        let (window, frame, previous_square) = state;

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
                let (width, height) = (buffer.width(), buffer.height());
                let square = square_at(*frame, width, height);

                match (buffer.age(), *previous_square, square) {
                    (1, Some(previous), Some(current))
                        if previous.x + previous.width.get() <= width.get()
                            && previous.y + previous.height.get() <= height.get() =>
                    {
                        fill(&mut buffer, previous, background);
                        fill(&mut buffer, current, |_, _| SQUARE_COLOR);
                        buffer.present_with_damage(&[previous, current]).unwrap();
                    }
                    _ => {
                        for (x, y, pixel) in buffer.pixels_iter() {
                            *pixel = background(x, y);
                        }
                        if let Some(current) = square {
                            fill(&mut buffer, current, |_, _| SQUARE_COLOR);
                        }
                        buffer.present().unwrap();
                    }
                }

                *previous_square = square;
                *frame = frame.wrapping_add(1);
                window.request_redraw();
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
