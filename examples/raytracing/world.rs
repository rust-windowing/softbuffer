use std::ops::Range;

use rand::{Rng, RngExt};

use crate::material::{Dielectric, Lambertian, Material, Metal};
use crate::objects::{Hit, Sphere};
use crate::ray::Ray;
use crate::vec3::{random_color, Color, Point3, Vec3};

#[derive(Default, Debug)]
pub struct World {
    spheres: Vec<Sphere>,
}

impl World {
    pub fn random_scene(rng: &mut impl Rng) -> Self {
        let mut spheres = Vec::new();

        let ground_material = Material::Lambertian(Lambertian::new(Color::new(0.5, 0.5, 0.5)));
        spheres.push(Sphere::new(
            Vec3::new(0.0, -1000.0, 0.0),
            1000.0,
            ground_material,
        ));

        for a in -11..11 {
            for b in -11..11 {
                let choose_mat = rng.random::<f32>();
                let center = Point3::new(
                    a as f32 + 0.9 * rng.random::<f32>(),
                    0.2,
                    b as f32 + 0.9 * rng.random::<f32>(),
                );

                if (center - Point3::new(4.0, 0.2, 0.0)).length() > 0.9 {
                    if choose_mat < 0.8 {
                        // Diffuse
                        let albedo = random_color(rng) * random_color(rng);
                        let sphere_material = Material::Lambertian(Lambertian::new(albedo));
                        spheres.push(Sphere::new(center, 0.2, sphere_material));
                    } else if choose_mat < 0.95 {
                        // Metal
                        let albedo = Vec3::new(
                            rng.random_range(0.5..=1.0),
                            rng.random_range(0.5..=1.0),
                            rng.random_range(0.5..=1.0),
                        );
                        let fuzz = rng.random_range(0.0..=0.5);
                        let sphere_material = Material::Metal(Metal::new(albedo, fuzz));
                        spheres.push(Sphere::new(center, 0.2, sphere_material));
                    } else {
                        // Glass
                        let sphere_material = Material::Dielectric(Dielectric::new(1.5));
                        spheres.push(Sphere::new(center, 0.2, sphere_material));
                    }
                }
            }
        }

        let material1 = Material::Dielectric(Dielectric::new(1.5));
        spheres.push(Sphere::new(Point3::new(0.0, 1.0, 0.0), 1.0, material1));

        let material2 = Material::Lambertian(Lambertian::new(Color::new(0.4, 0.2, 0.1)));
        spheres.push(Sphere::new(Point3::new(-4.0, 1.0, 0.0), 1.0, material2));

        let material3 = Material::Metal(Metal::new(Color::new(0.7, 0.6, 0.5), 0.0));
        spheres.push(Sphere::new(Point3::new(4.0, 1.0, 0.0), 1.0, material3));

        Self { spheres }
    }

    pub fn hit(&self, ray: &Ray, ray_t: Range<f32>) -> Option<Hit> {
        let mut closest_so_far = ray_t.end;
        let mut closest = None;

        for sphere in &self.spheres {
            if let Some(hit) = sphere.hit(ray, ray_t.start..closest_so_far) {
                closest_so_far = hit.distance;
                closest = Some(hit);
            }
        }

        closest
    }
}
