//! Criterion benchmarks for the 3D software render pipeline.
//!
//! Establishes baselines for:
//! - draw call throughput (rendering N cube instances per frame)
//! - shadow map generation (directional light shadow maps over N instances)
//! - full frame rendering (lit scene end-to-end across viewport sizes)
//!
//! Run with: `./scripts/rust_task.sh bench --bench render_bench`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::{generate_shadow_maps, SoftwareRenderer3D};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

/// Generates a deterministic grid of cube transforms centered on the origin
/// and pushed back along -Z so the default forward camera sees them.
fn grid_transforms(count: usize) -> Vec<Transform3D> {
    let side = ((count as f32).sqrt().ceil() as usize).max(1);
    let spacing = 2.5_f32;
    let offset = (side as f32 - 1.0) * 0.5 * spacing;
    (0..count)
        .map(|i| {
            let x = (i % side) as f32 * spacing - offset;
            let y = ((i / side) % side) as f32 * spacing - offset;
            let z = -10.0 - ((i / (side * side)) as f32) * spacing;
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(x, y, z),
            }
        })
        .collect()
}

/// Builds a renderer populated with `count` lit cubes.
fn build_scene(count: usize) -> SoftwareRenderer3D {
    let mut renderer = SoftwareRenderer3D::new();
    let mesh = Mesh3D::cube(1.0);
    let material = Material3D::default();
    for transform in grid_transforms(count) {
        let id = renderer.create_instance();
        renderer.set_mesh(id, mesh.clone());
        renderer.set_material(id, material.clone());
        renderer.set_transform(id, transform);
    }
    renderer
}

/// Builds a scene with a directional key light so shadow and lighting
/// paths are exercised during full-frame rendering.
fn build_lit_scene(count: usize) -> SoftwareRenderer3D {
    let mut renderer = build_scene(count);
    let light_id = Light3DId(1);
    renderer.add_light(light_id);
    let mut light = Light3D::directional(light_id);
    light.direction = Vector3::new(-0.3, -1.0, -0.2);
    light.shadow_enabled = true;
    renderer.update_light(&light);
    renderer
}

/// Builds a standalone instance list used by the shadow map bench.
fn build_instances(count: usize) -> Vec<Instance3D> {
    let mesh = Mesh3D::cube(1.0);
    grid_transforms(count)
        .into_iter()
        .enumerate()
        .map(|(i, transform)| {
            let mut inst = Instance3D::new(Instance3DId(i as u64 + 1));
            inst.mesh = Some(mesh.clone());
            inst.material = Some(Material3D::default());
            inst.transform = transform;
            inst
        })
        .collect()
}

fn bench_draw_call_throughput(c: &mut Criterion) {
    let viewport = Viewport3D::new(64, 64);
    let mut group = c.benchmark_group("draw_call_throughput");
    for &count in &[4_usize, 16, 64] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut renderer = build_scene(count);
            b.iter(|| {
                let frame = renderer.render_frame(&viewport);
                criterion::black_box(frame);
            });
        });
    }
    group.finish();
}

fn bench_shadow_map_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("shadow_map_generation");
    for &count in &[4_usize, 16, 64] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let instances = build_instances(count);
            let mut light = Light3D::directional(Light3DId(1));
            light.direction = Vector3::new(-0.3, -1.0, -0.2);
            light.shadow_enabled = true;
            let lights = vec![light];
            b.iter(|| {
                let maps = generate_shadow_maps(&lights, &instances);
                criterion::black_box(maps);
            });
        });
    }
    group.finish();
}

fn bench_full_frame_rendering(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_frame_rendering");
    for &(w, h) in &[(64_u32, 64_u32), (128, 128), (256, 256)] {
        let viewport = Viewport3D::new(w, h);
        let id = BenchmarkId::from_parameter(format!("{}x{}", w, h));
        group.throughput(Throughput::Elements((w * h) as u64));
        group.bench_with_input(id, &viewport, |b, viewport| {
            let mut renderer = build_lit_scene(16);
            b.iter(|| {
                let frame = renderer.render_frame(viewport);
                criterion::black_box(frame);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_draw_call_throughput,
    bench_shadow_map_generation,
    bench_full_frame_rendering
);
criterion_main!(benches);
