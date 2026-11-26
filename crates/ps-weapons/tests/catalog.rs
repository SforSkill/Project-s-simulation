//! Tests against the shipped `configs/weapons.toml`.

use engine_core::DetRng;
use engine_weapons::damage::apply_armor;
use engine_weapons::*;

fn catalog() -> Catalog {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../configs/weapons.toml");
    Catalog::from_toml(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn catalog_loads_with_every_required_type() {
    let c = catalog();
    for k in [
        WeaponKind::Knife,
        WeaponKind::Pistol,
        WeaponKind::Smg,
        WeaponKind::Rifle,
        WeaponKind::SniperLight,
        WeaponKind::SniperHeavy,
        WeaponKind::Bomb,
    ] {
        assert!(c.first_of(k).is_some(), "{k:?}");
    }
    for g in [
        GrenadeKind::Flash,
        GrenadeKind::Smoke,
        GrenadeKind::Molotov,
        GrenadeKind::He,
    ] {
        assert!(c.grenade(g).is_some(), "{g:?}");
    }
}

#[test]
fn zero_spread_standing_and_at_threshold() {
    let c = catalog();
    for w in c.weapons.iter().filter(|w| w.is_gun()) {
        for zoom in 0..=w.zoom.levels.len() as u8 {
            assert_eq!(movement_spread_deg(w, 0.0, true, zoom), 0.0, "{}", w.id);
            assert_eq!(
                movement_spread_deg(w, w.max_speed * w.accurate_fraction, true, zoom),
                0.0,
                "{}",
                w.id
            );
        }
        assert!(
            movement_spread_deg(w, w.max_speed, true, 0) > 0.0,
            "{}",
            w.id
        );
        assert!(movement_spread_deg(w, 0.0, false, 0) > 0.0, "{} air", w.id);
        // Monotonic in speed.
        let mut last = 0.0;
        for s in 0..=50 {
            let v = movement_spread_deg(w, w.max_speed * s as f32 / 50.0, true, 0);
            assert!(v >= last);
            last = v;
        }
    }
}

#[test]
fn spread_offset_is_zero_without_spread_and_bounded_otherwise() {
    let mut rng = DetRng::new(7);
    assert_eq!(spread_offset(0.0, &mut rng), (0.0, 0.0));
    for _ in 0..10_000 {
        let (p, y) = spread_offset(3.0, &mut rng);
        assert!((p * p + y * y).sqrt() <= 3.0 + 1e-4);
    }
}

#[test]
fn first_shot_is_exact_for_every_gun() {
    let c = catalog();
    for w in c.weapons.iter().filter(|w| w.is_gun()) {
        assert_eq!(recoil_offset(w, 0.0), (0.0, 0.0), "{}", w.id);
    }
}

#[test]
fn reference_kills() {
    let c = catalog();
    let get = |n: &str| c.get(c.by_name(n).unwrap()).clone();
    let rifle = get("rifle");
    let heavy = get("sniper_heavy");
    let light = get("sniper_light");
    let pistol = get("pistol");
    let raw =
        |w: &WeaponDef, g: HitGroup, d: f32| w.damage * g.multiplier(w) * distance_falloff(w, d);
    // Rifle: one headshot kills through a helmet at medium range.
    assert!(
        apply_armor(
            raw(&rifle, HitGroup::Head, 1000.0),
            HitGroup::Head,
            100,
            true,
            rifle.armor_ratio
        )
        .0 >= 100
    );
    // Heavy sniper: one body shot kills through kevlar.
    assert!(
        apply_armor(
            raw(&heavy, HitGroup::Chest, 2000.0),
            HitGroup::Chest,
            100,
            true,
            heavy.armor_ratio
        )
        .0 >= 100
    );
    // Light sniper: a headshot kills, a body shot through kevlar does not.
    assert!(
        apply_armor(
            raw(&light, HitGroup::Head, 1500.0),
            HitGroup::Head,
            100,
            true,
            light.armor_ratio
        )
        .0 >= 100
    );
    assert!(
        apply_armor(
            raw(&light, HitGroup::Chest, 1500.0),
            HitGroup::Chest,
            100,
            true,
            light.armor_ratio
        )
        .0 < 100
    );
    // Pistol: headshot kills an unhelmeted player at close range.
    assert!(
        apply_armor(
            raw(&pistol, HitGroup::Head, 200.0),
            HitGroup::Head,
            100,
            false,
            pistol.armor_ratio
        )
        .0 >= 100
    );
    // Damage is deterministic.
    assert_eq!(
        raw(&rifle, HitGroup::Chest, 777.0),
        raw(&rifle, HitGroup::Chest, 777.0)
    );
}

#[test]
fn snipers_have_scopes_and_unscoped_penalty() {
    let c = catalog();
    for k in [WeaponKind::SniperLight, WeaponKind::SniperHeavy] {
        let w = c.get(c.first_of(k).unwrap());
        assert!(!w.zoom.levels.is_empty());
        assert!(
            movement_spread_deg(w, w.max_speed, true, 0)
                > movement_spread_deg(w, w.max_speed, true, 1)
        );
    }
}

#[test]
fn automatic_sprays_climb_and_snipers_reset_between_shots() {
    let c = catalog();
    for w in c.weapons.iter().filter(|w| w.is_gun()) {
        let mut h = HandState::default();
        let mut g = GunState::full(w);
        let mut out = Vec::new();
        let mut t = 0.0;
        let mut prev = engine_core::cmd::Buttons(0);
        // Hold (or tap at the cycle rate) for 10 shots.
        for i in 0..2000 {
            let b = if w.automatic || i % 2 == 0 {
                engine_core::cmd::Buttons(engine_core::cmd::Buttons::ATTACK)
            } else {
                engine_core::cmd::Buttons(0)
            };
            let dt = if w.automatic {
                1.0 / 128.0
            } else {
                w.cycle_time as f64 / 2.0 + 1e-4
            };
            h.update(
                w,
                &mut g,
                &WeaponInput {
                    t0: t,
                    t1: t + dt,
                    buttons: b,
                    prev,
                    can_fire: true,
                },
                &mut out,
            );
            prev = b;
            t += dt;
            if out
                .iter()
                .filter(|e| matches!(e, WeaponEvent::Shot { .. }))
                .count()
                >= 4
            {
                break;
            }
        }
        let idx: Vec<f32> = out
            .iter()
            .filter_map(|e| {
                if let WeaponEvent::Shot { recoil_index, .. } = e {
                    Some(*recoil_index)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(idx.len(), 4, "{}", w.id);
        match w.kind {
            WeaponKind::SniperLight | WeaponKind::SniperHeavy => {
                assert!(idx.iter().all(|&i| i == 0.0), "{} {idx:?}", w.id)
            }
            _ => assert!(idx[3] > idx[1] && idx[1] > 0.0, "{} {idx:?}", w.id),
        }
    }
}
