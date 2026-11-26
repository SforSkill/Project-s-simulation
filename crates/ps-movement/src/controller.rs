//! Kinematic Source-style controller.

use crate::config::{CounterStrafeMode, MovementConfig};
use engine_core::cmd::{Buttons, UserCmd};
use engine_core::math::ViewAngles;
use engine_physics::world::TraceMask;
use engine_physics::{CollisionWorld, Trace};
use glam::Vec3;

/// Collision queries needed by the controller. The game simulation implements it
/// with the static world plus the other players' boxes.
pub trait MoveWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Trace;
}

impl MoveWorld for CollisionWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> Trace {
        self.trace_box(start, end, mins, maxs, TraceMask::Players)
    }
}

/// Counter-strafe memory for one local horizontal axis (buffered mode).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AxisBuffer {
    /// A counter input is being held against the current velocity on this axis.
    pub active: bool,
    /// Sign of the velocity being cancelled.
    pub sign: f32,
    /// |velocity| on this axis when the counter input started.
    pub start_speed: f32,
    /// Seconds the counter input has been held (sub-tick accurate).
    pub held: f32,
    /// Seconds of counter input needed to stop from `start_speed`.
    pub required: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MoveState {
    /// Feet position (bottom centre of the hull).
    pub origin: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub ground_normal: Vec3,
    /// Hull is the crouched one.
    pub ducked: bool,
    /// 0 = standing, 1 = fully crouched (drives eye height and speed).
    pub duck_amount: f32,
    /// Jump held since the last jump: a new press is needed (no auto-bhop).
    pub jump_latched: bool,
    /// Tagging speed multiplier (1.0 = not slowed).
    pub velocity_modifier: f32,
    /// [forward axis, right axis].
    pub axis_buffers: [AxisBuffer; 2],
}

impl MoveState {
    pub fn new(origin: Vec3) -> Self {
        Self {
            origin,
            velocity: Vec3::ZERO,
            on_ground: false,
            ground_normal: Vec3::Y,
            ducked: false,
            duck_amount: 0.0,
            jump_latched: false,
            velocity_modifier: 1.0,
            axis_buffers: [AxisBuffer::default(); 2],
        }
    }

    pub fn hull(&self, cfg: &MovementConfig) -> (Vec3, Vec3) {
        hull_for(cfg, self.ducked)
    }

    pub fn eye_height(&self, cfg: &MovementConfig) -> f32 {
        let h = cfg.eye_height + (cfg.duck_eye_height - cfg.eye_height) * self.duck_amount;
        // In-air crouch raises the feet: the eye stays at the standing height.
        if self.ducked && !self.on_ground {
            h + (cfg.hull_height - cfg.duck_hull_height)
        } else {
            h
        }
    }

    pub fn eye(&self, cfg: &MovementConfig) -> Vec3 {
        self.origin + Vec3::Y * self.eye_height(cfg)
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec3::new(self.velocity.x, 0.0, self.velocity.z).length()
    }

    /// Applies tagging: multiplies the speed modifier, capped at `floor`.
    pub fn apply_tag(&mut self, reduction: f32, floor: f32) {
        let r = reduction.clamp(0.0, 1.0);
        self.velocity_modifier = (self.velocity_modifier * (1.0 - r)).max(floor);
    }
}

pub fn hull_for(cfg: &MovementConfig, ducked: bool) -> (Vec3, Vec3) {
    let w = cfg.hull_half_width;
    let h = if ducked {
        cfg.duck_hull_height
    } else {
        cfg.hull_height
    };
    (Vec3::new(-w, 0.0, -w), Vec3::new(w, h, w))
}

/// What the game layer tells the controller for one segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveInput {
    pub buttons: Buttons,
    pub view: ViewAngles,
    /// Weapon dependent speed cap (u/s), before walk/crouch/tag factors.
    pub max_speed: f32,
    /// Freeze time, planting, defusing: no translation input and no jump.
    pub frozen: bool,
    pub dt: f32,
}

