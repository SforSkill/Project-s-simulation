//! The authoritative simulation. One `Sim::step` = one server tick.

use crate::bots::BotBrain;
use crate::collide::{PlayerWorld, player_box};
use crate::data::GameData;
use crate::events::GameEvent;
use crate::grenades::{Fire, Grenade, GrenadeConfig, GrenadeStep, throw_params};
use crate::level::Level;
use crate::player::{Controller, Player};
use crate::smoke::{SmokeConfig, SmokeVolume, smoke_blocks};
use engine_core::cmd::{Buttons, UserCmd};
use engine_core::math::offset_direction;
use engine_core::{Aabb, DetRng, MAX_PLAYERS, PlayerId, Team, TickRate, ViewAngles};
use engine_hitreg::{LagCompPolicy, Pose, PoseHistory, PoseRecord, hitboxes, ray_vs_hitboxes};
use engine_movement::MoveState;
use engine_movement::controller::{MoveInput, hull_for, simulate_segment};
use engine_physics::map::{SpawnDef, TargetKind};
use engine_physics::world::RaySegment;
use engine_weapons::damage::{PenetrationInput, penetrate};
use engine_weapons::{
    GrenadeKind, GunState, HandState, HitGroup, Slot, WeaponEvent, WeaponId, WeaponInput,
    WeaponKind, apply_armor, distance_falloff, movement_spread_deg, spread_offset,
};
use game_rules::{BombStatus, Match, MatchEvent, Mode, Phase, RulesConfig, add_money};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SimConfig {
    pub tick_rate: u32,
    pub lag_comp: LagCompPolicy,
    pub smoke: SmokeConfig,
    pub grenades: GrenadeConfig,
    /// 0 = easy, 1 = hard.
    pub bot_difficulty: f32,
    /// Pose history kept for lag compensation, seconds.
    pub history_seconds: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_rate: 128,
            lag_comp: LagCompPolicy::default(),
            smoke: SmokeConfig::default(),
            grenades: GrenadeConfig::default(),
            bot_difficulty: 0.5,
            history_seconds: 1.0,
        }
    }
}

