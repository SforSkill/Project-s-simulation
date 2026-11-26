//! Game protocol carried by `engine-network` channels.
//!
//! Control messages use postcard (serde); snapshots use a hand-written,
//! quantized and delta-compressed binary format (see `docs/network.md`).

use crate::events::GameEvent;
use crate::sim::SimConfig;
use engine_core::cmd::UserCmd;
use engine_core::{PlayerId, Team};
use engine_movement::MoveState;
use engine_network::NetError;
use engine_network::wire::{Reader, Writer};
use engine_weapons::{GrenadeKind, GunState, HandState, HitGroup, Slot, WeaponId};
use game_rules::{Phase, RulesConfig};
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Game protocol version (on top of the transport version).
pub const GAME_PROTOCOL: u32 = 1;
/// Commands repeated in every input packet (loss resilience).
pub const CMD_REDUNDANCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Join {
        name: String,
        protocol: u32,
        data_fingerprint: u64,
        version: String,
        reconnect: Option<u64>,
    },
    /// Latest commands, oldest first; `snapshot_ack` = newest snapshot tick received.
    Cmds {
        cmds: Vec<UserCmd>,
        snapshot_ack: u32,
        interp_ticks: f32,
    },
    Team(Team),
    Buy(String),
    Chat {
        team_only: bool,
        text: String,
    },
    /// Host commands (local listen server only): `bots 4`, `restart`, `start`...
    Host(String),
    Leave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub you: u8,
    pub server_name: String,
    pub tick_rate: u32,
    pub tick: u32,
    pub map: String,
    pub profile: String,
    pub rules: RulesConfig,
    pub sim: SimConfig,
    pub data_fingerprint: u64,
    pub host: bool,
    /// Secret used to take the same slot back after a disconnection.
    pub reconnect_token: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: u8,
    pub name: String,
    pub team: Team,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub score: i32,
    pub mvps: u32,
    pub headshots: u32,
    pub damage: u32,
    /// Only for team mates (and yourself).
    pub money: Option<u32>,
    pub ping_ms: u16,
    pub bot: bool,
    pub alive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmokeNet {
    pub id: u16,
    pub center: Vec3,
    pub origin: Vec3,
    pub dims: IVec3,
    pub voxel: f32,
    pub bits: Vec<u8>,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotNet {
    pub tick: u32,
    pub weapon: WeaponId,
    pub origin: Vec3,
    pub dir: Vec3,
    pub recoil: (f32, f32),
    pub spread_deg: f32,
    pub spread: (f32, f32),
    pub speed: f32,
    pub on_ground: bool,
    pub rewind_ms: f32,
    pub rewind_clamped: bool,
    pub surfaces: Vec<(String, f32, f32, bool)>,
    pub hit: Option<(u8, HitGroup, f32, u32, u32, bool)>,
    pub end: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome(Box<Welcome>),
    Events(Vec<GameEvent>),
    Smoke(SmokeNet),
    SmokeBlast {
        pos: Vec3,
        time: f64,
    },
    Fire {
        id: u16,
        cells: Vec<Vec3>,
        start: f64,
        end: f64,
    },
    Players(Vec<PlayerInfo>),
    Shot(Box<ShotNet>),
    Notice(String),
    Kicked(String),
}

pub fn encode<T: Serialize>(m: &T) -> Vec<u8> {
    postcard::to_stdvec(m).expect("serializable message")
}

pub fn decode<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, NetError> {
    postcard::from_bytes(b).map_err(|_| NetError::Malformed("bad game message"))
}

// ---------------------------------------------------------------------- snapshots

/// Position quantum: 1/8 unit.
pub const POS_Q: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntityQ {
    pub id: u8,
    pub flags: u16,
    pub pos: [i32; 3],
    pub vel: [i16; 3],
    pub yaw: u16,
    pub pitch: i16,
    pub duck: u8,
    pub team: u8,
    pub weapon: u8,
    pub health: u8,
    pub armor: u8,
}

pub mod eflags {
    pub const ALIVE: u16 = 1;
    pub const DUCKED: u16 = 2;
    pub const ON_GROUND: u16 = 4;
    pub const PLANTING: u16 = 8;
    pub const DEFUSING: u16 = 16;
    pub const HAS_BOMB: u16 = 32;
    pub const HELMET: u16 = 64;
    pub const RELOADING: u16 = 128;
    pub const ZOOMED: u16 = 256;
    pub const BOT: u16 = 512;
    pub const DUMMY: u16 = 1024;
    pub const NO_WEAPON: u16 = 2048;
    /// Not currently visible: last known / audible position only.
    pub const STALE: u16 = 4096;
}

pub fn q_pos(p: Vec3) -> [i32; 3] {
    [
        (p.x * POS_Q).round() as i32,
        (p.y * POS_Q).round() as i32,
        (p.z * POS_Q).round() as i32,
    ]
}

pub fn dq_pos(p: [i32; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) / POS_Q
}

pub fn q_vel(v: Vec3) -> [i16; 3] {
    let c = |x: f32| x.round().clamp(-32000.0, 32000.0) as i16;
    [c(v.x), c(v.y), c(v.z)]
}

pub fn dq_vel(v: [i16; 3]) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
}

