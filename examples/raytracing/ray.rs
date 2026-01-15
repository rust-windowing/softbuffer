use rand::Rng;

use crate::vec3::Color;
use crate::vec3::Point3;
use crate::vec3::Vec3;
use crate::world::World;

#[derive(Default, Debug)]
pub struct Ray {
    pub origin: Point3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Point3, direction: Vec3) -> Self {
        Self { origin, direction }
    }

    pub fn at(&self, t: f32) -> Point3 {
        self.origin + t * self.direction
    }

    /// Find the color for a given ray.
    pub fn trace(&self, world: &World, depth: i32, rng: &mut impl Rng) -> Color {
        if depth <= 0 {
            return Color::default();
        }

        if let Some(hit) = world.hit(self, 0.001..f32::INFINITY) {
            if let Some(res) = hit.mat.scatter(self, &hit, rng) {
                let scattered_ray = Ray::new(hit.point, res.scatter_direction);
                // Hadamard product (element-wise product)
                return res.attenuation * scattered_ray.trace(world, depth - 1, rng);
            }
            return Color::default();
        }

        // Sky color
        let unit_direction = self.direction.normalize();
        let t = 0.5 * (unit_direction.y + 1.0);
        (1.0 - t) * Color::new(1.0, 1.0, 1.0) + t * Color::new(0.5, 0.7, 1.0)
    }
}
