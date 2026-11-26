//! Deterministic replays: every input that changes the simulation goes through
//! [`apply`], and the recorder stores those inputs per tick. Replaying them on a
//! fresh `Sim` with the same data reproduces the match exactly (bots included,
//! since they are part of the simulation).

use crate::data::GameData;
use crate::level::Level;
use crate::player::Controller;
use crate::sim::{Sim, SimConfig};
use engine_core::cmd::UserCmd;
use engine_core::{MAX_PLAYERS, PlayerId, Team};
use engine_replay::{ReplayError, ReplayReader, ReplayWriter};
use game_rules::RulesConfig;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimInput {
    Join {
        slot: u8,
        name: String,
        bot: bool,
        team: Option<Team>,
    },
    Leave {
        slot: u8,
    },
    Team {
        slot: u8,
        team: Team,
    },
    Buy {
        slot: u8,
        item: String,
    },
    ForceStart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub game_version: String,
    pub map: String,
    pub profile: String,
    pub rules: RulesConfig,
    pub sim: SimConfig,
    pub seed: u64,
    pub data_fingerprint: u64,
    pub started_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayFrame {
    pub tick: u32,
    pub inputs: Vec<SimInput>,
    pub cmds: Vec<(u8, UserCmd)>,
}

/// Applies an input; returns the slot for joins.
pub fn apply(sim: &mut Sim, input: &SimInput) -> Result<Option<PlayerId>, String> {
    match input {
        SimInput::Join {
            slot,
            name,
            bot,
            team,
        } => {
            let c = if *bot {
                Controller::Bot
            } else {
                Controller::Human
            };
            let id = sim.add_player(name, c, *team).ok_or("serveur complet")?;
            if id.0 != *slot && *slot != u8::MAX {
                return Err(format!("slot attendu {slot}, obtenu {}", id.0));
            }
            Ok(Some(id))
        }
        SimInput::Leave { slot } => {
            sim.remove_player(PlayerId(*slot));
            Ok(None)
        }
        SimInput::Team { slot, team } => sim.set_team(PlayerId(*slot), *team).map(|_| None),
        SimInput::Buy { slot, item } => sim.buy(PlayerId(*slot), item).map(|_| None),
        SimInput::ForceStart => {
            sim.force_start();
            Ok(None)
        }
    }
}

pub struct Recorder<W: Write> {
    w: ReplayWriter<W>,
    frame: ReplayFrame,
}

impl<W: Write> Recorder<W> {
    pub fn new(out: W, header: &ReplayHeader) -> Result<Self, ReplayError> {
        let w = ReplayWriter::new(out, &crate::proto::encode(header))?;
        Ok(Self {
            w,
            frame: ReplayFrame {
                tick: 0,
                inputs: Vec::new(),
                cmds: Vec::new(),
            },
        })
    }

    /// Records an input applied before the step of `tick`.
    pub fn input(&mut self, tick: u32, i: SimInput) {
        self.frame.tick = tick;
        self.frame.inputs.push(i);
    }

    /// Records the commands used by the step of `tick` and closes the frame.
    pub fn step(&mut self, tick: u32, cmds: &[Option<UserCmd>]) -> Result<(), ReplayError> {
        self.frame.tick = tick;
        self.frame.cmds = cmds
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_ref().map(|c| (i as u8, c.clone())))
            .collect();
        // Empty frames every second keep a cut file playable up to its end.
        if !self.frame.inputs.is_empty() || !self.frame.cmds.is_empty() || tick.is_multiple_of(128)
        {
            self.w.frame(&crate::proto::encode(&self.frame))?;
        }
        self.frame.inputs.clear();
        self.frame.cmds.clear();
        Ok(())
    }

    /// Marks the end of the recording: `tick` is the simulation tick reached.
    pub fn finish(&mut self, tick: u32) -> Result<(), ReplayError> {
        self.w.frame(&crate::proto::encode(&ReplayFrame {
            tick,
            inputs: Vec::new(),
            cmds: Vec::new(),
        }))?;
        self.w.flush()
    }

    pub fn frames(&self) -> u64 {
        self.w.frames
    }

    pub fn bytes(&self) -> u64 {
        self.w.bytes
    }

    pub fn flush(&mut self) -> Result<(), ReplayError> {
        self.w.flush()
    }

    pub fn into_inner(self) -> W {
        self.w.into_inner()
    }
}

/// FNV hash of the gameplay state (used to verify replays and determinism).
pub fn state_hash(sim: &Sim) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    eat(&sim.tick.to_le_bytes());
    for p in sim.iter_players() {
        eat(&[p.id.0, p.alive as u8, p.team.to_u8()]);
        eat(&p.health.to_le_bytes());
        eat(&p.money.to_le_bytes());
        for c in p.mv.origin.to_array() {
            eat(&c.to_le_bytes());
        }
        eat(&p.stats.kills.to_le_bytes());
    }
    eat(&sim.rules.scores[0].to_le_bytes());
    eat(&sim.rules.scores[1].to_le_bytes());
    h
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySummary {
    pub header: ReplayHeader,
    pub ticks: u32,
    pub frames: u64,
    pub truncated: bool,
    pub final_hash: u64,
    pub scores: [u16; 2],
    pub kills: usize,
}

/// Re-simulates a replay from its inputs. `on_tick` sees the sim after each step.
pub fn play<R: Read>(
    data: Arc<GameData>,
    input: R,
    mut on_tick: impl FnMut(&mut Sim),
) -> Result<ReplaySummary, String> {
    let mut r = ReplayReader::open(input).map_err(|e| e.to_string())?;
    let header: ReplayHeader = crate::proto::decode(&r.header).map_err(|e| e.to_string())?;
    if header.data_fingerprint != data.fingerprint() {
        return Err("les données de jeu diffèrent de celles de l'enregistrement".into());
    }
    let def = data
        .maps
        .get(&header.map)
        .ok_or_else(|| format!("carte inconnue {}", header.map))?
        .clone();
    let level =
        Arc::new(Level::build(def, &data.materials, &data.movement).map_err(|e| e.to_string())?);
    let mut sim = Sim::new(
        data,
        level,
        header.rules.clone(),
        header.sim.clone(),
        header.seed,
    )?;
    let mut cmds: Vec<Option<UserCmd>> = (0..MAX_PLAYERS).map(|_| None).collect();
    let mut frames = 0;
    let mut kills = 0;
    while let Some(f) = r.next_frame() {
        let frame: ReplayFrame = crate::proto::decode(&f).map_err(|e| e.to_string())?;
        frames += 1;
        while sim.tick < frame.tick {
            sim.step(&mut cmds);
            kills += sim
                .drain()
                .0
                .iter()
                .filter(|e| matches!(e, crate::GameEvent::Kill { .. }))
                .count();
            on_tick(&mut sim);
        }
        if frame.inputs.is_empty() && frame.cmds.is_empty() {
            // Time marker: nothing happened at this tick (yet).
            continue;
        }
        for i in &frame.inputs {
            apply(&mut sim, i)?;
        }
        for (slot, c) in frame.cmds {
            if let Some(x) = cmds.get_mut(slot as usize) {
                *x = Some(c);
            }
        }
        sim.step(&mut cmds);
        kills += sim
            .drain()
            .0
            .iter()
            .filter(|e| matches!(e, crate::GameEvent::Kill { .. }))
            .count();
        on_tick(&mut sim);
    }
    Ok(ReplaySummary {
        ticks: sim.tick,
        frames,
        truncated: r.truncated,
        final_hash: state_hash(&sim),
        scores: sim.rules.scores,
        kills,
        header,
    })
}