/// Simulates a full command tick, segment by segment.
pub fn simulate_cmd(
    state: &mut MoveState,
    cmd: &UserCmd,
    max_speed: f32,
    frozen: bool,
    dt: f32,
    cfg: &MovementConfig,
    world: &impl MoveWorld,
) {
    for seg in cmd.segments() {
        let sdt = (seg.t1 - seg.t0) * dt;
        if sdt <= 0.0 {
            continue;
        }
        let input = MoveInput {
            buttons: seg.buttons,
            view: seg.view,
            max_speed,
            frozen,
            dt: sdt,
        };
        simulate_segment(state, &input, cfg, world);
    }
}

/// Simulates `input.dt` seconds with constant buttons.
pub fn simulate_segment(
    state: &mut MoveState,
    input: &MoveInput,
    cfg: &MovementConfig,
    world: &impl MoveWorld,
) {
    let dt = input.dt;
    let mut buttons = input.buttons;
    if input.frozen {
        buttons = Buttons(
            buttons.0
                & !(Buttons::FORWARD
                    | Buttons::BACK
                    | Buttons::LEFT
                    | Buttons::RIGHT
                    | Buttons::JUMP),
        );
    }

    update_duck(state, buttons.has(Buttons::DUCK), dt, cfg, world);
    categorize(state, cfg, world);

    // Jump: edge triggered, skips the friction of this step (bunny hop).
    if buttons.has(Buttons::JUMP) {
        if !state.jump_latched && state.on_ground {
            state.velocity.y = cfg.jump_impulse;
            state.on_ground = false;
            state.jump_latched = true;
        }
    } else {
        state.jump_latched = false;
    }

    if state.on_ground {
        apply_friction(state, dt, cfg);
    }

    // Wish direction from the local axes.
    let fwd = input.view.forward_flat();
    let right = input.view.right_flat();
    let (af, ar) = buttons.move_axes();
    let wish = fwd * af as f32 + right * ar as f32;
    let wishdir = wish.normalize_or_zero();
    let mut speed_cap = input.max_speed.min(cfg.max_velocity);
    if buttons.has(Buttons::WALK) {
        speed_cap *= cfg.walk_speed_factor;
    }
    speed_cap *= 1.0 + (cfg.duck_speed_factor - 1.0) * state.duck_amount;
    speed_cap *= state.velocity_modifier;
    let wishspeed = if wishdir == Vec3::ZERO {
        0.0
    } else {
        speed_cap
    };

    if state.on_ground {
        let before = [state.velocity.dot(fwd), state.velocity.dot(right)];
        let wish_axes = [af as f32, ar as f32];
        accelerate(state, wishdir, wishspeed, cfg.accelerate, dt);
        if cfg.counter_strafe == CounterStrafeMode::Buffered {
            buffered_counter_strafe(state, before, wish_axes, [fwd, right], wishspeed, dt, cfg);
        }
        // Tagging: the reduced cap applies to the current ground speed as well (the
        // floor keeps it above the accuracy threshold, see MovementConfig::tag_floor).
        let hs = state.horizontal_speed();
        if state.velocity_modifier < 1.0 && hs > speed_cap && speed_cap > 0.0 {
            let k = speed_cap / hs;
            state.velocity.x *= k;
            state.velocity.z *= k;
        }
        state.velocity.y = 0.0;
        walk_move(state, dt, cfg, world);
    } else {
        state.axis_buffers = [AxisBuffer::default(); 2];
        state.velocity.y -= cfg.gravity * dt * 0.5;
        air_accelerate(state, wishdir, wishspeed, cfg, dt);
        clamp_velocity(state, cfg);
        let (mins, maxs) = state.hull(cfg);
        try_player_move(state, dt, mins, maxs, world);
        state.velocity.y -= cfg.gravity * dt * 0.5;
    }

    categorize(state, cfg, world);
    if state.on_ground && state.velocity.y < 0.0 {
        state.velocity.y = 0.0;
    }

    // Tagging recovery.
    if state.velocity_modifier < 1.0 {
        let rate = (1.0 - cfg.tag_floor).max(0.01) / cfg.tag_recovery_time;
        state.velocity_modifier = (state.velocity_modifier + rate * dt).min(1.0);
    }
}

