use criterion::{Criterion, criterion_group, criterion_main};
use engine_core::Aabb;
use engine_core::cmd::{Buttons, UserCmd};
use engine_movement::{MoveState, MovementConfig, simulate_cmd};
use engine_physics::{Brush, CollisionWorld, MaterialId};
use glam::Vec3;

fn bench(c: &mut Criterion) {
    let mut b = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-4000.0, -32.0, -4000.0),
            Vec3::new(4000.0, 0.0, 4000.0),
        ),
        MaterialId(0),
    )];
    for i in 0..30 {
        let z = -200.0 - i as f32 * 16.0;
        b.push(Brush::from_box(
            Aabb::new(
                Vec3::new(-200.0, 0.0, -4000.0),
                Vec3::new(200.0, 16.0 * (i + 1) as f32, z),
            ),
            MaterialId(0),
        ));
    }
    let w = CollisionWorld::new(b);
    let cfg = MovementConfig::default();
    let cmd = UserCmd {
        buttons_start: Buttons(Buttons::FORWARD | Buttons::RIGHT),
        ..Default::default()
    };
    c.bench_function("player_tick_128hz_stairs", |bch| {
        let mut s = MoveState::new(Vec3::ZERO);
        bch.iter(|| {
            simulate_cmd(&mut s, &cmd, 250.0, false, 1.0 / 128.0, &cfg, &w);
            if s.origin.z < -600.0 {
                s = MoveState::new(Vec3::ZERO);
            }
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
