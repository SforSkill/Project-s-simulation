use criterion::{Criterion, criterion_group, criterion_main};
use engine_hitreg::{Pose, hitboxes, ray_vs_hitboxes};
use glam::Vec3;
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    let pose = Pose {
        origin: Vec3::new(0.0, 0.0, -500.0),
        yaw: 30.0,
        duck: 0.0,
    };
    let hb = hitboxes(&pose);
    c.bench_function("ray_vs_hitboxes", |b| {
        b.iter(|| {
            ray_vs_hitboxes(
                black_box(&hb),
                black_box(Vec3::new(0.0, 64.0, 0.0)),
                black_box(Vec3::new(0.0, -0.01, -1.0).normalize()),
                8192.0,
            )
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
