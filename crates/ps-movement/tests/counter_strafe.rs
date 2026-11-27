//! Counter-strafe: Source physics vs buffered cut.
//!
//! Run with `cargo test -p engine-movement --test counter_strafe -- --nocapture`
//! to print the comparison table recorded in `docs/validation-report.md`.

mod common;

use common::*;
use engine_core::cmd::Buttons;
use engine_movement::{CounterStrafeMode, MoveState, MovementConfig};

const L: u32 = Buttons::LEFT;
const R: u32 = Buttons::RIGHT;
/// Rifle speed and its accuracy threshold (34 %).
const SPEED: f32 = 220.0;
const ACCURATE: f32 = SPEED * 0.34;

#[derive(Debug, Clone, Copy)]
struct Outcome {
    /// From the counter press to speed ≤ accuracy threshold.
    t_accurate: f64,
    /// From the counter press to a full stop (< 0.1 u/s), if it happens.
    t_stop: Option<f64>,
    /// Distance travelled after the counter press until the stop (or the end).
    distance: f32,
    /// Speed just after the counter key release.
    speed_after_release: f32,
    /// Highest speed reached in the opposite direction after the counter press.
    max_reverse: f32,
}

fn cfg(mode: CounterStrafeMode) -> MovementConfig {
    MovementConfig {
        counter_strafe: mode,
        ..Default::default()
    }
}

/// Runs right at full speed, then presses left at t=1 s for `hold` seconds.
/// `hold = 0` means "release only" (no counter).
fn scenario(mode: CounterStrafeMode, hold: f64, hz: u32, both_keys: bool) -> Outcome {
    let c = cfg(mode);
    let w = flat_world();
    let mut s = MoveState::new(glam::Vec3::Y);
    let t_counter = 1.0;
    let mut timeline = vec![(0.0, R)];
    if hold > 0.0 {
        if both_keys {
            timeline.push((t_counter, R | L));
        } else {
            timeline.push((t_counter, L));
        }
        timeline.push((t_counter + hold, 0));
    } else {
        timeline.push((t_counter, 0));
    }
    let mut t_accurate = None;
    let mut t_stop = None;
    let mut x_at_counter = 0.0;
    let mut x_at_stop = None;
    let mut speed_after_release = f32::NAN;
    let mut last_x = 0.0;
    let release_t = t_counter + hold.max(0.0);
    let mut max_reverse: f32 = 0.0;
    run_timeline(&mut s, &timeline, 2.0, hz, SPEED, &w, &c, |t, st| {
        let sp = st.horizontal_speed();
        if t <= t_counter + 1e-9 {
            x_at_counter = st.origin.x;
        } else {
            if t_accurate.is_none() && sp <= ACCURATE {
                t_accurate = Some(t - t_counter);
            }
            if t_stop.is_none() && sp < 0.1 {
                t_stop = Some(t - t_counter);
                x_at_stop = Some(st.origin.x);
            }
            if speed_after_release.is_nan() && t >= release_t + 1e-9 {
                speed_after_release = sp;
            }
        }
        if t > t_counter {
            max_reverse = max_reverse.max(-st.velocity.x);
        }
        last_x = st.origin.x;
    });
    Outcome {
        t_accurate: t_accurate.unwrap_or(f64::INFINITY),
        t_stop,
        distance: x_at_stop.unwrap_or(last_x) - x_at_counter,
        speed_after_release,
        max_reverse,
    }
}

/// Counter hold time that brings the speed to zero (continuous Source model).
fn exact_hold(_hz: u32) -> f64 {
    let c = cfg(CounterStrafeMode::Source);
    engine_movement::controller::counter_stop_time(SPEED, SPEED, &c) as f64
}

#[test]
fn comparison_table() {
    println!(
        "\n| mode | scénario | Hz | t précis (ms) | t arrêt (ms) | distance (u) | vitesse après relâche (u/s) |"
    );
    println!("|---|---|---|---|---|---|---|");
    for hz in [64u32, 128, 256] {
        let exact = exact_hold(hz);
        for (label, hold, both) in [
            ("relâche seule", 0.0, false),
            ("contre 80 %", exact * 0.8, false),
            ("contre exact", exact, false),
            ("contre 130 %", exact * 1.3, false),
            ("A+D ensemble", exact, true),
        ] {
            for mode in [CounterStrafeMode::Source, CounterStrafeMode::Buffered] {
                let o = scenario(mode, hold, hz, both);
                println!(
                    "| {:?} | {label} | {hz} | {:.1} | {} | {:.2} | {:.2} |",
                    mode,
                    o.t_accurate * 1000.0,
                    o.t_stop
                        .map(|t| format!("{:.1}", t * 1000.0))
                        .unwrap_or("—".into()),
                    o.distance,
                    o.speed_after_release
                );
            }
        }
    }
}

