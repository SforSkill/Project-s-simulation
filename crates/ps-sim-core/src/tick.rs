//! Fixed simulation rate and fractional simulation time.

use serde::{Deserialize, Serialize};

/// Simulation frequency. 128 Hz is the competitive standard; 64/256/512/1024 Hz are
/// accepted for experiments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickRate(pub u32);

impl TickRate {
    pub const COMPETITIVE: TickRate = TickRate(128);
    pub const ALLOWED: [u32; 5] = [64, 128, 256, 512, 1024];

    pub fn new(hz: u32) -> Result<Self, String> {
        if Self::ALLOWED.contains(&hz) {
            Ok(Self(hz))
        } else {
            Err(format!(
                "tick rate {hz} Hz not supported (allowed: {:?})",
                Self::ALLOWED
            ))
        }
    }

    pub fn hz(self) -> u32 {
        self.0
    }

    /// Tick duration in seconds (7.8125 ms at 128 Hz).
    pub fn dt(self) -> f32 {
        1.0 / self.0 as f32
    }

    pub fn dt_f64(self) -> f64 {
        1.0 / self.0 as f64
    }

    pub fn seconds_to_ticks(self, s: f32) -> u32 {
        (s * self.0 as f32).round().max(0.0) as u32
    }

    pub fn ticks_to_seconds(self, t: u32) -> f32 {
        t as f32 / self.0 as f32
    }
}

impl Default for TickRate {
    fn default() -> Self {
        Self::COMPETITIVE
    }
}

/// A point on the server timeline: tick plus fraction of the following tick.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct SimTime {
    pub tick: u32,
    /// In [0, 1).
    pub frac: f32,
}

impl SimTime {
    pub fn new(tick: u32, frac: f32) -> Self {
        Self {
            tick,
            frac: frac.clamp(0.0, 0.999_999),
        }
    }

    pub fn from_ticks_f64(t: f64) -> Self {
        let t = t.max(0.0);
        Self::new(t.floor() as u32, (t - t.floor()) as f32)
    }

    pub fn as_f64(self) -> f64 {
        self.tick as f64 + self.frac as f64
    }

    /// Quantised fraction used on the wire (1/65536 tick).
    pub fn frac_u16(self) -> u16 {
        (self.frac * 65536.0).clamp(0.0, 65535.0) as u16
    }

    pub fn from_wire(tick: u32, frac: u16) -> Self {
        Self {
            tick,
            frac: frac as f32 / 65536.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competitive_tick_is_7_8125_ms() {
        assert_eq!(TickRate::COMPETITIVE.dt_f64() * 1000.0, 7.8125);
        assert!(TickRate::new(100).is_err());
        assert_eq!(TickRate::new(256).unwrap().dt_f64() * 1000.0, 3.90625);
    }

    #[test]
    fn sim_time_wire_roundtrip() {
        let t = SimTime::new(1234, 0.25);
        let w = SimTime::from_wire(t.tick, t.frac_u16());
        assert_eq!(w.tick, 1234);
        assert!((w.frac - 0.25).abs() < 1e-4);
    }
}
