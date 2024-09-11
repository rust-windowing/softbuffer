use softbuffer::RGBA;
use std::num::NonZeroU32;
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

#[path = "utils/winit_app.rs"]
mod winit_app;

use tiny_skia::{BlendMode, LineCap, Paint, PathBuilder, PixmapMut, Stroke, StrokeDash, Transform};

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let app = winit_app::WinitAppBuilder::with_init(|elwt| {
        let window = winit_app::make_window(elwt, |w| w.with_transparent(true));

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new_with_alpha(&context, window.clone()).unwrap();

        (window, surface)
    })
    .with_event_handler(|state, event, elwt| {
        let (window, surface) = state;
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent {
                window_id,
                event: WindowEvent::Resized(size),
            } if window_id == window.id() => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    surface.resize(width, height).unwrap();
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::RedrawRequested,
            } if window_id == window.id() => {
                let size = window.inner_size();
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    let mut buffer = surface.buffer_mut().unwrap();

                    //We draw the background of our window in softbuffer writing to individual pixels
                    for y in 0..height.get() {
                        for x in 0..width.get() {
                            const SCALE_FACTOR: u32 = 3;
                            let red = (x/SCALE_FACTOR) % 255;
                            let green = (y/SCALE_FACTOR) % 255;
                            let blue = ((x/SCALE_FACTOR) * (y/SCALE_FACTOR)) % 255;
                            let alpha = if blue > 255/2{
                                255
                            }else{
                                0
                            };
                            let index = y as usize * width.get() as usize + x as usize;
                            buffer.pixels_rgb_mut()[index] = softbuffer::RGBA::new_unchecked(red,green, blue, alpha);
                        }
                    }

                    // buffer.fill(RGBA::new_unchecked(50,0,50, 200)); // Alternatively we could fill with a solid color

                    //using tiny_skia that accepts the u8 rgba format, we draw a star on top of our background
                    buffer.pixel_u8_slice_rgba(|u8_buffer_rgba| {
                        let mut pixmap =
                            PixmapMut::from_bytes(u8_buffer_rgba, width.get(), height.get())
                                .unwrap();
                        let mut paint = Paint::default();
                        // paint.set_color_rgba8(255, 0, 255, 0); // <-- We could set the color, but because we are using BlendMode::Clear the color does not matter
                        paint.anti_alias = true;
                        paint.blend_mode = BlendMode::Clear; // <-- Set Blend mode so that we can draw transparent pixels

                        let path = {
                            let mut pb = PathBuilder::new();
                            let RADIUS: f32 = (width.get().min(height.get()) / 2) as f32;
                            let CENTER: f32 = (width.get().min(height.get()) / 2) as f32;
                            pb.move_to(CENTER + RADIUS, CENTER);
                            for i in 1..8 {
                                let a = 2.6927937 * i as f32;
                                pb.line_to(CENTER + RADIUS * a.cos(), CENTER + RADIUS * a.sin());
                            }
                            pb.finish().unwrap()
                        };

                        let mut stroke = Stroke::default();
                        stroke.width = 24.0;
                        stroke.line_cap = LineCap::Round;
                        stroke.dash = StrokeDash::new(vec![20.0, 40.0], 0.0);

                        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                    });


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
    });

    winit_app::run_app(event_loop, app);
}
