//! Shared foundations of the Project S simulation.
//!
//! Nothing in this crate depends on rendering, audio or networking: the dedicated
//! server, the client prediction and the offline tools all run the same code.
//!
//! Conventions (used by every crate):
//! * Units are Source-style "units" (1 unit = 1 inch = 2.54 cm). The client converts
//!   to metres only for rendering.
//! * The world is **Y-up**, right-handed. Yaw 0 looks toward −Z, yaw grows when the
//!   player turns left. Pitch is positive when looking up.
//! * Angles in user-facing data are degrees.
//!
//! Identifiers, commands and ticks are wire contracts owned by `ps-types`
//! (Project-s-protocol); they are re-exported here under their historical paths.

pub mod config;
pub mod math;
pub mod rng;
pub mod sensitivity;

pub use ps_types::{cmd, ids, tick};

pub use cmd::{ButtonEvent, Buttons, UserCmd};
pub use ids::{EntityId, MAX_PLAYERS, PlayerId, Team};
pub use math::{Aabb, ViewAngles};
pub use rng::DetRng;
pub use tick::{SimTime, TickRate};

pub use glam::{Vec2, Vec3};
