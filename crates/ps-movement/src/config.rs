//! Movement constants (loaded from `configs/movement.toml`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CounterStrafeMode {
    /// Pure Source physics: friction + acceleration toward the opposite key.
    Source,
    /// Same braking force, plus (1) sub-tick timestamped counter inputs, (2) the
    /// velocity component is clamped at zero on the segment where it would cross
    /// zero (no overshoot), (3) if the counter key is released after holding it for
    /// at least `completion_ratio` of the time needed to stop, the remaining
    /// component is removed (no residual slide).
    #[default]
    Buffered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementConfig {
    /// Reference `sv_maxspeed`; the effective cap comes from the equipped weapon.
    pub max_speed: f32,
    pub accelerate: f32,
    pub air_accelerate: f32,
    /// Wish speed cap used for air acceleration (Source: 30).
    pub air_wishspeed_cap: f32,
    pub friction: f32,
    pub stop_speed: f32,
    pub gravity: f32,
    /// Initial vertical velocity of a jump: sqrt(2 × 800 × 57) = 301.993.
    pub jump_impulse: f32,
    pub step_size: f32,
    /// Shift-walk speed fraction (silent).
    pub walk_speed_factor: f32,
    /// Fully crouched speed fraction.
    pub duck_speed_factor: f32,
    /// Seconds to go from standing to fully crouched (and back).
    pub duck_time: f32,
    pub hull_half_width: f32,
    pub hull_height: f32,
    pub duck_hull_height: f32,
    pub eye_height: f32,
    pub duck_eye_height: f32,
    /// Minimal normal.y of a walkable surface (cos 45.57°).
    pub walkable_normal: f32,
    pub max_velocity: f32,
    pub counter_strafe: CounterStrafeMode,
    /// Buffered mode: fraction of the needed braking time after which releasing the
    /// counter key completes the stop. 1.0 = only exact counter-strafes are completed.
    pub completion_ratio: f32,
    /// Speed under which a horizontal axis is considered stopped by the buffered mode.
    pub counter_min_speed: f32,
    /// Seconds for the tagging speed modifier to go back from its floor to 1.0.
    pub tag_recovery_time: f32,
    /// Lowest tagging modifier (never a full stop). Must stay above the weapons'
    /// accuracy threshold fraction so that being hit never grants instant accuracy.
    pub tag_floor: f32,
}

impl Default for MovementConfig {
    fn default() -> Self {
        Self {
            max_speed: 250.0,
            accelerate: 5.5,
            air_accelerate: 12.0,
            air_wishspeed_cap: 30.0,
            friction: 5.2,
            stop_speed: 80.0,
            gravity: 800.0,
            jump_impulse: 301.993,
            step_size: 18.0,
            walk_speed_factor: 0.52,
            duck_speed_factor: 0.34,
            duck_time: 0.2,
            hull_half_width: 16.0,
            hull_height: 72.0,
            duck_hull_height: 54.0,
            eye_height: 64.0,
            duck_eye_height: 46.0,
            walkable_normal: 0.7,
            max_velocity: 3500.0,
            counter_strafe: CounterStrafeMode::Buffered,
            completion_ratio: 0.9,
            counter_min_speed: 1.0,
            tag_recovery_time: 0.35,
            tag_floor: 0.5,
        }
    }
}

impl MovementConfig {
    pub fn validate(&self) -> Result<(), String> {
        let pos = [
            ("max_speed", self.max_speed),
            ("accelerate", self.accelerate),
            ("friction", self.friction),
            ("gravity", self.gravity),
            ("duck_time", self.duck_time),
            ("hull_height", self.hull_height),
            ("tag_recovery_time", self.tag_recovery_time),
        ];
        for (n, v) in pos {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("movement.{n} must be > 0"));
            }
        }
        if self.duck_hull_height >= self.hull_height {
            return Err("duck_hull_height must be < hull_height".into());
        }
        if !(0.0..=1.0).contains(&self.completion_ratio) {
            return Err("completion_ratio must be in [0,1]".into());
        }
        if !(0.0..1.0).contains(&self.tag_floor) {
            return Err("tag_floor must be in [0,1)".into());
        }
        Ok(())
    }
}
