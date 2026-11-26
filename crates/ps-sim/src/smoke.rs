//! Hybrid smoke: an opaque, bounded voxel volume (Valorant-like readability) that
//! fills the space around the detonation by flood fill, so it never leaks through
//! walls, plus a CS-like reaction to HE grenades (a hole is blown, then reforms).
//!
//! Server-authoritative: the voxel set is computed once on the server and sent to
//! clients; holes are computed from replicated HE events with the same formula.
//! Line of sight is symmetric (the same voxel test from both ends), which rules out
//! one-way smokes. Bullets do not modify the volume.

use engine_physics::CollisionWorld;
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmokeConfig {
    /// Total duration, seconds (12 s by default, configurable).
    pub duration: f32,
    /// Expansion time to the full radius.
    pub expand_time: f32,
    /// Fade (shrink) time at the end.
    pub fade_time: f32,
    pub radius: f32,
    pub height: f32,
    pub voxel: f32,
    /// HE: radius of the hole and time before reforming starts.
    pub he_hole_radius: f32,
    pub he_hole_time: f32,
    /// Time for the hole to refill from its edge to its centre.
    pub he_reform_time: f32,
}

impl Default for SmokeConfig {
    fn default() -> Self {
        Self {
            duration: 12.0,
            expand_time: 0.6,
            fade_time: 0.8,
            radius: 144.0,
            height: 112.0,
            voxel: 16.0,
            he_hole_radius: 112.0,
            he_hole_time: 0.6,
            he_reform_time: 1.6,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmokeVolume {
    pub id: u16,
    pub center: Vec3,
    /// Voxel grid origin (min corner) and dimensions.
    pub origin: Vec3,
    pub dims: IVec3,
    pub voxel: f32,
    /// Filled voxels (true = smoke can exist here).
    pub filled: Vec<bool>,
    /// Distance from the centre of each filled voxel, normalized to [0, 1].
    pub dist: Vec<f32>,
    pub start: f64,
    pub end: f64,
    /// Per voxel time until which it is cleared by an HE (0 = never).
    pub cleared_until: Vec<f64>,
}

impl SmokeVolume {
    /// Flood fill from the detonation point, stopped by solid geometry.
    pub fn fill(
        id: u16,
        center: Vec3,
        world: &CollisionWorld,
        cfg: &SmokeConfig,
        now: f64,
    ) -> SmokeVolume {
        let v = cfg.voxel;
        let r = cfg.radius;
        let n = (r / v).ceil() as i32;
        let hn = (cfg.height / v).ceil() as i32;
        let dims = IVec3::new(2 * n + 1, hn + 2, 2 * n + 1);
        // Centre voxel sits slightly above the detonation point.
        let base = center + Vec3::Y * (v * 0.5);
        let origin = base - Vec3::new((n as f32 + 0.5) * v, v, (n as f32 + 0.5) * v);
        let count = (dims.x * dims.y * dims.z) as usize;
        let mut filled = vec![false; count];
        let mut dist = vec![0.0; count];
        let idx = |p: IVec3| (p.x + dims.x * (p.y + dims.y * p.z)) as usize;
        let world_of = |p: IVec3| origin + (p.as_vec3() + Vec3::splat(0.5)) * v;
        let inside = |p: IVec3| -> Option<f32> {
            if p.x < 0 || p.y < 0 || p.z < 0 || p.x >= dims.x || p.y >= dims.y || p.z >= dims.z {
                return None;
            }
            let w = world_of(p) - center;
            // Flattened ellipsoid: full radius horizontally, `height` vertically.
            let e = (w.x * w.x + w.z * w.z) / (r * r)
                + (w.y.max(0.0) / cfg.height).powi(2)
                + (w.y.min(0.0) / (v * 1.5)).powi(2);
            (e <= 1.0).then(|| e.sqrt())
        };
        let mut start = IVec3::new(n, 1, n);
        // If the detonation voxel is inside a wall (grenade against a wall), pick the
        // nearest free voxel.
        if world.point_in_solid(world_of(start)) {
            start.y += 1;
        }
        let mut q = VecDeque::new();
        if inside(start).is_some() && !world.point_in_solid(world_of(start)) {
            filled[idx(start)] = true;
            q.push_back(start);
        }
        let dirs = [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ];
        while let Some(p) = q.pop_front() {
            for d in dirs {
                let np = p + d;
                let Some(e) = inside(np) else { continue };
                let i = idx(np);
                if filled[i] {
                    continue;
                }
                let a = world_of(p);
                let b = world_of(np);
                if world.point_in_solid(b) || !world.line_of_sight(a, b) {
                    continue;
                }
                filled[i] = true;
                dist[i] = e;
                q.push_back(np);
            }
        }
        SmokeVolume {
            id,
            center,
            origin,
            dims,
            voxel: v,
            filled,
            dist,
            start: now,
            end: now + cfg.duration as f64,
            cleared_until: vec![0.0; count],
        }
    }

    /// Rebuilds a volume received from the network (`filled` bitmask).
    pub fn from_parts(
        id: u16,
        center: Vec3,
        origin: Vec3,
        dims: IVec3,
        voxel: f32,
        filled: Vec<bool>,
        start: f64,
        end: f64,
        cfg: &SmokeConfig,
    ) -> SmokeVolume {
        let count = filled.len();
        let mut dist = vec![0.0; count];
        for z in 0..dims.z {
            for y in 0..dims.y {
                for x in 0..dims.x {
                    let i = (x + dims.x * (y + dims.y * z)) as usize;
                    if i < count && filled[i] {
                        let w = origin
                            + (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * voxel
                            - center;
                        let e = (w.x * w.x + w.z * w.z) / (cfg.radius * cfg.radius)
                            + (w.y.max(0.0) / cfg.height).powi(2)
                            + (w.y.min(0.0) / (voxel * 1.5)).powi(2);
                        dist[i] = e.sqrt().min(1.0);
                    }
                }
            }
        }
        SmokeVolume {
            id,
            center,
            origin,
            dims,
            voxel,
            filled,
            dist,
            start,
            end,
            cleared_until: vec![0.0; count],
        }
    }

    pub fn filled_count(&self) -> usize {
        self.filled.iter().filter(|f| **f).count()
    }

    /// Current radius scale in [0, 1] (expansion, then fade).
    pub fn scale(&self, now: f64, cfg: &SmokeConfig) -> f32 {
        if now < self.start || now >= self.end {
            return 0.0;
        }
        let grow = ((now - self.start) / cfg.expand_time.max(1e-3) as f64).min(1.0) as f32;
        let fade = ((self.end - now) / cfg.fade_time.max(1e-3) as f64).min(1.0) as f32;
        // Ease-out growth.
        let g = 1.0 - (1.0 - grow).powi(2);
        g.min(fade)
    }

    pub fn active(&self, now: f64) -> bool {
        now >= self.start && now < self.end
    }

    /// Is voxel `i` opaque at `now`?
    #[inline]
    pub fn voxel_on(&self, i: usize, now: f64, scale: f32) -> bool {
        self.filled[i] && self.dist[i] <= scale && self.cleared_until[i] <= now
    }

    pub fn voxel_index(&self, p: Vec3) -> Option<usize> {
        let l = ((p - self.origin) / self.voxel).floor();
        let c = IVec3::new(l.x as i32, l.y as i32, l.z as i32);
        if c.x < 0
            || c.y < 0
            || c.z < 0
            || c.x >= self.dims.x
            || c.y >= self.dims.y
            || c.z >= self.dims.z
        {
            return None;
        }
        Some((c.x + self.dims.x * (c.y + self.dims.y * c.z)) as usize)
    }

    pub fn voxel_center(&self, i: usize) -> Vec3 {
        let x = i as i32 % self.dims.x;
        let y = (i as i32 / self.dims.x) % self.dims.y;
        let z = i as i32 / (self.dims.x * self.dims.y);
        self.origin + (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * self.voxel
    }

    pub fn contains(&self, p: Vec3, now: f64, cfg: &SmokeConfig) -> bool {
        let s = self.scale(now, cfg);
        s > 0.0
            && self
                .voxel_index(p)
                .is_some_and(|i| self.voxel_on(i, now, s))
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.origin, self.origin + self.dims.as_vec3() * self.voxel)
    }

    /// True if the segment crosses at least one opaque voxel (3D DDA). Symmetric.
    pub fn blocks(&self, a: Vec3, b: Vec3, now: f64, cfg: &SmokeConfig) -> bool {
        let s = self.scale(now, cfg);
        if s <= 0.0 {
            return false;
        }
        let (bmin, bmax) = self.bounds();
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return self.contains(a, now, cfg);
        }
        let dir = d / len;
        // Clip the segment to the grid bounds.
        let aabb = engine_core::Aabb::new(bmin, bmax);
        let Some((t0, t1)) = aabb.ray_interval(a, dir) else {
            return false;
        };
        let t0 = t0.max(0.0);
        let t1 = t1.min(len);
        if t0 > t1 {
            return false;
        }
        let p = (a + dir * (t0 + 1e-3) - self.origin) / self.voxel;
        let mut c = IVec3::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let step = IVec3::new(
            dir.x.signum() as i32,
            dir.y.signum() as i32,
            dir.z.signum() as i32,
        );
        let inv = Vec3::new(
            1.0 / dir.x.abs().max(1e-9),
            1.0 / dir.y.abs().max(1e-9),
            1.0 / dir.z.abs().max(1e-9),
        ) * self.voxel;
        let frac = |pc: f32, cc: i32, st: i32| {
            if st > 0 {
                cc as f32 + 1.0 - pc
            } else {
                pc - cc as f32
            }
        };
        let mut tmax = Vec3::new(
            frac(p.x, c.x, step.x) * inv.x,
            frac(p.y, c.y, step.y) * inv.y,
            frac(p.z, c.z, step.z) * inv.z,
        ) + Vec3::splat(t0);
        let mut t = t0;
        let mut guard = 0;
        while t <= t1 && guard < 4096 {
            guard += 1;
            if c.x >= 0
                && c.y >= 0
                && c.z >= 0
                && c.x < self.dims.x
                && c.y < self.dims.y
                && c.z < self.dims.z
            {
                let i = (c.x + self.dims.x * (c.y + self.dims.y * c.z)) as usize;
                if self.voxel_on(i, now, s) {
                    return true;
                }
            } else if t > t0 + self.voxel {
                break;
            }
            if tmax.x <= tmax.y && tmax.x <= tmax.z {
                t = tmax.x;
                tmax.x += inv.x;
                c.x += step.x;
            } else if tmax.y <= tmax.z {
                t = tmax.y;
                tmax.y += inv.y;
                c.y += step.y;
            } else {
                t = tmax.z;
                tmax.z += inv.z;
                c.z += step.z;
            }
        }
        false
    }

    /// HE explosion: clears voxels in a sphere, they reform from the edge inward.
    pub fn blast(&mut self, at: Vec3, now: f64, cfg: &SmokeConfig) -> bool {
        let r = cfg.he_hole_radius;
        let (bmin, bmax) = self.bounds();
        let closest = at.clamp(bmin, bmax);
        if closest.distance(at) > r {
            return false;
        }
        let mut any = false;
        for i in 0..self.filled.len() {
            if !self.filled[i] {
                continue;
            }
            let d = self.voxel_center(i).distance(at);
            if d < r {
                let inner = 1.0 - d / r;
                let until = now + cfg.he_hole_time as f64 + (inner * cfg.he_reform_time) as f64;
                if until > self.cleared_until[i] {
                    self.cleared_until[i] = until;
                }
                any = true;
            }
        }
        any
    }

    /// Bitmask of `filled` for the network (LSB first).
    pub fn filled_bits(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.filled.len().div_ceil(8)];
        for (i, f) in self.filled.iter().enumerate() {
            if *f {
                out[i / 8] |= 1 << (i % 8);
            }
        }
        out
    }

    pub fn bits_to_filled(bits: &[u8], count: usize) -> Vec<bool> {
        (0..count)
            .map(|i| bits.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0))
            .collect()
    }
}

/// Line of sight through every active smoke.
pub fn smoke_blocks(smokes: &[SmokeVolume], a: Vec3, b: Vec3, now: f64, cfg: &SmokeConfig) -> bool {
    smokes.iter().any(|s| s.blocks(a, b, now, cfg))
}
