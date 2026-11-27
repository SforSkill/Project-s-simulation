//! Small geometric helpers shared by simulation and rendering.

pub use ps_types::angles::{
    MAX_PITCH, ViewAngles, angles_to_forward, forward_to_angles, wrap_degrees,
};

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Axis aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// Box from two arbitrary corners.
    pub fn from_corners(a: Vec3, b: Vec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && self.max.cmpge(o.min).all()
    }

    pub fn expanded(&self, by: Vec3) -> Aabb {
        Aabb {
            min: self.min - by,
            max: self.max + by,
        }
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    pub fn translated(&self, by: Vec3) -> Aabb {
        Aabb {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// Slab test. Returns the entry and exit distances along `dir` (unnormalised
    /// directions are allowed: distances are then in units of `dir`).
    pub fn ray_interval(&self, origin: Vec3, dir: Vec3) -> Option<(f32, f32)> {
        let mut t0 = f32::NEG_INFINITY;
        let mut t1 = f32::INFINITY;
        for i in 0..3 {
            let o = origin[i];
            let d = dir[i];
            if d.abs() < 1e-12 {
                if o < self.min[i] || o > self.max[i] {
                    return None;
                }
            } else {
                let inv = 1.0 / d;
                let mut a = (self.min[i] - o) * inv;
                let mut b = (self.max[i] - o) * inv;
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some((t0, t1))
    }

    /// Squared distance from a point to the box (0 inside).
    pub fn distance_sq(&self, p: Vec3) -> f32 {
        let c = p.clamp(self.min, self.max);
        (p - c).length_squared()
    }
}

/// Orthonormal basis (right, up) perpendicular to `dir`, used to apply angular offsets.
pub fn basis_from_forward(dir: Vec3) -> (Vec3, Vec3) {
    let f = dir.normalize_or(Vec3::NEG_Z);
    let world_up = if f.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
    let right = f.cross(world_up).normalize();
    let up = right.cross(f).normalize();
    (right, up)
}

/// Rotates `dir` by small angular offsets in degrees (pitch up / yaw left).
pub fn offset_direction(dir: Vec3, pitch_up_deg: f32, yaw_left_deg: f32) -> Vec3 {
    let a = forward_to_angles(dir);
    angles_to_forward(a.pitch + pitch_up_deg, a.yaw + yaw_left_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_ray() {
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let (t0, t1) = b.ray_interval(Vec3::new(-5.0, 0.0, 0.0), Vec3::X).unwrap();
        assert!((t0 - 4.0).abs() < 1e-6 && (t1 - 6.0).abs() < 1e-6);
        assert!(b.ray_interval(Vec3::new(-5.0, 3.0, 0.0), Vec3::X).is_none());
    }
}
