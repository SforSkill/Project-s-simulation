//! Convex brushes described by bounding planes (inside = behind every plane).

use crate::SURFACE_EPSILON;
use crate::material::MaterialId;
use engine_core::Aabb;
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

impl Plane {
    pub fn new(normal: Vec3, dist: f32) -> Self {
        Self { normal, dist }
    }

    pub fn from_point(normal: Vec3, p: Vec3) -> Self {
        let n = normal.normalize();
        Self {
            normal: n,
            dist: n.dot(p),
        }
    }

    pub fn distance(&self, p: Vec3) -> f32 {
        self.normal.dot(p) - self.dist
    }

    /// Plane distance expanded by an axis aligned box `[mins, maxs]` placed at the
    /// moving point (Minkowski sum), so that a point trace against the expanded plane
    /// is equivalent to a box trace against the original one.
    fn expanded_dist(&self, mins: Vec3, maxs: Vec3) -> f32 {
        let n = self.normal;
        let mut off = 0.0;
        for i in 0..3 {
            off += if n[i] >= 0.0 {
                n[i] * mins[i]
            } else {
                n[i] * maxs[i]
            };
        }
        self.dist - off
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Brush {
    /// Faces plus axial bevel planes.
    pub planes: Vec<Plane>,
    pub bounds: Aabb,
    pub material: MaterialId,
    /// Blocks players (false for bullet-only geometry, e.g. thin fences).
    pub blocks_players: bool,
    /// Blocks bullets and line of sight (false for player clips).
    pub blocks_bullets: bool,
    /// Rendered by the client.
    pub visible: bool,
}

/// Result of a box trace against a single brush.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushHit {
    pub fraction: f32,
    pub normal: Vec3,
    pub start_solid: bool,
    pub all_solid: bool,
}

impl Brush {
    pub fn from_box(b: Aabb, material: MaterialId) -> Brush {
        let planes = vec![
            Plane::new(Vec3::X, b.max.x),
            Plane::new(Vec3::NEG_X, -b.min.x),
            Plane::new(Vec3::Y, b.max.y),
            Plane::new(Vec3::NEG_Y, -b.min.y),
            Plane::new(Vec3::Z, b.max.z),
            Plane::new(Vec3::NEG_Z, -b.min.z),
        ];
        Brush {
            planes,
            bounds: b,
            material,
            blocks_players: true,
            blocks_bullets: true,
            visible: true,
        }
    }

    /// Wedge filling `b`, whose top surface rises toward `high_dir`
    /// (one of ±X / ±Z). The six axial planes of the box are kept as bevels.
    pub fn ramp(b: Aabb, high_dir: Vec3, material: MaterialId) -> Brush {
        let mut brush = Brush::from_box(b, material);
        let h = b.max.y - b.min.y;
        // Low edge at the top of the opposite side, high edge at max.y on `high_dir` side.
        let (low_point, run) = if high_dir.x > 0.5 {
            (Vec3::new(b.min.x, b.min.y, 0.0), b.max.x - b.min.x)
        } else if high_dir.x < -0.5 {
            (Vec3::new(b.max.x, b.min.y, 0.0), b.max.x - b.min.x)
        } else if high_dir.z > 0.5 {
            (Vec3::new(0.0, b.min.y, b.min.z), b.max.z - b.min.z)
        } else {
            (Vec3::new(0.0, b.min.y, b.max.z), b.max.z - b.min.z)
        };
        // Slope normal: up and away from the high side.
        let n = Vec3::new(-high_dir.x * h, run, -high_dir.z * h).normalize();
        brush.planes.push(Plane::from_point(n, low_point));
        brush
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes.iter().all(|pl| pl.distance(p) <= 0.0)
    }

    /// Swept box test (Quake III `CM_TraceThroughBrush`). Box is `[mins, maxs]` around
    /// the moving origin.
    pub fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Option<BrushHit> {
        let mut enter = -1.0f32;
        let mut leave = 1.0f32;
        let mut clip_normal = Vec3::ZERO;
        let mut start_out = false;
        let mut get_out = false;
        for pl in &self.planes {
            let dist = pl.expanded_dist(mins, maxs);
            let d1 = pl.normal.dot(start) - dist;
            let d2 = pl.normal.dot(end) - dist;
            if d2 > 0.0 {
                get_out = true;
            }
            if d1 > 0.0 {
                start_out = true;
            }
            // Completely in front of this plane: no contact.
            if d1 > 0.0 && (d2 >= SURFACE_EPSILON || d2 >= d1) {
                return None;
            }
            if d1 <= 0.0 && d2 <= 0.0 {
                continue;
            }
            if d1 > d2 {
                let f = ((d1 - SURFACE_EPSILON) / (d1 - d2)).max(0.0);
                if f > enter {
                    enter = f;
                    clip_normal = pl.normal;
                }
            } else {
                let f = ((d1 + SURFACE_EPSILON) / (d1 - d2)).min(1.0);
                if f < leave {
                    leave = f;
                }
            }
        }
        if !start_out {
            return Some(BrushHit {
                fraction: if get_out { 1.0 } else { 0.0 },
                normal: Vec3::ZERO,
                start_solid: true,
                all_solid: !get_out,
            });
        }
        if enter < leave && enter > -1.0 {
            return Some(BrushHit {
                fraction: enter.max(0.0),
                normal: clip_normal,
                start_solid: false,
                all_solid: false,
            });
        }
        None
    }