pub fn q_yaw(y: f32) -> u16 {
    ((y.rem_euclid(360.0) / 360.0) * 65536.0).round() as u32 as u16
}

pub fn dq_yaw(y: u16) -> f32 {
    y as f32 * 360.0 / 65536.0
}

impl EntityQ {
    fn write(&self, w: &mut Writer) {
        w.u8(self.id).u16(self.flags);
        for c in self.pos {
            w.svar(c as i64);
        }
        for c in self.vel {
            w.svar(c as i64);
        }
        w.u16(self.yaw)
            .i16(self.pitch)
            .u8(self.duck)
            .u8(self.team)
            .u8(self.weapon)
            .u8(self.health)
            .u8(self.armor);
    }

    fn read(r: &mut Reader) -> Result<EntityQ, NetError> {
        let id = r.u8()?;
        let flags = r.u16()?;
        let mut pos = [0i32; 3];
        for c in &mut pos {
            *c = r.svar()?.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        }
        let mut vel = [0i16; 3];
        for c in &mut vel {
            *c = r.svar()?.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        }
        Ok(EntityQ {
            id,
            flags,
            pos,
            vel,
            yaw: r.u16()?,
            pitch: r.i16()?,
            duck: r.u8()?,
            team: r.u8()?,
            weapon: r.u8()?,
            health: r.u8()?,
            armor: r.u8()?,
        })
    }

    pub fn origin(&self) -> Vec3 {
        dq_pos(self.pos)
    }

    pub fn alive(&self) -> bool {
        self.flags & eflags::ALIVE != 0
    }
}

