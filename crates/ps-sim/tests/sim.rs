//! Integration tests of the authoritative simulation.

use engine_core::cmd::{ButtonEvent, Buttons, UserCmd};
use engine_core::math::forward_to_angles;
use engine_core::{PlayerId, Team, Vec3};
use engine_hitreg::{Pose, hitboxes};
use engine_physics::map::{BrushDef, MapDef, SpawnDef, ZoneDef};
use engine_weapons::HitGroup;
use game_rules::{Mode, Phase};
use game_sim::smoke::smoke_blocks;
use game_sim::{
    BombState, Controller, GameData, GameEvent, Level, Sim, SimConfig, SmokeConfig, SmokeVolume,
};
use std::sync::Arc;

fn data() -> Arc<GameData> {
    let dir = engine_core::config::find_data_dir();
    Arc::new(GameData::load(&dir).expect("game data"))
}

fn bx(min: (f32, f32, f32), max: (f32, f32, f32), mat: &str) -> BrushDef {
    BrushDef::Box {
        min: Vec3::new(min.0, min.1, min.2),
        max: Vec3::new(max.0, max.1, max.2),
        mat: mat.into(),
        players: true,
        bullets: true,
    }
}

/// Flat test range: a wood wall (x 300..700) and a thick metal wall (x 1300..1700)
/// at z = -516..-500, one bomb site around (-800, -1600).
fn range_map() -> MapDef {
    MapDef {
        name: "test_range".into(),
        display_name: "Test".into(),
        version: 1,
        modes: vec!["training".into(), "competitive".into()],
        brushes: vec![
            bx((-3000.0, -32.0, -3000.0), (3000.0, 0.0, 3000.0), "concrete"),
            bx((300.0, 0.0, -516.0), (700.0, 200.0, -500.0), "wood"),
            bx(
                (1300.0, 0.0, -516.0),
                (1700.0, 200.0, -500.0),
                "metal_thick",
            ),
        ],
        spawns: vec![
            SpawnDef {
                team: Team::Attack,
                pos: Vec3::new(-500.0, 1.0, 0.0),
                yaw: 0.0,
            },
            SpawnDef {
                team: Team::Defense,
                pos: Vec3::new(-500.0, 1.0, -1000.0),
                yaw: 180.0,
            },
        ],
        dm_spawns: vec![],
        bomb_sites: vec![ZoneDef {
            name: "A".into(),
            team: Team::Spectator,
            min: Vec3::new(-1000.0, 0.0, -1800.0),
            max: Vec3::new(-600.0, 128.0, -1400.0),
        }],
        buy_zones: vec![],
        targets: vec![],
        labels: vec![],
        bounds_min: Vec3::new(-3000.0, -64.0, -3000.0),
        bounds_max: Vec3::new(3000.0, 512.0, 3000.0),
        sky_color: [0.5, 0.6, 0.7],
    }
}

fn sim_on(def: MapDef, profile: &str, cfg: SimConfig) -> Sim {
    let d = data();
    let level = Arc::new(Level::build(def, &d.materials, &d.movement).expect("level"));
    let rules = d.rules_profile(profile).expect("profile");
    Sim::new(d, level, rules, cfg, 42).expect("sim")
}

fn no_lagcomp() -> SimConfig {
    let mut c = SimConfig::default();
    c.lag_comp.enabled = false;
    c
}

fn idle(sim: &mut Sim, ticks: u32) {
    let mut cmds: Vec<Option<UserCmd>> = (0..64).map(|_| None).collect();
    for _ in 0..ticks {
        sim.step(&mut cmds);
    }
}

fn teleport(sim: &mut Sim, id: PlayerId, pos: Vec3) {
    let p = sim.player_mut(id).unwrap();
    p.mv.origin = pos;
    p.mv.velocity = Vec3::ZERO;
}