fn clamp_velocity(state: &mut MoveState, cfg: &MovementConfig) {
    state.velocity = state.velocity.clamp(
        Vec3::splat(-cfg.max_velocity),
        Vec3::splat(cfg.max_velocity),
    );
}

fn update_duck(
    state: &mut MoveState,
    wants: bool,
    dt: f32,
    cfg: &MovementConfig,
    world: &impl MoveWorld,
) {
    let diff = cfg.hull_height - cfg.duck_hull_height;
    if wants {
        state.duck_amount = (state.duck_amount + dt / cfg.duck_time).min(1.0);
        if !state.ducked {
            if !state.on_ground {
                // Crouch in the air: pull the legs up, instantly.
                let (mins, maxs) = hull_for(cfg, true);
                let up = state.origin + Vec3::Y * diff;
                let tr = world.trace_hull(state.origin, up, mins, maxs);
                state.origin = tr.end;
                state.ducked = true;
                state.duck_amount = 1.0;
            } else if state.duck_amount >= 1.0 {
                state.ducked = true;
            }
        }
    } else {
        if state.ducked {
            let (mins, maxs) = hull_for(cfg, false);
            let target = if state.on_ground {
                state.origin
            } else {
                state.origin - Vec3::Y * diff
            };
            let tr = world.trace_hull(target, target, mins, maxs);
            if tr.start_solid {
                // No room to stand: stay crouched.
                state.duck_amount = 1.0;
                return;
            }
            state.origin = target;
            state.ducked = false;
        }
        state.duck_amount = (state.duck_amount - dt / cfg.duck_time).max(0.0);
    }
}

/// Ground detection (Source `CategorizePosition`).
fn categorize(state: &mut MoveState, cfg: &MovementConfig, world: &impl MoveWorld) {
    if state.velocity.y > 140.0 {
        state.on_ground = false;
        return;
    }
    let (mins, maxs) = state.hull(cfg);
    let down = state.origin - Vec3::Y * 2.0;
    let tr = world.trace_hull(state.origin, down, mins, maxs);
    if tr.hit() && !tr.all_solid && tr.normal.y >= cfg.walkable_normal {
        state.on_ground = true;
        state.ground_normal = tr.normal;
        if tr.fraction > 0.0 {
            state.origin = tr.end;
        }
    } else {
        state.on_ground = false;
    }
}

fn apply_friction(state: &mut MoveState, dt: f32, cfg: &MovementConfig) {
    let v = state.velocity;
    let speed = Vec3::new(v.x, 0.0, v.z).length();
    if speed < 0.1 {
        state.velocity.x = 0.0;
        state.velocity.z = 0.0;
        return;
    }
    let control = speed.max(cfg.stop_speed);
    let drop = control * cfg.friction * dt;
    let new_speed = (speed - drop).max(0.0) / speed;
    state.velocity.x *= new_speed;
    state.velocity.z *= new_speed;
}

fn accelerate(state: &mut MoveState, wishdir: Vec3, wishspeed: f32, accel: f32, dt: f32) {
    if wishspeed <= 0.0 {
        return;
    }
    let current = state.velocity.dot(wishdir);
    let add = wishspeed - current;
    if add <= 0.0 {
        return;
    }
    let accel_speed = (accel * dt * wishspeed).min(add);
    state.velocity += wishdir * accel_speed;
}

fn air_accelerate(
    state: &mut MoveState,
    wishdir: Vec3,
    wishspeed: f32,
    cfg: &MovementConfig,
    dt: f32,
) {
    if wishspeed <= 0.0 {
        return;
    }
    let capped = wishspeed.min(cfg.air_wishspeed_cap);
    let current = state.velocity.dot(wishdir);
    let add = capped - current;
    if add <= 0.0 {
        return;
    }
    let accel_speed = (cfg.air_accelerate * wishspeed * dt).min(add);
    state.velocity += wishdir * accel_speed;
}

