//! User commands: what a client asks the simulation to do for one tick.
//!
//! A command carries the button state at the start of the tick plus the ordered list
//! of button transitions that happened *inside* the tick, each with its sub-tick
//! position (1/256 tick) and the view angles at that instant. The movement code
//! integrates the tick piecewise between transitions, so a key pressed at 30 % of a
//! tick takes effect at 30 % of the tick on both the client prediction and the
//! server — whatever the client frame rate was.

use crate::math::ViewAngles;
use crate::tick::SimTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct Buttons(pub u32);

impl Buttons {
    pub const FORWARD: u32 = 1 << 0;
    pub const BACK: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const JUMP: u32 = 1 << 4;
    pub const DUCK: u32 = 1 << 5;
    pub const WALK: u32 = 1 << 6;
    pub const ATTACK: u32 = 1 << 7;
    pub const ATTACK2: u32 = 1 << 8;
    pub const RELOAD: u32 = 1 << 9;
    pub const USE: u32 = 1 << 10;
    pub const DROP: u32 = 1 << 11;
    pub const INSPECT: u32 = 1 << 12;
    pub const ALL: u32 = (1 << 13) - 1;

    pub fn has(self, b: u32) -> bool {
        self.0 & b != 0
    }

    pub fn with(self, b: u32, on: bool) -> Buttons {
        if on {
            Buttons(self.0 | b)
        } else {
            Buttons(self.0 & !b)
        }
    }

    /// Bits that changed between two states.
    pub fn changed(self, other: Buttons) -> u32 {
        self.0 ^ other.0
    }

    /// Movement intent on the local axes: (forward, right) each in {-1, 0, 1}.
    /// Opposite keys held together cancel out: there is no "last key wins"
    /// (Snap Tap style) priority.
    pub fn move_axes(self) -> (i8, i8) {
        let f = self.has(Self::FORWARD) as i8 - self.has(Self::BACK) as i8;
        let r = self.has(Self::RIGHT) as i8 - self.has(Self::LEFT) as i8;
        (f, r)
    }
}

/// A button transition inside a tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ButtonEvent {
    /// Position inside the tick, 0..=255 (1/256 tick resolution).
    pub frac: u8,
    /// Full button state after the transition.
    pub buttons: Buttons,
    /// View angles at the instant of the transition (exact aim for the first shot).
    pub view: ViewAngles,
}

pub const MAX_EVENTS_PER_CMD: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UserCmd {
    /// Server tick this command simulates.
    pub tick: u32,
    pub buttons_start: Buttons,
    pub view_start: ViewAngles,
    pub view_end: ViewAngles,
    /// Ordered transitions, at most [`MAX_EVENTS_PER_CMD`].
    pub events: Vec<ButtonEvent>,
    /// Weapon slot requested this tick (0 = none, 1..=5 slot, 6 = last weapon).
    pub weapon_select: u8,
    /// Server time at which the client was presenting remote entities when the
    /// command ended (used for bounded lag compensation).
    pub view_time: SimTime,
}

/// A piece of a tick with constant buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CmdSegment {
    /// Start and end of the segment inside the tick, in [0, 1].
    pub t0: f32,
    pub t1: f32,
    pub buttons: Buttons,
    /// Buttons before this segment (to detect presses at `t0`).
    pub prev_buttons: Buttons,
    /// View at the start of the segment.
    pub view: ViewAngles,
}

impl UserCmd {
    pub fn buttons_end(&self) -> Buttons {
        self.events
            .last()
            .map(|e| e.buttons)
            .unwrap_or(self.buttons_start)
    }

    /// View angles at a position of the tick: exact at transitions, linearly
    /// interpolated between them.
    pub fn view_at(&self, t: f32) -> ViewAngles {
        let mut prev_t = 0.0;
        let mut prev_v = self.view_start;
        for e in &self.events {
            let et = e.frac as f32 / 256.0;
            if et >= t {
                let span = et - prev_t;
                let k = if span > 1e-6 {
                    (t - prev_t) / span
                } else {
                    1.0
                };
                return prev_v.lerp(e.view, k.clamp(0.0, 1.0));
            }
            prev_t = et;
            prev_v = e.view;
        }
        let span = 1.0 - prev_t;
        let k = if span > 1e-6 {
            (t - prev_t) / span
        } else {
            1.0
        };
        prev_v.lerp(self.view_end, k.clamp(0.0, 1.0))
    }

