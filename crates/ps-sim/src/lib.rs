//! Projet S authoritative game simulation, shared by the dedicated server and the
//! local (embedded) server of the client.
//!
//! Pure Rust, no I/O except [`data::GameData::load`]; deterministic for a given
//! seed and command stream.

pub mod bots;
pub mod collide;
pub mod data;
pub mod events;
pub mod grenades;
pub mod level;
pub mod player;
pub mod predict;
pub mod proto;
pub mod record;
pub mod sim;
pub mod smoke;

pub use data::GameData;
pub use events::GameEvent;
pub use level::{Level, NavGraph};
pub use player::{Controller, Inventory, Player, Stats};
pub use sim::{BombState, Dropped, ShotHit, ShotReport, Sim, SimConfig, SurfaceCross};
pub use smoke::{SmokeConfig, SmokeVolume};
