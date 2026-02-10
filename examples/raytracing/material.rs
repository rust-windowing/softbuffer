use rand::{Rng, RngExt};

use crate::objects::Hit;
use crate::ray::Ray;
use crate::vec3::{self, Color, Vec3};

#[derive(Debug)]
pub struct ScatterResult {
    pub attenuation: Color,
    pub scatter_direction: Vec3,
}

#[derive(Debug, Clone)]
pub enum Material {
    Lambertian(Lambertian),
    Metal(Metal),
    Dielectric(Dielectric),
}

impl Material {
    pub fn scatter(&self, r_in: &Ray, hit: &Hit, rng: &mut impl Rng) -> Option<ScatterResult> {
        match self {
            Self::Lambertian(material) => material.scatter(r_in, hit, rng),
            Self::Metal(material) => material.scatter(r_in, hit, rng),
            Self::Dielectric(material) => material.scatter(r_in, hit, rng),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lambertian {
    albedo: Color,
}

impl Lambertian {
    pub fn new(albedo: Color) -> Self {
        Self { albedo }
    }
}

impl Lambertian {
    pub fn scatter(&self, _r_in: &Ray, hit: &Hit, rng: &mut impl Rng) -> Option<ScatterResult> {
        let mut scatter_direction = hit.normal + vec3::random_unit_vector(rng);

        fn near_zero(vec: Vec3) -> bool {
            vec.x.abs() < f32::EPSILON && vec.y.abs() < f32::EPSILON && vec.z.abs() < f32::EPSILON
        }

        // Catch degenerate scatter direction
        if near_zero(scatter_direction) {
            scatter_direction = hit.normal;
        }

        Some(ScatterResult {
            attenuation: self.albedo,
            scatter_direction,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Metal {
    albedo: Color,
    fuzz: f32,
}

impl Metal {
    pub fn new(albedo: Color, fuzz: f32) -> Self {
        Self {
            albedo,
            fuzz: fuzz.min(1.0),
        }
    }
}

impl Metal {
    pub fn scatter(&self, r_in: &Ray, hit: &Hit, rng: &mut impl Rng) -> Option<ScatterResult> {
        let reflected: Vec3 = r_in.direction.normalize().reflect(hit.normal);
        let scatter_direction = reflected + self.fuzz * vec3::random_in_unit_sphere(rng);
        if scatter_direction.dot(hit.normal) > 0.0 {
            Some(ScatterResult {
                attenuation: self.albedo,
                scatter_direction,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dielectric {
    ir: f32,
}

impl Dielectric {
    pub fn new(index_of_refraction: f32) -> Self {
        Self {
            ir: index_of_refraction,
        }
    }

    fn reflectance(cosine: f32, ref_idx: f32) -> f32 {
        // Use Schlick's approximation for reflectance
        let mut r0 = (1.0 - ref_idx) / (1.0 + ref_idx);
        r0 = r0 * r0;
        r0 + (1.0 - r0) * f32::powf(1.0 - cosine, 5.0)
    }
}

impl Dielectric {
    pub fn scatter(&self, r_in: &Ray, hit: &Hit, rng: &mut impl Rng) -> Option<ScatterResult> {
        let refraction_ratio = if hit.front_face {
            1.0 / self.ir
        } else {
            self.ir
        };

        let unit_direction = r_in.direction.normalize();
        let cos_theta = f32::min((-unit_direction).dot(hit.normal), 1.0);
        let sin_theta = f32::sqrt(1.0 - cos_theta * cos_theta);

        let cannot_refract = refraction_ratio * sin_theta > 1.0;
        let direction =
            if cannot_refract || Self::reflectance(cos_theta, refraction_ratio) > rng.random() {
                unit_direction.reflect(hit.normal)
            } else {
                unit_direction.refract(hit.normal, refraction_ratio)
            };

        Some(ScatterResult {
            attenuation: Color::new(1.0, 1.0, 1.0),
            scatter_direction: direction,
        })
    }
}
