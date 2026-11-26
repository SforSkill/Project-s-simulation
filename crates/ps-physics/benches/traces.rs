//! Criterion benchmark of the collision queries used every tick.

use criterion::{Criterion, criterion_group, criterion_main};
use engine_core::Aabb;
use engine_physics::world::TraceMask;
use engine_physics::{Brush, CollisionWorld, MaterialId};
use glam::Vec3;
use std::hint::black_box;

fn grid_world() -> CollisionWorld {
    let mut brushes = vec![Brush::from_box(
        Aabb::new(
            Vec3::new(-4096.0, -16.0, -4096.0),
            Vec3::new(4096.0, 0.0, 4096.0),
        ),
        MaterialId(0),
    )];
    for i in 0..20 {
        for j in 0..20 {
            let c = Vec3::new(i as f32 * 400.0 - 4000.0, 0.0, j as f32 * 400.0 - 4000.0);
            brushes.push(Brush::from_box(
                Aabb::new(c, c + Vec3::new(64.0, 128.0, 64.0)),
                MaterialId(1),
            ));
        }
    }
    CollisionWorld::new(brushes)
}

fn bench(c: &mut Criterion) {
    let w = grid_world();
    let mins = Vec3::new(-16.0, 0.0, -16.0);
    let maxs = Vec3::new(16.0, 72.0, 16.0);
    c.bench_function("trace_box_player_step", |b| {
        b.iter(|| {
            w.trace_box(
                black_box(Vec3::new(10.0, 1.0, 10.0)),
                black_box(Vec3::new(12.0, 1.0, 11.0)),
                mins,
                maxs,
                TraceMask::Players,
            )
        })
    });
    c.bench_function("trace_ray_long", |b| {
        b.iter(|| {
            w.trace_ray(
                black_box(Vec3::new(-3900.0, 64.0, 10.0)),
                black_box(Vec3::new(3900.0, 64.0, 30.0)),
                TraceMask::Bullets,
            )
        })
    });
    let mut segs = Vec::new();
    c.bench_function("ray_segments_long", |b| {
        b.iter(|| {
            w.ray_segments(
                black_box(Vec3::new(-3900.0, 64.0, 10.0)),
                Vec3::X,
                7800.0,
                &mut segs,
            );
            segs.len()
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