#[test]
fn same_braking_force_same_time_to_accuracy() {
    // Neither mode reaches accuracy sooner: the buffer removes the slide, not the
    // braking time (no artificial advantage).
    for hz in [64u32, 128, 256] {
        let hold = exact_hold(hz);
        let a = scenario(CounterStrafeMode::Source, hold, hz, false);
        let b = scenario(CounterStrafeMode::Buffered, hold, hz, false);
        let tick = 1.0 / hz as f64;
        assert!(
            (a.t_accurate - b.t_accurate).abs() <= tick + 1e-9,
            "{hz} Hz: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn buffered_release_near_the_stop_leaves_no_slide() {
    // Releasing within [completion_ratio, 1] of the needed time completes the stop;
    // Source leaves a residual slide in the original direction.
    for hz in [64u32, 128, 256] {
        let exact = exact_hold(hz);
        for k in [0.92, 0.96] {
            let b = scenario(CounterStrafeMode::Buffered, exact * k, hz, false);
            let s = scenario(CounterStrafeMode::Source, exact * k, hz, false);
            assert!(
                b.speed_after_release < 0.1,
                "{hz} Hz ×{k}: residual {}",
                b.speed_after_release
            );
            assert!(
                b.max_reverse < 0.5,
                "{hz} Hz ×{k}: must not move the other way"
            );
            assert!(
                s.speed_after_release >= b.speed_after_release,
                "{hz} Hz ×{k}: {s:?}"
            );
        }
    }
}

#[test]
fn buffered_never_overshoots_on_the_crossing_segment() {
    // Holding one tick past the stop: the reverse speed is what one extra tick of
    // input produces (bounded by two ticks of acceleration because of the sub-tick
    // release position), and never larger than Source's overshoot.
    for hz in [64u32, 128, 256] {
        let hold = exact_hold(hz) + 1.0 / hz as f64;
        let b = scenario(CounterStrafeMode::Buffered, hold, hz, false);
        let s = scenario(CounterStrafeMode::Source, hold, hz, false);
        let one_tick_accel = 5.5 * SPEED / hz as f32;
        assert!(
            b.max_reverse <= 2.0 * one_tick_accel + 0.01,
            "{hz} Hz: {b:?}"
        );
        assert!(
            b.max_reverse <= s.max_reverse + 0.01,
            "{hz} Hz: {b:?} vs {s:?}"
        );
    }
}

#[test]
fn buffered_does_not_help_short_taps() {
    // A tap shorter than completion_ratio of the needed time is left to physics:
    // same result as Source (no automation).
    for hz in [64u32, 128, 256] {
        let hold = exact_hold(hz) * 0.5;
        let a = scenario(CounterStrafeMode::Source, hold, hz, false);
        let b = scenario(CounterStrafeMode::Buffered, hold, hz, false);
        assert!(
            (a.speed_after_release - b.speed_after_release).abs() < 0.5,
            "{hz} Hz: {a:?} vs {b:?}"
        );
        assert!(b.speed_after_release > 10.0, "still sliding: {b:?}");
    }
}

#[test]
fn holding_too_long_moves_the_other_way_in_both_modes() {
    for mode in [CounterStrafeMode::Source, CounterStrafeMode::Buffered] {
        let o = scenario(mode, 0.6, 128, false);
        assert!(o.max_reverse > 100.0, "{mode:?}: {o:?}");
    }
}

#[test]
fn opposite_keys_together_equal_release() {
    // A+D held together: no priority, both keys cancel, plain friction.
    for mode in [CounterStrafeMode::Source, CounterStrafeMode::Buffered] {
        let both = scenario(mode, 1.0, 128, true);
        let rel = scenario(mode, 0.0, 128, false);
        assert!((both.t_accurate - rel.t_accurate).abs() < 1e-9, "{mode:?}");
        assert!((both.distance - rel.distance).abs() < 0.01, "{mode:?}");
    }
}

#[test]
fn counter_beats_release_only() {
    for mode in [CounterStrafeMode::Source, CounterStrafeMode::Buffered] {
        let hold = exact_hold(128);
        let c = scenario(mode, hold, 128, false);
        let r = scenario(mode, 0.0, 128, false);
        assert!(
            c.t_accurate < r.t_accurate * 0.75,
            "{mode:?}: {c:?} vs {r:?}"
        );
        assert!(c.distance < r.distance, "{mode:?}");
    }
}

#[test]
fn tick_rate_independent_within_one_tick() {
    for mode in [CounterStrafeMode::Source, CounterStrafeMode::Buffered] {
        let r: Vec<Outcome> = [64u32, 128, 256]
            .iter()
            .map(|&hz| scenario(mode, exact_hold(128), hz, false))
            .collect();
        for o in &r[1..] {
            // Bounded by the coarsest tick (15.6 ms at 64 Hz).
            assert!(
                (o.t_accurate - r[0].t_accurate).abs() <= 1.0 / 64.0 + 1e-9,
                "{mode:?}: {r:?}"
            );
            assert!((o.distance - r[0].distance).abs() < 2.0, "{mode:?}: {r:?}");
        }
    }
}

#[test]
fn successive_direction_changes_are_symmetric() {
    // Right → left → right … the same counter timing gives mirrored results.
    let c = cfg(CounterStrafeMode::Buffered);
    let w = flat_world();
    let mut s = MoveState::new(glam::Vec3::Y);
    let hold = exact_hold(128);
    let mut timeline = vec![(0.0, R)];
    let mut t = 0.6;
    let mut dir = R;
    for _ in 0..6 {
        let opp = if dir == R { L } else { R };
        timeline.push((t, opp));
        timeline.push((t + hold, 0));
        timeline.push((t + hold + 0.05, opp));
        dir = opp;
        t += hold + 0.6;
    }
    let mut max_speed: f32 = 0.0;
    run_timeline(&mut s, &timeline, t, 128, SPEED, &w, &c, |_, st| {
        max_speed = max_speed.max(st.horizontal_speed())
    });
    assert!(max_speed <= SPEED + 0.01);
    assert!(
        s.origin.x.abs() < 400.0,
        "oscillation stays centred: {:?}",
        s.origin
    );
}
