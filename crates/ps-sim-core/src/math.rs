//! Small geometric helpers shared by simulation and rendering.

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

/// View angles in degrees.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewAngles {
    /// Positive looks up, clamped to ±89°.
    pub pitch: f32,
    /// Positive turns left; kept in (−180, 180].
    pub yaw: f32,
}

pub const MAX_PITCH: f32 = 89.0;

impl ViewAngles {
    pub fn new(pitch: f32, yaw: f32) -> Self {
        Self { pitch, yaw }.normalized()
    }

    pub fn normalized(self) -> Self {
        Self {
            pitch: self.pitch.clamp(-MAX_PITCH, MAX_PITCH),
            yaw: wrap_degrees(self.yaw),
        }
    }

    pub fn forward(&self) -> Vec3 {
        angles_to_forward(self.pitch, self.yaw)
    }

    /// Horizontal right vector.
    pub fn right_flat(&self) -> Vec3 {
        let y = self.yaw.to_radians();
        Vec3::new(y.cos(), 0.0, -y.sin())
    }

    /// Horizontal forward vector.
    pub fn forward_flat(&self) -> Vec3 {
        let y = self.yaw.to_radians();
        Vec3::new(-y.sin(), 0.0, -y.cos())
    }

    /// Shortest interpolation (yaw wraps).
    pub fn lerp(self, other: ViewAngles, t: f32) -> ViewAngles {
        let dy = wrap_degrees(other.yaw - self.yaw);
        ViewAngles {
            pitch: self.pitch + (other.pitch - self.pitch) * t,
            yaw: wrap_degrees(self.yaw + dy * t),
        }
    }
}

/// Wraps an angle in degrees to (−180, 180].
pub fn wrap_degrees(a: f32) -> f32 {
    let mut r = a % 360.0;
    if r > 180.0 {
        r -= 360.0;
    } else if r <= -180.0 {
        r += 360.0;
    }
    r
}

/// Unit direction for (pitch, yaw) in degrees.
pub fn angles_to_forward(pitch: f32, yaw: f32) -> Vec3 {
    let p = pitch.to_radians();
    let y = yaw.to_radians();
    Vec3::new(-y.sin() * p.cos(), p.sin(), -y.cos() * p.cos())
}

/// (pitch, yaw) in degrees of a direction.
pub fn forward_to_angles(dir: Vec3) -> ViewAngles {
    let d = dir.normalize_or(Vec3::NEG_Z);
    let pitch = d.y.clamp(-1.0, 1.0).asin().to_degrees();
    let yaw = (-d.x).atan2(-d.z).to_degrees();
    ViewAngles { pitch, yaw }
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
    fn forward_conventions() {
        assert!(angles_to_forward(0.0, 0.0).abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!(angles_to_forward(0.0, 90.0).abs_diff_eq(Vec3::NEG_X, 1e-6));
        assert!(angles_to_forward(90.0, 0.0).abs_diff_eq(Vec3::Y, 1e-6));
        let a = ViewAngles::new(10.0, 37.0);
        let back = forward_to_angles(a.forward());
        assert!((back.pitch - 10.0).abs() < 1e-3 && (back.yaw - 37.0).abs() < 1e-3);
    }

    #[test]
    fn right_is_perpendicular_and_to_the_right() {
        let a = ViewAngles::new(0.0, 0.0);
        assert!(a.right_flat().abs_diff_eq(Vec3::X, 1e-6));
        for yaw in [-170.0f32, -45.0, 12.0, 133.0] {
            let v = ViewAngles::new(0.0, yaw);
            assert!(v.right_flat().dot(v.forward_flat()).abs() < 1e-6);
            // right = forward × up
            assert!(
                v.forward_flat()
                    .cross(Vec3::Y)
                    .abs_diff_eq(v.right_flat(), 1e-5)
            );
        }
    }

    #[test]
    fn wrap() {
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-180.0), 180.0);
        assert_eq!(wrap_degrees(540.0), 180.0);
    }

    #[test]
    fn aabb_ray() {
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let (t0, t1) = b.ray_interval(Vec3::new(-5.0, 0.0, 0.0), Vec3::X).unwrap();
        assert!((t0 - 4.0).abs() < 1e-6 && (t1 - 6.0).abs() < 1e-6);
        assert!(b.ray_interval(Vec3::new(-5.0, 3.0, 0.0), Vec3::X).is_none());
    }
}
