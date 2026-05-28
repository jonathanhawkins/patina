//! Criterion benchmarks for the 3D physics pipeline.
//!
//! Establishes baselines for:
//! - broadphase throughput (stepping many non-overlapping bodies so the
//!   layer/mask filter + pair iteration dominates)
//! - narrowphase throughput (direct `test_collision` calls across a packed
//!   grid of shapes, exercising sphere/box/capsule/cylinder pairs)
//! - constraint solver throughput (stepping pin-joint chains of varying
//!   length so `apply_joint_constraints` dominates the frame)
//!
//! Run with: `./scripts/rust_task.sh bench --bench physics_bench`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gdcore::math::Vector3;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::collision::test_collision;
use gdphysics3d::joint::{Joint3D, JointId3D, PinJoint3D};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;

/// Build a world of `count` rigid sphere bodies laid out on a wide grid so
/// no pair actually overlaps. This isolates the broadphase/pair-iteration
/// cost from narrowphase work.
fn build_broadphase_world(count: usize) -> PhysicsWorld3D {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    let side = ((count as f32).sqrt().ceil() as usize).max(1);
    let spacing = 10.0_f32;
    for i in 0..count {
        let x = (i % side) as f32 * spacing;
        let y = ((i / side) % side) as f32 * spacing;
        let z = (i / (side * side)) as f32 * spacing;
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(x, y, z),
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        world.add_body(body);
    }
    world
}

/// Build a deterministic set of shape pairs that mixes every supported
/// primitive so the narrowphase bench exercises each `test_collision` arm.
fn build_narrowphase_pairs(count: usize) -> Vec<(Vector3, Shape3D, Vector3, Shape3D)> {
    let mut out = Vec::with_capacity(count);
    let side = ((count as f32).sqrt().ceil() as usize).max(1);
    let spacing = 1.5_f32;
    for i in 0..count {
        let x = (i % side) as f32 * spacing;
        let y = ((i / side) % side) as f32 * spacing;
        let pos_a = Vector3::new(x, y, 0.0);
        let pos_b = Vector3::new(x + 0.8, y + 0.4, 0.0);
        let (shape_a, shape_b) = match i % 4 {
            0 => (
                Shape3D::Sphere { radius: 1.0 },
                Shape3D::Sphere { radius: 1.0 },
            ),
            1 => (
                Shape3D::Sphere { radius: 1.0 },
                Shape3D::BoxShape {
                    half_extents: Vector3::new(0.5, 0.5, 0.5),
                },
            ),
            2 => (
                Shape3D::Sphere { radius: 1.0 },
                Shape3D::CapsuleShape {
                    radius: 0.5,
                    height: 1.0,
                },
            ),
            _ => (
                Shape3D::Sphere { radius: 1.0 },
                Shape3D::CylinderShape {
                    radius: 0.5,
                    height: 1.0,
                },
            ),
        };
        out.push((pos_a, shape_a, pos_b, shape_b));
    }
    out
}

/// Build a pin-joint chain of `count` bodies. The first body is static,
/// each subsequent body is pinned to its predecessor so the solver has
/// `count - 1` constraints to resolve each step.
fn build_joint_chain_world(count: usize) -> PhysicsWorld3D {
    let mut world = PhysicsWorld3D::new();
    let mut prev: Option<BodyId3D> = None;
    for i in 0..count {
        let body_type = if i == 0 {
            BodyType3D::Static
        } else {
            BodyType3D::Rigid
        };
        let body = PhysicsBody3D::new(
            BodyId3D(0),
            body_type,
            Vector3::new(i as f32 * 1.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 0.25 },
            1.0,
        );
        let id = world.add_body(body);
        if let Some(parent) = prev {
            let pin = PinJoint3D::new(JointId3D(0), parent, id);
            world.add_joint(Joint3D::Pin(pin));
        }
        prev = Some(id);
    }
    world
}

fn bench_broadphase_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("broadphase_step");
    for &count in &[16_usize, 64, 256] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut world = build_broadphase_world(count);
            b.iter(|| {
                world.step(1.0 / 60.0);
                criterion::black_box(world.body_count());
            });
        });
    }
    group.finish();
}

fn bench_narrowphase_collision(c: &mut Criterion) {
    let mut group = c.benchmark_group("narrowphase_collision");
    for &count in &[16_usize, 64, 256] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let pairs = build_narrowphase_pairs(count);
            b.iter(|| {
                for (pos_a, shape_a, pos_b, shape_b) in &pairs {
                    let result = test_collision(*pos_a, shape_a, *pos_b, shape_b);
                    criterion::black_box(result);
                }
            });
        });
    }
    group.finish();
}

fn bench_constraint_solver(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraint_solver");
    for &count in &[4_usize, 16, 64] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut world = build_joint_chain_world(count);
            b.iter(|| {
                world.step(1.0 / 60.0);
                criterion::black_box(world.joint_count());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_broadphase_step,
    bench_narrowphase_collision,
    bench_constraint_solver
);
criterion_main!(benches);