/// Time (s) to bring `speed` to zero while holding the opposite key, with Source
/// friction and acceleration. Integrated numerically at 1 ms.
pub fn counter_stop_time(speed: f32, wishspeed: f32, cfg: &MovementConfig) -> f32 {
    let mut v = speed;
    let mut t = 0.0;
    let step = 0.001;
    while v > 0.0 && t < 2.0 {
        let control = v.max(cfg.stop_speed);
        v -= control * cfg.friction * step;
        v -= cfg.accelerate * wishspeed * step;
        t += step;
    }
    t
}

fn buffered_counter_strafe(
    state: &mut MoveState,
    before: [f32; 2],
    wish: [f32; 2],
    axes: [Vec3; 2],
    wishspeed: f32,
    dt: f32,
    cfg: &MovementConfig,
) {
    let mut comps = [state.velocity.dot(axes[0]), state.velocity.dot(axes[1])];
    for i in 0..2 {
        let w = wish[i];
        let v0 = before[i];
        let buf = &mut state.axis_buffers[i];
        let countering = w != 0.0 && v0 * w < 0.0 && v0.abs() > cfg.counter_min_speed;
        if countering {
            if !buf.active || buf.sign != v0.signum() {
                *buf = AxisBuffer {
                    active: true,
                    sign: v0.signum(),
                    start_speed: v0.abs(),
                    held: 0.0,
                    required: counter_stop_time(v0.abs(), wishspeed.max(1.0), cfg),
                };
            }
            buf.held += dt;
            // No overshoot: the axis stops exactly where it would cross zero.
            if comps[i] * buf.sign <= 0.0 {
                comps[i] = 0.0;
                *buf = AxisBuffer::default();
            }
        } else if buf.active {
            // Counter input released (or both keys held): complete the stop if the
            // player held it long enough, otherwise leave it to friction.
            let released = w == 0.0;
            if released && buf.held >= cfg.completion_ratio * buf.required {
                comps[i] = 0.0;
            }
            *buf = AxisBuffer::default();
        }
    }
    let vy = state.velocity.y;
    state.velocity = axes[0] * comps[0] + axes[1] * comps[1];
    state.velocity.y = vy;
}

fn clip_velocity(v: Vec3, normal: Vec3, overbounce: f32) -> Vec3 {
    let backoff = v.dot(normal) * overbounce;
    let mut out = v - normal * backoff;
    for i in 0..3 {
        if out[i].abs() < 0.1 {
            out[i] = 0.0;
        }
    }
    let adjust = out.dot(normal);
    if adjust < 0.0 {
        out -= normal * adjust;
    }
    out
}

/// Slide move (Source `TryPlayerMove`). Returns true if blocked at least once.
fn try_player_move(
    state: &mut MoveState,
    dt: f32,
    mins: Vec3,
    maxs: Vec3,
    world: &impl MoveWorld,
) -> bool {
    let mut time_left = dt;
    let primal = state.velocity;
    let mut original = state.velocity;
    let mut planes: [Vec3; 5] = [Vec3::ZERO; 5];
    let mut num_planes = 0usize;
    let mut blocked = false;
    for _ in 0..4 {
        if state.velocity.length_squared() < 1e-8 {
            break;
        }
        let end = state.origin + state.velocity * time_left;
        let tr = world.trace_hull(state.origin, end, mins, maxs);
        if tr.all_solid {
            state.velocity = Vec3::ZERO;
            return true;
        }
        if tr.fraction > 0.0 {
            state.origin = tr.end;
            original = state.velocity;
            num_planes = 0;
        }
        if tr.fraction >= 1.0 {
            break;
        }
        blocked = true;
        time_left -= time_left * tr.fraction;
        if num_planes >= planes.len() {
            state.velocity = Vec3::ZERO;
            break;
        }
        planes[num_planes] = tr.normal;
        num_planes += 1;

        if num_planes == 1 && !state.on_ground {
            // Source uses an overbounce of 1.0 for players (sv_bounce 0).
            state.velocity = clip_velocity(original, tr.normal, 1.0);
            original = state.velocity;
        } else {
            let mut ok = false;
            for i in 0..num_planes {
                let v = clip_velocity(original, planes[i], 1.0);
                if (0..num_planes).all(|j| j == i || v.dot(planes[j]) >= 0.0) {
                    state.velocity = v;
                    ok = true;
                    break;
                }
            }
            if !ok {
                if num_planes != 2 {
                    state.velocity = Vec3::ZERO;
                    break;
                }
                let dir = planes[0].cross(planes[1]).normalize_or_zero();
                state.velocity = dir * dir.dot(state.velocity);
            }
        }
        if state.velocity.dot(primal) <= 0.0 {
            state.velocity = Vec3::ZERO;
            break;
        }
    }
    blocked
}