fn head_center(sim: &Sim, id: PlayerId) -> Vec3 {
    let p = sim.player(id).unwrap();
    let hb = hitboxes(&Pose {
        origin: p.mv.origin,
        yaw: p.view.yaw,
        duck: p.mv.duck_amount,
    });
    let c = hb
        .capsules
        .iter()
        .find(|c| c.group == HitGroup::Head)
        .unwrap();
    (c.a + c.b) * 0.5
}

fn chest_center(sim: &Sim, id: PlayerId) -> Vec3 {
    let p = sim.player(id).unwrap();
    let hb = hitboxes(&Pose {
        origin: p.mv.origin,
        yaw: p.view.yaw,
        duck: p.mv.duck_amount,
    });
    let c = hb
        .capsules
        .iter()
        .find(|c| c.group == HitGroup::Chest)
        .unwrap();
    (c.a + c.b) * 0.5
}

/// One tick where `id` presses `extra` + attack at the start of the tick, aiming at `point`.
fn shoot_at(sim: &mut Sim, id: PlayerId, point: Vec3, extra: u32) {
    let eye = sim.player(id).unwrap().mv.eye(&sim.data.movement);
    let view = forward_to_angles((point - eye).normalize());
    sim.player_mut(id).unwrap().view = view;
    let mut cmds: Vec<Option<UserCmd>> = (0..64).map(|_| None).collect();
    cmds[id.0 as usize] = Some(UserCmd {
        buttons_start: Buttons(extra),
        view_start: view,
        view_end: view,
        events: vec![ButtonEvent {
            frac: 0,
            buttons: Buttons(extra | Buttons::ATTACK),
            view,
        }],
        ..Default::default()
    });
    sim.step(&mut cmds);
}

fn duel_setup(shooter_pos: Vec3, victim_pos: Vec3) -> (Sim, PlayerId, PlayerId) {
    let mut sim = sim_on(range_map(), "training", no_lagcomp());
    let a = sim
        .add_player("A", Controller::Human, Some(Team::Attack))
        .unwrap();
    let b = sim
        .add_player("B", Controller::Human, Some(Team::Defense))
        .unwrap();
    idle(&mut sim, 160); // deploy
    teleport(&mut sim, a, shooter_pos);
    teleport(&mut sim, b, victim_pos);
    let pb = sim.player_mut(b).unwrap();
    pb.armor = 0;
    pb.helmet = false;
    (sim, a, b)
}

#[test]
fn shipped_data_and_maps_are_valid() {
    let d = data();
    for name in ["ps_citadel", "ps_training_lab", "ps_duel_lab"] {
        let def = d
            .maps
            .get(name)
            .unwrap_or_else(|| panic!("missing map {name}"))
            .clone();
        let level = Level::build(def, &d.materials, &d.movement).expect("map builds");
        assert!(level.nav.len() > 100, "{name}: nav too small");
    }
    for p in [
        "competitive",
        "competitive_mr12",
        "competitive_mr16",
        "duel",
        "deathmatch",
        "training",
    ] {
        d.rules_profile(p).expect(p);
    }
}

#[test]
fn citadel_nav_connects_spawns_to_both_sites() {
    let d = data();
    let level = Level::build(d.maps["ps_citadel"].clone(), &d.materials, &d.movement).unwrap();
    assert_eq!(level.def.bomb_sites.len(), 2);
    for s in &level.def.spawns {
        for site in &level.def.bomb_sites {
            let a = site.aabb();
            let target = Vec3::new(a.center().x, a.min.y + 1.0, a.center().z);
            assert!(
                level.nav.path(s.pos, target).is_some(),
                "no path from {:?} spawn {:?} to site {}",
                s.team,
                s.pos,
                site.name
            );
        }
    }
}

