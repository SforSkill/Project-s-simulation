//! Weapons of Projet S.
//!
//! Rules enforced by this crate (and tested in `tests/`):
//! * No intrinsic spread: a weapon fired by a player at or below the accuracy speed
//!   threshold, on the ground, goes exactly where the view + accumulated recoil points.
//! * Recoil is a fixed pattern per weapon, indexed by a continuously recovering shot
//!   counter: identical inputs give identical sprays. No random recoil.
//! * Damage is deterministic (distance falloff, hit group, armor, penetration).
//! * Random spread only exists while moving faster than the threshold or airborne,
//!   and grows with the real velocity.
//! * Fire rates are expressed in seconds and evaluated at sub-tick time stamps, so
//!   they do not depend on the frame rate or the tick rate.
//! * Nothing here moves the player's view: there is no aim punch.

pub mod controller;
pub mod damage;
pub mod defs;
pub mod spread;

pub use controller::{GunState, HandState, WeaponEvent, WeaponInput};
pub use damage::{HitGroup, apply_armor, distance_falloff};
pub use defs::{Catalog, EquipmentDef, GrenadeKind, Slot, WeaponDef, WeaponId, WeaponKind};
pub use spread::{movement_spread_deg, recoil_offset, spread_offset};
