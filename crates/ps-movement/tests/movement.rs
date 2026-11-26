mod common;

use common::*;
use engine_core::Aabb;
use engine_core::cmd::{Buttons, UserCmd};
use engine_core::math::ViewAngles;
use engine_movement::{MoveState, MovementConfig, simulate_cmd};
use engine_physics::{Brush, CollisionWorld, MaterialId};
use glam::Vec3;

const F: u32 = Buttons::FORWARD;
const R: u32 = Buttons::RIGHT;

#[test]
fn reaches_exact_max_speed() {
    let cfg = MovementConfig::default();
    let w = flat_world();
    let mut s = spawn(&w, &cfg);
    for _ in 0..128 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert!((hspeed(&s) - 250.0).abs() < 0.01, "{}", hspeed(&s));
    assert!(s.on_ground);
    assert!(s.origin.z < -150.0, "moved forward (−Z): {:?}", s.origin);
}

#[test]
fn walk_and_crouch_speeds() {
    let cfg = MovementConfig::default();
    let w = flat_world();
    for (b, expected) in [
        (F | Buttons::WALK, 250.0 * 0.52),
        (F | Buttons::DUCK, 250.0 * 0.34),
    ] {
        let mut s = spawn(&w, &cfg);
        for _ in 0..256 {
            step(&mut s, Buttons(b), 128, 250.0, &w, &cfg);
        }
        assert!(
            (hspeed(&s) - expected).abs() < 0.05,
            "{b}: {} vs {expected}",
            hspeed(&s)
        );
    }
}

#[test]
fn jump_apex_is_about_57_units_and_crouch_jump_higher() {
    let cfg = MovementConfig::default();
    let w = flat_world();
    let mut s = spawn(&w, &cfg);
    let mut apex: f32 = 0.0;
    step(&mut s, Buttons(Buttons::JUMP), 128, 250.0, &w, &cfg);
    for _ in 0..200 {
        step(&mut s, Buttons(0), 128, 250.0, &w, &cfg);
        apex = apex.max(s.origin.y);
    }
    assert!((apex - 57.0).abs() < 1.0, "apex {apex}");
    assert!(s.on_ground);

    // Crouch jump: feet rise by 18 more.
    let mut s = spawn(&w, &cfg);
    let mut apex_c: f32 = 0.0;
    step(&mut s, Buttons(Buttons::JUMP), 128, 250.0, &w, &cfg);
    for _ in 0..200 {
        step(&mut s, Buttons(Buttons::DUCK), 128, 250.0, &w, &cfg);
        apex_c = apex_c.max(s.origin.y);
    }
    assert!(apex_c > apex + 15.0, "crouch jump {apex_c} vs {apex}");
}

#[test]
fn holding_jump_does_not_auto_bhop() {
    let cfg = MovementConfig::default();
    let w = flat_world();
    let mut s = spawn(&w, &cfg);
    let mut jumps = 0;
    let mut was_ground = true;
    for _ in 0..512 {
        step(&mut s, Buttons(Buttons::JUMP), 128, 250.0, &w, &cfg);
        if was_ground && !s.on_ground {
            jumps += 1;
        }
        was_ground = s.on_ground;
    }
    assert_eq!(jumps, 1);
}

#[test]
fn air_strafe_gains_speed_bhop() {
    // Classic air strafe: hold right and turn right smoothly while airborne.
    let cfg = MovementConfig::default();
    let w = flat_world();
    let mut s = spawn(&w, &cfg);
    for _ in 0..128 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    let start_speed = hspeed(&s);
    let mut yaw = 0.0f32;
    let mut cmd = UserCmd {
        buttons_start: Buttons(Buttons::JUMP),
        ..Default::default()
    };
    for i in 0..400 {
        let prev = yaw;
        yaw -= 1.2; // turning right
        cmd.view_start = ViewAngles::new(0.0, prev);
        cmd.view_end = ViewAngles::new(0.0, yaw);
        // Re-press jump on landing (scroll-wheel style: press on alternate ticks).
        let jump = if i % 2 == 0 { Buttons::JUMP } else { 0 };
        cmd.buttons_start = Buttons(R | jump);
        simulate_cmd(&mut s, &cmd, 250.0, false, 1.0 / 128.0, &cfg, &w);
    }
    assert!(
        hspeed(&s) > start_speed + 40.0,
        "bhop+strafe {} vs {}",
        hspeed(&s),
        start_speed
    );
}

