//! Player movement of Projet S.
//!
//! A re-implementation of the Source engine `CGameMovement` model (the one CS:GO
//! uses), written from the public description of the algorithm: ground friction
//! with `stopspeed`, `accelerate` toward a capped wish speed, air acceleration with a
//! 30 u/s wish cap (air strafe and bunny hop emerge from it), half-step gravity, slide
//! along clip planes, step-up of 18 units, ground snapping on stairs and slopes.
//!
//! No sprint, no stamina, no automatic counter-strafe.
//!
//! Counter-strafing has two interchangeable implementations ([`CounterStrafeMode`]):
//! the pure Source physics, and a buffered variant that keeps timestamped counter
//! inputs and removes the residual slide. Both are compared by the tests in
//! `tests/counter_strafe.rs` and documented in `docs/movement.md`.

pub mod config;
pub mod controller;

pub use config::{CounterStrafeMode, MovementConfig};
pub use controller::{AxisBuffer, MoveInput, MoveState, MoveWorld, simulate_cmd, simulate_segment};
