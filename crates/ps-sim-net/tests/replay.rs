//! Replays: recording a match and playing it back must give the same state.

use ps_sim::{GameData, Level, Sim, SimConfig};
use ps_sim_core::cmd::UserCmd;
use ps_sim_net::record::{Recorder, ReplayHeader, SimInput, apply, play, state_hash};
use std::sync::Arc;

fn data() -> Arc<GameData> {
    let dir = ps_sim_core::config::find_data_dir();
    Arc::new(GameData::load(&dir).expect("game data"))
}

#[test]
fn replay_reproduces_the_match() {
    let d = data();
    let level =
        Arc::new(Level::build(d.maps["ps_citadel"].clone(), &d.materials, &d.movement).unwrap());
    let mut rules = d.rules_profile("competitive").unwrap();
    rules.warmup_time = 0.0;
    let header = ReplayHeader {
        game_version: "test".into(),
        map: "ps_citadel".into(),
        profile: "competitive".into(),
        rules: rules.clone(),
        sim: SimConfig::default(),
        seed: 99,
        data_fingerprint: d.fingerprint(),
        started_unix: 0,
    };
    let mut sim = Sim::new(d.clone(), level, rules, SimConfig::default(), 99).unwrap();
    let mut rec = Recorder::new(Vec::new(), &header).unwrap();
    let mut cmds: Vec<Option<UserCmd>> = (0..64).map(|_| None).collect();
    for i in 0..6u8 {
        let input = SimInput::Join {
            slot: i,
            name: format!("Bot {i}"),
            bot: true,
            team: None,
        };
        apply(&mut sim, &input).unwrap();
        rec.input(sim.tick, input);
    }
    apply(&mut sim, &SimInput::ForceStart).unwrap();
    rec.input(sim.tick, SimInput::ForceStart);
    for _ in 0..(128 * 60) {
        rec.step(sim.tick, &cmds).unwrap();
        sim.step(&mut cmds);
        sim.drain();
    }
    let expected = state_hash(&sim);
    rec.finish(sim.tick).unwrap();
    let bytes = rec_into_bytes(rec);
    let summary = play(d, &bytes[..], |_| {}).unwrap();
    assert_eq!(summary.ticks, sim.tick);
    assert_eq!(summary.final_hash, expected, "replay diverged");
    assert!(!summary.truncated);
}

fn rec_into_bytes(mut rec: ps_sim_net::record::Recorder<Vec<u8>>) -> Vec<u8> {
    rec.flush().unwrap();
    rec.into_inner()
}
