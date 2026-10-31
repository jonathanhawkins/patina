//! pat-mgixk.68 — a Godot-authored particle scene's material parameters reach
//! the simulators.
//!
//! Godot saves a particle system's sub-emitter trigger, collision response and
//! attractor opt-out on the `ParticleProcessMaterial`
//! (`doc/classes/ParticleProcessMaterial.xml`), i.e. on the
//! `[sub_resource type="ParticleProcessMaterial"]`, never on the emitter node.
//!
//! Patina's flattener copied exactly eight keys off that resource — direction,
//! gravity, spread, initial_velocity_min/max, color, scale_min/max — while
//! `MainLoop` reads a dozen more straight off the NODE: `sub_emitter_mode`,
//! `sub_emitter_amount_at_end`, `collision_mode`, `collision_friction`,
//! `collision_bounce`, `attractor_interaction_enabled`. Its comments asserted
//! that this flattener landed them there. It did not.
//!
//! # Why the gap read as "done"
//!
//! ⚠️ Every existing green test authors those keys **on the node**
//! (`subemitter_chain.rs:47` sets `sub_emitter_mode` on the GPUParticles2D;
//! `particle_attractors_and_collision_in_scene.rs:248` sets `collision_mode`
//! with `set_property`). So the simulators were genuinely correct and genuinely
//! tested — but nothing reachable from an editor-saved scene ever set the
//! inputs. The feature worked in tests and silently did nothing in a game.
//! That is the shape this file exists to close: the keys are authored ONLY on
//! the material here.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;

/// A parent emitter whose sub-emitter trigger lives ONLY on its material.
///
/// Mirrors `subemitter_chain.rs`'s timing so the observable is the same: 2
/// particles over a 0.2s lifetime with zero velocity, so both die well inside
/// the one second the test runs and neither drifts from the spawn point. The
/// child has a long lifetime so its particles are still alive to count.
///
/// ⚠️ `SubEmitterMode` discriminants are not in name order
/// (`particle_process_material.h`: DISABLED, CONSTANT, AT_END, AT_COLLISION,
/// AT_START), so AT_END is **2**.
fn scene_source(mode: i64) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
spread = 0.0
initial_velocity_min = 0.0
initial_velocity_max = 0.0
sub_emitter_mode = {mode}
sub_emitter_amount_at_end = 2

[node name="World" type="Node2D"]

[node name="Parent" type="GPUParticles2D" parent="."]
position = Vector2(30, 40)
amount = 2
lifetime = 0.2
one_shot = true
emitting = true
sub_emitter = NodePath("../Sub")
process_material = SubResource("ParticleProcessMaterial_1")

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

/// A 3D emitter whose collision and attractor settings live ONLY on its
/// material, alongside the friction/bounce the collision response reads.
const COLLISION_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
collision_mode = 1
collision_friction = 0.25
collision_bounce = 0.5
attractor_interaction_enabled = false
lifetime_randomness = 0.3
damping_min = 1.5

[node name="Emitter" type="GPUParticles3D"]
amount = 4
lifetime = 1.0
process_material = SubResource("ParticleProcessMaterial_1")
"#;

/// The acceptance says these keys must not appear in any node section. Asserted
/// rather than trusted, so the test cannot later be "fixed" by moving a key
/// onto the node — which is exactly the mistake that let this gap survive.
fn assert_keys_only_on_the_material(source: &str, keys: &[&str]) {
    let node_sections = &source[source
        .find("[node ")
        .expect("the fixture must declare at least one node")..];
    for key in keys {
        assert!(
            !node_sections.contains(key),
            "`{key}` must appear ONLY on the material sub_resource, never in a \
             node section — otherwise this test measures the node path that \
             already worked instead of the material path under test"
        );
    }
}

