//! Player state.

use engine_core::cmd::UserCmd;
use engine_core::{PlayerId, Team, ViewAngles};
use engine_movement::MoveState;
use engine_weapons::{Catalog, GunState, HandState, Slot, WeaponDef, WeaponId, WeaponKind};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Controller {
    #[default]
    Human,
    Bot,
    /// Training target (never counts for the match).
    Dummy,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub headshots: u32,
    pub damage: u32,
    pub shots: u32,
    pub hits: u32,
    pub mvps: u32,
    pub score: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    pub primary: Option<(WeaponId, GunState)>,
    pub secondary: Option<(WeaponId, GunState)>,
    pub knife: Option<WeaponId>,
    pub grenades: Vec<WeaponId>,
    pub bomb: bool,
}

impl Inventory {
    pub fn weapon_in(&self, slot: Slot, grenade_index: usize) -> Option<WeaponId> {
        match slot {
            Slot::Primary => self.primary.map(|p| p.0),
            Slot::Secondary => self.secondary.map(|p| p.0),
            Slot::Melee => self.knife,
            Slot::Grenade => self
                .grenades
                .get(grenade_index.min(self.grenades.len().saturating_sub(1)))
                .copied(),
            Slot::Bomb => None,
        }
    }

    pub fn has(&self, slot: Slot) -> bool {
        match slot {
            Slot::Bomb => self.bomb,
            Slot::Grenade => !self.grenades.is_empty(),
            s => self.weapon_in(s, 0).is_some(),
        }
    }

    pub fn gun_mut(&mut self, slot: Slot) -> Option<&mut GunState> {
        match slot {
            Slot::Primary => self.primary.as_mut().map(|p| &mut p.1),
            Slot::Secondary => self.secondary.as_mut().map(|p| &mut p.1),
            _ => None,
        }
    }

    pub fn count_grenade(&self, id: WeaponId) -> usize {
        self.grenades.iter().filter(|g| **g == id).count()
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub team: Team,
    /// Roster (0/1) for competitive scores; sides swap, squads do not.
    pub squad: u8,
    pub controller: Controller,
    pub connected: bool,
    pub alive: bool,
    pub health: i32,
    pub armor: u8,
    pub helmet: bool,
    pub defuse_kit: bool,
    pub money: u32,
    pub mv: MoveState,
    pub view: ViewAngles,
    pub inv: Inventory,
    pub active: Slot,
    pub last_slot: Slot,
    pub grenade_index: usize,
    pub hand: HandState,
    pub stats: Stats,
    pub respawn_at: Option<f64>,
    /// Flash: time it started, total duration (s) and peak alpha.
    pub flash: (f64, f32, f32),
    pub last_cmd: UserCmd,
    pub last_cmd_tick: u32,
    pub planting_since: Option<f64>,
    pub defusing_since: Option<f64>,
    /// Damage received this round per attacker (assists).
    pub damage_taken: Vec<(PlayerId, u32)>,
    /// Shots fired since spawn (deterministic spread stream).
    pub shot_counter: u32,
    pub last_hurt: f64,
    /// Dummy behaviour anchor and phase (training targets).
    pub anchor: Vec3,
    pub dummy_kind: u8,
    pub dummy_armor: (u8, bool),
    pub disconnected_at: Option<f64>,
    /// Server-side estimate of what the client displays: tick - interp - rtt/2.
    pub expected_view_lag: f64,
}

impl Player {
    pub fn new(id: PlayerId, name: String, team: Team, controller: Controller) -> Player {
        Player {
            id,
            name,
            team,
            squad: 0,
            controller,
            connected: true,
            alive: false,
            health: 0,
            armor: 0,
            helmet: false,
            defuse_kit: false,
            money: 800,
            mv: MoveState::new(Vec3::ZERO),
            view: ViewAngles::default(),
            inv: Inventory::default(),
            active: Slot::Melee,
            last_slot: Slot::Secondary,
            grenade_index: 0,
            hand: HandState::default(),
            stats: Stats::default(),
            respawn_at: None,
            flash: (0.0, 0.0, 0.0),
            last_cmd: UserCmd::default(),
            last_cmd_tick: 0,
            planting_since: None,
            defusing_since: None,
            damage_taken: Vec::new(),
            shot_counter: 0,
            last_hurt: -100.0,
            anchor: Vec3::ZERO,
            dummy_kind: 0,
            dummy_armor: (0, false),
            disconnected_at: None,
            expected_view_lag: 0.0,
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.controller == Controller::Dummy
    }

    pub fn active_weapon(&self) -> Option<WeaponId> {
        if self.active == Slot::Bomb {
            return None;
        }
        self.inv.weapon_in(self.active, self.grenade_index)
    }

    pub fn active_def<'a>(&self, cat: &'a Catalog) -> Option<&'a WeaponDef> {
        if self.active == Slot::Bomb {
            return cat.first_of(WeaponKind::Bomb).map(|b| cat.get(b));
        }
        self.active_weapon().map(|w| cat.get(w))
    }

    /// Movement speed cap given by the weapon in hand (scoped speed included).
    pub fn speed_cap(&self, cat: &Catalog) -> f32 {
        self.active_def(cat)
            .map(|w| w.speed(self.hand.zoom))
            .unwrap_or(250.0)
    }

    pub fn flash_alpha(&self, now: f64) -> f32 {
        let (start, dur, peak) = self.flash;
        if dur <= 0.0 {
            return 0.0;
        }
        let t = (now - start) as f32;
        if t < 0.0 || t > dur {
            return 0.0;
        }
        // Full white for 40 % of the duration, then a linear fade.
        let hold = dur * 0.4;
        if t < hold {
            peak
        } else {
            peak * (1.0 - (t - hold) / (dur - hold))
        }
    }

    pub fn reset_round_state(&mut self) {
        self.planting_since = None;
        self.defusing_since = None;
        self.damage_taken.clear();
        self.flash = (0.0, 0.0, 0.0);
    }
}
