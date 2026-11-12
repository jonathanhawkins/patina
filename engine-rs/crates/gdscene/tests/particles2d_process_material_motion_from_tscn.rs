//! pat-mgixk.67 acceptance — a Godot-authored `GPUParticles2D` actually moves,
//! scales and colours the way its `ParticleProcessMaterial` says.
//!
//! Godot ground truth: `ParticleProcessMaterial.direction` and `.gravity` are
//! **Vector3** for the 2D emitter as well as the 3D one
//! (`doc/classes/ParticleProcessMaterial.xml`); `scale_min`/`scale_max` size each
//! particle, and `color_ramp` (a `GradientTexture1D`) colours it over its life
//! (`scene/resources/particle_process_material.cpp`).
//!
//! ⚠️ EVERY ONE OF THESE FAILED SILENTLY, and the type is the whole story.
//! `packed_scene` flattens the material onto the node UNCHANGED — gravity and
//! direction as `Variant::Vector3` (pinned by
//! `particle_process_material_flattens.rs`) — while the 2D emitter block in
//! `MainLoop::process_particles` matched only `Variant::Vector2`. So the match
//! fell through and the emitter kept Patina's defaults: gravity ZERO, direction
//! UP. `scale_min/max` and `__color_ramp_*` were read only by
//! `read_emitter_3d`, so a 2D emitter got constant scale and flat colour too.
//! Nothing errored; the particles just sat there.
//!
//! 🔑 WHY `initial_velocity` IS ZERO IN THE FIXTURE. It makes gravity the ONLY
//! thing that can move a particle. With the bug, gravity is dropped and the
//! initial velocity is zero, so every particle stays at y = 0 forever — the
//! assertion below cannot pass by accident on some other source of motion.
//!
//! Entry point, per the bead's Wiring requirement: `.tscn` -> PackedScene
//! flatten -> `MainLoop::step` -> `ParticleSimulator` -> the published
//! `_particle_*` node properties a renderer reads. Nothing calls the simulator
//! directly.

use gdcore::math::{Color, Vector2};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The shape Godot writes: `color_ramp` is typed Texture2D, so the editor
/// assigns a `GradientTexture1D` whose own `gradient` holds the stops — the
/// stops are TWO hops from the material.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3]

[sub_resource type="Gradient" id="Gradient_1"]
offsets = PackedFloat32Array(0, 1)
colors = PackedColorArray(1, 0, 0, 1, 0, 0, 1, 1)

[sub_resource type="GradientTexture1D" id="GradientTexture1D_1"]
gradient = SubResource("Gradient_1")

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
gravity = Vector3(0, 400, 0)
direction = Vector3(0, 1, 0)
spread = 0.0
initial_velocity_min = 0.0
initial_velocity_max = 0.0
scale_min = 3.0
scale_max = 3.0
color_ramp = SubResource("GradientTexture1D_1")

[node name="Emitter" type="GPUParticles2D"]
process_material = SubResource("ParticleProcessMaterial_1")
amount = 16
lifetime = 1.0
"#;

/// 45 frames at 1/60 = 0.75 s. ⚠️ Chosen so that with `amount = 16` over a
/// `lifetime` of 1.0 s roughly a dozen particles are alive with ages spread
/// across 0..0.75 — enough separation for the old-vs-young colour comparison,
/// and short enough that none has died and been recycled.
const FRAMES: usize = 45;
const DT: f64 = 1.0 / 60.0;

fn main_loop_with_scene() -> (MainLoop, gdscene::node::NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (MainLoop::new(tree), emitter)
}

fn positions(ml: &MainLoop, id: gdscene::node::NodeId) -> Vec<Vector2> {
    match ml
        .tree()
        .get_node(id)
        .expect("emitter exists")
        .get_property("_particle_positions")
    {
        Variant::PackedVector2Array(v) => v,
        other => panic!("_particle_positions must be a PackedVector2Array, got {other:?}"),
    }
}

fn colors(ml: &MainLoop, id: gdscene::node::NodeId) -> Vec<Color> {
    match ml
        .tree()
        .get_node(id)
        .expect("emitter exists")
        .get_property("_particle_colors")
    {
        Variant::PackedColorArray(v) => v,
        other => panic!("_particle_colors must be a PackedColorArray, got {other:?}"),
    }
}