#[test]
fn first_shot_standing_is_exact_and_headshot_kills() {
    let (mut sim, a, b) = duel_setup(Vec3::new(-500.0, 1.0, 0.0), Vec3::new(-500.0, 1.0, -1000.0));
    let head = head_center(&sim, b);
    shoot_at(&mut sim, a, head, 0);
    let (events, shots) = sim.drain();
    assert_eq!(shots.len(), 1, "one bullet");
    let s = &shots[0];
    assert_eq!(s.spread_deg, 0.0);
    assert_eq!(s.recoil, (0.0, 0.0));
    assert!(
        (s.dir - s.view.forward()).length() < 1e-6,
        "no hidden random spread"
    );
    let hit = s.hit.as_ref().expect("hit");
    assert_eq!(hit.group, HitGroup::Head);
    assert!(hit.killed);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::Kill { victim, headshot: true, .. } if *victim == b))
    );
}

#[test]
fn moving_shots_get_spread() {
    let (mut sim, a, b) = duel_setup(Vec3::new(-500.0, 1.0, 0.0), Vec3::new(-500.0, 1.0, -1000.0));
    sim.player_mut(a).unwrap().mv.velocity = Vec3::new(220.0, 0.0, 0.0);
    let chest = chest_center(&sim, b);
    shoot_at(&mut sim, a, chest, Buttons::RIGHT);
    let (_, shots) = sim.drain();
    assert!(
        shots[0].spread_deg > 1.0,
        "running spread {}",
        shots[0].spread_deg
    );
}

#[test]
fn wood_is_penetrable_and_thick_metal_is_not() {
    let (mut sim, a, b) = duel_setup(Vec3::new(500.0, 1.0, 0.0), Vec3::new(500.0, 1.0, -1000.0));
    let chest = chest_center(&sim, b);
    shoot_at(&mut sim, a, chest, 0);
    let (_, shots) = sim.drain();
    let s = &shots[0];
    assert_eq!(s.surfaces.len(), 1);
    assert_eq!(s.surfaces[0].material, "wood");
    assert!(!s.surfaces[0].stopped);
    let hit = s.hit.as_ref().expect("wallbang hit");
    let rifle = sim.data.catalog.by_name("rifle").unwrap();
    assert!(
        hit.raw_damage < sim.data.catalog.get(rifle).damage,
        "penetration loses damage"
    );
    assert!(hit.health_damage > 0);

    let (mut sim, a, b) = duel_setup(Vec3::new(1500.0, 1.0, 0.0), Vec3::new(1500.0, 1.0, -1000.0));
    let chest = chest_center(&sim, b);
    shoot_at(&mut sim, a, chest, 0);
    let (_, shots) = sim.drain();
    assert!(shots[0].hit.is_none());
    assert!(shots[0].surfaces[0].stopped);
}

#[test]
fn tagging_only_without_armor_and_no_aim_punch() {
    let (mut sim, a, b) = duel_setup(Vec3::new(-500.0, 1.0, 0.0), Vec3::new(-500.0, 1.0, -1000.0));
    let rifle = sim.data.catalog.by_name("rifle").unwrap();
    let def = sim.data.catalog.get(rifle).clone();
    // Armored: no slowdown.
    {
        let p = sim.player_mut(b).unwrap();
        p.armor = 100;
        p.mv.velocity_modifier = 1.0;
    }
    let view_before = sim.player(b).unwrap().view;
    sim.apply_damage(
        b,
        Some(a),
        Some(rifle),
        20.0,
        Some(HitGroup::Chest),
        def.armor_ratio,
        def.tagging,
        Vec3::ZERO,
        false,
    );
    let p = sim.player(b).unwrap();
    assert_eq!(p.mv.velocity_modifier, 1.0, "kevlar prevents tagging");
    assert_eq!(p.view, view_before, "no aim punch");
    // Armor destroyed: tagging returns.
    sim.player_mut(b).unwrap().armor = 0;
    sim.apply_damage(
        b,
        Some(a),
        Some(rifle),
        20.0,
        Some(HitGroup::Chest),
        def.armor_ratio,
        def.tagging,
        Vec3::ZERO,
        false,
    );
    let p = sim.player(b).unwrap();
    assert!(p.mv.velocity_modifier < 1.0, "tagged without armor");
    assert_eq!(p.view, view_before, "still no aim punch");
}