    /// Exact entry/exit parameters of a ray (`origin + t·dir`, `t` in [0, max_t]).
    /// Returns `(t_enter, t_exit, entry_normal)`.
    pub fn ray_interval(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<(f32, f32, Vec3)> {
        let mut t0 = 0.0f32;
        let mut t1 = max_t;
        let mut n_enter = Vec3::ZERO;
        let mut inside_at_start = true;
        for pl in &self.planes {
            let denom = pl.normal.dot(dir);
            let dist = pl.distance(origin);
            if dist > 0.0 {
                inside_at_start = false;
            }
            if denom.abs() < 1e-9 {
                if dist > 0.0 {
                    return None;
                }
                continue;
            }
            let t = -dist / denom;
            if denom < 0.0 {
                if t > t0 {
                    t0 = t;
                    n_enter = pl.normal;
                }
            } else if t < t1 {
                t1 = t;
            }
            if t0 > t1 {
                return None;
            }
        }
        if inside_at_start {
            n_enter = -dir.normalize_or_zero();
        }
        Some((t0, t1, n_enter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Brush {
        Brush::from_box(
            Aabb::new(Vec3::splat(-10.0), Vec3::splat(10.0)),
            MaterialId(0),
        )
    }

    #[test]
    fn ray_trace_hits_face() {
        let b = unit_box();
        let h = b
            .trace(
                Vec3::new(-30.0, 0.0, 0.0),
                Vec3::new(30.0, 0.0, 0.0),
                Vec3::ZERO,
                Vec3::ZERO,
            )
            .unwrap();
        // Hits at x = −10 minus epsilon → fraction ≈ 20 / 60.
        assert!(
            (h.fraction - (20.0 - SURFACE_EPSILON) / 60.0).abs() < 1e-5,
            "{}",
            h.fraction
        );
        assert_eq!(h.normal, Vec3::NEG_X);
    }

    #[test]
    fn box_trace_is_expanded() {
        let b = unit_box();
        let mins = Vec3::new(-16.0, 0.0, -16.0);
        let maxs = Vec3::new(16.0, 72.0, 16.0);
        let h = b
            .trace(
                Vec3::new(-60.0, -20.0, 0.0),
                Vec3::new(0.0, -20.0, 0.0),
                mins,
                maxs,
            )
            .unwrap();
        // Box front face at x+16 touches x = −10 when origin.x = −26.
        let x = -60.0 + 60.0 * h.fraction;
        assert!((x - (-26.0 - SURFACE_EPSILON)).abs() < 1e-3, "{x}");
    }

    #[test]
    fn miss_and_start_solid() {
        let b = unit_box();
        assert!(
            b.trace(
                Vec3::new(-30.0, 50.0, 0.0),
                Vec3::new(30.0, 50.0, 0.0),
                Vec3::ZERO,
                Vec3::ZERO
            )
            .is_none()
        );
        let h = b
            .trace(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, Vec3::ZERO)
            .unwrap();
        assert!(h.start_solid && h.all_solid);
    }

    #[test]
    fn ramp_surface() {
        // 64 long, 32 high, rising toward +X: height at x is (x/64)·32.
        let r = Brush::ramp(
            Aabb::new(Vec3::new(0.0, 0.0, -32.0), Vec3::new(64.0, 32.0, 32.0)),
            Vec3::X,
            MaterialId(0),
        );
        assert!(r.contains_point(Vec3::new(60.0, 20.0, 0.0)));
        assert!(!r.contains_point(Vec3::new(10.0, 20.0, 0.0)));
        let h = r
            .trace(
                Vec3::new(32.0, 100.0, 0.0),
                Vec3::new(32.0, -10.0, 0.0),
                Vec3::ZERO,
                Vec3::ZERO,
            )
            .unwrap();
        let y = 100.0 - 110.0 * h.fraction;
        assert!((y - 16.0).abs() < 0.1, "{y}");
        assert!(h.normal.y > 0.8 && h.normal.x < 0.0);
    }

    #[test]
    fn ray_interval_thickness() {
        let b = unit_box();
        let (t0, t1, n) = b
            .ray_interval(Vec3::new(-30.0, 0.0, 0.0), Vec3::X, 100.0)
            .unwrap();
        assert!((t0 - 20.0).abs() < 1e-5 && (t1 - 40.0).abs() < 1e-5);
        assert_eq!(n, Vec3::NEG_X);
    }
}
