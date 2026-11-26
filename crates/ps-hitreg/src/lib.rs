//! Hit registration: capsule hitboxes, pose history and bounded lag compensation.
//!
//! The server keeps, for every player, the poses of the last ticks. When a shot
//! arrives it is evaluated against the poses the shooter was *seeing*
//! (`UserCmd::view_time`), clamped to a bounded window (`LagCompPolicy`). Hitboxes
//! are a deterministic function of `(origin, yaw, duck)` shared by the client (debug
//! drawing, Shot Inspector) and the server (authoritative test), so both sides agree
//! exactly when the poses agree.

use engine_core::Aabb;
use engine_core::math::wrap_degrees;
use engine_weapons::HitGroup;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Animation-independent pose: what the hitboxes depend on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Pose {
    /// Feet position.
    pub origin: Vec3,
    /// Body yaw (degrees, same convention as `ViewAngles`).
    pub yaw: f32,
    /// 0 = standing, 1 = fully crouched.
    pub duck: f32,
}

impl Pose {
    pub fn lerp(&self, o: &Pose, t: f32) -> Pose {
        Pose {
            origin: self.origin.lerp(o.origin, t),
            yaw: self.yaw + wrap_degrees(o.yaw - self.yaw) * t,
            duck: self.duck + (o.duck - self.duck) * t,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
    pub group: HitGroup,
}

/// Local capsule (x right, y up, z backward — i.e. -z is forward) before posing.
const LOCAL: [(Vec3, Vec3, f32, HitGroup); 9] = [
    (
        Vec3::new(0.0, 65.5, -1.0),
        Vec3::new(0.0, 66.5, -1.0),
        6.0,
        HitGroup::Head,
    ),
    (
        Vec3::new(0.0, 49.0, 0.0),
        Vec3::new(0.0, 57.0, 0.0),
        9.0,
        HitGroup::Chest,
    ),
    (
        Vec3::new(0.0, 37.0, 0.0),
        Vec3::new(0.0, 44.0, 0.0),
        8.5,
        HitGroup::Stomach,
    ),
    (
        Vec3::new(-5.0, 4.0, 0.0),
        Vec3::new(-5.0, 32.0, 0.0),
        4.4,
        HitGroup::Legs,
    ),
    (
        Vec3::new(5.0, 4.0, 0.0),
        Vec3::new(5.0, 32.0, 0.0),
        4.4,
        HitGroup::Legs,
    ),
    // Arms brought forward, holding a weapon.
    (
        Vec3::new(-10.5, 55.0, 0.0),
        Vec3::new(-6.0, 46.0, -12.0),
        3.2,
        HitGroup::Arms,
    ),
    (
        Vec3::new(10.5, 55.0, 0.0),
        Vec3::new(5.0, 47.0, -13.0),
        3.2,
        HitGroup::Arms,
    ),
    (
        Vec3::new(-5.0, 47.0, -13.0),
        Vec3::new(1.0, 52.0, -20.0),
        2.8,
        HitGroup::Arms,
    ),
    (
        Vec3::new(5.0, 47.0, -13.0),
        Vec3::new(1.0, 52.0, -20.0),
        2.8,
        HitGroup::Arms,
    ),
];

/// Eye drop when fully crouched (64 → 46). The upper body moves down rigidly, the
/// legs (below `HIP_HEIGHT`) fold.
pub const DUCK_DROP: f32 = 18.0;
pub const HIP_HEIGHT: f32 = 36.0;

fn duck_height(y: f32, duck: f32) -> f32 {
    if y >= HIP_HEIGHT {
        y - DUCK_DROP * duck
    } else {
        y * (1.0 - DUCK_DROP / HIP_HEIGHT * duck)
    }
}
pub const HITBOX_COUNT: usize = LOCAL.len();

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hitboxes {
    pub capsules: [Capsule; HITBOX_COUNT],
    pub bounds: Aabb,
}

/// Places the hitboxes for a pose.
pub fn hitboxes(p: &Pose) -> Hitboxes {
    let (s, c) = p.yaw.to_radians().sin_cos();
    let right = Vec3::new(c, 0.0, -s);
    let back = Vec3::new(s, 0.0, c);
    let duck = p.duck.clamp(0.0, 1.0);
    let place = |v: Vec3| p.origin + right * v.x + Vec3::Y * duck_height(v.y, duck) + back * v.z;
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let capsules = LOCAL.map(|(a, b, r, g)| {
        let a = place(a);
        let b = place(b);
        min = min.min(a.min(b) - r);
        max = max.max(a.max(b) + r);
        Capsule {
            a,
            b,
            radius: r,
            group: g,
        }
    });
    Hitboxes {
        capsules,
        bounds: Aabb::new(min, max),
    }
}

/// Smallest `t >= 0` where the ray `o + d·t` enters the capsule. `d` normalized.
pub fn ray_capsule(o: Vec3, d: Vec3, c: &Capsule) -> Option<f32> {
    let ba = c.b - c.a;
    let oa = o - c.a;
    let baba = ba.dot(ba);
    let bard = ba.dot(d);
    let baoa = ba.dot(oa);
    let rdoa = d.dot(oa);
    let oaoa = oa.dot(oa);
    let r2 = c.radius * c.radius;
    let a = baba - bard * bard;
    let b = baba * rdoa - baoa * bard;
    let cc = baba * oaoa - baoa * baoa - r2 * baba;
    if a > 1e-8 {
        let h = b * b - a * cc;
        if h >= 0.0 {
            let t = (-b - h.sqrt()) / a;
            let y = baoa + t * bard;
            if y > 0.0 && y < baba && t >= 0.0 {
                return Some(t);
            }
        }
    }
    // Caps (also handles degenerate capsules = spheres).
    let mut best: Option<f32> = None;
    for center in [c.a, c.b] {
        let oc = o - center;
        let b = oc.dot(d);
        let cc = oc.dot(oc) - r2;
        let h = b * b - cc;
        if h >= 0.0 {
            let t = -b - h.sqrt();
            let t = if t < 0.0 && cc < 0.0 { 0.0 } else { t };
            if t >= 0.0 && best.is_none_or(|x| t < x) {
                best = Some(t);
            }
        }
    }
    best
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyHit {
    pub t: f32,
    pub group: HitGroup,
    pub point: Vec3,
}

/// Nearest hitbox hit along a ray, within `max_t`.
pub fn ray_vs_hitboxes(h: &Hitboxes, o: Vec3, d: Vec3, max_t: f32) -> Option<BodyHit> {
    let (t0, _) = h.bounds.ray_interval(o, d)?;
    if t0 > max_t {
        return None;
    }
    let mut best: Option<BodyHit> = None;
    for c in &h.capsules {
        if let Some(t) = ray_capsule(o, d, c)
            && t <= max_t
            && best.is_none_or(|b| t < b.t)
        {
            best = Some(BodyHit {
                t,
                group: c.group,
                point: o + d * t,
            });
        }
    }
    best
}

/// One recorded pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoseRecord {
    /// Simulation time in ticks (fractional allowed).
    pub time: f64,
    pub pose: Pose,
    pub alive: bool,
    /// Teleports (respawn) break interpolation.
    pub teleported: bool,
}

/// Ring buffer of past poses of one player.
#[derive(Debug, Clone, Default)]
pub struct PoseHistory {
    records: VecDeque<PoseRecord>,
    capacity: usize,
}

impl PoseHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity: capacity.max(2),
        }
    }

    pub fn push(&mut self, r: PoseRecord) {
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(r);
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }

    pub fn latest(&self) -> Option<&PoseRecord> {
        self.records.back()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Pose at `time` (ticks), interpolated between the two surrounding records.
    /// Returns `None` if the player was dead at that time.
    pub fn sample(&self, time: f64) -> Option<Pose> {
        let first = self.records.front()?;
        if time <= first.time {
            return first.alive.then_some(first.pose);
        }
        // Binary search on the monotonic times.
        let idx = self.records.partition_point(|r| r.time <= time);
        if idx >= self.records.len() {
            let last = self.records.back()?;
            return last.alive.then_some(last.pose);
        }
        let a = &self.records[idx - 1];
        let b = &self.records[idx];
        if !a.alive || !b.alive {
            return None;
        }
        if b.teleported {
            return Some(a.pose);
        }
        let t = ((time - a.time) / (b.time - a.time).max(1e-9)) as f32;
        Some(a.pose.lerp(&b.pose, t))
    }
}