/// Ground move with step-up (Source `WalkMove` + `StepMove` + `StayOnGround`).
fn walk_move(state: &mut MoveState, dt: f32, cfg: &MovementConfig, world: &impl MoveWorld) {
    let (mins, maxs) = state.hull(cfg);
    let speed = state.horizontal_speed();
    if speed < 1e-4 {
        stay_on_ground(state, cfg, world);
        return;
    }
    let dest = state.origin + state.velocity * dt;
    let tr = world.trace_hull(state.origin, dest, mins, maxs);
    if !tr.hit() {
        state.origin = tr.end;
        stay_on_ground(state, cfg, world);
        return;
    }

    let start = *state;
    // Plain slide.
    try_player_move(state, dt, mins, maxs, world);
    let down = *state;

    // Step: up, slide, down.
    *state = start;
    let up_tr = world.trace_hull(
        state.origin,
        state.origin + Vec3::Y * cfg.step_size,
        mins,
        maxs,
    );
    if !up_tr.all_solid {
        state.origin = up_tr.end;
    }
    try_player_move(state, dt, mins, maxs, world);
    let down_tr = world.trace_hull(
        state.origin,
        state.origin - Vec3::Y * (cfg.step_size + SURFACE),
        mins,
        maxs,
    );
    let mut use_down = false;
    if down_tr.all_solid || down_tr.normal.y < cfg.walkable_normal || !down_tr.hit() {
        use_down = true;
    } else {
        state.origin = down_tr.end;
    }
    if !use_down {
        let d_down = Vec3::new(
            down.origin.x - start.origin.x,
            0.0,
            down.origin.z - start.origin.z,
        )
        .length_squared();
        let d_up = Vec3::new(
            state.origin.x - start.origin.x,
            0.0,
            state.origin.z - start.origin.z,
        )
        .length_squared();
        if d_down >= d_up {
            use_down = true;
        } else {
            // Keep the slide's vertical velocity as Source does.
            state.velocity.y = down.velocity.y;
        }
    }
    if use_down {
        *state = down;
    }
    stay_on_ground(state, cfg, world);
}

fn stay_on_ground(state: &mut MoveState, cfg: &MovementConfig, world: &impl MoveWorld) {
    let (mins, maxs) = state.hull(cfg);
    let start = state.origin + Vec3::Y * 2.0;
    let up = world.trace_hull(state.origin, start, mins, maxs);
    let from = up.end;
    let end = state.origin - Vec3::Y * cfg.step_size;
    let tr = world.trace_hull(from, end, mins, maxs);
    if tr.fraction > 0.0
        && tr.fraction < 1.0
        && !tr.start_solid
        && tr.normal.y >= cfg.walkable_normal
    {
        let delta = (from.y - tr.end.y).abs();
        if delta > 0.5 * SURFACE {
            state.origin = tr.end;
        }
    }
}

const SURFACE: f32 = engine_physics::SURFACE_EPSILON;
