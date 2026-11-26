//! Match rules: formats (MR12/MR15/MR16, configurable), round phases, side swaps,
//! overtime and the competitive economy. Pure state machine: it knows nothing about
//! the world, the sim feeds it alive counts and the bomb status.

use engine_core::Team;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Free practice: infinite money, instant respawn, no rounds.
    #[default]
    Training,
    /// Free-for-all, respawn, time/frag limit.
    Deathmatch,
    /// 1v1 rounds, free loadout, no bomb.
    Duel,
    /// 5v5 bomb defusal with economy.
    Competitive,
}

impl Mode {
    pub fn has_rounds(self) -> bool {
        matches!(self, Mode::Duel | Mode::Competitive)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Training => "Entraînement",
            Mode::Deathmatch => "Deathmatch",
            Mode::Duel => "Duel 1v1",
            Mode::Competitive => "Compétitif 5v5",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EconomyConfig {
    pub enabled: bool,
    pub start_money: u32,
    pub max_money: u32,
    pub win_elimination: u32,
    pub win_bomb: u32,
    pub win_defuse: u32,
    pub win_time: u32,
    /// Loss bonus by consecutive-loss count (1, 2, 3, ...); the last value repeats.
    pub loss_bonus: Vec<u32>,
    /// Paid to every attacker when they lose a round after planting.
    pub plant_team_bonus: u32,
    pub plant_reward: u32,
    pub defuse_reward: u32,
    /// Attackers still alive when the round time runs out get no loss bonus.
    pub no_loss_bonus_for_time_survivors: bool,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            start_money: 800,
            max_money: 16000,
            win_elimination: 3250,
            win_bomb: 3500,
            win_defuse: 3500,
            win_time: 3250,
            loss_bonus: vec![1400, 1900, 2400, 2900, 3400],
            plant_team_bonus: 800,
            plant_reward: 300,
            defuse_reward: 300,
            no_loss_bonus_for_time_survivors: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BombConfig {
    pub enabled: bool,
    pub timer: f32,
    pub plant_time: f32,
    pub defuse_time: f32,
    pub defuse_time_kit: f32,
    pub defuse_range: f32,
}

impl Default for BombConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timer: 40.0,
            plant_time: 3.2,
            defuse_time: 10.0,
            defuse_time_kit: 5.0,
            defuse_range: 72.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OvertimeConfig {
    pub enabled: bool,
    /// Rounds per overtime (MR3 = 6).
    pub rounds: u16,
    pub start_money: u32,
}

impl Default for OvertimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rounds: 6,
            start_money: 10000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub mode: Mode,
    /// Regulation rounds (MR15 = 30). The first to `max_rounds / 2 + 1` wins.
    pub max_rounds: u16,
    pub team_size: u8,
    pub warmup_time: f32,
    pub freeze_time: f32,
    pub buy_time: f32,
    pub round_time: f32,
    pub round_end_delay: f32,
    pub halftime_delay: f32,
    pub overtime: OvertimeConfig,
    pub economy: EconomyConfig,
    pub bomb: BombConfig,
    pub friendly_fire: bool,
    /// Deathmatch / training respawn delay.
    pub respawn_time: f32,
    /// Deathmatch time limit in seconds (0 = none).
    pub time_limit: f32,
    pub frag_limit: u32,
    /// Free kevlar+helmet at each spawn (duel, deathmatch).
    pub free_armor: bool,
    /// Loadout given at spawn when the economy is disabled (weapon ids).
    pub free_loadout: Vec<String>,
    /// Allow buying anywhere at any time (training).
    pub buy_anywhere: bool,
    /// Seconds a disconnected player's slot stays reserved for reconnection.
    pub reconnect_grace: f32,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Competitive,
            max_rounds: 30,
            team_size: 5,
            warmup_time: 60.0,
            freeze_time: 15.0,
            buy_time: 20.0,
            round_time: 115.0,
            round_end_delay: 5.0,
            halftime_delay: 15.0,
            overtime: OvertimeConfig::default(),
            economy: EconomyConfig::default(),
            bomb: BombConfig::default(),
            friendly_fire: false,
            respawn_time: 2.0,
            time_limit: 0.0,
            frag_limit: 0,
            free_armor: false,
            free_loadout: Vec::new(),
            buy_anywhere: false,
            reconnect_grace: 180.0,
        }
    }
}