/// The local player's exact state (prediction / reconciliation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfState {
    pub alive: bool,
    pub team: Team,
    pub mv: MoveState,
    pub health: i32,
    pub armor: u8,
    pub helmet: bool,
    pub defuse_kit: bool,
    pub money: u32,
    pub active: Slot,
    pub primary: Option<(WeaponId, GunState)>,
    pub secondary: Option<(WeaponId, GunState)>,
    pub knife: Option<WeaponId>,
    pub grenades: Vec<WeaponId>,
    pub grenade_index: u8,
    pub bomb: bool,
    pub hand: HandState,
    pub flash: (f64, f32, f32),
    pub planting_since: Option<f64>,
    pub defusing_since: Option<f64>,
    pub can_buy: bool,
    pub respawn_in: Option<f32>,
    /// Spectated player when dead.
    pub spectating: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BombNet {
    /// 0 none, 1 carried, 2 dropped, 3 planted, 4 exploded, 5 defused.
    pub kind: u8,
    pub pos: Vec3,
    pub carrier: u8,
    pub time_left: f32,
    pub defuse_left: Option<f32>,
    pub site: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrenadeQ {
    pub id: u16,
    pub kind: GrenadeKind,
    pub pos: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DropQ {
    pub id: u16,
    pub weapon: WeaponId,
    pub pos: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub tick: u32,
    /// Tick of the snapshot this one is delta-encoded against (0 = full).
    pub baseline: u32,
    /// Last command tick the server simulated for this client.
    pub last_cmd: u32,
    /// Newest command tick received minus the tick just simulated (clock sync).
    pub cmd_buffer: i16,
    pub phase: Phase,
    pub time_left: f32,
    pub round: u16,
    pub scores: [u16; 2],
    /// Side of squad 0 (squad 1 has the other).
    pub squad0_side: Team,
    pub bomb: BombNet,
    pub me: Option<SelfState>,
    pub entities: Vec<EntityQ>,
    pub grenades: Vec<GrenadeQ>,
    pub drops: Vec<DropQ>,
}

/// Untimed phases (deathmatch without limit) use -1.
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { -1.0 }
}

/// Encodes `snap` against `base` (entities identical to the baseline are sent as
/// one id byte).
pub fn encode_snapshot(snap: &Snapshot, base: Option<&HashMap<u8, EntityQ>>) -> Vec<u8> {
    let mut w = Writer::with_capacity(1024);
    w.u32(snap.tick)
        .u32(if base.is_some() { snap.baseline } else { 0 })
        .u32(snap.last_cmd)
        .i16(snap.cmd_buffer);
    w.u8(snap.phase.to_u8())
        .f32(finite(snap.time_left))
        .u16(snap.round)
        .u16(snap.scores[0])
        .u16(snap.scores[1])
        .u8(snap.squad0_side.to_u8());
    let b = &snap.bomb;
    w.u8(b.kind);
    if b.kind != 0 {
        let p = q_pos(b.pos);
        w.svar(p[0] as i64)
            .svar(p[1] as i64)
            .svar(p[2] as i64)
            .u8(b.carrier)
            .f32(finite(b.time_left))
            .u8(b.site);
        match b.defuse_left {
            Some(t) => w.u8(1).f32(finite(t)),
            None => w.u8(0),
        };
    }
    match &snap.me {
        Some(me) => w.u8(1).blob(&encode(me)),
        None => w.u8(0),
    };
    let mut same = Vec::new();
    let mut changed = Vec::new();
    for e in &snap.entities {
        match base.and_then(|b| b.get(&e.id)) {
            Some(old) if old == e => same.push(e.id),
            _ => changed.push(*e),
        }
    }
    w.blob(&same);
    w.var(changed.len() as u64);
    for e in &changed {
        e.write(&mut w);
    }
    w.var(snap.grenades.len() as u64);
    for g in &snap.grenades {
        let p = q_pos(g.pos);
        w.u16(g.id)
            .u8(g.kind.to_u8())
            .svar(p[0] as i64)
            .svar(p[1] as i64)
            .svar(p[2] as i64);
    }
    w.var(snap.drops.len() as u64);
    for d in &snap.drops {
        let p = q_pos(d.pos);
        w.u16(d.id)
            .u8(d.weapon.0)
            .svar(p[0] as i64)
            .svar(p[1] as i64)
            .svar(p[2] as i64);
    }
    w.finish()
}

/// Header of an encoded snapshot: (tick, baseline).
pub fn snapshot_ticks(b: &[u8]) -> Result<(u32, u32), NetError> {
    let mut r = Reader::new(b);
    Ok((r.u32()?, r.u32()?))
}

/// Decodes a snapshot; `base` must be the entities of the baseline tick when it is
/// not 0 (returns `Err` if missing).
pub fn decode_snapshot(
    b: &[u8],
    base: Option<&HashMap<u8, EntityQ>>,
) -> Result<Snapshot, NetError> {
    let mut r = Reader::new(b);
    let tick = r.u32()?;
    let baseline = r.u32()?;
    let last_cmd = r.u32()?;
    let cmd_buffer = r.i16()?;
    let phase = Phase::from_u8(r.u8()?);
    let time_left = r.f32()?;
    let round = r.u16()?;
    let scores = [r.u16()?, r.u16()?];
    let squad0_side = Team::from_u8(r.u8()?).ok_or(NetError::Malformed("team"))?;
    let kind = r.u8()?;
    let mut bomb = BombNet {
        kind,
        ..Default::default()
    };
    if kind != 0 {
        let p = [r.svar()? as i32, r.svar()? as i32, r.svar()? as i32];
        bomb.pos = dq_pos(p);
        bomb.carrier = r.u8()?;
        bomb.time_left = r.f32()?;
        bomb.site = r.u8()?;
        if r.u8()? == 1 {
            bomb.defuse_left = Some(r.f32()?);
        }
    }
    let me = if r.u8()? == 1 {
        Some(decode::<SelfState>(r.blob(4096)?)?)
    } else {
        None
    };
    let same = r.blob(256)?.to_vec();
    let mut entities = Vec::new();
    if baseline != 0 {
        let base = base.ok_or(NetError::Malformed("missing baseline"))?;
        for id in same {
            entities.push(
                *base
                    .get(&id)
                    .ok_or(NetError::Malformed("baseline entity"))?,
            );
        }
    } else if !same.is_empty() {
        return Err(NetError::Malformed("delta without baseline"));
    }
    let n = r.var()? as usize;
    if n > 256 {
        return Err(NetError::Malformed("entity count"));
    }
    for _ in 0..n {
        entities.push(EntityQ::read(&mut r)?);
    }
    let n = r.var()? as usize;
    if n > 256 {
        return Err(NetError::Malformed("grenade count"));
    }
    let mut grenades = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u16()?;
        let kind = GrenadeKind::from_u8(r.u8()?).ok_or(NetError::Malformed("grenade kind"))?;
        let pos = dq_pos([r.svar()? as i32, r.svar()? as i32, r.svar()? as i32]);
        grenades.push(GrenadeQ { id, kind, pos });
    }
    let n = r.var()? as usize;
    if n > 256 {
        return Err(NetError::Malformed("drop count"));
    }
    let mut drops = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u16()?;
        let weapon = WeaponId(r.u8()?);
        let pos = dq_pos([r.svar()? as i32, r.svar()? as i32, r.svar()? as i32]);
        drops.push(DropQ { id, weapon, pos });
    }
    entities.sort_by_key(|e| e.id);
    Ok(Snapshot {
        tick,
        baseline,
        last_cmd,
        cmd_buffer,
        phase,
        time_left,
        round,
        scores,
        squad0_side,
        bomb,
        me,
        entities,
        grenades,
        drops,
    })
}

