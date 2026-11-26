#![allow(dead_code)]

use engine_core::Aabb;
use engine_core::cmd::{ButtonEvent, Buttons, UserCmd};
use engine_core::math::ViewAngles;
use engine_movement::{MoveState, MovementConfig, simulate_cmd};
use engine_physics::{Brush, CollisionWorld, MaterialId};
use glam::Vec3;

pub fn flat_world() -> CollisionWorld {
    CollisionWorld::new(vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-8000.0, -32.0, -8000.0),
            Vec3::new(8000.0, 0.0, 8000.0),
        ),
        MaterialId(0),
    )])
}

pub fn spawn(world: &CollisionWorld, cfg: &MovementConfig) -> MoveState {
    let mut s = MoveState::new(Vec3::new(0.0, 1.0, 0.0));
    // Settle on the floor.
    for _ in 0..16 {
        step(&mut s, Buttons(0), 128, 250.0, world, cfg);
    }
    s
}

/// One tick with constant buttons, view yaw 0 (forward = −Z, right = +X).
pub fn step(
    s: &mut MoveState,
    b: Buttons,
    hz: u32,
    max_speed: f32,
    world: &CollisionWorld,
    cfg: &MovementConfig,
) {
    let cmd = UserCmd {
        buttons_start: b,
        ..Default::default()
    };
    simulate_cmd(s, &cmd, max_speed, false, 1.0 / hz as f32, cfg, world);
}

/// A key timeline: (time in seconds, buttons from that time on). Builds the
/// per-tick commands with sub-tick events, exactly as the client does.
pub fn run_timeline(
    s: &mut MoveState,
    timeline: &[(f64, u32)],
    duration: f64,
    hz: u32,
    max_speed: f32,
    world: &CollisionWorld,
    cfg: &MovementConfig,
    mut on_tick: impl FnMut(f64, &MoveState),
) {
    let dt = 1.0 / hz as f64;
    let ticks = (duration / dt).round() as u32;
    let mut cur = Buttons(0);
    let mut idx = 0;
    // Apply events at t <= 0 immediately.
    while idx < timeline.len() && timeline[idx].0 <= 0.0 {
        cur = Buttons(timeline[idx].1);
        idx += 1;
    }
    for t in 0..ticks {
        let t0 = t as f64 * dt;
        let t1 = t0 + dt;
        let mut cmd = UserCmd {
            tick: t,
            buttons_start: cur,
            view_start: ViewAngles::default(),
            view_end: ViewAngles::default(),
            ..Default::default()
        };
        while idx < timeline.len() && timeline[idx].0 < t1 {
            let frac = (((timeline[idx].0 - t0) / dt) * 256.0)
                .floor()
                .clamp(0.0, 255.0) as u8;
            cur = Buttons(timeline[idx].1);
            cmd.events.push(ButtonEvent {
                frac,
                buttons: cur,
                view: ViewAngles::default(),
            });
            idx += 1;
        }
        simulate_cmd(s, &cmd, max_speed, false, dt as f32, cfg, world);
        on_tick(t1, s);
    }
}

pub fn hspeed(s: &MoveState) -> f32 {
    s.horizontal_speed()
}
