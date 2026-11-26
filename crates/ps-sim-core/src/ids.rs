//! Identifiers shared by simulation, network and UI.

use serde::{Deserialize, Serialize};

/// Player slot on a server (0..=63).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerId(pub u8);

/// Network entity id (players use their slot, other entities start at 64).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u16);

impl From<PlayerId> for EntityId {
    fn from(p: PlayerId) -> Self {
        EntityId(p.0 as u16)
    }
}

pub const MAX_PLAYERS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Team {
    #[default]
    Spectator,
    /// Attackers (carry and plant the bomb).
    Attack,
    /// Defenders (defuse the bomb).
    Defense,
}

impl Team {
    pub fn opponent(self) -> Team {
        match self {
            Team::Attack => Team::Defense,
            Team::Defense => Team::Attack,
            Team::Spectator => Team::Spectator,
        }
    }

    pub fn is_playing(self) -> bool {
        !matches!(self, Team::Spectator)
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Team::Spectator => 0,
            Team::Attack => 1,
            Team::Defense => 2,
        }
    }

    pub fn from_u8(v: u8) -> Option<Team> {
        match v {
            0 => Some(Team::Spectator),
            1 => Some(Team::Attack),
            2 => Some(Team::Defense),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Team::Spectator => "Spectateurs",
            Team::Attack => "Attaque",
            Team::Defense => "Défense",
        }
    }
}