impl RulesConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.mode.has_rounds() {
            if self.max_rounds < 2 || !self.max_rounds.is_multiple_of(2) {
                return Err("max_rounds must be even and >= 2".into());
            }
            if self.overtime.enabled
                && (self.overtime.rounds < 2 || !self.overtime.rounds.is_multiple_of(2))
            {
                return Err("overtime.rounds must be even and >= 2".into());
            }
        }
        if self.round_time <= 0.0 || self.freeze_time < 0.0 {
            return Err("round_time must be > 0".into());
        }
        if self.economy.loss_bonus.is_empty() {
            return Err("economy.loss_bonus needs at least one value".into());
        }
        if self.team_size == 0 || self.team_size > 32 {
            return Err("team_size in 1..=32".into());
        }
        Ok(())
    }

    pub fn rounds_to_win(&self) -> u16 {
        self.max_rounds / 2 + 1
    }
}

/// `configs/rules.toml`: named profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RulesFile {
    #[serde(flatten)]
    pub profiles: BTreeMap<String, RulesConfig>,
}

impl RulesFile {
    pub fn from_toml(text: &str) -> Result<RulesFile, String> {
        let f: RulesFile = toml::from_str(text).map_err(|e| e.to_string())?;
        for (name, p) in &f.profiles {
            p.validate().map_err(|e| format!("profile {name}: {e}"))?;
        }
        Ok(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Warmup,
    Freeze,
    Live,
    RoundOver,
    HalfTime,
    GameOver,
}

impl Phase {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Phase {
        match v {
            0 => Phase::Warmup,
            1 => Phase::Freeze,
            2 => Phase::Live,
            3 => Phase::RoundOver,
            4 => Phase::HalfTime,
            _ => Phase::GameOver,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Warmup => "Échauffement",
            Phase::Freeze => "Phase d'achat",
            Phase::Live => "Manche en cours",
            Phase::RoundOver => "Fin de manche",
            Phase::HalfTime => "Mi-temps",
            Phase::GameOver => "Fin du match",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundEndReason {
    AttackersEliminated,
    DefendersEliminated,
    BombExploded,
    BombDefused,
    TimeExpired,
}

impl RoundEndReason {
    pub fn winner(self) -> Team {
        match self {
            RoundEndReason::DefendersEliminated | RoundEndReason::BombExploded => Team::Attack,
            _ => Team::Defense,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => RoundEndReason::AttackersEliminated,
            1 => RoundEndReason::DefendersEliminated,
            2 => RoundEndReason::BombExploded,
            3 => RoundEndReason::BombDefused,
            4 => RoundEndReason::TimeExpired,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            RoundEndReason::AttackersEliminated => "Attaquants éliminés",
            RoundEndReason::DefendersEliminated => "Défenseurs éliminés",
            RoundEndReason::BombExploded => "La charge a explosé",
            RoundEndReason::BombDefused => "Charge désamorcée",
            RoundEndReason::TimeExpired => "Temps écoulé",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BombStatus {
    NotPlanted,
    Planted,
    Exploded,
    Defused,
}

/// One of the two rosters. Rosters keep their score while sides swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Squad(pub u8);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MatchEvent {
    WarmupEnded,
    /// Money of every player must be set to this value (match start, half, overtime).
    MoneyReset {
        money: u32,
    },
    SidesSwapped,
    /// Players must be respawned at their team spawns, frozen.
    RoundPrepared {
        round: u16,
    },
    RoundLive {
        round: u16,
    },
    RoundEnded {
        round: u16,
        winner: Team,
        reason: RoundEndReason,
        /// Paid to every player of the winning side.
        win_money: u32,
        /// Paid to every player of the losing side (loss bonus, plant bonus included).
        lose_money: u32,
        /// Losing attackers alive at time expiry receive nothing.
        survivors_unpaid: bool,
    },
    MatchEnded {
        winner: Option<Squad>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    pub cfg: RulesConfig,
    pub phase: Phase,
    pub phase_end: f64,
    /// 1-based round number (0 before the first round).
    pub round: u16,
    pub scores: [u16; 2],
    /// Side currently played by each squad.
    pub sides: [Team; 2],
    /// Consecutive losses per squad (drives the loss bonus).
    pub loss_streak: [u8; 2],
    pub round_started_at: f64,
    pub last_reason: Option<RoundEndReason>,
    pub last_winner: Option<Team>,
    winner: Option<Option<Squad>>,
}

impl Match {
    pub fn new(cfg: RulesConfig, now: f64) -> Match {
        let phase_end = now + cfg.warmup_time as f64;
        Match {
            cfg,
            phase: Phase::Warmup,
            phase_end,
            round: 0,
            scores: [0, 0],
            sides: [Team::Attack, Team::Defense],
            loss_streak: [0, 0],
            round_started_at: now,
            last_reason: None,
            last_winner: None,
            winner: None,
        }
    }

    pub fn squad_of(&self, side: Team) -> Option<Squad> {
        self.sides
            .iter()
            .position(|&s| s == side)
            .map(|i| Squad(i as u8))
    }

    pub fn side_of(&self, squad: Squad) -> Team {
        self.sides[squad.0 as usize]
    }

    pub fn score_of_side(&self, side: Team) -> u16 {
        self.squad_of(side)
            .map(|s| self.scores[s.0 as usize])
            .unwrap_or(0)
    }

    pub fn winner(&self) -> Option<Option<Squad>> {
        self.winner
    }

    pub fn total_rounds_played(&self) -> u16 {
        self.scores[0] + self.scores[1]
    }

    pub fn is_overtime(&self) -> bool {
        self.round > self.cfg.max_rounds
    }

    pub fn in_buy_time(&self, now: f64) -> bool {
        match self.phase {
            Phase::Warmup => true,
            Phase::Freeze => true,
            Phase::Live => now < self.round_started_at + self.cfg.buy_time as f64,
            _ => false,
        }
    }

    pub fn players_frozen(&self) -> bool {
        matches!(
            self.phase,
            Phase::Freeze | Phase::HalfTime | Phase::GameOver
        )
    }

    /// Time left in the current phase.
    pub fn time_left(&self, now: f64) -> f64 {
        (self.phase_end - now).max(0.0)
    }

    /// Skips the warmup (admin command or enough players ready).
    pub fn force_start(&mut self, now: f64) -> Vec<MatchEvent> {
        if self.phase != Phase::Warmup {
            return Vec::new();
        }
        let mut ev = vec![
            MatchEvent::WarmupEnded,
            MatchEvent::MoneyReset {
                money: self.cfg.economy.start_money,
            },
        ];
        self.prepare_round(now, &mut ev);
        ev
    }

    fn prepare_round(&mut self, now: f64, ev: &mut Vec<MatchEvent>) {
        self.round += 1;
        self.phase = Phase::Freeze;
        self.phase_end = now + self.cfg.freeze_time as f64;
        ev.push(MatchEvent::RoundPrepared { round: self.round });
        if self.cfg.freeze_time <= 0.0 {
            self.go_live(now, ev);
        }
    }

    fn go_live(&mut self, now: f64, ev: &mut Vec<MatchEvent>) {
        self.phase = Phase::Live;
        self.round_started_at = now;
        self.phase_end = now + self.cfg.round_time as f64;
        ev.push(MatchEvent::RoundLive { round: self.round });
    }

    /// Ends the live round. Ignored outside `Live`.
    pub fn end_round(
        &mut self,
        now: f64,
        reason: RoundEndReason,
        planted: bool,
    ) -> Vec<MatchEvent> {
        let mut ev = Vec::new();
        if self.phase != Phase::Live {
            return ev;
        }
        let winner_side = reason.winner();
        let w = self
            .squad_of(winner_side)
            .map(|s| s.0 as usize)
            .unwrap_or(0);
        let l = 1 - w;
        self.scores[w] += 1;
        let e = &self.cfg.economy;
        let win_money = match reason {
            RoundEndReason::BombExploded => e.win_bomb,
            RoundEndReason::BombDefused => e.win_defuse,
            RoundEndReason::TimeExpired => e.win_time,
            _ => e.win_elimination,
        };
        // CS:GO rule: the loss counter grows on a loss and shrinks on a win.
        self.loss_streak[l] = self.loss_streak[l].saturating_add(1);
        self.loss_streak[w] = self.loss_streak[w].saturating_sub(1);
        let n = self.loss_streak[l] as usize;
        let mut lose_money = e.loss_bonus[(n.max(1) - 1).min(e.loss_bonus.len() - 1)];
        if planted && self.sides[l] == Team::Attack {
            lose_money += e.plant_team_bonus;
        }
        let survivors_unpaid =
            reason == RoundEndReason::TimeExpired && e.no_loss_bonus_for_time_survivors;
        let (win_money, lose_money) = if e.enabled {
            (win_money, lose_money)
        } else {
            (0, 0)
        };
        ev.push(MatchEvent::RoundEnded {
            round: self.round,
            winner: winner_side,
            reason,
            win_money,
            lose_money,
            survivors_unpaid,
        });
        self.last_reason = Some(reason);
        self.last_winner = Some(winner_side);
        self.phase = Phase::RoundOver;
        self.phase_end = now + self.cfg.round_end_delay as f64;
        ev
    }

    /// Decides what follows a finished round: match end, half time or next round.
    fn after_round(&mut self, now: f64, ev: &mut Vec<MatchEvent>) {
        let reg = self.cfg.max_rounds;
        let half = reg / 2;
        let played = self.total_rounds_played();
        let [a, b] = self.scores;
        let finished = if played <= reg {
            let to_win = self.cfg.rounds_to_win();
            if a >= to_win || b >= to_win {
                Some(Some(Squad(if a > b { 0 } else { 1 })))
            } else if played == reg && !self.cfg.overtime.enabled {
                Some(if a == b {
                    None
                } else {
                    Some(Squad(if a > b { 0 } else { 1 }))
                })
            } else {
                None
            }
        } else {
            // Overtime blocks of `ot` rounds: the first to ot/2+1 in the block wins.
            let ot = self.cfg.overtime.rounds;
            let in_block = (played - reg - 1) % ot + 1;
            let base = reg / 2 + ((played - reg - 1) / ot) * (ot / 2);
            let need = base + ot / 2 + 1;
            if a >= need || b >= need {
                Some(Some(Squad(if a > b { 0 } else { 1 })))
            } else {
                let _ = in_block;
                None
            }
        };
        if let Some(w) = finished {
            self.winner = Some(w);
            self.phase = Phase::GameOver;
            self.phase_end = f64::INFINITY;
            ev.push(MatchEvent::MatchEnded { winner: w });
            return;
        }
        let swap_regulation = played == half;
        let (swap_ot, ot_start) = if played >= reg && self.cfg.overtime.enabled {
            let ot = self.cfg.overtime.rounds;
            let k = played - reg;
            (k % ot == ot / 2, k.is_multiple_of(ot))
        } else {
            (false, false)
        };
        if swap_regulation || swap_ot {
            self.sides.swap(0, 1);
            self.loss_streak = [0, 0];
            ev.push(MatchEvent::SidesSwapped);
            if swap_regulation {
                ev.push(MatchEvent::MoneyReset {
                    money: self.cfg.economy.start_money,
                });
            } else {
                ev.push(MatchEvent::MoneyReset {
                    money: self.cfg.overtime.start_money,
                });
            }
            self.phase = Phase::HalfTime;
            self.phase_end = now + self.cfg.halftime_delay as f64;
            return;
        }
        if ot_start {
            self.loss_streak = [0, 0];
            ev.push(MatchEvent::MoneyReset {
                money: self.cfg.overtime.start_money,
            });
        }
        self.prepare_round(now, ev);
    }

    /// Advances timers. `alive` = alive players per side, `bomb` = bomb status.
    pub fn update(
        &mut self,
        now: f64,
        alive_attack: usize,
        alive_defense: usize,
        bomb: BombStatus,
    ) -> Vec<MatchEvent> {
        let mut ev = Vec::new();
        match self.phase {
            Phase::Warmup => {
                if now >= self.phase_end {
                    ev = self.force_start(now);
                }
            }
            Phase::Freeze => {
                if now >= self.phase_end {
                    self.go_live(now, &mut ev);
                }
            }
            Phase::Live => {
                let reason = match bomb {
                    BombStatus::Exploded => Some(RoundEndReason::BombExploded),
                    BombStatus::Defused => Some(RoundEndReason::BombDefused),
                    BombStatus::Planted if alive_defense == 0 => {
                        Some(RoundEndReason::DefendersEliminated)
                    }
                    BombStatus::Planted => None,
                    BombStatus::NotPlanted => {
                        if alive_attack == 0 {
                            Some(RoundEndReason::AttackersEliminated)
                        } else if alive_defense == 0 {
                            Some(RoundEndReason::DefendersEliminated)
                        } else if now >= self.phase_end {
                            Some(RoundEndReason::TimeExpired)
                        } else {
                            None
                        }
                    }
                };
                if let Some(r) = reason {
                    ev = self.end_round(
                        now,
                        r,
                        matches!(
                            bomb,
                            BombStatus::Planted | BombStatus::Exploded | BombStatus::Defused
                        ),
                    );
                }
            }
            Phase::RoundOver => {
                if now >= self.phase_end {
                    self.after_round(now, &mut ev);
                }
            }
            Phase::HalfTime => {
                if now >= self.phase_end {
                    self.prepare_round(now, &mut ev);
                }
            }
            Phase::GameOver => {}
        }
        ev
    }
}

/// Money after a reward, capped.
pub fn add_money(current: u32, amount: u32, cfg: &EconomyConfig) -> u32 {
    (current + amount).min(cfg.max_money)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(max_rounds: u16) -> RulesConfig {
        RulesConfig {
            max_rounds,
            freeze_time: 1.0,
            round_end_delay: 1.0,
            halftime_delay: 1.0,
            warmup_time: 0.0,
            ..Default::default()
        }
    }

    /// Plays rounds where `attack_wins(round)` decides; returns events.
    fn play(
        m: &mut Match,
        mut attack_wins: impl FnMut(u16) -> bool,
        max_steps: usize,
    ) -> Vec<MatchEvent> {
        let mut all = Vec::new();
        let mut t = 0.0;
        all.extend(m.force_start(t));
        for _ in 0..max_steps {
            t += 0.5;
            let (a, d) = if m.phase == Phase::Live {
                if attack_wins(m.round) { (5, 0) } else { (0, 5) }
            } else {
                (5, 5)
            };
            all.extend(m.update(t, a, d, BombStatus::NotPlanted));
            if m.phase == Phase::GameOver {
                break;
            }
        }
        all
    }

    #[test]
    fn mr15_side_swap_after_15_and_overtime_on_tie() {
        let mut m = Match::new(cfg(30), 0.0);
        // The attacking side always wins: 15-0 for squad 0, swap, then 15-15.
        let ev = play(&mut m, |_| true, 30 * 6 + 10);
        let swaps = ev
            .iter()
            .filter(|e| matches!(e, MatchEvent::SidesSwapped))
            .count();
        assert!(swaps >= 1);
        let first_swap = ev
            .iter()
            .position(|e| matches!(e, MatchEvent::SidesSwapped))
            .unwrap();
        let rounds_before = ev[..first_swap]
            .iter()
            .filter(|e| matches!(e, MatchEvent::RoundEnded { .. }))
            .count();
        assert_eq!(rounds_before, 15);
        assert!(m.total_rounds_played() >= 30);
        assert_ne!(m.phase, Phase::GameOver, "15-15 goes to overtime");
        assert!(m.is_overtime() || m.total_rounds_played() == 30);
    }

    #[test]
    fn tie_without_overtime_is_a_draw() {
        let mut c = cfg(30);
        c.overtime.enabled = false;
        let mut m = Match::new(c, 0.0);
        play(&mut m, |_| true, 10_000);
        assert_eq!(m.phase, Phase::GameOver);
        assert_eq!(m.scores, [15, 15]);
        assert_eq!(m.winner(), Some(None));
    }

    #[test]
    fn mr12_and_mr16_formats() {
        for (mr, to_win) in [(24u16, 13u16), (30, 16), (32, 17)] {
            let c = cfg(mr);
            assert_eq!(c.rounds_to_win(), to_win);
            let mut m = Match::new(c, 0.0);
            let mut t = 0.0;
            m.force_start(t);
            // Squad 0 wins every round.
            while m.phase != Phase::GameOver {
                t += 0.5;
                let s0_attack = m.sides[0] == Team::Attack;
                let (a, d) = if m.phase == Phase::Live {
                    if s0_attack { (5, 0) } else { (0, 5) }
                } else {
                    (5, 5)
                };
                m.update(t, a, d, BombStatus::NotPlanted);
            }
            assert_eq!(m.scores, [to_win, 0]);
            assert_eq!(m.winner(), Some(Some(Squad(0))));
        }
    }

    #[test]
    fn overtime_after_15_15() {
        let mut m = Match::new(cfg(30), 0.0);
        let mut t = 0.0;
        m.force_start(t);
        let mut ot_money = false;
        // Alternate winners by round number so regulation ends 15-15, then squad 1 wins.
        while m.phase != Phase::GameOver && t < 100_000.0 {
            t += 0.5;
            let squad0_wins = m.round <= 30 && m.round % 2 == 1;
            let s0_attack = m.sides[0] == Team::Attack;
            let attack_wins = squad0_wins == s0_attack;
            let (a, d) = if m.phase == Phase::Live {
                if attack_wins { (5, 0) } else { (0, 5) }
            } else {
                (5, 5)
            };
            for e in m.update(t, a, d, BombStatus::NotPlanted) {
                if e == (MatchEvent::MoneyReset { money: 10000 }) {
                    ot_money = true;
                }
            }
        }
        assert!(ot_money);
        assert_eq!(m.scores, [15, 19]);
    }

    #[test]
    fn economy_loss_bonus_and_plant() {
        let mut m = Match::new(cfg(30), 0.0);
        m.force_start(0.0);
        let mut t = 1.0;
        m.update(t, 5, 5, BombStatus::NotPlanted);
        assert_eq!(m.phase, Phase::Live);
        let mut lose = Vec::new();
        for _ in 0..6 {
            // Defense always wins by defuse, attack planted: attack gets loss bonus + 800.
            t += 0.5;
            for e in m.update(t, 5, 5, BombStatus::Defused) {
                if let MatchEvent::RoundEnded {
                    lose_money,
                    win_money,
                    ..
                } = e
                {
                    assert_eq!(win_money, 3500);
                    lose.push(lose_money);
                }
            }
            while m.phase != Phase::Live {
                t += 0.5;
                m.update(t, 5, 5, BombStatus::NotPlanted);
            }
        }
        assert_eq!(lose, vec![2200, 2700, 3200, 3700, 4200, 4200]);
    }

    #[test]
    fn bomb_rules() {
        let mut m = Match::new(cfg(30), 0.0);
        m.force_start(0.0);
        m.update(1.0, 5, 5, BombStatus::NotPlanted);
        // Planted, attackers all dead: round continues.
        assert!(m.update(2.0, 0, 3, BombStatus::Planted).is_empty());
        // Time is not an issue after the plant.
        assert!(m.update(1000.0, 0, 3, BombStatus::Planted).is_empty());
        let ev = m.update(1001.0, 0, 3, BombStatus::Exploded);
        assert!(matches!(
            ev[0],
            MatchEvent::RoundEnded {
                winner: Team::Attack,
                reason: RoundEndReason::BombExploded,
                ..
            }
        ));
    }

    #[test]
    fn time_expiry_defense_wins() {
        let mut m = Match::new(cfg(30), 0.0);
        m.force_start(0.0);
        m.update(1.0, 5, 5, BombStatus::NotPlanted);
        let ev = m.update(1.0 + 115.0, 5, 5, BombStatus::NotPlanted);
        assert!(matches!(
            ev[0],
            MatchEvent::RoundEnded {
                winner: Team::Defense,
                reason: RoundEndReason::TimeExpired,
                survivors_unpaid: true,
                ..
            }
        ));
    }

    #[test]
    fn shipped_rules_file_is_valid() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../configs/rules.toml");
        let f = RulesFile::from_toml(&std::fs::read_to_string(path).unwrap()).unwrap();
        for p in [
            "competitive",
            "competitive_mr12",
            "competitive_mr16",
            "duel",
            "deathmatch",
            "training",
        ] {
            assert!(f.profiles.contains_key(p), "{p}");
        }
        assert_eq!(f.profiles["competitive"].max_rounds, 30);
    }
}