fn run_subemitter(mode: i64) -> (MainLoop, NodeId, NodeId) {
    // The particle pass admits a node by class name, but the scene loader needs
    // the 2D classes registered for the node tree to resolve.
    gdobject::class_db::register_2d_classes();

    let source = scene_source(mode);
    assert_keys_only_on_the_material(&source, &["sub_emitter_mode", "sub_emitter_amount_at_end"]);

    let scene = PackedScene::from_tscn(&source).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let parent = tree
        .get_node_or_null(scene_root, "Parent")
        .expect("Parent node");
    let sub = tree.get_node_or_null(scene_root, "Sub").expect("Sub node");

    let mut ml = MainLoop::new(tree);
    // One second at 60Hz: the parent's 2 particles are emitted and both die
    // inside this window.
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

/// **The acceptance test.** A sub-emitter trigger authored only on the material
/// spawns children.
#[test]
fn particle_material_params_from_godot_tscn() {
    // AT_END = 2, amount_at_end = 2, both only on the material.
    let (ml, parent, sub) = run_subemitter(2);

    // The parent really ran and drained, or "the child got particles" could be
    // true for the wrong reason.
    assert_eq!(
        int_prop(&ml, parent, "particle_count"),
        0,
        "the one-shot parent's 2 particles should all have died inside 1s"
    );

    // 2 parent particles x sub_emitter_amount_at_end = 2 -> 4 children.
    //
    // ⚠️ This is the discriminating number. At HEAD the flattener copies only
    // eight keys, so `sub_emitter_mode` never reaches the node, `MainLoop`
    // reads the registered default (DISABLED) and this is **0**. Asserting
    // "> 0" would still catch that, but 4 also pins `amount_at_end` as having
    // come from the material rather than a default of 1.
    assert_eq!(
        int_prop(&ml, sub, "particle_count"),
        4,
        "each dying parent particle must spawn amount_at_end children when the \
         trigger is authored on the ParticleProcessMaterial. 0 means the \
         material's sub_emitter_mode never reached the emitter — the \
         pat-mgixk.68 bug."
    );
}

/// The control: the same scene with the mode DISABLED on the material spawns
/// none.
///
/// ⚠️ Without this, the test above would pass for any implementation that
/// spawned children whenever a material was present. This proves the material's
/// VALUE is read, not merely its existence.
#[test]
fn a_disabled_mode_on_the_material_spawns_no_children() {
    let (ml, _parent, sub) = run_subemitter(0);
    assert_eq!(
        int_prop(&ml, sub, "particle_count"),
        0,
        "SUB_EMITTER_DISABLED on the material must spawn nothing; children here \
         would mean the flattener's value is ignored and the trigger is driven \
         by the material merely existing"
    );
}

/// Collision, attractor and motion parameters authored only on the material
/// reach the emitter node, which is where every simulator reads them.
///
/// ⚠️ This asserts the flattened node state rather than a bounce trajectory on
/// purpose. The simulators' response to these values is already covered for
/// node-authored scenes by `particle_attractors_and_collision_in_scene.rs`;
/// what was missing — and all this bead changes — is the material-to-node hop.
/// Re-simulating a bounce here would re-test their work and would not
/// distinguish my change from theirs.
#[test]
fn collision_and_attractor_params_flatten_from_the_material() {
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();

    assert_keys_only_on_the_material(
        COLLISION_SCENE,
        &[
            "collision_mode",
            "collision_friction",
            "collision_bounce",
            "attractor_interaction_enabled",
            "lifetime_randomness",
            "damping_min",
        ],
    );

    let scene = PackedScene::from_tscn(COLLISION_SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let emitter = tree.get_node(scene_root).map(|_| scene_root).unwrap();

    let prop = |key: &str| {
        tree.get_node(emitter)
            .map(|n| n.get_property(key))
            .unwrap_or(Variant::Nil)
    };

    assert_eq!(
        prop("collision_mode"),
        Variant::Int(1),
        "`collision_mode` must reach the node: it is read at main_loop.rs as a \
         NODE property, so a scene that declares it only on the material got \
         the default and never collided"
    );
    assert!(
        matches!(prop("collision_friction"), Variant::Float(f) if (f - 0.25).abs() < 1e-6),
        "`collision_friction` must reach the node, got {:?}",
        prop("collision_friction")
    );
    assert!(
        matches!(prop("collision_bounce"), Variant::Float(f) if (f - 0.5).abs() < 1e-6),
        "`collision_bounce` must reach the node, got {:?}",
        prop("collision_bounce")
    );
    // ⚠️ A FALSE that must survive. The opt-out is the interesting direction:
    // the default is true, so a flattener that dropped the key would leave
    // attractors engaged and the scene would look "fine" while ignoring the
    // author's instruction.
    assert_eq!(
        prop("attractor_interaction_enabled"),
        Variant::Bool(false),
        "`attractor_interaction_enabled = false` must reach the node; Nil or \
         true means the author's opt-out was dropped"
    );
    // ⚠️ These two reach the NODE but are NOT yet consumed: `main_loop.rs` has
    // no reader for either (grep for them there returns nothing), even though
    // the simulators carry the fields. So this asserts the flatten only — do
    // not read it as "lifetime randomisation and damping now work". Wiring the
    // readers is the other half of pat-mgixk.68 and lives in `main_loop.rs`,
    // which was under another agent's exclusive lease; it is reported as
    // remaining rather than claimed. They are asserted here because the generic
    // copy carries them for free, and pinning that now means the reader work is
    // a one-line change with a test already waiting for it.
    assert!(
        matches!(prop("lifetime_randomness"), Variant::Float(f) if (f - 0.3).abs() < 1e-6),
        "`lifetime_randomness` must reach the node, got {:?}",
        prop("lifetime_randomness")
    );
    assert!(
        matches!(prop("damping_min"), Variant::Float(f) if (f - 1.5).abs() < 1e-6),
        "`damping_min` must reach the node, got {:?}",
        prop("damping_min")
    );
}
