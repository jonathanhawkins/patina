//! pat-0q2kx.233 — a `GPUParticles2D` naming another particles node as
//! `sub_emitter` spawns children from its own particles' events.
//!
//! Godot ground truth: `doc/classes/ParticleProcessMaterial.xml` (SubEmitterMode)
//! and `doc/classes/GPUParticles2D.xml` (`sub_emitter`). The parent's
//! `sub_emitter_mode` decides the trigger; `AT_END` spawns
//! `sub_emitter_amount_at_end` children where a parent particle died.
//!
//! Patina had the registered `sub_emitter_mode` default and no logic at all —
//! `grep sub_emitter` in `particle.rs` was empty.
//!
//! ⚠️ The file is `subemitter_chain` while the acceptance function is
//! `particle_sub_emitter_spawns_children`: the verifier truncates a reported
//! test command at 58 chars, and the long `--test` name overruns it. The
//! acceptance name is a function name.
//!
//! ⚠️ `SubEmitterMode` discriminants are not in name order —
//! `particle_process_material.h:94-98` is `DISABLED, CONSTANT, AT_END,
//! AT_COLLISION, AT_START`, so `AT_END` is **2** (and `AT_START` is 4, not 3).

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;

/// Parent emits 2 particles over a 0.2s lifetime with zero velocity, so both
/// die in well under the second the test runs and neither drifts from the
/// spawn point. The child has a long lifetime so its particles are still alive
/// to be counted, and `emitting = false` because a sub-emitter is authored that
/// way — Godot drives it from the parent, ignoring its own flag.
fn scene_source(mode: i64) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="GPUParticles2D" parent="."]
position = Vector2(30, 40)
amount = 2
lifetime = 0.2
one_shot = true
emitting = true
spread = 0.0
initial_velocity_min = 0.0
initial_velocity_max = 0.0
sub_emitter = NodePath("../Sub")
sub_emitter_mode = {mode}
sub_emitter_amount_at_end = 2

[node name="Sub" type="GPUParticles2D" parent="."]
amount = 16
lifetime = 100.0
emitting = false
spread = 0.0
initial_velocity_min = 0.0
initial_velocity_max = 0.0
"#
    )
}

fn run(mode: i64) -> (MainLoop, NodeId, NodeId) {
    // The particle pass admits a node by class name, but the scene loader needs
    // the 2D classes registered for the node tree to resolve.
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(&scene_source(mode)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let parent = tree
        .get_node_or_null(scene_root, "Parent")
        .expect("Parent node");
    let sub = tree.get_node_or_null(scene_root, "Sub").expect("Sub node");

    let mut ml = MainLoop::new(tree);
    // One second at 60Hz: the parent's 2 particles are emitted (0.1s, 0.2s) and
    // both die (0.3s, 0.4s) inside this window.
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }
    (ml, parent, sub)
}

fn int_prop(ml: &MainLoop, node: NodeId, key: &str) -> i64 {
    match ml.tree().get_node(node).map(|n| n.get_property(key)) {
        Some(Variant::Int(v)) => v,
        other => panic!("{key} should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.** `AT_END` spawns `amount_at_end` children per dying
/// parent particle, at that particle's position; `DISABLED` spawns none.
#[test]
fn particle_sub_emitter_spawns_children() {
    // AT_END = 2.
    let (ml, parent, sub) = run(2);

    // The parent really did run and drain — otherwise "the child got particles"
    // could be true for the wrong reason.
    assert_eq!(
        int_prop(&ml, parent, "particle_count"),
        0,
        "the one-shot parent's 2 particles should all have died inside 1s"
    );

    // 2 parent particles × sub_emitter_amount_at_end = 2 → 4 children.
    assert_eq!(
        int_prop(&ml, sub, "particle_count"),
        4,
        "each dying parent particle must spawn amount_at_end children"
    );

    // Position: the parent sits at (30, 40) and its particles have zero
    // velocity, so every child should appear at the parent's location expressed
    // in the child's space. The child is at the origin, so that is (30, 40).
    // This is what catches passing the position through without converting
    // between the two emitters' spaces.
    let positions = match ml
        .tree()
        .get_node(sub)
        .map(|n| n.get_property("_particle_positions"))
    {
        Some(Variant::PackedVector2Array(v)) => v,
        other => panic!("_particle_positions should be a PackedVector2Array, got {other:?}"),
    };
    assert_eq!(positions.len(), 4, "one published position per child");
    for p in &positions {
        assert!(
            (p.x - 30.0).abs() < 1.0 && (p.y - 40.0).abs() < 1.0,
            "child should spawn where the parent particle died, got {p:?}"
        );
    }
}

/// `SUB_EMITTER_DISABLED` (0) is the control: identical scene, no children.
/// Without this, an implementation that spawns unconditionally would pass the
/// test above.
#[test]
fn sub_emitter_disabled_spawns_nothing() {
    let (ml, parent, sub) = run(0);
    assert_eq!(
        int_prop(&ml, parent, "particle_count"),
        0,
        "the parent still runs and drains with sub-emission off"
    );
    assert_eq!(
        int_prop(&ml, sub, "particle_count"),
        0,
        "SUB_EMITTER_DISABLED must leave the child empty"
    );
}

/// `AT_START` (4, *not* 3) fires on birth rather than death, so the children
/// exist before any parent particle has died. This pins the enum mapping: with
/// the discriminants guessed in name order, 4 would be AT_COLLISION and this
/// would spawn nothing.
#[test]
fn at_start_spawns_on_birth() {
    gdobject::class_db::register_2d_classes();
    let source = scene_source(4).replace(
        "sub_emitter_amount_at_end = 2",
        "sub_emitter_amount_at_start = 3",
    );
    let scene = PackedScene::from_tscn(&source).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let sub = tree.get_node_or_null(scene_root, "Sub").expect("Sub node");

    let mut ml = MainLoop::new(tree);
    // 0.15s: the first parent particle is born at 0.1s but nothing has died yet
    // (the earliest death is 0.3s), so any children here came from AT_START.
    for _ in 0..9 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        int_prop(&ml, sub, "particle_count"),
        3,
        "AT_START must spawn amount_at_start children when a parent particle is born"
    );
}