#[test]
fn smoke_never_crosses_walls_blocks_sight_and_reforms_after_he() {
    let d = data();
    let level = Level::build(range_map(), &d.materials, &d.movement).unwrap();
    let cfg = SmokeConfig::default();
    let center = Vec3::new(1500.0, 8.0, -440.0);
    let s = SmokeVolume::fill(1, center, &level.world, &cfg, 0.0);
    assert!(s.filled_count() > 100);
    for i in 0..s.filled.len() {
        if !s.filled[i] {
            continue;
        }
        let v = s.voxel_center(i);
        assert!(
            !(v.z < -500.0 && (1300.0..=1700.0).contains(&v.x) && v.y < 200.0),
            "voxel behind the wall: {v:?}"
        );
        assert!(v.y > 0.0, "voxel below the floor: {v:?}");
    }
    let smokes = vec![s];
    let a = Vec3::new(1500.0 - 400.0, 60.0, -400.0);
    let b = Vec3::new(1500.0 + 400.0, 60.0, -400.0);
    assert!(smoke_blocks(&smokes, a, b, 3.0, &cfg), "opaque");
    assert!(smoke_blocks(&smokes, b, a, 3.0, &cfg), "symmetric");
    assert!(
        !smoke_blocks(&smokes, a, b, cfg.duration as f64 + 1.0, &cfg),
        "expires"
    );
    let mut smokes = smokes;
    let probe = center + Vec3::Y * 40.0;
    assert!(smokes[0].contains(probe, 3.0, &cfg));
    assert!(smokes[0].blast(probe, 3.0, &cfg));
    assert!(!smokes[0].contains(probe, 3.1, &cfg), "HE opens a hole");
    let back = 3.0 + (cfg.he_hole_time + cfg.he_reform_time) as f64 + 0.1;
    assert!(smokes[0].contains(probe, back, &cfg), "smoke reforms");
}

#[test]
fn bomb_plant_and_defuse() {
    let mut sim = sim_on(range_map(), "competitive", no_lagcomp());
    sim.rules.cfg.freeze_time = 1.0;
    let a = sim
        .add_player("Att", Controller::Human, Some(Team::Attack))
        .unwrap();
    let d = sim
        .add_player("Def", Controller::Human, Some(Team::Defense))
        .unwrap();
    sim.force_start();
    let mut guard = 0;
    while sim.rules.phase != Phase::Live {
        idle(&mut sim, 1);
        guard += 1;
        assert!(guard < 128 * 30, "round never went live");
    }
    assert_eq!(sim.bomb, BombState::Carried(a));
    let site = Vec3::new(-800.0, 1.0, -1600.0);
    teleport(&mut sim, a, site);
    teleport(&mut sim, d, Vec3::new(-800.0, 1.0, -1550.0));
    let mut cmds: Vec<Option<UserCmd>> = (0..64).map(|_| None).collect();
    cmds[a.0 as usize] = Some(UserCmd {
        weapon_select: 5,
        ..Default::default()
    });
    sim.step(&mut cmds);
    for _ in 0..(128.0 * 3.4) as u32 {
        cmds[a.0 as usize] = Some(UserCmd {
            buttons_start: Buttons(Buttons::ATTACK),
            ..Default::default()
        });
        sim.step(&mut cmds);
    }
    assert!(
        matches!(sim.bomb, BombState::Planted { .. }),
        "planted: {:?}",
        sim.bomb
    );
    let (events, _) = sim.drain();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::BombPlanted { .. }))
    );
    for _ in 0..(128.0 * 10.2) as u32 {
        cmds[d.0 as usize] = Some(UserCmd {
            buttons_start: Buttons(Buttons::USE),
            ..Default::default()
        });
        sim.step(&mut cmds);
    }
    assert!(
        matches!(sim.bomb, BombState::Defused(_)),
        "defused: {:?}",
        sim.bomb
    );
    let (events, _) = sim.drain();
    assert!(events.iter().any(|e| matches!(
        e,
        GameEvent::RoundEnded {
            winner: Team::Defense,
            ..
        }
    )));
}