pub fn entity_map(entities: &[EntityQ]) -> HashMap<u8, EntityQ> {
    entities.iter().map(|e| (e.id, *e)).collect()
}

pub fn player_id(e: &EntityQ) -> PlayerId {
    PlayerId(e.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(tick: u32, n: u8) -> Snapshot {
        Snapshot {
            tick,
            baseline: 0,
            last_cmd: tick,
            cmd_buffer: 2,
            phase: Phase::Live,
            time_left: 80.5,
            round: 3,
            scores: [2, 1],
            squad0_side: Team::Attack,
            bomb: BombNet {
                kind: 3,
                pos: Vec3::new(10.0, 0.0, -20.0),
                carrier: 0,
                time_left: 30.0,
                defuse_left: Some(4.0),
                site: 1,
            },
            me: None,
            entities: (0..n)
                .map(|i| EntityQ {
                    id: i,
                    flags: eflags::ALIVE,
                    pos: [i as i32 * 100, 8, -400],
                    vel: [250, 0, 0],
                    yaw: 1000,
                    pitch: -50,
                    duck: 0,
                    team: 1,
                    weapon: 3,
                    health: 100,
                    armor: 100,
                })
                .collect(),
            grenades: vec![GrenadeQ {
                id: 70,
                kind: GrenadeKind::Smoke,
                pos: Vec3::new(1.0, 2.0, 3.0),
            }],
            drops: vec![],
        }
    }

    #[test]
    fn full_and_delta_roundtrip() {
        let a = snap(10, 10);
        let full = encode_snapshot(&a, None);
        let da = decode_snapshot(&full, None).unwrap();
        assert_eq!(da.entities, a.entities);
        assert_eq!(da.bomb, a.bomb);
        let mut b = snap(11, 10);
        b.baseline = 10;
        b.entities[3].pos[0] += 16;
        b.entities.remove(7);
        let base = entity_map(&a.entities);
        let delta = encode_snapshot(&b, Some(&base));
        assert!(
            delta.len() < full.len() / 2,
            "delta {} vs full {}",
            delta.len(),
            full.len()
        );
        let db = decode_snapshot(&delta, Some(&base)).unwrap();
        assert_eq!(db.entities, b.entities);
        assert!(decode_snapshot(&delta, None).is_err());
        let mut c = snap(12, 1);
        c.time_left = f32::INFINITY;
        assert_eq!(
            decode_snapshot(&encode_snapshot(&c, None), None)
                .unwrap()
                .time_left,
            -1.0
        );
    }

    #[test]
    fn quantization_is_precise_enough() {
        let p = Vec3::new(1234.567, -12.3, 987.65);
        assert!((dq_pos(q_pos(p)) - p).length() < 0.11);
        for y in [0.0f32, 90.0, 179.99, 359.0] {
            assert!((dq_yaw(q_yaw(y)) - y).abs() < 0.01);
        }
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode_snapshot(&[1, 2, 3], None).is_err());
        assert!(decode::<ClientMsg>(&[250, 1, 2]).is_err());
        let msg = ClientMsg::Buy("rifle".into());
        assert_eq!(decode::<ClientMsg>(&encode(&msg)).unwrap(), msg);
    }
}
