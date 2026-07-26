//! pat-0q2kx.154 (partial) — `GPUParticles3D`/`CPUParticles3D` simulate in 3D
//! and respond to their authored `Vector3` gravity.
//!
//! ⚠️ This does NOT satisfy the whole acceptance. The clause it does not cover
//! is billboard RENDERING in the editor 3D viewport frame — `rg particle` in
//! `render_server_3d.rs` and `viewport_3d_render.rs` matches nothing, so 3D
//! particles are still never drawn. See the completion report.
//!
//! A complete 1096-line `ParticleSimulator3D` — `Vector3` direction/gravity,
//! `Particle3D`, emitters, GPU instances — already existed with ZERO consumers,
//! while both 3D particle classes were routed to the **2D** simulator. That
//! simulator reads `direction`/`gravity` as `Variant::Vector2`, so a 3D node's
//! `Vector3` gravity never matched the pattern and the emitter silently kept a
//! 2D default: the particles fell the wrong way, or not at all, with no error
//! anywhere.

use gdcore::math::{Vector2, Vector3};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// An emitting particle node of `class` with the given properties.
///
/// `amount`/`lifetime` are load-bearing, not arbitrary. The simulators emit one
/// particle every `lifetime / amount` seconds (`particle3d.rs`, the
/// `emit_interval` loop), so a long lifetime with a small amount emits nothing
/// at all over a short run: `amount = 8, lifetime = 10.0` is one particle per
/// **1.25 s**, and these tests step 10–30 frames (0.17–0.5 s). Every assertion
/// then reads an empty particle set — `min` over nothing is `inf`, which is
/// exactly how this first failed.
///
/// 32 over 2.0 s is one particle every 62.5 ms: several are alive within a few
/// frames, and none expire during the run.
fn particle_scene(class: &str, props: &[(&str, Variant)]) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("P", class)).unwrap();
    {
        let n = tree.get_node_mut(node).unwrap();
        n.set_property("emitting", Variant::Bool(true));
        n.set_property("amount", Variant::Int(32));
        n.set_property("lifetime", Variant::Float(2.0));
        for (k, v) in props {
            n.set_property(k, v.clone());
        }
    }
    (MainLoop::new(tree), node)
}

fn step(ml: &mut MainLoop, frames: usize) {
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
}

/// A 3D emitter simulates, and its published positions are 3D.
#[test]
fn particles3d_simulate_and_publish_3d_positions() {
    let (mut ml, node) = particle_scene(
        "GPUParticles3D",
        &[
            ("direction", Variant::Vector3(Vector3::new(0.0, 1.0, 0.0))),
            ("initial_velocity_min", Variant::Float(5.0)),
            ("initial_velocity_max", Variant::Float(5.0)),
            ("gravity", Variant::Vector3(Vector3::new(0.0, -9.8, 0.0))),
            ("spread", Variant::Float(0.0)),
        ],
    );
    step(&mut ml, 10);

    assert!(
        ml.particle_count(node) > 0,
        "an emitting GPUParticles3D must simulate particles"
    );
    let positions = ml.particle_positions_3d(node);
    assert!(
        !positions.is_empty(),
        "the 3D simulator must publish 3D positions"
    );

    // The published property must be a Vector3 array — the 2D path published
    // `PackedVector2Array`, which is what made 3D particles unrenderable in 3D.
    match ml
        .tree()
        .get_node(node)
        .unwrap()
        .get_property("_particle_positions")
    {
        Variant::PackedVector3Array(v) => assert_eq!(v.len(), positions.len()),
        other => panic!("3D particles must publish a PackedVector3Array, got {other:?}"),
    }
}

/// The acceptance's core claim: the authored `Vector3` gravity drives the
/// motion. Launched upward, particles must decelerate and come back down.
#[test]
fn vector3_gravity_pulls_particles_down() {
    let props = |gravity: Vector3| {
        vec![
            ("direction", Variant::Vector3(Vector3::new(0.0, 1.0, 0.0))),
            ("initial_velocity_min", Variant::Float(5.0)),
            ("initial_velocity_max", Variant::Float(5.0)),
            ("gravity", Variant::Vector3(gravity)),
            ("spread", Variant::Float(0.0)),
        ]
    };

    // Strong downward gravity.
    let (mut heavy, hn) = particle_scene("GPUParticles3D", &props(Vector3::new(0.0, -50.0, 0.0)));
    step(&mut heavy, 30);
    let heavy_y = heavy
        .particle_positions_3d(hn)
        .iter()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);

    // No gravity at all: the same launch keeps rising.
    let (mut floaty, fname) = particle_scene("GPUParticles3D", &props(Vector3::ZERO));
    step(&mut floaty, 30);
    let floaty_y = floaty
        .particle_positions_3d(fname)
        .iter()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);

    assert!(
        heavy_y < floaty_y,
        "downward Vector3 gravity must pull particles below the ungravitied \
         case; heavy min y={heavy_y}, floaty min y={floaty_y}"
    );
}

/// Gravity on the Z axis moves particles in Z — the axis a 2D simulator cannot
/// represent at all, so this fails outright on the old path.
#[test]
fn gravity_on_the_z_axis_is_honored() {
    let (mut ml, node) = particle_scene(
        "CPUParticles3D",
        &[
            ("gravity", Variant::Vector3(Vector3::new(0.0, 0.0, -20.0))),
            ("initial_velocity_min", Variant::Float(0.0)),
            ("initial_velocity_max", Variant::Float(0.0)),
        ],
    );
    step(&mut ml, 30);
    let min_z = ml
        .particle_positions_3d(node)
        .iter()
        .map(|p| p.z)
        .fold(f32::INFINITY, f32::min);
    assert!(
        min_z < -0.01,
        "gravity on Z must move particles along Z, got min z={min_z}"
    );
}

/// 2D particle nodes still use the 2D simulator and still publish
/// `PackedVector2Array` — the split must not disturb them.
#[test]
fn particles2d_are_unaffected() {
    let (mut ml, node) = particle_scene(
        "GPUParticles2D",
        &[
            ("direction", Variant::Vector2(Vector2::new(0.0, -1.0))),
            ("initial_velocity_min", Variant::Float(50.0)),
            ("initial_velocity_max", Variant::Float(50.0)),
        ],
    );
    step(&mut ml, 10);
    assert!(
        ml.particle_count(node) > 0,
        "a 2D emitter must still simulate"
    );
    match ml
        .tree()
        .get_node(node)
        .unwrap()
        .get_property("_particle_positions")
    {
        Variant::PackedVector2Array(_) => {}
        other => panic!("2D particles must still publish PackedVector2Array, got {other:?}"),
    }
    // ...and it must NOT have been given a 3D simulator.
    assert!(
        ml.particle_positions_3d(node).is_empty(),
        "a 2D node must not get a 3D simulator"
    );
}