    /// Splits the tick into constant-button segments.
    pub fn segments(&self) -> Vec<CmdSegment> {
        let mut out = Vec::with_capacity(self.events.len() + 1);
        let mut t0 = 0.0f32;
        let mut cur = self.buttons_start;
        let mut prev = self.buttons_start;
        for e in &self.events {
            let t = e.frac as f32 / 256.0;
            if t > t0 {
                out.push(CmdSegment {
                    t0,
                    t1: t,
                    buttons: cur,
                    prev_buttons: prev,
                    view: self.view_at(t0),
                });
                prev = cur;
                t0 = t;
            }
            cur = e.buttons;
        }
        out.push(CmdSegment {
            t0,
            t1: 1.0,
            buttons: cur,
            prev_buttons: prev,
            view: self.view_at(t0),
        });
        out
    }

    /// Rejects impossible or malformed commands (server side validation).
    pub fn sanitize(&mut self) -> Result<(), &'static str> {
        if self.events.len() > MAX_EVENTS_PER_CMD {
            return Err("too many events");
        }
        let mut last = 0u8;
        for e in &self.events {
            if e.frac < last {
                return Err("events out of order");
            }
            last = e.frac;
            if !e.view.pitch.is_finite() || !e.view.yaw.is_finite() {
                return Err("non finite angle");
            }
        }
        for v in [self.view_start, self.view_end] {
            if !v.pitch.is_finite() || !v.yaw.is_finite() {
                return Err("non finite angle");
            }
        }
        if !self.view_time.frac.is_finite() {
            return Err("non finite time");
        }
        self.buttons_start.0 &= Buttons::ALL;
        for e in &mut self.events {
            e.buttons.0 &= Buttons::ALL;
            e.view = e.view.normalized();
        }
        self.view_start = self.view_start.normalized();
        self.view_end = self.view_end.normalized();
        if self.weapon_select > 6 {
            self.weapon_select = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_keys_cancel() {
        let b = Buttons(Buttons::LEFT | Buttons::RIGHT | Buttons::FORWARD);
        assert_eq!(b.move_axes(), (1, 0));
    }

    #[test]
    fn segments_follow_events() {
        let cmd = UserCmd {
            buttons_start: Buttons(Buttons::LEFT),
            events: vec![
                ButtonEvent {
                    frac: 64,
                    buttons: Buttons(0),
                    view: ViewAngles::default(),
                },
                ButtonEvent {
                    frac: 128,
                    buttons: Buttons(Buttons::RIGHT),
                    view: ViewAngles::default(),
                },
            ],
            ..Default::default()
        };
        let s = cmd.segments();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].buttons, Buttons(Buttons::LEFT));
        assert_eq!((s[1].t0, s[1].t1), (0.25, 0.5));
        assert_eq!(s[2].buttons, Buttons(Buttons::RIGHT));
        assert_eq!(s[2].prev_buttons, Buttons(0));
        let total: f32 = s.iter().map(|x| x.t1 - x.t0).sum();
        assert!((total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn view_interpolates_between_events() {
        let cmd = UserCmd {
            view_start: ViewAngles::new(0.0, 0.0),
            view_end: ViewAngles::new(0.0, 10.0),
            ..Default::default()
        };
        assert!((cmd.view_at(0.5).yaw - 5.0).abs() < 1e-5);
    }

    #[test]
    fn sanitize_rejects_garbage() {
        let mut c = UserCmd {
            view_end: ViewAngles {
                pitch: f32::NAN,
                yaw: 0.0,
            },
            ..Default::default()
        };
        assert!(c.sanitize().is_err());
        let mut c = UserCmd {
            events: vec![
                ButtonEvent {
                    frac: 100,
                    buttons: Buttons(0),
                    view: ViewAngles::default(),
                },
                ButtonEvent {
                    frac: 10,
                    buttons: Buttons(0),
                    view: ViewAngles::default(),
                },
            ],
            ..Default::default()
        };
        assert!(c.sanitize().is_err());
        let mut c = UserCmd {
            buttons_start: Buttons(u32::MAX),
            view_end: ViewAngles {
                pitch: 300.0,
                yaw: 0.0,
            },
            ..Default::default()
        };
        c.sanitize().unwrap();
        assert_eq!(c.buttons_start.0, Buttons::ALL);
        assert_eq!(c.view_end.pitch, 89.0);
    }
}
