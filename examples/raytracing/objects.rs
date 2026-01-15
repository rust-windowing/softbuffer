use std::ops::Range;

use crate::material::Material;
use crate::ray::Ray;
use crate::vec3::{Point3, Vec3};

#[derive(Debug)]
pub struct Sphere {
    center: Point3,
    radius: f32,
    mat: Material,
}

impl Sphere {
    pub fn new(center: Point3, radius: f32, mat: Material) -> Self {
        Self {
            center,
            radius,
            mat,
        }
    }

    pub fn hit(&self, ray: &Ray, ray_t: Range<f32>) -> Option<Hit> {
        let oc = ray.origin - self.center;
        let a = ray.direction.length_squared();
        let half_b = oc.dot(ray.direction);
        let c = oc.length_squared() - self.radius * self.radius;

        let discriminant = half_b * half_b - a * c;
        if discriminant < 0.0 {
            return None;
        }
        let sqrtd = discriminant.sqrt();

        // Find the nearest root that lies in the acceptable range.
        let mut root = (-half_b - sqrtd) / a;
        if !ray_t.contains(&root) {
            root = (-half_b + sqrtd) / a;
            if !ray_t.contains(&root) {
                return None;
            }
        }

        let distance = root;
        let point = ray.at(distance);
        let outward_normal = (point - self.center) / self.radius;
        let front_face = ray.direction.dot(outward_normal) < 0.0;
        let normal = if front_face {
            outward_normal
        } else {
            -outward_normal
        };

        Some(Hit {
            distance,
            point,
            normal,
            mat: self.mat.clone(),
            front_face,
        })
    }
}

#[derive(Debug)]
pub struct Hit {
    pub point: Point3,
    pub normal: Vec3,
    pub mat: Material,
    pub distance: f32,
    pub front_face: bool,
}
