//! Shared foundations of the Projet S simulation.
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

pub mod cmd;
pub mod config;
pub mod ids;
pub mod math;
pub mod rng;
pub mod sensitivity;
pub mod tick;

pub use cmd::{ButtonEvent, Buttons, UserCmd};
pub use ids::{EntityId, MAX_PLAYERS, PlayerId, Team};
pub use math::{Aabb, ViewAngles};
pub use rng::DetRng;
pub use tick::{SimTime, TickRate};

pub use glam::{Vec2, Vec3};

/// Protocol and data-format version; client and server must agree.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");
