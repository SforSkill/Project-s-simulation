//! Map description format (RON files in `maps/`).
//!
//! A map is a list of convex brushes (boxes and ramps) carrying a material, plus
//! gameplay markers (spawns, bomb sites, buy zones, training targets). The same file
//! is loaded by the server (collision, rules) and the client (meshes, collision for
//! prediction). See `docs/maps.md` for the authoring guide.

use crate::brush::Brush;
use crate::material::{MaterialId, MaterialTable};
use engine_core::{Aabb, Team};
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RampDir {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl RampDir {
    pub fn vec(self) -> Vec3 {
        match self {
            RampDir::PosX => Vec3::X,
            RampDir::NegX => Vec3::NEG_X,
            RampDir::PosZ => Vec3::Z,
            RampDir::NegZ => Vec3::NEG_Z,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BrushDef {
    /// Solid box.
    Box {
        min: Vec3,
        max: Vec3,
        mat: String,
        #[serde(default = "default_true")]
        players: bool,
        #[serde(default = "default_true")]
        bullets: bool,
    },
    /// Wedge rising toward `high`.
    Ramp {
        min: Vec3,
        max: Vec3,
        high: RampDir,
        mat: String,
    },
    /// Invisible player clip (blocks players only).
    Clip { min: Vec3, max: Vec3 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnDef {
    pub team: Team,
    pub pos: Vec3,
    pub yaw: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneDef {
    pub name: String,
    #[serde(default)]
    pub team: Team,
    pub min: Vec3,
    pub max: Vec3,
}

impl ZoneDef {
    pub fn aabb(&self) -> Aabb {
        Aabb::from_corners(self.min, self.max)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TargetKind {
    /// Stands still.
    #[default]
    Static,
    /// Strafes left/right (counter-strafe and tracking practice).
    Strafe,
    /// Crouches periodically.
    Crouch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetDef {
    pub pos: Vec3,
    pub yaw: f32,
    #[serde(default)]
    pub kind: TargetKind,
    /// Armor given to the target (0, or 100 with/without helmet).
    #[serde(default)]
    pub armor: u8,
    #[serde(default)]
    pub helmet: bool,
}

/// A labelled marker shown by the client (e.g. "wallbang 8u wood").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelDef {
    pub pos: Vec3,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapDef {
    pub name: String,
    pub display_name: String,
    pub version: u32,
    /// Game modes this map supports ("training", "deathmatch", "duel", "competitive").
    pub modes: Vec<String>,
    pub brushes: Vec<BrushDef>,
    pub spawns: Vec<SpawnDef>,
    #[serde(default)]
    pub dm_spawns: Vec<SpawnDef>,
    #[serde(default)]
    pub bomb_sites: Vec<ZoneDef>,
    #[serde(default)]
    pub buy_zones: Vec<ZoneDef>,
    #[serde(default)]
    pub targets: Vec<TargetDef>,
    #[serde(default)]
    pub labels: Vec<LabelDef>,
    /// Navigation / playable bounds.
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    #[serde(default = "default_sky")]
    pub sky_color: [f32; 3],
}

fn default_sky() -> [f32; 3] {
    [0.55, 0.65, 0.78]
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("unknown material '{0}'")]
    UnknownMaterial(String),
    #[error("invalid brush #{0}: {1}")]
    InvalidBrush(usize, String),
    #[error("map has no spawn for {0:?}")]
    MissingSpawn(Team),
    #[error("spawn #{0} at {1:?} is inside solid geometry")]
    SpawnInSolid(usize, Vec3),
}

impl MapDef {
    pub fn bounds(&self) -> Aabb {
        Aabb::from_corners(self.bounds_min, self.bounds_max)
    }

    pub fn supports(&self, mode: &str) -> bool {
        self.modes.iter().any(|m| m == mode)
    }

    /// Builds collision brushes, resolving material names.
    pub fn build_brushes(&self, mats: &MaterialTable) -> Result<Vec<Brush>, MapError> {
        let clip_mat = MaterialId(0);
        let mut out = Vec::with_capacity(self.brushes.len());
        for (i, b) in self.brushes.iter().enumerate() {
            let check = |min: Vec3, max: Vec3| -> Result<Aabb, MapError> {
                let a = Aabb::from_corners(min, max);
                let s = a.size();
                if s.min_element() <= 0.0 || !s.is_finite() {
                    return Err(MapError::InvalidBrush(i, format!("degenerate size {s:?}")));
                }
                Ok(a)
            };
            let resolve = |m: &str| {
                mats.id_of(m)
                    .ok_or_else(|| MapError::UnknownMaterial(m.to_string()))
            };
            let brush = match b {
                BrushDef::Box {
                    min,
                    max,
                    mat,
                    players,
                    bullets,
                } => {
                    let mut br = Brush::from_box(check(*min, *max)?, resolve(mat)?);
                    br.blocks_players = *players;
                    br.blocks_bullets = *bullets;
                    br
                }
                BrushDef::Ramp {
                    min,
                    max,
                    high,
                    mat,
                } => Brush::ramp(check(*min, *max)?, high.vec(), resolve(mat)?),
                BrushDef::Clip { min, max } => {
                    let mut br = Brush::from_box(check(*min, *max)?, clip_mat);
                    br.blocks_bullets = false;
                    br.visible = false;
                    br
                }
            };
            out.push(brush);
        }
        Ok(out)
    }
}
