//! Tick cost of the simulation with bots (5v5 on the competitive map).

use criterion::{Criterion, criterion_group, criterion_main};
use game_sim::{Controller, GameData, Level, Sim, SimConfig};
use std::sync::Arc;

fn bench(c: &mut Criterion) {
    let dir = engine_core::config::find_data_dir();
    let data = Arc::new(GameData::load(&dir).expect("data"));
    let def = data.maps["ps_citadel"].clone();
    let level = Arc::new(Level::build(def, &data.materials, &data.movement).expect("level"));
    let mut rules = data.rules_profile("competitive").expect("rules");
    rules.warmup_time = 0.0;
    let mut sim = Sim::new(data, level, rules, SimConfig::default(), 1).expect("sim");
    for i in 0..10 {
        sim.add_player(&format!("Bot {i}"), Controller::Bot, None);
    }
    let mut cmds: Vec<_> = (0..64).map(|_| None).collect();
    c.bench_function("sim_tick_5v5_bots", |b| {
        b.iter(|| {
            sim.step(&mut cmds);
            sim.drain();
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