/// Server policy for lag compensation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LagCompPolicy {
    pub enabled: bool,
    /// Maximum rewind in seconds (CS uses 200 ms with `sv_maxunlag`).
    pub max_rewind: f32,
    /// Accepted error between the claimed view time and the time the server
    /// estimates the client was viewing (latency + interpolation), in seconds.
    pub tolerance: f32,
}

impl Default for LagCompPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_rewind: 0.2,
            tolerance: 0.05,
        }
    }
}

impl LagCompPolicy {
    /// Time (ticks) used to evaluate a shot.
    /// * `now`: current server tick;
    /// * `claimed`: `UserCmd::view_time` sent by the client;
    /// * `expected`: server estimate of the client's view time (`now - rtt/2 - interp`).
    ///
    /// Returns `(time, clamped)`.
    pub fn rewind_time(&self, now: f64, claimed: f64, expected: f64, tick_dt: f64) -> (f64, bool) {
        if !self.enabled {
            return (now, claimed != now);
        }
        let max_back = self.max_rewind as f64 / tick_dt;
        let tol = self.tolerance as f64 / tick_dt;
        let lo = (now - max_back).max(expected - tol);
        let hi = now.min(expected + tol).max(lo);
        let t = claimed.clamp(lo, hi);
        (t, (t - claimed).abs() > 1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing() -> Pose {
        Pose {
            origin: Vec3::new(0.0, 0.0, -500.0),
            yaw: 0.0,
            duck: 0.0,
        }
    }

    fn shoot(p: &Pose, height: f32) -> Option<BodyHit> {
        let o = Vec3::new(0.0, height, 0.0);
        let d = (p.origin + Vec3::Y * height - o).normalize();
        ray_vs_hitboxes(&hitboxes(p), o, d, 8192.0)
    }

    #[test]
    fn groups_by_height() {
        let p = standing();
        assert_eq!(shoot(&p, 66.0).unwrap().group, HitGroup::Head);
        assert_eq!(shoot(&p, 53.0).unwrap().group, HitGroup::Chest);
        assert_eq!(shoot(&p, 40.0).unwrap().group, HitGroup::Stomach);
        assert!(shoot(&p, 20.0).is_none(), "between the legs");
        assert!(shoot(&p, 80.0).is_none());
    }

    #[test]
    fn legs_hit_off_center() {
        let p = Pose {
            origin: Vec3::new(5.0, 0.0, -500.0),
            ..standing()
        };
        let o = Vec3::new(0.0, 20.0, 0.0);
        let d = (Vec3::new(10.0, 20.0, -500.0) - o).normalize();
        assert_eq!(
            ray_vs_hitboxes(&hitboxes(&p), o, d, 8192.0).unwrap().group,
            HitGroup::Legs
        );
    }

    #[test]
    fn crouch_lowers_the_head() {
        let p = Pose {
            duck: 1.0,
            ..standing()
        };
        assert!(shoot(&p, 66.0).is_none());
        assert_eq!(shoot(&p, 66.0 - DUCK_DROP).unwrap().group, HitGroup::Head);
    }

    #[test]
    fn yaw_rotates_the_arms() {
        let a = hitboxes(&standing());
        let b = hitboxes(&Pose {
            yaw: 90.0,
            ..standing()
        });
        assert_ne!(a.capsules[6].b, b.capsules[6].b);
        // Head stays on the vertical axis (almost).
        assert!((a.capsules[0].a.y - b.capsules[0].a.y).abs() < 1e-4);
    }

    #[test]
    fn capsule_math() {
        let c = Capsule {
            a: Vec3::new(0.0, 0.0, -10.0),
            b: Vec3::new(0.0, 10.0, -10.0),
            radius: 2.0,
            group: HitGroup::Chest,
        };
        let t = ray_capsule(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Z, &c).unwrap();
        assert!((t - 8.0).abs() < 1e-4);
        let t = ray_capsule(Vec3::new(0.0, 11.5, 0.0), Vec3::NEG_Z, &c).unwrap();
        assert!(t > 8.0 && t < 10.0);
        assert!(ray_capsule(Vec3::new(3.0, 5.0, 0.0), Vec3::NEG_Z, &c).is_none());
        assert!(ray_capsule(Vec3::new(0.0, 5.0, 0.0), Vec3::Z, &c).is_none());
    }

    #[test]
    fn history_interpolates_and_handles_death() {
        let mut h = PoseHistory::new(8);
        for i in 0..5 {
            h.push(PoseRecord {
                time: i as f64,
                pose: Pose {
                    origin: Vec3::X * (i as f32 * 10.0),
                    ..Default::default()
                },
                alive: i != 3,
                teleported: false,
            });
        }
        assert_eq!(h.sample(1.5).unwrap().origin, Vec3::X * 15.0);
        assert!(h.sample(2.5).is_none());
        assert_eq!(h.sample(-1.0).unwrap().origin, Vec3::ZERO);
        for i in 5..20 {
            h.push(PoseRecord {
                time: i as f64,
                pose: Pose::default(),
                alive: true,
                teleported: false,
            });
        }
        assert_eq!(h.len(), 8);
    }

    #[test]
    fn rewind_is_bounded() {
        let p = LagCompPolicy::default();
        let dt = 1.0 / 128.0;
        // Honest claim close to expectation is kept exactly.
        assert_eq!(p.rewind_time(1000.0, 990.0, 991.0, dt), (990.0, false));
        // Claiming 1 s in the past is clamped to the tolerance window.
        let (t, clamped) = p.rewind_time(1000.0, 872.0, 991.0, dt);
        assert!(clamped && (t - (991.0 - 0.05 * 128.0)).abs() < 1e-3);
        // Never beyond max_rewind, even if the server expectation is far.
        let (t, _) = p.rewind_time(1000.0, 900.0, 900.0, dt);
        assert!((t - (1000.0 - 0.2 * 128.0)).abs() < 1e-3);
        // Never in the future.
        assert_eq!(p.rewind_time(1000.0, 1010.0, 1000.0, dt).0, 1000.0);
    }
}