/// Material crossed by a bullet (Shot Inspector).
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCross {
    pub material: String,
    pub thickness: f32,
    pub damage_after: f32,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShotHit {
    pub victim: PlayerId,
    pub group: HitGroup,
    pub distance: f32,
    pub raw_damage: f32,
    pub health_damage: u32,
    pub armor_damage: u32,
    pub point: Vec3,
    pub killed: bool,
}

/// Everything the server knows about one bullet (Shot Inspector, telemetry, replay).
#[derive(Debug, Clone, PartialEq)]
pub struct ShotReport {
    pub tick: u32,
    pub time: f64,
    pub shooter: PlayerId,
    pub weapon: WeaponId,
    pub origin: Vec3,
    pub dir: Vec3,
    pub view: ViewAngles,
    pub recoil: (f32, f32),
    pub recoil_index: f32,
    pub spread_deg: f32,
    pub spread: (f32, f32),
    pub speed: f32,
    pub on_ground: bool,
    pub claimed_view_time: f64,
    pub rewind_time: f64,
    pub rewind_clamped: bool,
    pub surfaces: Vec<SurfaceCross>,
    pub hit: Option<ShotHit>,
    pub end: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BombState {
    None,
    Carried(PlayerId),
    Dropped(Vec3),
    Planted {
        pos: Vec3,
        site: u8,
        planted_at: f64,
        explode_at: f64,
        defuser: Option<(PlayerId, f64)>,
    },
    Exploded(Vec3),
    Defused(Vec3),
}

impl BombState {
    pub fn status(&self) -> BombStatus {
        match self {
            BombState::Planted { .. } => BombStatus::Planted,
            BombState::Exploded(_) => BombStatus::Exploded,
            BombState::Defused(_) => BombStatus::Defused,
            _ => BombStatus::NotPlanted,
        }
    }

    pub fn planted(&self) -> bool {
        matches!(
            self,
            BombState::Planted { .. } | BombState::Exploded(_) | BombState::Defused(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    pub id: u16,
    pub weapon: WeaponId,
    pub gun: GunState,
    pub pos: Vec3,
    pub vel: Vec3,
    pub dropped_at: f64,
    pub dropper: PlayerId,
}

pub struct Sim {
    pub data: Arc<GameData>,
    pub level: Arc<Level>,
    pub cfg: SimConfig,
    pub rate: TickRate,
    pub tick: u32,
    pub seed: u64,
    pub players: Vec<Option<Player>>,
    pub history: Vec<PoseHistory>,
    pub grenades: Vec<Grenade>,
    pub smokes: Vec<SmokeVolume>,
    pub fires: Vec<Fire>,
    pub drops: Vec<Dropped>,
    pub bomb: BombState,
    pub rules: Match,
    pub events: Vec<GameEvent>,
    pub shots: Vec<ShotReport>,
    pub bots: HashMap<PlayerId, BotBrain>,
    pub round_kills: HashMap<PlayerId, u32>,
    pub deathmatch_end: f64,
    rng: DetRng,
    next_entity: u16,
    strip_on_next_round: bool,
    step_dist: [f32; MAX_PLAYERS],
    scratch: Vec<RaySegment>,
}

/// Time (seconds) from the start of the simulation.
pub fn tick_time(tick: u32, frac: f32, dt: f64) -> f64 {
    (tick as f64 + frac as f64) * dt
}

impl Sim {
    pub fn new(
        data: Arc<GameData>,
        level: Arc<Level>,
        rules: RulesConfig,
        cfg: SimConfig,
        seed: u64,
    ) -> Result<Sim, String> {
        let rate = TickRate::new(cfg.tick_rate)?;
        rules.validate()?;
        let hist_cap = (cfg.history_seconds * rate.hz() as f32).ceil() as usize + 8;
        let mode = rules.mode;
        let deathmatch_end = if rules.time_limit > 0.0 {
            rules.time_limit as f64
        } else {
            f64::INFINITY
        };
        let mut m = Match::new(rules, 0.0);
        if !mode.has_rounds() {
            m.phase = Phase::Live;
            m.phase_end = f64::INFINITY;
        }
        let mut sim = Sim {
            data,
            level,
            cfg,
            rate,
            tick: 0,
            seed,
            players: (0..MAX_PLAYERS).map(|_| None).collect(),
            history: (0..MAX_PLAYERS)
                .map(|_| PoseHistory::new(hist_cap))
                .collect(),
            grenades: Vec::new(),
            smokes: Vec::new(),
            fires: Vec::new(),
            drops: Vec::new(),
            bomb: BombState::None,
            rules: m,
            events: Vec::new(),
            shots: Vec::new(),
            bots: HashMap::new(),
            round_kills: HashMap::new(),
            deathmatch_end,
            rng: DetRng::new(seed ^ 0x5eed),
            next_entity: 64,
            strip_on_next_round: true,
            step_dist: [0.0; MAX_PLAYERS],
            scratch: Vec::new(),
        };
        if mode == Mode::Training {
            sim.spawn_training_targets();
        }
        Ok(sim)
    }

    pub fn dt(&self) -> f64 {
        self.rate.dt_f64()
    }

    /// Current simulation time, seconds (start of the next tick).
    pub fn now(&self) -> f64 {
        self.tick as f64 * self.dt()
    }

    pub fn mode(&self) -> Mode {
        self.rules.cfg.mode
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(id.0 as usize).and_then(|p| p.as_ref())
    }

    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.get_mut(id.0 as usize).and_then(|p| p.as_mut())
    }

    pub fn iter_players(&self) -> impl Iterator<Item = &Player> {
        self.players.iter().flatten()
    }

    fn alloc_entity(&mut self) -> u16 {
        let id = self.next_entity;
        self.next_entity = if self.next_entity >= u16::MAX - 1 {
            64
        } else {
            self.next_entity + 1
        };
        id
    }

    // ------------------------------------------------------------------ players

    /// Adds a player in the first free slot (dummies use the high slots).
    pub fn add_player(
        &mut self,
        name: &str,
        controller: Controller,
        team: Option<Team>,
    ) -> Option<PlayerId> {
        let range: Box<dyn Iterator<Item = usize>> = if controller == Controller::Dummy {
            Box::new((0..MAX_PLAYERS).rev())
        } else {
            Box::new(0..MAX_PLAYERS)
        };
        let mut slot = None;
        for i in range {
            if self.players[i].is_none() {
                slot = Some(i);
                break;
            }
        }
        let i = slot?;
        let id = PlayerId(i as u8);
        let team = team.unwrap_or_else(|| self.auto_team());
        let mut p = Player::new(id, sanitize_name(name), team, controller);
        p.squad = self.rules.squad_of(team).map(|s| s.0).unwrap_or(0);
        p.money = if self.rules.cfg.economy.enabled {
            self.rules.cfg.economy.start_money
        } else {
            0
        };
        self.players[i] = Some(p);
        self.history[i].clear();
        if controller == Controller::Bot {
            self.bots
                .insert(id, BotBrain::new(id, self.seed, self.cfg.bot_difficulty));
        }
        // Joining mid-round: wait for the next round in round modes.
        let can_spawn_now =
            !self.mode().has_rounds() || matches!(self.rules.phase, Phase::Warmup | Phase::Freeze);
        if team.is_playing() && can_spawn_now && controller != Controller::Dummy {
            self.spawn(id);
        }
        Some(id)
    }

    pub fn remove_player(&mut self, id: PlayerId) {
        if let Some(p) = self.players[id.0 as usize].take() {
            if self.bomb == BombState::Carried(id) {
                self.bomb = BombState::Dropped(p.mv.origin + Vec3::Y * 8.0);
                self.events
                    .push(GameEvent::BombDropped { pos: p.mv.origin });
            }
            if let BombState::Planted { defuser, .. } = &mut self.bomb
                && defuser.is_some_and(|d| d.0 == id)
            {
                *defuser = None;
            }
        }
        self.bots.remove(&id);
        self.history[id.0 as usize].clear();
    }

    /// Team with fewer players (excluding dummies).
    pub fn auto_team(&self) -> Team {
        let count = |t: Team| {
            self.iter_players()
                .filter(|p| p.team == t && !p.is_dummy())
                .count()
        };
        if self.mode() == Mode::Training {
            return Team::Attack;
        }
        if count(Team::Attack) <= count(Team::Defense) {
            Team::Attack
        } else {
            Team::Defense
        }
    }

    pub fn team_count(&self, t: Team) -> usize {
        self.iter_players()
            .filter(|p| p.team == t && !p.is_dummy())
            .count()
    }

    pub fn set_team(&mut self, id: PlayerId, team: Team) -> Result<(), String> {
        let max = self.rules.cfg.team_size as usize;
        if team.is_playing() && self.team_count(team) >= max && self.mode() != Mode::Training {
            return Err("Équipe complète".into());
        }
        let squad = self.rules.squad_of(team).map(|s| s.0).unwrap_or(0);
        let respawn =
            !self.mode().has_rounds() || matches!(self.rules.phase, Phase::Warmup | Phase::Freeze);
        let Some(p) = self.player_mut(id) else {
            return Err("joueur inconnu".into());
        };
        if p.team == team {
            return Ok(());
        }
        p.team = team;
        p.squad = squad;
        p.alive = false;
        p.inv = Default::default();
        if team.is_playing() && respawn {
            self.spawn(id);
        }
        Ok(())
    }

    fn spawn_points(&self, team: Team) -> Vec<SpawnDef> {
        let def = &self.level.def;
        let dm = matches!(self.mode(), Mode::Deathmatch)
            || (self.rules.phase == Phase::Warmup
                && self.mode().has_rounds()
                && !def.dm_spawns.is_empty()
                && false);
        if dm && !def.dm_spawns.is_empty() {
            return def.dm_spawns.clone();
        }
        let s: Vec<SpawnDef> = def
            .spawns
            .iter()
            .filter(|s| s.team == team)
            .cloned()
            .collect();
        if s.is_empty() { def.spawns.clone() } else { s }
    }

    /// Picks a free spawn: team order in round modes, farthest from enemies in DM.
    fn choose_spawn(&mut self, id: PlayerId) -> SpawnDef {
        let team = self.player(id).map(|p| p.team).unwrap_or(Team::Attack);
        let points = self.spawn_points(team);
        let (mins, maxs) = hull_for(&self.data.movement, false);
        let occupied = |pos: Vec3, me: PlayerId, players: &[Option<Player>]| {
            players.iter().flatten().any(|o| {
                o.id != me
                    && o.alive
                    && player_box(o.mv.origin, mins, maxs).intersects(&player_box(pos, mins, maxs))
            })
        };
        if self.mode() == Mode::Deathmatch {
            let enemies: Vec<Vec3> = self
                .iter_players()
                .filter(|o| o.alive && o.id != id)
                .map(|o| o.mv.origin)
                .collect();
            let mut best = None;
            let mut best_d = -1.0;
            let start = self.rng.below(points.len() as u32) as usize;
            for k in 0..points.len() {
                let s = &points[(start + k) % points.len()];
                if occupied(s.pos, id, &self.players) {
                    continue;
                }
                let d = enemies
                    .iter()
                    .map(|e| e.distance(s.pos))
                    .fold(f32::MAX, f32::min);
                if d > best_d {
                    best_d = d;
                    best = Some(s.clone());
                }
            }
            if let Some(b) = best {
                return b;
            }
        }
        let order = self
            .iter_players()
            .filter(|p| p.team == team && !p.is_dummy() && p.id < id)
            .count();
        for k in 0..points.len() {
            let s = &points[(order + k) % points.len()];
            if !occupied(s.pos, id, &self.players) {
                return s.clone();
            }
        }
        points[order % points.len()].clone()
    }

    fn default_loadout(&self, p: &mut Player) {
        let cat = &self.data.catalog;
        p.inv.knife = cat.first_of(WeaponKind::Knife);
        if p.inv.secondary.is_none()
            && let Some(pistol) = cat.first_of(WeaponKind::Pistol)
        {
            p.inv.secondary = Some((pistol, GunState::full(cat.get(pistol))));
        }
        for name in &self.rules.cfg.free_loadout {
            if let Some(w) = cat.by_name(name) {
                let def = cat.get(w);
                match def.slot {
                    Slot::Primary if p.inv.primary.is_none() => {
                        p.inv.primary = Some((w, GunState::full(def)))
                    }
                    Slot::Secondary => p.inv.secondary = Some((w, GunState::full(def))),
                    _ => {}
                }
            }
        }
        if self.rules.cfg.free_armor {
            p.armor = 100;
            p.helmet = true;
        }
    }

    /// (Re)spawns a player at a spawn point with a fresh loadout if needed.
    pub fn spawn(&mut self, id: PlayerId) {
        let s = self.choose_spawn(id);
        let now = self.now();
        let mut p = match self.players[id.0 as usize].take() {
            Some(p) => p,
            None => return,
        };
        if !p.alive {
            p.inv = Default::default();
            p.armor = 0;
            p.helmet = false;
            p.defuse_kit = false;
        }
        self.default_loadout(&mut p);
        p.alive = true;
        p.health = 100;
        p.mv = MoveState::new(s.pos);
        p.view = ViewAngles::new(0.0, s.yaw);
        p.respawn_at = None;
        p.reset_round_state();
        p.active = if p.inv.primary.is_some() {
            Slot::Primary
        } else {
            Slot::Secondary
        };
        p.last_slot = Slot::Melee;
        let def = p.active_weapon().map(|w| self.data.catalog.get(w).clone());
        p.hand = def
            .map(|d| HandState::deploy(&d, now, None))
            .unwrap_or_default();
        p.last_cmd = UserCmd {
            view_start: p.view,
            view_end: p.view,
            ..Default::default()
        };
        if self.mode() == Mode::Training && !p.is_dummy() {
            p.money = 16000;
        }
        self.players[id.0 as usize] = Some(p);
        let pose = self.pose_of(id);
        self.history[id.0 as usize].push(PoseRecord {
            time: self.tick as f64,
            pose,
            alive: true,
            teleported: true,
        });
        self.events.push(GameEvent::Spawn { player: id });
    }

    fn spawn_training_targets(&mut self) {
        let targets = self.level.def.targets.clone();
        for (i, t) in targets.iter().enumerate() {
            let Some(id) = self.add_player(
                &format!("Cible {}", i + 1),
                Controller::Dummy,
                Some(Team::Defense),
            ) else {
                break;
            };
            self.spawn_dummy(id, t.pos, t.yaw, t.kind, t.armor, t.helmet);
        }
    }

    fn spawn_dummy(
        &mut self,
        id: PlayerId,
        pos: Vec3,
        yaw: f32,
        kind: TargetKind,
        armor: u8,
        helmet: bool,
    ) {
        let now = self.now();
        if let Some(p) = self.player_mut(id) {
            p.alive = true;
            p.health = 100;
            p.armor = armor;
            p.helmet = helmet;
            p.mv = MoveState::new(pos);
            p.view = ViewAngles::new(0.0, yaw);
            p.anchor = pos;
            p.dummy_kind = match kind {
                TargetKind::Static => 0,
                TargetKind::Strafe => 1,
                TargetKind::Crouch => 2,
            };
            p.dummy_armor = (armor, helmet);
            p.respawn_at = None;
            p.inv = Default::default();
            p.active = Slot::Melee;
            p.hand = HandState {
                deploy_until: now,
                ..Default::default()
            };
        }
        self.history[id.0 as usize].clear();
        let pose = self.pose_of(id);
        self.history[id.0 as usize].push(PoseRecord {
            time: self.tick as f64,
            pose,
            alive: true,
            teleported: true,
        });
    }

    pub fn pose_of(&self, id: PlayerId) -> Pose {
        let p = self.player(id).expect("player");
        Pose {
            origin: p.mv.origin,
            yaw: p.view.yaw,
            duck: p.mv.duck_amount,
        }
    }

    // ------------------------------------------------------------------ buying

    pub fn can_buy_here(&self, id: PlayerId) -> bool {
        let Some(p) = self.player(id) else {
            return false;
        };
        if !p.alive || !p.team.is_playing() {
            return false;
        }
        let r = &self.rules;
        if r.cfg.buy_anywhere {
            return true;
        }
        if !r.in_buy_time(self.now()) {
            return false;
        }
        let zones = &self.level.def.buy_zones;
        zones.is_empty()
            || zones.iter().any(|z| {
                (z.team == p.team || z.team == Team::Spectator)
                    && z.aabb().contains(p.mv.origin + Vec3::Y)
            })
    }

    /// Buys an item: weapon id, `kevlar`, `kevlar_helmet` or `defuse_kit`.
    pub fn buy(&mut self, id: PlayerId, item: &str) -> Result<(), String> {
        if !self.can_buy_here(id) {
            return Err("Achat impossible ici ou maintenant".into());
        }
        let data = self.data.clone();
        let cat = &data.catalog;
        let eq = &cat.equipment;
        let economy = self.rules.cfg.economy.enabled && self.mode() != Mode::Training;
        let now = self.now();
        let p = self.player_mut(id).ok_or("joueur inconnu")?;
        let pay = |p: &mut Player, price: u32| -> Result<(), String> {
            if !economy {
                return Ok(());
            }
            if p.money < price {
                return Err(format!("Fonds insuffisants ({price} $)"));
            }
            p.money -= price;
            Ok(())
        };
        match item {
            "kevlar" => {
                if p.armor >= 100 {
                    return Err("Déjà équipé".into());
                }
                pay(p, eq.kevlar_price)?;
                p.armor = 100;
            }
            "kevlar_helmet" => {
                if p.armor >= 100 && p.helmet {
                    return Err("Déjà équipé".into());
                }
                let price = if p.armor >= 100 {
                    eq.helmet_upgrade_price
                } else {
                    eq.kevlar_helmet_price
                };
                pay(p, price)?;
                p.armor = 100;
                p.helmet = true;
            }
            "defuse_kit" => {
                if p.team != Team::Defense || p.defuse_kit {
                    return Err("Indisponible".into());
                }
                pay(p, eq.defuse_kit_price)?;
                p.defuse_kit = true;
            }
            name => {
                let w = cat
                    .by_name(name)
                    .ok_or_else(|| format!("Objet inconnu: {name}"))?;
                let def = cat.get(w);
                match def.kind {
                    WeaponKind::Grenade => {
                        let total = p.inv.grenades.len();
                        if total >= eq.max_grenades as usize
                            || p.inv.count_grenade(w) >= def.max_carry as usize
                        {
                            return Err("Limite de grenades".into());
                        }
                        pay(p, def.price)?;
                        p.inv.grenades.push(w);
                    }
                    WeaponKind::Knife | WeaponKind::Bomb => return Err("Non achetable".into()),
                    _ => {
                        let cur = match def.slot {
                            Slot::Primary => p.inv.primary.map(|x| x.0),
                            _ => p.inv.secondary.map(|x| x.0),
                        };
                        if cur == Some(w) {
                            return Err("Déjà équipé".into());
                        }
                        pay(p, def.price)?;
                        let entry = Some((w, GunState::full(def)));
                        let old = if def.slot == Slot::Primary {
                            std::mem::replace(&mut p.inv.primary, entry)
                        } else {
                            std::mem::replace(&mut p.inv.secondary, entry)
                        };
                        p.last_slot = p.active;
                        p.active = def.slot;
                        p.hand = HandState::deploy(def, now, None);
                        let origin = p.mv.origin;
                        if let Some((ow, og)) = old
                            && economy
                        {
                            let did = self.alloc_entity();
                            self.drops.push(Dropped {
                                id: did,
                                weapon: ow,
                                gun: og,
                                pos: origin + Vec3::Y * 16.0,
                                vel: Vec3::ZERO,
                                dropped_at: now,
                                dropper: id,
                            });
                        }
                    }
                }
            }
        }
        self.events.push(GameEvent::Bought {
            player: id,
            item: item.to_string(),
        });
        Ok(())
    }

    // ------------------------------------------------------------------ tick

    /// Advances one tick. `cmds[i]` is the command of player slot `i` for this tick
    /// (missing commands repeat the last known buttons, without new events).
    pub fn step(&mut self, cmds: &mut [Option<UserCmd>]) {
        let now = self.now();
        self.bot_cmds(cmds);
        self.dummy_cmds(cmds);
        self.respawns(now);
        for i in 0..MAX_PLAYERS {
            let cmd = cmds.get_mut(i).and_then(|c| c.take());
            let Some(p) = &self.players[i] else { continue };
            let cmd = match cmd {
                Some(mut c) => {
                    c.tick = self.tick;
                    if c.sanitize().is_err() {
                        continue;
                    }
                    c
                }
                None => {
                    // Packet loss: hold the last buttons and view.
                    let last = &p.last_cmd;
                    UserCmd {
                        tick: self.tick,
                        buttons_start: last.buttons_end(),
                        view_start: last.view_end,
                        view_end: last.view_end,
                        events: Vec::new(),
                        weapon_select: 0,
                        view_time: last.view_time,
                    }
                }
            };
            self.run_cmd(PlayerId(i as u8), cmd);
        }
        self.update_grenades(now);
        self.update_fires(now);
        self.update_bomb(now);
        self.update_drops(now);
        let dt = self.dt() as f32;
        let _ = dt;
        self.tick += 1;
        // Record poses at the new time (end of this tick).
        for i in 0..MAX_PLAYERS {
            if let Some(p) = &self.players[i] {
                let rec = PoseRecord {
                    time: self.tick as f64,
                    pose: Pose {
                        origin: p.mv.origin,
                        yaw: p.view.yaw,
                        duck: p.mv.duck_amount,
                    },
                    alive: p.alive,
                    teleported: false,
                };
                self.history[i].push(rec);
            }
        }
        self.update_rules();
    }

    fn respawns(&mut self, now: f64) {
        let mut todo = Vec::new();
        for p in self.players.iter().flatten() {
            if !p.alive
                && let Some(t) = p.respawn_at
                && now >= t
            {
                todo.push(p.id);
            }
        }
        for id in todo {
            if self.player(id).is_some_and(|p| p.is_dummy()) {
                let p = self.player(id).unwrap();
                let (pos, yaw, armor) = (p.anchor, p.view.yaw, p.dummy_armor);
                let kind = match p.dummy_kind {
                    1 => TargetKind::Strafe,
                    2 => TargetKind::Crouch,
                    _ => TargetKind::Static,
                };
                self.spawn_dummy(id, pos, yaw, kind, armor.0, armor.1);
            } else {
                self.spawn(id);
            }
        }
    }

    fn other_boxes(&self, me: usize) -> Vec<Aabb> {
        let (mins, maxs) = hull_for(&self.data.movement, false);
        let (dmins, dmaxs) = hull_for(&self.data.movement, true);
        self.players
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let p = p.as_ref()?;
                if i == me || !p.alive {
                    return None;
                }
                let (a, b) = if p.mv.ducked {
                    (dmins, dmaxs)
                } else {
                    (mins, maxs)
                };
                Some(player_box(p.mv.origin, a, b))
            })
            .collect()
    }

    fn select_weapon(&mut self, id: PlayerId, sel: u8, now: f64) {
        let data = self.data.clone();
        let Some(p) = self.player_mut(id) else { return };
        let target = match sel {
            1 => Slot::Primary,
            2 => Slot::Secondary,
            3 => Slot::Melee,
            4 => Slot::Grenade,
            5 => Slot::Bomb,
            6 => p.last_slot,
            _ => return,
        };
        if !p.inv.has(target) {
            return;
        }
        if target == p.active {
            if target == Slot::Grenade && p.inv.grenades.len() > 1 {
                p.grenade_index = (p.grenade_index + 1) % p.inv.grenades.len();
            } else {
                return;
            }
        } else {
            p.last_slot = p.active;
            p.active = target;
        }
        p.planting_since = None;
        if let Some(def) = p.active_def(&data.catalog) {
            p.hand = HandState::deploy(def, now, Some(&p.hand));
        }
    }

    fn run_cmd(&mut self, id: PlayerId, cmd: UserCmd) {
        let dt = self.dt();
        let tick = self.tick;
        let data = self.data.clone();
        let cat = &data.catalog;
        let mvcfg = &data.movement;
        let frozen_rules = self.mode().has_rounds() && self.rules.players_frozen();
        let Some(p) = self.player(id) else { return };
        if !p.alive || !p.team.is_playing() {
            if let Some(p) = self.player_mut(id) {
                p.view = cmd.view_end;
                p.last_cmd = cmd;
                p.last_cmd_tick = tick;
            }
            return;
        }
        if cmd.weapon_select != 0 {
            self.select_weapon(id, cmd.weapon_select, tick_time(tick, 0.0, dt));
        }
        let boxes = self.other_boxes(id.0 as usize);
        let level = self.level.clone();
        let pw = PlayerWorld {
            world: &level.world,
            boxes: &boxes,
        };
        let segs = cmd.segments();
        let mut wevents: Vec<(WeaponEvent, Vec3, Vec3, ViewAngles, f32, bool)> = Vec::new();
        let mut was_on_ground;
        let mut landed = false;
        let mut jumped = false;
        let mut traveled = 0.0;
        {
            let p = self.players[id.0 as usize].as_mut().unwrap();
            was_on_ground = p.mv.on_ground;
            for seg in &segs {
                let sdt = (seg.t1 - seg.t0) * dt as f32;
                if sdt <= 0.0 {
                    continue;
                }
                let t0 = tick_time(tick, seg.t0, dt);
                let t1 = tick_time(tick, seg.t1, dt);
                let busy = p.planting_since.is_some() || p.defusing_since.is_some();
                let frozen = frozen_rules || busy;
                // Weapon first: shots at t0 use the position at t0.
                if let Some(w) = p.active_weapon() {
                    let def = cat.get(w);
                    let mut out = Vec::new();
                    let slot = p.active;
                    let mut dummy = GunState::default();
                    let gun = p.inv.gun_mut(slot).unwrap_or(&mut dummy);
                    p.hand.update(
                        def,
                        gun,
                        &WeaponInput {
                            t0,
                            t1,
                            buttons: seg.buttons,
                            prev: seg.prev_buttons,
                            can_fire: !frozen,
                        },
                        &mut out,
                    );
                    for e in out {
                        let et = match e {
                            WeaponEvent::Shot { time, .. }
                            | WeaponEvent::Melee { time, .. }
                            | WeaponEvent::Throw { time, .. } => time,
                            _ => t0,
                        };
                        let frac_in_tick = ((et / dt) - tick as f64) as f32;
                        let view = cmd.view_at(frac_in_tick.clamp(0.0, 1.0));
                        let eye = p.mv.eye(mvcfg) + p.mv.velocity * (et - t0) as f32;
                        wevents.push((
                            e,
                            eye,
                            p.mv.velocity,
                            view,
                            p.mv.horizontal_speed(),
                            p.mv.on_ground,
                        ));
                    }
                }
                let speed = p.speed_cap(cat);
                let before = p.mv.origin;
                let input = MoveInput {
                    buttons: seg.buttons,
                    view: seg.view,
                    max_speed: speed,
                    frozen,
                    dt: sdt,
                };
                let vy = p.mv.velocity.y;
                simulate_segment(&mut p.mv, &input, mvcfg, &pw);
                if was_on_ground && !p.mv.on_ground && p.mv.velocity.y > vy + 100.0 {
                    jumped = true;
                }
                if !was_on_ground && p.mv.on_ground {
                    landed = true;
                }
                was_on_ground = p.mv.on_ground;
                traveled +=
                    Vec3::new(p.mv.origin.x - before.x, 0.0, p.mv.origin.z - before.z).length();
            }
            p.view = cmd.view_end;
        }
        // Sounds.
        let (origin, hspeed, on_ground) = {
            let p = self.player(id).unwrap();
            (p.mv.origin, p.mv.horizontal_speed(), p.mv.on_ground)
        };
        if jumped {
            self.events.push(GameEvent::Jump {
                player: id,
                pos: origin,
            });
        }
        if landed {
            self.events.push(GameEvent::Land {
                player: id,
                pos: origin,
            });
        }
        let walking = cmd.buttons_end().has(Buttons::WALK) || cmd.buttons_end().has(Buttons::DUCK);
        if on_ground && hspeed > 135.0 && !walking {
            self.step_dist[id.0 as usize] += traveled;
            if self.step_dist[id.0 as usize] > 72.0 {
                self.step_dist[id.0 as usize] = 0.0;
                self.events.push(GameEvent::Footstep {
                    player: id,
                    pos: origin,
                    loud: hspeed > 200.0,
                });
            }
        }
        for (e, eye, vel, view, speed, ground) in wevents {
            self.weapon_event(id, e, eye, vel, view, speed, ground, &cmd);
        }
        self.bomb_actions(id, &cmd);
        self.pickups(id);
        let p = self.player_mut(id).unwrap();
        if cmd.buttons_end().has(Buttons::DROP) && !cmd.buttons_start.has(Buttons::DROP) {
            let _ = p;
            self.drop_active(id);
        }
        let p = self.player_mut(id).unwrap();
        p.last_cmd = cmd;
        p.last_cmd_tick = tick;
    }

    fn weapon_event(
        &mut self,
        id: PlayerId,
        e: WeaponEvent,
        eye: Vec3,
        vel: Vec3,
        view: ViewAngles,
        speed: f32,
        ground: bool,
        cmd: &UserCmd,
    ) {
        match e {
            WeaponEvent::Shot {
                time,
                recoil,
                recoil_index,
                ..
            } => self.fire_bullet(
                id,
                time,
                eye,
                view,
                recoil,
                recoil_index,
                speed,
                ground,
                cmd,
            ),
            WeaponEvent::Melee { time, heavy } => self.melee(id, time, eye, view, heavy),
            WeaponEvent::Throw { time, strength } => {
                self.throw_grenade(id, time, eye, vel, view, strength)
            }
            WeaponEvent::ReloadStarted { .. } => self.events.push(GameEvent::Reload { player: id }),
            WeaponEvent::DryFire { .. } => self.events.push(GameEvent::DryFire { player: id }),
            WeaponEvent::Zoom { level, .. } => {
                self.events.push(GameEvent::Zoom { player: id, level })
            }
            WeaponEvent::ReloadFinished { .. } => {}
        }
    }

    // ------------------------------------------------------------------ combat

    fn can_damage(&self, attacker: PlayerId, victim: &Player) -> bool {
        if victim.id == attacker || !victim.alive {
            return false;
        }
        let Some(a) = self.player(attacker) else {
            return true;
        };
        if self.mode() == Mode::Deathmatch {
            return true;
        }
        victim.team != a.team || self.rules.cfg.friendly_fire || victim.is_dummy()
    }

    /// Server-authoritative hitscan with bounded lag compensation and penetration.
    fn fire_bullet(
        &mut self,
        id: PlayerId,
        time: f64,
        eye: Vec3,
        view: ViewAngles,
        recoil: (f32, f32),
        recoil_index: f32,
        speed: f32,
        on_ground: bool,
        cmd: &UserCmd,
    ) {
        let data = self.data.clone();
        let Some(p) = self.player(id) else { return };
        let Some(wid) = p.active_weapon() else { return };
        let w = data.catalog.get(wid);
        let zoom = p.hand.zoom;
        let shot_index = p.shot_counter;
        let is_bot = p.controller != Controller::Human;
        let expected = self.tick as f64 - p.expected_view_lag;
        let spread_deg = movement_spread_deg(w, speed, on_ground, zoom);
        let mut rng = DetRng::for_key(self.seed, id.0 as u64, shot_index as u64);
        let spread = spread_offset(spread_deg, &mut rng);
        let dir = offset_direction(view.forward(), recoil.0 + spread.0, recoil.1 + spread.1);
        let now_ticks = self.tick as f64;
        let claimed = if is_bot {
            now_ticks
        } else {
            cmd.view_time.as_f64()
        };
        let (rewind, clamped) = if is_bot {
            (now_ticks, false)
        } else {
            self.cfg
                .lag_comp
                .rewind_time(now_ticks, claimed, expected, self.dt())
        };

        // Candidate victims posed at the rewound time.
        let mut victims = Vec::new();
        for (i, v) in self.players.iter().enumerate() {
            let Some(v) = v else { continue };
            if !self.can_damage(id, v) {
                continue;
            }
            let pose = if v.is_dummy() || !self.cfg.lag_comp.enabled {
                Some(Pose {
                    origin: v.mv.origin,
                    yaw: v.view.yaw,
                    duck: v.mv.duck_amount,
                })
            } else {
                self.history[i].sample(rewind)
            };
            if let Some(pose) = pose {
                victims.push((v.id, hitboxes(&pose)));
            }
        }
        let mut segs = std::mem::take(&mut self.scratch);
        self.level.world.ray_segments(eye, dir, w.range, &mut segs);
        let mut damage = w.damage;
        let mut power = w.penetration;
        let mut t = 0.0f32;
        let mut surfaces = Vec::new();
        let mut impacts = Vec::new();
        let mut hit: Option<(PlayerId, engine_hitreg::BodyHit)> = None;
        let mut penetrated = false;
        let mut end_t = w.range;
        let mut k = 0;
        let nearest_victim = |t0: f32, t1: f32| -> Option<(PlayerId, engine_hitreg::BodyHit)> {
            let mut best: Option<(PlayerId, engine_hitreg::BodyHit)> = None;
            for (vid, hb) in &victims {
                if let Some(h) = ray_vs_hitboxes(hb, eye, dir, t1)
                    && h.t >= t0
                    && best.is_none_or(|b| h.t < b.1.t)
                {
                    best = Some((*vid, h));
                }
            }
            best
        };
        loop {
            // Next solid piece (merging overlapping brushes).
            while k < segs.len() && segs[k].t_exit <= t {
                k += 1;
            }
            let next = segs.get(k).copied();
            let limit = next.map(|s| s.t_enter.max(t)).unwrap_or(w.range);
            if let Some(h) = nearest_victim(t, limit) {
                hit = Some(h);
                end_t = h.1.t;
                break;
            }
            let Some(s) = next else { break };
            let enter = s.t_enter.max(t);
            impacts.push(eye + dir * enter);
            // Thickness up to the exit of this brush or of overlapping brushes.
            let mut exit = s.t_exit;
            let mut j = k + 1;
            while j < segs.len() && segs[j].t_enter <= exit + 0.01 {
                exit = exit.max(segs[j].t_exit);
                j += 1;
            }
            let thickness = exit - enter;
            let mat = data.materials.get(s.material);
            let ok = penetrate(
                &mut damage,
                &mut power,
                PenetrationInput {
                    thickness,
                    modifier: mat.penetration_modifier,
                    retention_per_unit: mat.damage_retention_per_unit,
                    entry_loss: mat.entry_loss,
                },
            );
            surfaces.push(SurfaceCross {
                material: mat.name.clone(),
                thickness,
                damage_after: if ok { damage } else { 0.0 },
                stopped: !ok,
            });
            if !ok {
                end_t = enter;
                break;
            }
            penetrated = true;
            impacts.push(eye + dir * exit);
            t = exit + 0.01;
            k = j;
        }
        self.scratch = segs;
        let mut shot_hit = None;
        if let Some((vid, bh)) = hit {
            let dist = bh.t;
            let raw = damage * bh.group.multiplier(w) * distance_falloff(w, dist);
            let (hd, ad, killed) = self.apply_damage(
                vid,
                Some(id),
                Some(wid),
                raw,
                Some(bh.group),
                w.armor_ratio,
                w.tagging,
                eye,
                penetrated,
            );
            shot_hit = Some(ShotHit {
                victim: vid,
                group: bh.group,
                distance: dist,
                raw_damage: raw,
                health_damage: hd,
                armor_damage: ad,
                point: bh.point,
                killed,
            });
        }
        if let Some(p) = self.player_mut(id) {
            p.shot_counter += 1;
            p.stats.shots += 1;
            if shot_hit.is_some() {
                p.stats.hits += 1;
            }
        }
        let end = eye + dir * end_t;
        self.events.push(GameEvent::Shot {
            shooter: id,
            weapon: wid,
            origin: eye,
            end,
            impacts,
        });
        self.shots.push(ShotReport {
            tick: self.tick,
            time,
            shooter: id,
            weapon: wid,
            origin: eye,
            dir,
            view,
            recoil,
            recoil_index,
            spread_deg,
            spread,
            speed,
            on_ground,
            claimed_view_time: claimed,
            rewind_time: rewind,
            rewind_clamped: clamped,
            surfaces,
            hit: shot_hit,
            end,
        });
    }

    fn melee(&mut self, id: PlayerId, _time: f64, eye: Vec3, view: ViewAngles, heavy: bool) {
        let data = self.data.clone();
        let Some(p) = self.player(id) else { return };
        let Some(wid) = p.active_weapon() else { return };
        let w = data.catalog.get(wid);
        let range = if heavy {
            w.melee.heavy_range
        } else {
            w.melee.range
        };
        let dir = view.forward();
        let mut best: Option<(PlayerId, f32, HitGroup, f32)> = None;
        for v in self.players.iter().flatten() {
            if !self.can_damage(id, v) {
                continue;
            }
            let pose = Pose {
                origin: v.mv.origin,
                yaw: v.view.yaw,
                duck: v.mv.duck_amount,
            };
            if let Some(h) = ray_vs_hitboxes(&hitboxes(&pose), eye, dir, range + 16.0)
                && self.level.world.line_of_sight(eye, h.point)
                && best.is_none_or(|b| h.t < b.1)
            {
                // Backstab: attacker behind the victim (victim forward · dir > 0.5).
                let back = v
                    .view
                    .forward_flat()
                    .dot(Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero())
                    > 0.5;
                let dmg = match (heavy, back) {
                    (false, false) => w.melee.damage,
                    (false, true) => w.melee.backstab_damage,
                    (true, false) => w.melee.heavy_damage,
                    (true, true) => w.melee.heavy_backstab_damage,
                };
                best = Some((v.id, h.t, h.group, dmg));
            }
        }
        if let Some((vid, _, group, dmg)) = best {
            self.apply_damage(
                vid,
                Some(id),
                Some(wid),
                dmg,
                Some(group),
                0.85,
                0.0,
                eye,
                false,
            );
        }
    }

    /// Applies damage; returns `(health_damage, armor_damage, killed)`.
    pub fn apply_damage(
        &mut self,
        victim: PlayerId,
        attacker: Option<PlayerId>,
        weapon: Option<WeaponId>,
        raw: f32,
        group: Option<HitGroup>,
        armor_ratio: f32,
        tagging: f32,
        from: Vec3,
        penetrated: bool,
    ) -> (u32, u32, bool) {
        let now = self.now();
        let mvcfg = self.data.movement.clone();
        let Some(v) = self.player_mut(victim) else {
            return (0, 0, false);
        };
        if !v.alive {
            return (0, 0, false);
        }
        let (hd, ad) = match group {
            Some(g) => apply_armor(raw, g, v.armor, v.helmet, armor_ratio),
            None => apply_armor(raw, HitGroup::Stomach, v.armor, false, armor_ratio),
        };
        let hd_applied = hd.min(v.health.max(0) as u32);
        v.health -= hd as i32;
        v.armor = v.armor.saturating_sub(ad.min(255) as u8);
        if v.armor == 0 {
            v.helmet = false;
        }
        v.last_hurt = now;
        // Tagging only without kevlar (it comes back once the armor is destroyed).
        // No aim punch: the victim's view is never touched.
        if v.armor == 0 && tagging > 0.0 {
            v.mv.apply_tag(tagging, mvcfg.tag_floor);
        }
        if let Some(a) = attacker {
            if let Some(e) = v.damage_taken.iter_mut().find(|e| e.0 == a) {
                e.1 += hd_applied;
            } else {
                v.damage_taken.push((a, hd_applied));
            }
        }
        let health = v.health;
        let armor = v.armor;
        let killed = health <= 0;
        if let Some(a) = attacker
            && let Some(ap) = self.player_mut(a)
        {
            ap.stats.damage += hd_applied;
        }
        self.events.push(GameEvent::Damage {
            attacker,
            victim,
            amount: hd_applied.min(u16::MAX as u32) as u16,
            group,
            health: health.max(0) as i16,
            armor,
            from,
        });
        if killed {
            self.kill(
                victim,
                attacker,
                weapon,
                group == Some(HitGroup::Head),
                penetrated,
            );
        }
        (hd_applied, ad, killed)
    }

    fn kill(
        &mut self,
        victim: PlayerId,
        killer: Option<PlayerId>,
        weapon: Option<WeaponId>,
        headshot: bool,
        penetrated: bool,
    ) {
        let now = self.now();
        let data = self.data.clone();
        let mode = self.mode();
        let respawn = match mode {
            Mode::Training | Mode::Deathmatch => Some(self.rules.cfg.respawn_time as f64),
            _ if self.rules.phase == Phase::Warmup => Some(2.0),
            _ => None,
        };
        let economy = self.rules.cfg.economy.enabled
            && mode == Mode::Competitive
            && self.rules.phase != Phase::Warmup;
        let (vteam, origin, drop, assister, dummy) = {
            let v = self.player_mut(victim).unwrap();
            v.alive = false;
            v.health = 0;
            v.stats.deaths += 1;
            v.planting_since = None;
            v.defusing_since = None;
            v.respawn_at = if v.is_dummy() {
                Some(now + 1.5)
            } else {
                respawn.map(|r| now + r)
            };
            let assister = v
                .damage_taken
                .iter()
                .filter(|d| Some(d.0) != killer && d.1 >= 41)
                .max_by_key(|d| d.1)
                .map(|d| d.0);
            let drop = v.inv.primary.take().or_else(|| v.inv.secondary.take());
            (v.team, v.mv.origin, drop, assister, v.is_dummy())
        };
        if let Some(a) = assister
            && let Some(ap) = self.player_mut(a)
        {
            ap.stats.assists += 1;
        }
        if let Some(k) = killer
            && k != victim
        {
            let reward = weapon
                .map(|w| data.catalog.get(w).kill_reward)
                .unwrap_or(300);
            let max = self.rules.cfg.economy.clone();
            let kp = self.player_mut(k).unwrap();
            let team_kill = kp.team == vteam && mode != Mode::Deathmatch && !dummy;
            if team_kill {
                kp.stats.kills = kp.stats.kills.saturating_sub(1);
                kp.stats.score -= 1;
                if economy {
                    kp.money = kp.money.saturating_sub(300);
                }
            } else {
                kp.stats.kills += 1;
                kp.stats.score += 2;
                if headshot {
                    kp.stats.headshots += 1;
                }
                if economy {
                    kp.money = add_money(kp.money, reward, &max);
                }
                *self.round_kills.entry(k).or_default() += 1;
            }
        }
        if !dummy
            && mode != Mode::Deathmatch
            && mode != Mode::Training
            && let Some((w, g)) = drop
        {
            let id = self.alloc_entity();
            self.drops.push(Dropped {
                id,
                weapon: w,
                gun: g,
                pos: origin + Vec3::Y * 32.0,
                vel: Vec3::ZERO,
                dropped_at: now,
                dropper: victim,
            });
        }
        if self.bomb == BombState::Carried(victim) {
            self.bomb = BombState::Dropped(origin + Vec3::Y * 8.0);
            if let Some(v) = self.player_mut(victim) {
                v.inv.bomb = false;
            }
            self.events.push(GameEvent::BombDropped { pos: origin });
        }
        if let BombState::Planted { defuser, .. } = &mut self.bomb
            && defuser.is_some_and(|d| d.0 == victim)
        {
            *defuser = None;
        }
        self.events.push(GameEvent::Kill {
            killer,
            victim,
            weapon,
            headshot,
            penetrated,
            assister,
        });
    }

    fn drop_active(&mut self, id: PlayerId) {
        let now = self.now();
        let data = self.data.clone();
        let Some(p) = self.player_mut(id) else { return };
        if !p.alive {
            return;
        }
        let origin = p.mv.origin + Vec3::Y * 48.0;
        let fwd = p.view.forward();
        match p.active {
            Slot::Bomb if p.inv.bomb => {
                p.inv.bomb = false;
                p.active = if p.inv.primary.is_some() {
                    Slot::Primary
                } else {
                    Slot::Secondary
                };
                self.bomb = BombState::Dropped(origin);
                self.events.push(GameEvent::BombDropped { pos: origin });
            }
            Slot::Primary | Slot::Secondary => {
                let taken = if p.active == Slot::Primary {
                    p.inv.primary.take()
                } else {
                    p.inv.secondary.take()
                };
                if let Some((w, g)) = taken {
                    p.active = if p.inv.secondary.is_some() {
                        Slot::Secondary
                    } else {
                        Slot::Melee
                    };
                    if let Some(def) = p.active_def(&data.catalog) {
                        p.hand = HandState::deploy(def, now, None);
                    }
                    let did = self.alloc_entity();
                    self.drops.push(Dropped {
                        id: did,
                        weapon: w,
                        gun: g,
                        pos: origin,
                        vel: fwd * 250.0,
                        dropped_at: now,
                        dropper: id,
                    });
                }
            }
            _ => {}
        }
    }

    fn pickups(&mut self, id: PlayerId) {
        let now = self.now();
        let data = self.data.clone();
        let Some(p) = self.player(id) else { return };
        if !p.alive || p.is_dummy() {
            return;
        }
        let feet = p.mv.origin;
        let team = p.team;
        let near = |pos: Vec3| (pos - (feet + Vec3::Y * 16.0)).length() < 56.0;
        if let BombState::Dropped(pos) = self.bomb
            && team == Team::Attack
            && near(pos)
        {
            self.bomb = BombState::Carried(id);
            self.player_mut(id).unwrap().inv.bomb = true;
            self.events.push(GameEvent::BombPickedUp { player: id });
        }
        let mut taken = None;
        for (i, d) in self.drops.iter().enumerate() {
            if !near(d.pos) || (d.dropper == id && now - d.dropped_at < 1.0) {
                continue;
            }
            let def = data.catalog.get(d.weapon);
            let p = self.player(id).unwrap();
            let free = match def.slot {
                Slot::Primary => p.inv.primary.is_none(),
                Slot::Secondary => p.inv.secondary.is_none(),
                _ => false,
            };
            if free {
                taken = Some(i);
                break;
            }
        }
        if let Some(i) = taken {
            let d = self.drops.remove(i);
            let def = data.catalog.get(d.weapon);
            let p = self.player_mut(id).unwrap();
            if def.slot == Slot::Primary {
                p.inv.primary = Some((d.weapon, d.gun));
            } else {
                p.inv.secondary = Some((d.weapon, d.gun));
            }
        }
    }

    fn update_drops(&mut self, _now: f64) {
        let dt = self.dt() as f32;
        let g = self.data.movement.gravity;
        let world = &self.level.world;
        let h = Vec3::splat(4.0);
        for d in &mut self.drops {
            if d.vel == Vec3::ZERO {
                continue;
            }
            d.vel.y -= g * dt;
            let target = d.pos + d.vel * dt;
            let tr = world.trace_box(
                d.pos,
                target,
                -h,
                h,
                engine_physics::world::TraceMask::Players,
            );
            d.pos = tr.end;
            if tr.fraction < 1.0 {
                if tr.normal.y > 0.7 {
                    d.vel = Vec3::ZERO;
                } else {
                    d.vel -= tr.normal * d.vel.dot(tr.normal);
                }
            }
        }
        let mut b = self.bomb.clone();
        if let BombState::Dropped(pos) = &mut b {
            let tr = world.trace_box(
                *pos,
                *pos - Vec3::Y * 8.0,
                -h,
                h,
                engine_physics::world::TraceMask::Players,
            );
            if tr.fraction >= 1.0 {
                *pos = tr.end;
            }
        }
        self.bomb = b;
        // Keep at most 32 dropped weapons.
        if self.drops.len() > 32 {
            let n = self.drops.len() - 32;
            self.drops.drain(0..n);
        }
    }

    // ------------------------------------------------------------------ grenades

    fn throw_grenade(
        &mut self,
        id: PlayerId,
        time: f64,
        eye: Vec3,
        vel: Vec3,
        view: ViewAngles,
        strength: f32,
    ) {
        let data = self.data.clone();
        let Some(p) = self.player(id) else { return };
        let Some(wid) = p.active_weapon() else { return };
        let Some(kind) = data.catalog.get(wid).grenade else {
            return;
        };
        let team = p.team;
        let (pos, v) = throw_params(
            eye,
            view,
            vel,
            strength,
            &self.cfg.grenades,
            &self.level.world,
        );
        let gid = self.alloc_entity();
        self.grenades.push(Grenade {
            id: gid,
            kind,
            thrower: id,
            team,
            pos,
            vel: v,
            thrown_at: time,
            rest_since: None,
            bounces: 0,
        });
        self.events.push(GameEvent::GrenadeThrown {
            id: gid,
            kind,
            thrower: id,
        });
        let now = self.now();
        let p = self.player_mut(id).unwrap();
        let idx = p.inv.grenades.iter().position(|g| *g == wid).unwrap_or(0);
        if idx < p.inv.grenades.len() {
            p.inv.grenades.remove(idx);
        }
        p.grenade_index = 0;
        if p.inv.grenades.is_empty() {
            p.active = if p.inv.primary.is_some() {
                Slot::Primary
            } else if p.inv.secondary.is_some() {
                Slot::Secondary
            } else {
                Slot::Melee
            };
        }
        if let Some(def) = p.active_def(&data.catalog) {
            p.hand = HandState::deploy(def, now, None);
        }
    }

    fn update_grenades(&mut self, now: f64) {
        let dt = self.dt() as f32;
        let gcfg = self.cfg.grenades.clone();
        let gravity = self.data.movement.gravity;
        let level = self.level.clone();
        let bounds = level.def.bounds().expanded(Vec3::splat(512.0));
        let mut detonations = Vec::new();
        self.grenades.retain_mut(|g| {
            if !bounds.contains(g.pos) {
                return false;
            }
            match g.step(dt, now + dt as f64, &gcfg, gravity, &level.world) {
                GrenadeStep::Flying => true,
                GrenadeStep::Detonate => {
                    detonations.push((g.clone(), true));
                    false
                }
                GrenadeStep::Fizzle => {
                    detonations.push((g.clone(), false));
                    false
                }
            }
        });
        for (g, ok) in detonations {
            self.events.push(GameEvent::GrenadeDetonated {
                id: g.id,
                kind: g.kind,
                pos: g.pos,
            });
            if !ok {
                continue;
            }
            match g.kind {
                GrenadeKind::Smoke => self.detonate_smoke(&g, now),
                GrenadeKind::He => self.detonate_he(&g, now),
                GrenadeKind::Flash => self.detonate_flash(&g, now),
                GrenadeKind::Molotov => {
                    if self
                        .smokes
                        .iter()
                        .any(|s| s.contains(g.pos + Vec3::Y * 8.0, now, &self.cfg.smoke))
                    {
                        continue;
                    }
                    let fire =
                        Fire::spread(g.id, g.thrower, g.team, g.pos, now, &gcfg, &level.world);
                    self.fires.push(fire);
                    self.events.push(GameEvent::FireStarted { id: g.id });
                }
            }
        }
        let smoke_cfg = self.cfg.smoke.clone();
        self.smokes
            .retain(|s| s.end > now - 0.1 && s.active(now) || now < s.start);
        let _ = smoke_cfg;
    }

    fn detonate_smoke(&mut self, g: &Grenade, now: f64) {
        let vol = SmokeVolume::fill(g.id, g.pos, &self.level.world, &self.cfg.smoke, now);
        // A smoke extinguishes the fire it covers.
        let cfg = self.cfg.smoke.clone();
        for f in &mut self.fires {
            f.cells
                .retain(|c| !vol.contains(*c + Vec3::Y * 12.0, now + cfg.expand_time as f64, &cfg));
        }
        self.fires.retain(|f| !f.cells.is_empty());
        self.smokes.push(vol);
        self.events.push(GameEvent::SmokeSpawned { id: g.id });
    }

    fn detonate_he(&mut self, g: &Grenade, now: f64) {
        let cfg = self.cfg.grenades.clone();
        let smoke_cfg = self.cfg.smoke.clone();
        let mut blasted = false;
        for s in &mut self.smokes {
            blasted |= s.blast(g.pos, now, &smoke_cfg);
        }
        if blasted {
            self.events.push(GameEvent::SmokeBlast { pos: g.pos });
        }
        let mvcfg = self.data.movement.clone();
        let mut hits = Vec::new();
        for v in self.players.iter().flatten() {
            if !v.alive
                || (v.team == g.team
                    && v.id != g.thrower
                    && !self.rules.cfg.friendly_fire
                    && self.mode() != Mode::Deathmatch
                    && !v.is_dummy())
            {
                continue;
            }
            let center = v.mv.origin + Vec3::Y * (v.mv.eye_height(&mvcfg) * 0.6);
            let d = center.distance(g.pos);
            if d > cfg.he_radius {
                continue;
            }
            if !self
                .level
                .world
                .line_of_sight(g.pos + Vec3::Y * 4.0, center)
            {
                continue;
            }
            let falloff = 1.0 - (d / cfg.he_radius);
            let raw = cfg.he_damage * falloff.powf(1.3);
            if raw >= 1.0 {
                hits.push((v.id, raw));
            }
        }
        let he = self.data.catalog.grenade(GrenadeKind::He);
        for (vid, raw) in hits {
            self.apply_damage(vid, Some(g.thrower), he, raw, None, 0.5, 0.0, g.pos, false);
        }
    }

    fn detonate_flash(&mut self, g: &Grenade, now: f64) {
        let cfg = self.cfg.grenades.clone();
        let mvcfg = self.data.movement.clone();
        let mut flashed = Vec::new();
        for v in self.players.iter().flatten() {
            if !v.alive || v.is_dummy() {
                continue;
            }
            let eye = v.mv.eye(&mvcfg);
            let d = eye.distance(g.pos);
            if d > cfg.flash_range
                || !self.level.world.line_of_sight(g.pos, eye)
                || smoke_blocks(&self.smokes, g.pos, eye, now, &self.cfg.smoke)
            {
                continue;
            }
            let to = (g.pos - eye).normalize_or_zero();
            let dot = v.view.forward().dot(to);
            // Facing: 1.0 in front, 0.4 at 90°, 0.1 behind.
            let angle = if dot > 0.6 {
                1.0
            } else if dot > 0.0 {
                0.4 + dot
            } else {
                0.1 + 0.3 * (1.0 + dot)
            };
            let dist = (1.0 - (d / cfg.flash_range).powf(1.5)).clamp(0.0, 1.0);
            let k = angle * dist;
            if k < 0.05 {
                continue;
            }
            let duration = cfg.flash_max_duration * k;
            let alpha = (k * 1.4).min(1.0);
            flashed.push((v.id, duration, alpha));
        }
        for (id, duration, alpha) in flashed {
            let p = self.player_mut(id).unwrap();
            // Keep the strongest flash.
            if p.flash_alpha(now) < alpha {
                p.flash = (now, duration, alpha);
            }
            self.events.push(GameEvent::Flashed {
                player: id,
                duration,
                alpha,
            });
        }
    }

    fn update_fires(&mut self, now: f64) {
        let dps = self.cfg.grenades.fire_dps;
        let dt = self.dt() as f32;
        self.fires.retain(|f| f.end > now);
        let mut burns = Vec::new();
        for f in &self.fires {
            for v in self.players.iter().flatten() {
                if v.alive
                    && (v.team != f.team
                        || v.id == f.thrower
                        || self.mode() == Mode::Deathmatch
                        || v.is_dummy())
                    && f.touches(v.mv.origin)
                {
                    burns.push((v.id, f.thrower));
                }
            }
        }
        burns.sort_by_key(|b| b.0);
        burns.dedup_by_key(|b| b.0);
        let molly = self.data.catalog.grenade(GrenadeKind::Molotov);
        for (vid, thrower) in burns {
            // Accumulate fractional damage per tick deterministically: dps * dt.
            let raw = dps * dt;
            let pos = self.player(vid).map(|p| p.mv.origin).unwrap_or_default();
            if self.tick.is_multiple_of((1.0 / dt / 8.0).max(1.0) as u32) {
                // Apply in 8 Hz chunks to avoid zero-damage rounding.
                self.apply_damage(
                    vid,
                    Some(thrower),
                    molly,
                    raw * (1.0 / dt / 8.0).max(1.0),
                    None,
                    1.0,
                    0.0,
                    pos,
                    false,
                );
            }
        }
    }

    // ------------------------------------------------------------------ bomb

    fn bomb_actions(&mut self, id: PlayerId, cmd: &UserCmd) {
        if !self.rules.cfg.bomb.enabled || self.mode() != Mode::Competitive {
            return;
        }
        let now = self.now() + self.dt();
        let bomb_cfg = self.rules.cfg.bomb.clone();
        let live = self.rules.phase == Phase::Live;
        let Some(p) = self.player(id) else { return };
        let buttons = cmd.buttons_end();
        let feet = p.mv.origin;
        // Planting: bomb in hand, attack held, on ground, in a site.
        if p.team == Team::Attack && p.active == Slot::Bomb && self.bomb == BombState::Carried(id) {
            let site = self
                .level
                .def
                .bomb_sites
                .iter()
                .position(|z| z.aabb().contains(feet + Vec3::Y));
            let holding = buttons.has(Buttons::ATTACK) && p.mv.on_ground && live;
            match (holding, site, p.planting_since) {
                (true, Some(_), None) => {
                    self.player_mut(id).unwrap().planting_since = Some(now);
                    self.events.push(GameEvent::PlantStarted { player: id });
                }
                (true, Some(site), Some(since)) if now - since >= bomb_cfg.plant_time as f64 => {
                    let p = self.player_mut(id).unwrap();
                    p.planting_since = None;
                    p.inv.bomb = false;
                    p.active = if p.inv.primary.is_some() {
                        Slot::Primary
                    } else {
                        Slot::Secondary
                    };
                    self.bomb = BombState::Planted {
                        pos: feet,
                        site: site as u8,
                        planted_at: now,
                        explode_at: now + bomb_cfg.timer as f64,
                        defuser: None,
                    };
                    let economy = self.rules.cfg.economy.clone();
                    let p = self.player_mut(id).unwrap();
                    if economy.enabled {
                        p.money = add_money(p.money, economy.plant_reward, &economy);
                    }
                    p.stats.score += 2;
                    self.events.push(GameEvent::BombPlanted {
                        planter: id,
                        pos: feet,
                        site: site as u8,
                    });
                }
                (true, Some(_), Some(_)) => {}
                _ => {
                    if let Some(p) = self.player_mut(id) {
                        p.planting_since = None;
                    }
                }
            }
            return;
        }
        // Defusing: defender holding USE near the planted bomb.
        if p.team == Team::Defense
            && let BombState::Planted { pos, defuser, .. } = self.bomb.clone()
        {
            let close = (feet - pos).length() < bomb_cfg.defuse_range + 16.0;
            let holding = buttons.has(Buttons::USE) && close && p.mv.on_ground;
            let kit = p.defuse_kit;
            let mine = defuser.is_some_and(|d| d.0 == id);
            if holding && defuser.is_none() {
                if let BombState::Planted { defuser, .. } = &mut self.bomb {
                    *defuser = Some((id, now));
                }
                self.player_mut(id).unwrap().defusing_since = Some(now);
                self.events
                    .push(GameEvent::DefuseStarted { player: id, kit });
            } else if holding && mine {
                let since = defuser.unwrap().1;
                let need = if kit {
                    bomb_cfg.defuse_time_kit
                } else {
                    bomb_cfg.defuse_time
                } as f64;
                if now - since >= need {
                    self.bomb = BombState::Defused(pos);
                    let economy = self.rules.cfg.economy.clone();
                    let p = self.player_mut(id).unwrap();
                    p.defusing_since = None;
                    if economy.enabled {
                        p.money = add_money(p.money, economy.defuse_reward, &economy);
                    }
                    p.stats.score += 2;
                    self.events.push(GameEvent::BombDefused { defuser: id });
                }
            } else if mine {
                if let BombState::Planted { defuser, .. } = &mut self.bomb {
                    *defuser = None;
                }
                self.player_mut(id).unwrap().defusing_since = None;
            }
        }
    }

    fn update_bomb(&mut self, now: f64) {
        if let BombState::Planted {
            pos, explode_at, ..
        } = self.bomb
            && now >= explode_at
        {
            self.bomb = BombState::Exploded(pos);
            self.events.push(GameEvent::BombExploded { pos });
            let mut hits = Vec::new();
            for v in self.players.iter().flatten() {
                if !v.alive {
                    continue;
                }
                let d = v.mv.origin.distance(pos);
                let sigma = 1750.0 / 3.0;
                let raw = 500.0 * (-(d * d) / (2.0 * sigma * sigma)).exp();
                if raw > 1.0 {
                    hits.push((v.id, raw));
                }
            }
            for (id, raw) in hits {
                self.apply_damage(id, None, None, raw, None, 0.5, 0.0, pos, false);
            }
        }
    }

    // ------------------------------------------------------------------ rules

    fn alive_count(&self, t: Team) -> usize {
        self.iter_players()
            .filter(|p| p.team == t && p.alive && !p.is_dummy())
            .count()
    }

    fn update_rules(&mut self) {
        let now = self.now();
        match self.mode() {
            Mode::Training => return,
            Mode::Deathmatch => {
                let frag = self.rules.cfg.frag_limit;
                let top = self
                    .iter_players()
                    .map(|p| p.stats.kills)
                    .max()
                    .unwrap_or(0);
                if self.rules.phase == Phase::Live
                    && (now >= self.deathmatch_end || (frag > 0 && top >= frag))
                {
                    self.rules.phase = Phase::GameOver;
                    self.events.push(GameEvent::RoundPhase {
                        phase: Phase::GameOver,
                        round: 0,
                    });
                    self.events.push(GameEvent::MatchEnded {
                        winner_team_score: [0, 0],
                    });
                }
                return;
            }
            _ => {}
        }
        // Warmup waits for at least one player on each side.
        if self.rules.phase == Phase::Warmup
            && (self.team_count(Team::Attack) == 0 || self.team_count(Team::Defense) == 0)
        {
            self.rules.phase_end = self
                .rules
                .phase_end
                .max(now + 1.0)
                .min(now + self.rules.cfg.warmup_time as f64);
            if self.team_count(Team::Attack) == 0 || self.team_count(Team::Defense) == 0 {
                self.rules.phase_end = now + self.rules.cfg.warmup_time as f64;
            }
        }
        let a = self.alive_count(Team::Attack);
        let d = self.alive_count(Team::Defense);
        let status = self.bomb.status();
        let events = self.rules.update(now, a, d, status);
        self.apply_match_events(events, now);
    }

    /// Admin: end the warmup now.
    pub fn force_start(&mut self) {
        let now = self.now();
        let ev = self.rules.force_start(now);
        self.apply_match_events(ev, now);
    }

    fn apply_match_events(&mut self, events: Vec<MatchEvent>, now: f64) {
        let economy = self.rules.cfg.economy.clone();
        for e in events {
            match e {
                MatchEvent::WarmupEnded => {
                    for p in self.players.iter_mut().flatten() {
                        p.stats = Default::default();
                    }
                    self.strip_on_next_round = true;
                }
                MatchEvent::MoneyReset { money } => {
                    for p in self.players.iter_mut().flatten() {
                        p.money = if economy.enabled { money } else { 0 };
                    }
                    self.strip_on_next_round = true;
                }
                MatchEvent::SidesSwapped => {
                    for p in self.players.iter_mut().flatten() {
                        if p.team.is_playing() && !p.is_dummy() {
                            p.team = p.team.opponent();
                        }
                    }
                    self.events.push(GameEvent::Notice {
                        text: "Changement de côté".into(),
                    });
                }
                MatchEvent::RoundPrepared { round } => {
                    self.prepare_round();
                    self.events.push(GameEvent::RoundPhase {
                        phase: Phase::Freeze,
                        round,
                    });
                }
                MatchEvent::RoundLive { round } => {
                    self.events.push(GameEvent::RoundPhase {
                        phase: Phase::Live,
                        round,
                    });
                }
                MatchEvent::RoundEnded {
                    round,
                    winner,
                    reason,
                    win_money,
                    lose_money,
                    survivors_unpaid,
                } => {
                    for p in self.players.iter_mut().flatten() {
                        if !p.team.is_playing() || p.is_dummy() {
                            continue;
                        }
                        if p.team == winner {
                            p.money = add_money(p.money, win_money, &economy);
                        } else if !(survivors_unpaid && p.alive && p.team == Team::Attack) {
                            p.money = add_money(p.money, lose_money, &economy);
                        }
                    }
                    let mvp = self
                        .round_kills
                        .iter()
                        .filter(|(id, _)| self.player(**id).is_some_and(|p| p.team == winner))
                        .max_by_key(|(id, k)| (**k, std::cmp::Reverse(id.0)))
                        .map(|(id, _)| *id);
                    if let Some(m) = mvp
                        && let Some(p) = self.player_mut(m)
                    {
                        p.stats.mvps += 1;
                    }
                    self.events.push(GameEvent::RoundEnded {
                        winner,
                        reason,
                        mvp,
                    });
                    self.events.push(GameEvent::RoundPhase {
                        phase: Phase::RoundOver,
                        round,
                    });
                }
                MatchEvent::MatchEnded { .. } => {
                    self.events.push(GameEvent::RoundPhase {
                        phase: Phase::GameOver,
                        round: self.rules.round,
                    });
                    self.events.push(GameEvent::MatchEnded {
                        winner_team_score: self.rules.scores,
                    });
                }
            }
        }
        let _ = now;
    }

    fn prepare_round(&mut self) {
        self.grenades.clear();
        self.smokes.clear();
        self.fires.clear();
        self.drops.clear();
        self.round_kills.clear();
        self.bomb = BombState::None;
        let strip = std::mem::replace(&mut self.strip_on_next_round, false);
        let ids: Vec<PlayerId> = self
            .iter_players()
            .filter(|p| p.team.is_playing() && !p.is_dummy())
            .map(|p| p.id)
            .collect();
        for &id in &ids {
            if let Some(p) = self.player_mut(id) {
                if strip {
                    p.alive = false;
                }
                p.inv.bomb = false;
            }
            self.spawn(id);
        }
        // Bomb to a random attacker.
        if self.rules.cfg.bomb.enabled && self.mode() == Mode::Competitive {
            let attackers: Vec<PlayerId> = ids
                .iter()
                .copied()
                .filter(|id| self.player(*id).is_some_and(|p| p.team == Team::Attack))
                .collect();
            if !attackers.is_empty() {
                let k = self.rng.below(attackers.len() as u32) as usize;
                let id = attackers[k];
                self.player_mut(id).unwrap().inv.bomb = true;
                self.bomb = BombState::Carried(id);
            }
        }
        // Bots buy.
        let bots: Vec<PlayerId> = self.bots.keys().copied().collect();
        for id in bots {
            crate::bots::bot_buy(self, id);
        }
    }

    // ------------------------------------------------------------------ dummies & bots

    fn dummy_cmds(&mut self, cmds: &mut [Option<UserCmd>]) {
        let now = self.now();
        for p in self.players.iter_mut().flatten() {
            if !p.is_dummy() || !p.alive {
                continue;
            }
            // Health refills 3 s after the last hit (training comfort).
            if now - p.last_hurt > 3.0 && p.health < 100 {
                p.health = 100;
                p.armor = p.dummy_armor.0;
                p.helmet = p.dummy_armor.1;
            }
            let mut b = Buttons(0);
            let phase = now + p.id.0 as f64 * 0.37;
            match p.dummy_kind {
                1 => {
                    // Strafe 0.8 s each way with a counter-strafe stop in between.
                    let t = phase % 2.0;
                    let off = p.mv.origin - p.anchor;
                    let right = p.view.right_flat();
                    let side = off.dot(right);
                    if t < 0.8 {
                        b = b.with(
                            if side > 80.0 {
                                Buttons::LEFT
                            } else {
                                Buttons::RIGHT
                            },
                            true,
                        );
                    } else if (1.0..1.8).contains(&t) {
                        b = b.with(
                            if side < -80.0 {
                                Buttons::RIGHT
                            } else {
                                Buttons::LEFT
                            },
                            true,
                        );
                    }
                }
                2 if (phase % 2.0) < 1.0 => {
                    b = b.with(Buttons::DUCK, true);
                }
                _ => {}
            }
            let v = p.view;
            cmds[p.id.0 as usize] = Some(UserCmd {
                tick: self.tick,
                buttons_start: b,
                view_start: v,
                view_end: v,
                view_time: engine_core::SimTime::new(self.tick, 0.0),
                ..Default::default()
            });
        }
    }

    fn bot_cmds(&mut self, cmds: &mut [Option<UserCmd>]) {
        let ids: Vec<PlayerId> = self.bots.keys().copied().collect();
        for id in ids {
            let Some(mut brain) = self.bots.remove(&id) else {
                continue;
            };
            if let Some(cmd) = brain.think(self) {
                cmds[id.0 as usize] = Some(cmd);
            }
            self.bots.insert(id, brain);
        }
    }

    /// Line of sight for players and bots: world and smoke.
    pub fn visible(&self, a: Vec3, b: Vec3) -> bool {
        self.level.world.line_of_sight(a, b)
            && !smoke_blocks(&self.smokes, a, b, self.now(), &self.cfg.smoke)
    }

    /// Drains the events and shot reports produced since the last call.
    pub fn drain(&mut self) -> (Vec<GameEvent>, Vec<ShotReport>) {
        (
            std::mem::take(&mut self.events),
            std::mem::take(&mut self.shots),
        )
    }

    pub fn bomb_time_left(&self) -> Option<f64> {
        match self.bomb {
            BombState::Planted { explode_at, .. } => Some((explode_at - self.now()).max(0.0)),
            _ => None,
        }
    }
}

fn sanitize_name(n: &str) -> String {
    let s: String = n.chars().filter(|c| !c.is_control()).take(24).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "Joueur".into() } else { s }
}
