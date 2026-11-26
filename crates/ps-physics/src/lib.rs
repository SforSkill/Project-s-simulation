//! Collision world of Projet S.
//!
//! Why not Avian3D for the authoritative simulation? The Source-style movement
//! controller is defined in terms of *swept axis-aligned box traces against convex
//! brushes* (`TraceHull`), with exact fractions, plane normals and the 1/32 unit
//! surface epsilon. Reproducing that on top of a general rigid-body engine costs
//! more than the ~400 lines below, would tie the headless server to the Bevy ECS,
//! and makes bit-for-bit agreement between client prediction and server harder.
//! See `docs/decisions/0002-collision.md`.

pub mod brush;
pub mod map;
pub mod material;
pub mod world;

pub use brush::{Brush, Plane};
pub use map::{BrushDef, MapDef, MapError, RampDir};
pub use material::{MaterialDef, MaterialId, MaterialTable};
pub use world::{CollisionWorld, RaySegment, Trace};

/// Source `DIST_EPSILON`: traces stop this far in front of surfaces.
pub const SURFACE_EPSILON: f32 = 0.03125;