fn bot_match(
    map: &str,
    profile: &str,
    bots: usize,
    seconds: f32,
    seed: u64,
) -> (Sim, Vec<GameEvent>) {
    let d = data();
    let level = Arc::new(Level::build(d.maps[map].clone(), &d.materials, &d.movement).unwrap());
    let mut rules = d.rules_profile(profile).unwrap();
    rules.warmup_time = 0.0;
    let mut sim = Sim::new(d, level, rules, SimConfig::default(), seed).unwrap();
    for i in 0..bots {
        sim.add_player(&format!("Bot {i}"), Controller::Bot, None)
            .unwrap();
    }
    if sim.mode() == Mode::Competitive {
        sim.force_start();
    }
    let mut all = Vec::new();
    let mut cmds: Vec<Option<UserCmd>> = (0..64).map(|_| None).collect();
    for _ in 0..(seconds * 128.0) as u32 {
        sim.step(&mut cmds);
        let (e, _) = sim.drain();
        all.extend(e);
    }
    (sim, all)
}

#[test]
fn deathmatch_bots_fight() {
    let (sim, events) = bot_match("ps_citadel", "deathmatch", 6, 90.0, 7);
    let kills = events
        .iter()
        .filter(|e| matches!(e, GameEvent::Kill { .. }))
        .count();
    assert!(kills > 0, "bots must find and kill each other");
    assert!(sim.iter_players().any(|p| p.stats.shots > 0));
}

#[test]
fn competitive_5v5_bots_finish_a_round() {
    let (sim, events) = bot_match("ps_citadel", "competitive", 10, 150.0, 3);
    assert_eq!(sim.team_count(Team::Attack), 5);
    assert_eq!(sim.team_count(Team::Defense), 5);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameEvent::RoundEnded { .. })),
        "a round must end"
    );
}

#[test]
fn simulation_is_deterministic() {
    let (a, _) = bot_match("ps_citadel", "deathmatch", 4, 20.0, 11);
    let (b, _) = bot_match("ps_citadel", "deathmatch", 4, 20.0, 11);
    for (pa, pb) in a.iter_players().zip(b.iter_players()) {
        assert_eq!(pa.mv.origin, pb.mv.origin);
        assert_eq!(pa.health, pb.health);
    }
}

#[test]
#[ignore = "diagnostic: prints bot match statistics"]
fn bot_match_report() {
    let t = std::time::Instant::now();
    let (sim, events) = bot_match("ps_citadel", "competitive", 10, 600.0, 3);
    let rounds = events
        .iter()
        .filter(|e| matches!(e, GameEvent::RoundEnded { .. }))
        .count();
    let kills = events
        .iter()
        .filter(|e| matches!(e, GameEvent::Kill { .. }))
        .count();
    let plants = events
        .iter()
        .filter(|e| matches!(e, GameEvent::BombPlanted { .. }))
        .count();
    for e in &events {
        if let GameEvent::RoundEnded { winner, reason, .. } = e {
            println!("round: {winner:?} {reason:?}");
        }
    }
    for p in sim.iter_players() {
        println!(
            "{} {:?} k{} d{} shots {} hits {} ${}",
            p.name, p.team, p.stats.kills, p.stats.deaths, p.stats.shots, p.stats.hits, p.money
        );
    }
    println!(
        "rounds {rounds} kills {kills} plants {plants} score {:?} in {:?}",
        sim.rules.scores,
        t.elapsed()
    );
}

#[test]
fn replay_reproduces_the_match() {
    use game_sim::record::{Recorder, ReplayHeader, SimInput, apply, play, state_hash};
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

fn rec_into_bytes(mut rec: game_sim::record::Recorder<Vec<u8>>) -> Vec<u8> {
    rec.flush().unwrap();
    rec.into_inner()
}
