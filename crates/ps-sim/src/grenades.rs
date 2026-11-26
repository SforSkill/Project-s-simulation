//! Grenade projectiles and molotov fire.

use engine_core::math::angles_to_forward;
use engine_core::{PlayerId, Team, ViewAngles};
use engine_physics::CollisionWorld;
use engine_physics::world::TraceMask;
use engine_weapons::GrenadeKind;
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrenadeConfig {
    pub throw_speed: f32,
    pub gravity_scale: f32,
    pub elasticity: f32,
    pub fuse_flash: f32,
    pub fuse_he: f32,
    pub molotov_max_air: f32,
    pub he_damage: f32,
    pub he_radius: f32,
    pub flash_max_duration: f32,
    pub flash_range: f32,
    pub fire_duration: f32,
    pub fire_radius: f32,
    pub fire_dps: f32,
}

impl Default for GrenadeConfig {
    fn default() -> Self {
        Self {
            throw_speed: 750.0,
            gravity_scale: 0.4,
            elasticity: 0.45,
            fuse_flash: 1.6,
            fuse_he: 1.6,
            molotov_max_air: 2.0,
            he_damage: 98.0,
            he_radius: 350.0,
            flash_max_duration: 4.9,
            flash_range: 2000.0,
            fire_duration: 7.0,
            fire_radius: 150.0,
            fire_dps: 40.0,
        }
    }
}

pub const GRENADE_HALF: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Grenade {
    pub id: u16,
    pub kind: GrenadeKind,
    pub thrower: PlayerId,
    pub team: Team,
    pub pos: Vec3,
    pub vel: Vec3,
    pub thrown_at: f64,
    pub rest_since: Option<f64>,
    pub bounces: u16,
}

/// Throw origin and velocity (CS-like: pitch bias, strength 0..1, player velocity).
pub fn throw_params(
    eye: Vec3,
    view: ViewAngles,
    player_vel: Vec3,
    strength: f32,
    cfg: &GrenadeConfig,
    world: &CollisionWorld,
) -> (Vec3, Vec3) {
    let pitch = (10.0 + view.pitch * 100.0 / 90.0).clamp(-89.0, 89.0);
    let fwd = angles_to_forward(pitch, view.yaw);
    let speed = (cfg.throw_speed * 0.9).clamp(15.0, cfg.throw_speed)
        * (strength.clamp(0.0, 1.0) * 0.7 + 0.3);
    let vel = fwd * speed + player_vel * 1.25;
    let h = Vec3::splat(GRENADE_HALF);
    let tr = world.trace_box(eye, eye + fwd * 16.0, -h, h, TraceMask::Bullets);
    let start = if tr.start_solid {
        eye
    } else {
        tr.end - fwd * 0.5
    };
    (start, vel)
}

pub enum GrenadeStep {
    Flying,
    Detonate,
    /// Molotov exploding in the air: no fire.
    Fizzle,
}

impl Grenade {
    /// Advances `dt` seconds. Returns whether it detonates.
    pub fn step(
        &mut self,
        dt: f32,
        now: f64,
        cfg: &GrenadeConfig,
        gravity: f32,
        world: &CollisionWorld,
    ) -> GrenadeStep {
        let age = (now - self.thrown_at) as f32;
        match self.kind {
            GrenadeKind::Flash if age >= cfg.fuse_flash => return GrenadeStep::Detonate,
            GrenadeKind::He if age >= cfg.fuse_he => return GrenadeStep::Detonate,
            GrenadeKind::Molotov if age >= cfg.molotov_max_air => return GrenadeStep::Fizzle,
            _ => {}
        }
        let g = gravity * cfg.gravity_scale;
        let h = Vec3::splat(GRENADE_HALF);
        let mut remaining = dt;
        let mut on_ground = false;
        for _ in 0..4 {
            if remaining <= 0.0 {
                break;
            }
            let v0 = self.vel;
            self.vel.y -= g * remaining;
            let target = self.pos + (v0 + self.vel) * 0.5 * remaining;
            let tr = world.trace_box(self.pos, target, -h, h, TraceMask::Bullets);
            if tr.start_solid {
                // Stuck: stop it where it is.
                self.vel = Vec3::ZERO;
                on_ground = true;
                break;
            }
            self.pos = tr.end;
            if tr.fraction >= 1.0 {
                break;
            }
            remaining *= 1.0 - tr.fraction;
            let n = tr.normal;
            if self.kind == GrenadeKind::Molotov && n.y > 0.7 {
                return GrenadeStep::Detonate;
            }
            // Reflect with energy loss.
            let vn = self.vel.dot(n);
            self.vel -= n * vn * (1.0 + cfg.elasticity);
            self.vel *= cfg.elasticity.max(0.3) + 0.3;
            self.bounces += 1;
            if n.y > 0.7 {
                on_ground = true;
                if self.vel.length() < 20.0 {
                    self.vel = Vec3::ZERO;
                }
            }
        }
        if on_ground && self.vel.length_squared() < 1.0 {
            self.vel = Vec3::ZERO;
            let since = *self.rest_since.get_or_insert(now);
            if self.kind == GrenadeKind::Smoke && now - since >= 0.1 {
                return GrenadeStep::Detonate;
            }
        } else if self.vel.length_squared() > 1.0 {
            self.rest_since = None;
        }
        // Smoke detonates after a long time even if it keeps sliding.
        if self.kind == GrenadeKind::Smoke && age > 6.0 {
            return GrenadeStep::Detonate;
        }
        GrenadeStep::Flying
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub id: u16,
    pub thrower: PlayerId,
    pub team: Team,
    pub origin: Vec3,
    pub cells: Vec<Vec3>,
    pub start: f64,
    pub end: f64,
}

pub const FIRE_CELL: f32 = 40.0;

impl Fire {
    /// Spreads fire cells on the floor around `origin`, never through walls.
    pub fn spread(
        id: u16,
        thrower: PlayerId,
        team: Team,
        origin: Vec3,
        now: f64,
        cfg: &GrenadeConfig,
        world: &CollisionWorld,
    ) -> Fire {
        let up = origin + Vec3::Y * 8.0;
        let mut cells = Vec::new();
        let n = (cfg.fire_radius / FIRE_CELL).ceil() as i32;
        for i in -n..=n {
            for j in -n..=n {
                let off = Vec3::new(i as f32, 0.0, j as f32) * FIRE_CELL;
                if off.length() > cfg.fire_radius {
                    continue;
                }
                let top = up + off;
                if !world.line_of_sight(up, top) {
                    continue;
                }
                let tr = world.trace_ray(top, top - Vec3::Y * 64.0, TraceMask::Players);
                if tr.fraction < 1.0 && tr.normal.y > 0.7 {
                    cells.push(tr.end + Vec3::Y * 0.5);
                }
            }
        }
        Fire {
            id,
            thrower,
            team,
            origin,
            cells,
            start: now,
            end: now + cfg.fire_duration as f64,
        }
    }

    pub fn touches(&self, feet: Vec3) -> bool {
        self.cells.iter().any(|c| {
            let d = feet - *c;
            d.y.abs() < 40.0 && Vec3::new(d.x, 0.0, d.z).length() < FIRE_CELL * 0.75 + 16.0
        })
    }
}