fn scales(ml: &MainLoop, id: gdscene::node::NodeId) -> Vec<f32> {
    match ml
        .tree()
        .get_node(id)
        .expect("emitter exists")
        .get_property("_particle_scales")
    {
        Variant::PackedFloat32Array(v) => v,
        other => panic!("_particle_scales must be a PackedFloat32Array, got {other:?}"),
    }
}

#[test]
fn particles2d_process_material_motion_from_tscn() {
    let (mut ml, emitter) = main_loop_with_scene();

    // Fixture precondition: the material really was flattened, and as a
    // Vector3. If this were a Vector2 the test would be measuring the old
    // CPUParticles2D spelling and would pass without the fix.
    match ml
        .tree()
        .get_node(emitter)
        .expect("emitter exists")
        .get_property("gravity")
    {
        Variant::Vector3(g) => assert!(
            (g.y - 400.0).abs() < 0.001,
            "fixture: gravity must flatten as Vector3(0, 400, 0), got {g:?}"
        ),
        other => panic!(
            "fixture: `gravity` must reach the node as a Vector3 — that type is \
             exactly what this bead is about. Got {other:?}"
        ),
    }

    for _ in 0..FRAMES {
        ml.step(DT);
    }

    let pos = positions(&ml, emitter);
    let col = colors(&ml, emitter);
    let scl = scales(&ml, emitter);
    assert!(
        pos.len() >= 4,
        "the emitter must have published several live particles after {FRAMES} \
         frames; got {}. Zero means nothing emitted at all, which is a different \
         bug from the one under test",
        pos.len()
    );
    assert_eq!(
        (col.len(), scl.len()),
        (pos.len(), pos.len()),
        "the three published arrays must be parallel — they are indexed together \
         below"
    );

    // -- (a) gravity reached the solver ------------------------------------
    //
    // 🔑 The whole point. Initial velocity is zero, so with the Vector3 gravity
    // dropped every particle stays exactly at its spawn point and max_y is 0.
    let max_y = pos.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let min_y = pos.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(
        max_y > 20.0,
        "a particle must have accelerated DOWNWARD (+y in 2D) under the \
         material's gravity of 400: the furthest-fallen particle is at y={max_y}. \
         ~0 is the old behaviour — the Vector3 gravity never matched the \
         emitter's Vector2-only read, so the emitter used its default of ZERO \
         and nothing moved. Positions: {pos:?}"
    );
    assert!(
        min_y >= -0.001,
        "gravity is +400 (down), so no particle should be ABOVE its spawn \
         point; min y={min_y}. A negative value means the sign or the axis was \
         taken wrong when narrowing Vector3 to Vector2"
    );

    // -- (b) scale_min/scale_max reached the emitter ------------------------
    for (i, s) in scl.iter().enumerate() {
        assert!(
            (s - 3.0).abs() < 0.001,
            "every particle must be scaled 3.0 by the material's \
             scale_min/scale_max; particle {i} is {s}. 1.0 is the old behaviour \
             — those keys were read only by the 3D emitter. Scales: {scl:?}"
        );
    }

    // -- (c) the colour ramp shaped the particle over its life -------------
    //
    // ⚠️ Compared by AGE rather than by array index: the published order is the
    // simulator's draw order, which is not a contract. The particle that has
    // fallen furthest is the oldest, so it must be the bluest.
    let oldest = pos
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .map(|(i, _)| i)
        .expect("at least one particle");
    let youngest = pos
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .map(|(i, _)| i)
        .expect("at least one particle");
    assert!(
        col[oldest].b > col[youngest].b + 0.05,
        "the ramp runs red -> blue over each particle's life, so the oldest \
         particle (fallen to y={:.1}, blue={:.3}) must be bluer than the \
         youngest (y={:.1}, blue={:.3}). Equal blues mean the ramp never \
         reached the 2D emitter and every particle used the flat `color`, which \
         defaults to white",
        pos[oldest].y,
        col[oldest].b,
        pos[youngest].y,
        col[youngest].b
    );
    assert!(
        col[youngest].r > col[youngest].b,
        "and the youngest particle must still be mostly RED (r={:.3}, \
         b={:.3}) — if it were already blue the ramp would be sampled at the \
         wrong end of the life",
        col[youngest].r,
        col[youngest].b
    );
}
