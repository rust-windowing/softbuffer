//! A software raytracer based on [Ray Tracing in One Weekend].
//!
//! Note that this is quite slow, you probably don't want to do realtime CPU raytracing in practice.
//!
//! [Ray Tracing in One Weekend]: https://raytracing.github.io/books/RayTracingInOneWeekend.html
use std::num::NonZeroU32;
use winit::event::{DeviceEvent, ElementState, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::CursorGrabMode;

use crate::game::{Game, MOUSE_SENSITIVITY, MOVEMENT_SPEED};

mod camera;
mod game;
mod material;
mod objects;
mod ray;
#[path = "../util/mod.rs"]
mod util;
mod vec3;
mod world;

fn main() {
    util::setup();

    let event_loop = EventLoop::new().unwrap();
    let context = softbuffer::Context::new(event_loop.owned_display_handle()).unwrap();

    let app = util::WinitAppBuilder::with_init(
        |elwt| util::make_window(elwt, |w| w),
        move |_elwt, window| {
            let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
            surface
                .resize(
                    NonZeroU32::new(window.inner_size().width).unwrap(),
                    NonZeroU32::new(window.inner_size().height).unwrap(),
                )
                .unwrap();
            let game = Game::new();
            (surface, game)
        },
    )
    .with_event_handler(|window, surface, window_id, event, elwt| {
        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                let Some((surface, _)) = surface else {
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
                let Some((surface, game)) = surface else {
                    tracing::error!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };

                game.update();

                let mut buffer = surface.buffer_mut().unwrap();
                game.draw(&mut buffer, window.scale_factor() as f32);
                buffer.present().unwrap();
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                let Some((_, game)) = surface else {
                    tracing::error!("KeyboardInput fired before Resumed or after Suspended");
                    return;
                };

                let value = match state {
                    ElementState::Pressed => MOVEMENT_SPEED,
                    ElementState::Released => -MOVEMENT_SPEED,
                };

                match code {
                    KeyCode::KeyW => game.camera_velocity.z += value,
                    KeyCode::KeyS => game.camera_velocity.z -= value,
                    KeyCode::KeyD => game.camera_velocity.x += value,
                    KeyCode::KeyA => game.camera_velocity.x -= value,
                    KeyCode::Space => game.camera_velocity.y += value,
                    KeyCode::ShiftLeft => game.camera_velocity.y -= value,
                    _ => {}
                }
            }
            WindowEvent::Focused(focused) => {
                window.set_cursor_visible(!focused);
                window
                    .set_cursor_grab(if focused {
                        CursorGrabMode::Locked
                    } else {
                        CursorGrabMode::None
                    })
                    .unwrap();
            }
            _ => {}
        }
    })
    .with_device_event_handler(|_window, surface, event, _elwt| {
        if let DeviceEvent::MouseMotion { delta } = event {
            let Some((_, game)) = surface else {
                tracing::error!("CursorMoved fired before Resumed or after Suspended");
                return;
            };

            game.camera_yaw -= delta.0 as f32 * MOUSE_SENSITIVITY;
            game.camera_pitch -= delta.1 as f32 * MOUSE_SENSITIVITY;
            game.camera_pitch = game.camera_pitch.clamp(
                -std::f32::consts::FRAC_PI_2 + 0.01,
                std::f32::consts::FRAC_PI_2 - 0.01,
            );
        }
    });

    util::run_app(event_loop, app);
}
