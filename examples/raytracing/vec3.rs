use rand::Rng;

pub use glam::Vec3;
pub type Point3 = Vec3;
pub type Color = Vec3;

pub fn random_in_unit_sphere(rng: &mut impl Rng) -> Vec3 {
    loop {
        let p = Vec3::new(
            rng.random_range(-1.0..=1.0),
            rng.random_range(-1.0..=1.0),
            rng.random_range(-1.0..=1.0),
        );
        if p.length_squared() >= 1.0 {
            continue;
        }
        return p;
    }
}

pub fn random_unit_vector(rng: &mut impl Rng) -> Vec3 {
    random_in_unit_sphere(rng).normalize()
}

pub fn random_in_unit_disk(rng: &mut impl Rng) -> Vec3 {
    loop {
        let p = Vec3::new(
            rng.random_range(-1.0..=1.0),
            rng.random_range(-1.0..=1.0),
            0.0,
        );
        if p.length_squared() >= 1.0 {
            continue;
        }
        return p;
    }
}
