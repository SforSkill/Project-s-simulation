//! Recoil (deterministic) and movement spread (random, velocity dependent).

use crate::defs::WeaponDef;
use engine_core::DetRng;

/// Recoil offset `(pitch up, yaw left)` in degrees at a fractional shot index.
/// Linear interpolation between pattern entries; past the end the last entry holds.
pub fn recoil_offset(w: &WeaponDef, index: f32) -> (f32, f32) {
    let p = &w.recoil_pattern;
    if index <= 0.0 || p.is_empty() {
        return (0.0, 0.0);
    }
    let last = p.len() - 1;
    let i = index.floor() as usize;
    if i >= last {
        return (p[last][0], p[last][1]);
    }
    let f = index - i as f32;
    let a = p[i];
    let b = p[i + 1];
    (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f)
}

/// Movement spread half-angle in degrees. Zero when on the ground at or below the
/// accuracy threshold — there is no other source of spread.
pub fn movement_spread_deg(
    w: &WeaponDef,
    horizontal_speed: f32,
    on_ground: bool,
    zoom_level: u8,
) -> f32 {
    let mult = if !w.zoom.levels.is_empty() && zoom_level == 0 {
        w.unscoped_move_spread_mult
    } else {
        1.0
    };
    if !on_ground {
        return w.air_spread * mult;
    }
    let max = w.max_speed;
    let threshold = max * w.accurate_fraction;
    if horizontal_speed <= threshold {
        return 0.0;
    }
    let k = ((horizontal_speed - threshold) / (max - threshold)).clamp(0.0, 1.0);
    w.move_spread * k * mult
}

/// Random angular offset `(pitch up, yaw left)` inside a cone of half-angle `spread`,
/// uniformly distributed over the disc. Uses the server-side shot stream.
pub fn spread_offset(spread_deg: f32, rng: &mut DetRng) -> (f32, f32) {
    if spread_deg <= 0.0 {
        return (0.0, 0.0);
    }
    let r = spread_deg * rng.next_f32().sqrt();
    let theta = rng.next_f32() * std::f32::consts::TAU;
    (r * theta.sin(), r * theta.cos())
}
