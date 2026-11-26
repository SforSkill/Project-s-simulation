//! Movement collision against the static world plus the other players' boxes.

use engine_core::Aabb;
use engine_movement::controller::MoveWorld;
use engine_physics::world::TraceMask;
use engine_physics::{CollisionWorld, Trace};
use glam::Vec3;

pub struct PlayerWorld<'a> {
    pub world: &'a CollisionWorld,
    /// Other players' boxes (world space).
    pub boxes: &'a [Aabb],
}

/// Swept point vs box: entry fraction and face normal. `None` if no hit or if the
/// start is already inside (players overlapping can always separate).
pub fn sweep_point_box(start: Vec3, end: Vec3, b: &Aabb) -> Option<(f32, Vec3)> {
    let d = end - start;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    let mut normal = Vec3::ZERO;
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if start[i] <= b.min[i] || start[i] >= b.max[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[i];
        let (mut a, mut c) = ((b.min[i] - start[i]) * inv, (b.max[i] - start[i]) * inv);
        let mut n = Vec3::ZERO;
        n[i] = -d[i].signum();
        if a > c {
            std::mem::swap(&mut a, &mut c);
        }
        if a > t0 {
            t0 = a;
            normal = n;
        }
        t1 = t1.min(c);
        if t0 > t1 {
            return None;
        }
    }
    if normal == Vec3::ZERO {
        // Started inside.
        return None;
    }
    Some((t0, normal))
}

impl MoveWorld for PlayerWorld<'_> {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Trace {
        let mut tr = self
            .world
            .trace_box(start, end, mins, maxs, TraceMask::Players);
        if tr.start_solid {
            return tr;
        }
        for b in self.boxes {
            let expanded = Aabb::new(b.min - maxs, b.max - mins);
            if let Some((f, n)) = sweep_point_box(start, end, &expanded) {
                // Same epsilon as the brush traces: stop just before the surface.
                let len = (end - start).length().max(1e-6);
                let f = (f - engine_physics::SURFACE_EPSILON / len).max(0.0);
                if f < tr.fraction {
                    tr.fraction = f;
                    tr.normal = n;
                    tr.end = start + (end - start) * f;
                    tr.brush = None;
                }
            }
        }
        tr
    }
}

/// Default hull box of a player at `origin`.
pub fn player_box(origin: Vec3, mins: Vec3, maxs: Vec3) -> Aabb {
    Aabb::new(origin + mins, origin + maxs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep() {
        let b = Aabb::new(Vec3::new(10.0, -1.0, -1.0), Vec3::new(12.0, 1.0, 1.0));
        let (f, n) = sweep_point_box(Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), &b).unwrap();
        assert!((f - 0.5).abs() < 1e-6 && n == Vec3::NEG_X);
        assert!(
            sweep_point_box(Vec3::new(11.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), &b).is_none()
        );
        assert!(sweep_point_box(Vec3::new(0.0, 5.0, 0.0), Vec3::new(20.0, 5.0, 0.0), &b).is_none());
    }
}
