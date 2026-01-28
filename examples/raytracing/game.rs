use std::time::{Duration, Instant};

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use softbuffer::{Buffer, Pixel};

use crate::camera::Camera;
use crate::vec3::{Color, Point3, Vec3};
use crate::world::World;

pub struct Game {
    world: World,
    up: Vec3,
    camera_position: Point3,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    /// x = right, y = up, z = forwards
    pub camera_velocity: Vec3,
    elapsed_time: Instant,
}

const SAMPLES_PER_PIXEL: i32 = 3;
const MAX_DEPTH: i32 = 5;
pub const MOVEMENT_SPEED: f32 = 10.0;
pub const MOUSE_SENSITIVITY: f32 = 0.005;
const DURATION_BETWEEN_TICKS: Duration = Duration::from_millis(10);

impl Game {
    pub fn new() -> Self {
        let mut rng = SmallRng::from_os_rng();
        let position = Point3::new(13.0, 2.0, 3.0);
        let looking_at = Point3::new(0.0, 0.0, 0.0);
        let camera_direction = (looking_at - position).normalize();
        let up = Vec3::new(0.0, 1.0, 0.0);
        Self {
            world: World::random_scene(&mut rng),
            up,
            camera_position: position,
            camera_yaw: camera_direction.x.atan2(camera_direction.z),
            camera_pitch: camera_direction.y.clamp(-1.0, 1.0).asin(),
            camera_velocity: Vec3::new(0.0, 0.0, 0.0),
            elapsed_time: Instant::now(),
        }
    }

    pub fn draw(&self, buffer: &mut Buffer<'_>, scale_factor: f32) {
        self.draw_scene(buffer, scale_factor);
        self.draw_ui(buffer, scale_factor);
    }

    /// Draw the 3D scene.
    fn draw_scene(&self, buffer: &mut Buffer<'_>, scale_factor: f32) {
        // Raytracing is expensive, so we only do it once every 4x4 pixel.
        //
        // FIXME(madsmtm): Avoid the need for this once we can do hardware scaling.
        // https://github.com/rust-windowing/softbuffer/issues/177
        let scale_factor = scale_factor * 4.0;

        let width = buffer.width().get() as f32 / scale_factor;
        let height = buffer.height().get() as f32 / scale_factor;

        let dist_to_focus = 10.0;
        let aperture = 0.1;
        let cam = Camera::new(
            self.camera_position,
            self.camera_direction(),
            self.up,
            20.0,
            width / height,
            aperture,
            dist_to_focus,
        );

        let mut pixels = vec![Pixel::default(); width as usize * height as usize];

        let each_pixel = |rng: &mut SmallRng, i, pixel: &mut Pixel| {
            let y = i % (width as usize);
            let x = i / (width as usize);
            let mut pixel_color = Color::default();
            for _ in 0..SAMPLES_PER_PIXEL {
                let s = (y as f32 + rng.random::<f32>()) / (width - 1.0);
                let t = 1.0 - (x as f32 + rng.random::<f32>()) / (height - 1.0);
                let r = cam.get_ray(s, t, rng);
                pixel_color += r.trace(&self.world, MAX_DEPTH, rng);
            }
            *pixel = color_to_pixel(pixel_color, SAMPLES_PER_PIXEL);
        };

        // Render in parallel with rayon.
        #[cfg(not(target_family = "wasm"))]
        {
            use rayon::prelude::*;

            pixels
                .par_iter_mut()
                .enumerate()
                .for_each_init(SmallRng::from_os_rng, move |rng, (i, pixel)| {
                    each_pixel(rng, i, pixel)
                });
        };
        #[cfg(target_family = "wasm")]
        {
            let mut rng = SmallRng::from_os_rng();
            pixels
                .iter_mut()
                .enumerate()
                .for_each(|(i, pixel)| each_pixel(&mut rng, i, pixel));
        }

        // Upscale by `scale_factor`.
        let width = (buffer.width().get() as f32 / scale_factor) as usize;
        buffer.pixels_iter().for_each(|(x, y, pixel)| {
            let x = (x as f32 / scale_factor) as usize;
            let y = (y as f32 / scale_factor) as usize;
            if let Some(x) = pixels.get(x * width + y) {
                *pixel = *x;
            }
        });
    }

    /// Draw a simple example UI on top of the scene.
    fn draw_ui(&self, buffer: &mut Buffer<'_>, scale_factor: f32) {
        struct Rect {
            left: f32,
            right: f32,
            top: f32,
            bottom: f32,
            color: Pixel,
        }

        let width = buffer.width().get() as f32 / scale_factor;
        let height = buffer.height().get() as f32 / scale_factor;
        let rects = &[
            Rect {
                left: 10.0,
                right: width - 10.0,
                top: height - 90.0,
                bottom: height - 10.0,
                color: Pixel::new_rgb(0xee, 0xaa, 0xaa),
            },
            Rect {
                left: 30.0,
                right: 70.0,
                top: height - 70.0,
                bottom: height - 30.0,
                color: Pixel::new_rgb(0xaa, 0xaa, 0xee),
            },
        ];

        for (y, row) in buffer.pixel_rows().enumerate() {
            for rect in rects {
                let rect_vertical =
                    (rect.top * scale_factor) as usize..(rect.bottom * scale_factor) as usize;
                let rect_horizontal =
                    (rect.left * scale_factor) as usize..(rect.right * scale_factor) as usize;
                if rect_vertical.contains(&y) {
                    if let Some(row) = row.get_mut(rect_horizontal) {
                        row.fill(rect.color);
                    }
                }
            }
        }
    }

    fn tick(&mut self) {
        let forward = self.camera_direction().with_y(0.0);
        let right = forward.cross(self.up).normalize();
        let up = right.cross(forward);
        let movement = forward * self.camera_velocity.z
            + up * self.camera_velocity.y
            + right * self.camera_velocity.x;
        self.camera_position += movement * DURATION_BETWEEN_TICKS.as_secs_f32();
    }

    pub fn update(&mut self) {
        // Update game state.
        let now = Instant::now();
        while let Some(_remainder) = now
            .duration_since(self.elapsed_time)
            .checked_sub(DURATION_BETWEEN_TICKS)
        {
            self.elapsed_time += DURATION_BETWEEN_TICKS;
            self.tick();
        }
    }

    fn camera_direction(&self) -> Vec3 {
        Vec3::new(
            self.camera_pitch.cos() * self.camera_yaw.sin(),
            self.camera_pitch.sin(),
            self.camera_pitch.cos() * self.camera_yaw.cos(),
        )
    }
}

fn color_to_pixel(pixel_color: Color, samples_per_pixel: i32) -> Pixel {
    let mut r = pixel_color.x;
    let mut g = pixel_color.y;
    let mut b = pixel_color.z;

    let scale = 1.0 / samples_per_pixel as f32;
    r = f32::sqrt(scale * r);
    g = f32::sqrt(scale * g);
    b = f32::sqrt(scale * b);

    Pixel::new_rgb(
        (256.0 * r.clamp(0.0, 0.999)) as u8,
        (256.0 * g.clamp(0.0, 0.999)) as u8,
        (256.0 * b.clamp(0.0, 0.999)) as u8,
    )
}
