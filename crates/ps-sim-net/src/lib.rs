//! What the simulation exchanges with the outside: game messages between client
//! and server ([`proto`]), client-side prediction that replays `Sim::run_cmd`
//! ([`predict`]) and deterministic replays ([`record`]).

pub mod predict;
pub mod proto;
pub mod record;
