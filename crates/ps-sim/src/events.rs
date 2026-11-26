//! Events emitted by the simulation (replicated, recorded, shown in the HUD).

use engine_core::{PlayerId, Team};
use engine_weapons::{GrenadeKind, HitGroup, WeaponId};
use game_rules::{Phase, RoundEndReason};
use glam::Vec3;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GameEvent {
    /// A bullet: start, end of the visible tracer, impacts on surfaces.
    Shot {
        shooter: PlayerId,
        weapon: WeaponId,
        origin: Vec3,
        end: Vec3,
        impacts: Vec<Vec3>,
    },
    /// Damage dealt (hit marker for the attacker, damage indicator for the victim).
    Damage {
        attacker: Option<PlayerId>,
        victim: PlayerId,
        amount: u16,
        group: Option<HitGroup>,
        health: i16,
        armor: u8,
        from: Vec3,
    },
    Kill {
        killer: Option<PlayerId>,
        victim: PlayerId,
        weapon: Option<WeaponId>,
        headshot: bool,
        penetrated: bool,
        assister: Option<PlayerId>,
    },
    Spawn {
        player: PlayerId,
    },
    GrenadeThrown {
        id: u16,
        kind: GrenadeKind,
        thrower: PlayerId,
    },
    GrenadeDetonated {
        id: u16,
        kind: GrenadeKind,
        pos: Vec3,
    },
    /// Opaque smoke volume (full description; sent reliably).
    SmokeSpawned {
        id: u16,
    },
    SmokeBlast {
        pos: Vec3,
    },
    Flashed {
        player: PlayerId,
        duration: f32,
        alpha: f32,
    },
    FireStarted {
        id: u16,
    },
    BombPlanted {
        planter: PlayerId,
        pos: Vec3,
        site: u8,
    },
    BombDefused {
        defuser: PlayerId,
    },
    BombExploded {
        pos: Vec3,
    },
    BombDropped {
        pos: Vec3,
    },
    BombPickedUp {
        player: PlayerId,
    },
    PlantStarted {
        player: PlayerId,
    },
    DefuseStarted {
        player: PlayerId,
        kit: bool,
    },
    RoundPhase {
        phase: Phase,
        round: u16,
    },
    RoundEnded {
        winner: Team,
        reason: RoundEndReason,
        mvp: Option<PlayerId>,
    },
    MatchEnded {
        winner_team_score: [u16; 2],
    },
    Footstep {
        player: PlayerId,
        pos: Vec3,
        loud: bool,
    },
    Jump {
        player: PlayerId,
        pos: Vec3,
    },
    Land {
        player: PlayerId,
        pos: Vec3,
    },
    Reload {
        player: PlayerId,
    },
    DryFire {
        player: PlayerId,
    },
    Zoom {
        player: PlayerId,
        level: u8,
    },
    Bought {
        player: PlayerId,
        item: String,
    },
    Chat {
        from: Option<PlayerId>,
        team_only: bool,
        text: String,
    },
    Notice {
        text: String,
    },
}

impl GameEvent {
    /// Position for audibility filtering (`None` = global).
    pub fn position(&self) -> Option<Vec3> {
        match self {
            GameEvent::Shot { origin, .. } => Some(*origin),
            GameEvent::Footstep { pos, .. }
            | GameEvent::Jump { pos, .. }
            | GameEvent::Land { pos, .. } => Some(*pos),
            GameEvent::GrenadeDetonated { pos, .. } => Some(*pos),
            _ => None,
        }
    }
}