fn stairs_world(step_h: f32) -> CollisionWorld {
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, -32.0, -2000.0),
            Vec3::new(2000.0, 0.0, 2000.0),
        ),
        MaterialId(0),
    )];
    for i in 0..8 {
        let z1 = -100.0 - i as f32 * 16.0;
        b.push(Brush::from_box(
            Aabb::new(
                Vec3::new(-200.0, 0.0, -2000.0),
                Vec3::new(200.0, step_h * (i + 1) as f32, z1),
            ),
            MaterialId(0),
        ));
    }
    CollisionWorld::new(b)
}

#[test]
fn climbs_stairs_up_to_step_size() {
    let cfg = MovementConfig::default();
    let w = stairs_world(16.0);
    let mut s = spawn(&w, &cfg);
    for _ in 0..256 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert!(
        (s.origin.y - 128.0).abs() < 0.5,
        "top of the stairs: {:?}",
        s.origin
    );
    assert!(hspeed(&s) > 200.0, "stairs keep speed: {}", hspeed(&s));

    let w = stairs_world(24.0);
    let mut s = spawn(&w, &cfg);
    for _ in 0..256 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert!(s.origin.y < 1.0, "24u steps are too high: {:?}", s.origin);
    assert!(s.origin.z > -100.0, "blocked by the first step");
}

#[test]
fn walks_up_and_down_ramps() {
    let cfg = MovementConfig::default();
    // 30° ramp (walkable) going up toward −Z.
    let len = 256.0f32;
    let h = len * 30f32.to_radians().tan();
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, -32.0, -2000.0),
            Vec3::new(2000.0, 0.0, 2000.0),
        ),
        MaterialId(0),
    )];
    b.push(Brush::ramp(
        Aabb::new(
            Vec3::new(-200.0, 0.0, -100.0 - len),
            Vec3::new(200.0, h, -100.0),
        ),
        Vec3::NEG_Z,
        MaterialId(0),
    ));
    b.push(Brush::from_box(
        Aabb::new(
            Vec3::new(-200.0, 0.0, -1000.0),
            Vec3::new(200.0, h, -100.0 - len),
        ),
        MaterialId(0),
    ));
    let w = CollisionWorld::new(b);
    let mut s = spawn(&w, &cfg);
    for _ in 0..384 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert!(
        (s.origin.y - h).abs() < 1.0,
        "on top: {:?} (h={h})",
        s.origin
    );
    // Walk back down: must stay glued to the slope (no "stair flying").
    let mut airborne_ticks = 0;
    for _ in 0..384 {
        step(&mut s, Buttons(Buttons::BACK), 128, 250.0, &w, &cfg);
        if !s.on_ground {
            airborne_ticks += 1;
        }
    }
    assert!(s.origin.y < 1.0, "back down: {:?}", s.origin);
    assert!(
        airborne_ticks < 4,
        "left the ground {airborne_ticks} ticks going down"
    );
}

#[test]
fn steep_slope_is_not_walkable() {
    let cfg = MovementConfig::default();
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, -32.0, -2000.0),
            Vec3::new(2000.0, 0.0, 2000.0),
        ),
        MaterialId(0),
    )];
    b.push(Brush::ramp(
        Aabb::new(
            Vec3::new(-200.0, 0.0, -300.0),
            Vec3::new(200.0, 300.0, -100.0),
        ),
        Vec3::NEG_Z,
        MaterialId(0),
    ));
    let w = CollisionWorld::new(b);
    let mut s = spawn(&w, &cfg);
    for _ in 0..512 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert!(
        s.origin.y < 60.0,
        "56° slope must not be climbable: {:?}",
        s.origin
    );
}

#[test]
fn walls_block_and_slide() {
    let cfg = MovementConfig::default();
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, -32.0, -2000.0),
            Vec3::new(2000.0, 0.0, 2000.0),
        ),
        MaterialId(0),
    )];
    b.push(Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, 0.0, -200.0),
            Vec3::new(2000.0, 256.0, -150.0),
        ),
        MaterialId(0),
    ));
    let w = CollisionWorld::new(b);
    let mut s = spawn(&w, &cfg);
    let cmd = UserCmd {
        buttons_start: Buttons(F),
        view_start: ViewAngles::new(0.0, -30.0),
        view_end: ViewAngles::new(0.0, -30.0),
        ..Default::default()
    };
    for _ in 0..512 {
        simulate_cmd(&mut s, &cmd, 250.0, false, 1.0 / 128.0, &cfg, &w);
    }
    assert!(
        s.origin.z > -150.0 + 15.0,
        "never inside the wall: {:?}",
        s.origin
    );
    assert!(s.origin.x > 300.0, "slides along the wall: {:?}", s.origin);
}

#[test]
fn cannot_stand_under_low_ceiling() {
    let cfg = MovementConfig::default();
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-2000.0, -32.0, -2000.0),
            Vec3::new(2000.0, 0.0, 2000.0),
        ),
        MaterialId(0),
    )];
    b.push(Brush::from_box(
        Aabb::new(
            Vec3::new(-100.0, 60.0, -400.0),
            Vec3::new(100.0, 100.0, -100.0),
        ),
        MaterialId(0),
    ));
    let w = CollisionWorld::new(b);
    let mut s = spawn(&w, &cfg);
    for _ in 0..300 {
        step(&mut s, Buttons(F | Buttons::DUCK), 128, 250.0, &w, &cfg);
    }
    assert!(
        s.ducked && s.origin.z < -150.0,
        "crouched under: {:?}",
        s.origin
    );
    for _ in 0..60 {
        step(&mut s, Buttons(0), 128, 250.0, &w, &cfg);
    }
    assert!(s.ducked, "no room to stand");
}

#[test]
fn deterministic_bit_for_bit() {
    let cfg = MovementConfig::default();
    let w = stairs_world(16.0);
    let timeline = [
        (0.0, F),
        (0.31, F | R),
        (0.5, Buttons::JUMP | F),
        (0.7, Buttons::LEFT),
        (1.1, 0),
    ];
    let mut a = MoveState::new(Vec3::Y);
    let mut b = a;
    run_timeline(&mut a, &timeline, 2.0, 128, 250.0, &w, &cfg, |_, _| {});
    run_timeline(&mut b, &timeline, 2.0, 128, 250.0, &w, &cfg, |_, _| {});
    assert_eq!(
        a.origin.to_array().map(f32::to_bits),
        b.origin.to_array().map(f32::to_bits)
    );
    assert_eq!(
        a.velocity.to_array().map(f32::to_bits),
        b.velocity.to_array().map(f32::to_bits)
    );
}

#[test]
fn tagging_slows_without_full_stop_and_recovers() {
    let cfg = MovementConfig::default();
    let w = flat_world();
    let mut s = spawn(&w, &cfg);
    for _ in 0..128 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    s.apply_tag(0.9, cfg.tag_floor);
    assert_eq!(s.velocity_modifier, cfg.tag_floor);
    step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    let slowed = hspeed(&s);
    assert!(
        slowed > 0.34 * 250.0 + 1.0,
        "never below the accuracy threshold: {slowed}"
    );
    assert!(slowed < 140.0, "tag applied: {slowed}");
    for _ in 0..64 {
        step(&mut s, Buttons(F), 128, 250.0, &w, &cfg);
    }
    assert_eq!(s.velocity_modifier, 1.0, "recovered after 0.5 s");
}
