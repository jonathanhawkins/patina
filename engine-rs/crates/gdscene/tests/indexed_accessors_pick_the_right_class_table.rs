
//! pat-mgixk.31 (increment 4): an indexed accessor must resolve against the
//! table for the node's OWN class, and `CPUParticles3D.set_param_min/max`.
//!
//! ## 🔑 The regression this exists to catch
//!
//! `set_param` is **overloaded** in Godot. `Light3D` binds it, and so do
//! `PinJoint3D`, `HingeJoint3D`, `SliderJoint3D` and `ConeTwistJoint3D` — 26
//! registered properties between them, each class with its own param enum and
//! its own property names (`params/bias`, `angular_limit/upper`, …).
//!
//! Increment 1 of this bead added an indexed `set_param` arm that was **not
//! gated by class**, so it answered a joint's `set_param(PARAM_BIAS, 0.3)` out
//! of the *Light3D* table and wrote `light_energy` onto the joint. Property bags
//! accept any key, so the write succeeded silently. Those joint properties were
//! already served by the generic reverse lookup *before* that arm existed, which
//! makes it a regression rather than a missing feature — the worst kind, because
//! nothing failed.
//!
//! The first assertion below is that guard. It would have caught it.
//!
//! ## And the arity hole underneath it
//!
//! The generic `Accessor::Set` arm wrote `args[0]` for any call to a registered
//! setter. For an indexed call `args[0]` is the **index**, so every indexed
//! accessor without a table — a joint's `set_param`, `CPUParticles2D`'s
//! `set_param_min`, `set_surface_override_material` — stored an index where a
//! value belonged. It now requires exactly one argument and declines otherwise,
//! so a missing table is a loud unknown-method instead of a plausible-looking
//! wrong number.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Values chosen to differ from every registered default they could land on:
/// `initial_velocity_min`/`_max` default to 0.0 and `scale_amount_min`/`_max`
/// to 1.0, so neither assertion can be satisfied by an untouched property.
const VELOCITY_MIN: f64 = 3.5;
const SCALE_MAX: f64 = 2.25;
/// What the joint probe tries to write. Must not appear on the joint at all.
const JOINT_VALUE: f64 = 7.0;

const STAGES: [&str; 4] = [
    "$Fx.set_param_min(CPUParticles3D.PARAM_INITIAL_LINEAR_VELOCITY, 3.5)",
    "$Fx.set_param_max(CPUParticles3D.PARAM_SCALE, 2.25)",
    "$Fx.get_param_min(CPUParticles3D.PARAM_INITIAL_LINEAR_VELOCITY)",
    "$Fx.get_param_max(CPUParticles3D.PARAM_SCALE)",
];

/// ⚠️ `PARAM_SCALE` maps to `scale_amount_*`, not `scale_*` — the one stem in
/// the table that is not its lower-cased constant name. It is in here on
/// purpose: a table built by lower-casing the enum would pass every other
/// assertion in this file and fail only on that pair.
const PARTICLES: &str = "\
extends Node3D
var stage = 0
var vel_read = 0.0
var scale_read = 0.0
var ran = false

func _ready():
    $Fx.set_param_min(CPUParticles3D.PARAM_INITIAL_LINEAR_VELOCITY, 3.5)
    stage = 1
    $Fx.set_param_max(CPUParticles3D.PARAM_SCALE, 2.25)
    stage = 2
    vel_read = $Fx.get_param_min(CPUParticles3D.PARAM_INITIAL_LINEAR_VELOCITY)
    stage = 3
    scale_read = $Fx.get_param_max(CPUParticles3D.PARAM_SCALE)
    stage = 4
    ran = true
";

/// The joint probe lives on its own node with the joint as its child, so that
/// `set_param` declining — which raises and aborts this handler — cannot stop
/// the particle script above from completing.
///
/// Nothing asserts whether this handler finishes. Whether a joint's indexed
/// `set_param` is served is a separate open question (see the report); what must
/// hold either way is that it does not write a LIGHT property onto a joint.
const JOINT_PROBE: &str = "\
extends Node3D
var reached_end = false

func _ready():
    $Joint.set_param(0, 7.0)
    reached_end = true
";

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn as_f64(v: Variant, what: &str) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{what} should be a number, got {other:?}"),
    }
}

#[test]
fn indexed_accessors_pick_the_right_class_table() {
    // ⚠️ Needed HERE: nextest runs each `#[test]` in its own process. Both the
    // `CPUParticles3D.PARAM_*` constants and the dispatch gates'
    // `is_parent_class` lookups read this registry, and `is_parent_class`
    // answers FALSE against an empty one — so every accessor would decline.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    let fx = tree
        .add_child(root, Node::new("Fx", "CPUParticles3D"))
        .unwrap();
    let probe = tree.add_child(root, Node::new("Probe", "Node3D")).unwrap();
    let joint = tree
        .add_child(probe, Node::new("Joint", "PinJoint3D"))
        .unwrap();

    let particles = GDScriptNodeInstance::from_source(PARTICLES, root).expect("script parses");
    tree.attach_script(root, Box::new(particles));
    let jp = GDScriptNodeInstance::from_source(JOINT_PROBE, probe).expect("probe parses");
    tree.attach_script(probe, Box::new(jp));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(root);
    ml.tree_mut().process_script_ready(probe);

    // -- 1. 🔑 THE REGRESSION GUARD -----------------------------------------
    //
    // A PinJoint3D descends from Joint3D, not Light3D, and registers no
    // `light_energy`. With the Light3D arm ungated, `set_param(0, 7.0)` on it
    // resolved index 0 out of the light table and wrote `light_energy = 7.0`.
    assert_ne!(
        ml.tree()
            .get_node(joint)
            .expect("the Joint node")
            .get_property("light_energy"),
        Variant::Float(JOINT_VALUE),
        "a joint's set_param must NOT be answered from the Light3D param \
         table. `light_energy` = {JOINT_VALUE} on a PinJoint3D means the \
         indexed light arm is not gated by class — it wrote a light property \
         onto a joint, silently, because a property bag accepts any key"
    );

    // -- 2. the particle script completed -----------------------------------
    let stage = match script_var(ml.tree(), root, "stage") {
        Variant::Int(i) => i as usize,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        script_var(ml.tree(), root, "ran"),
        Variant::Bool(true),
        "`_ready` aborted after stage {stage} ({}), so the call that raised is \
         `{}`. An UndefinedVariable on CPUParticles3D.PARAM_* looks the same \
         from here as an undispatched set_param_min",
        STAGES
            .get(stage.saturating_sub(1))
            .copied()
            .unwrap_or("(none yet)"),
        STAGES.get(stage).copied().unwrap_or("(unknown)"),
    );

    let node = ml.tree().get_node(fx).expect("the Fx node");

    // -- 3. the write landed on the property the simulation reads ------------
    //
    // `main_loop.rs:414` reads `initial_velocity_min` into the frame's
    // `ParticleEmitter`, so this is the key the engine actually consumes.
    assert!(
        (as_f64(
            node.get_property("initial_velocity_min"),
            "initial_velocity_min"
        ) - VELOCITY_MIN)
            .abs()
            < 1e-9,
        "set_param_min(PARAM_INITIAL_LINEAR_VELOCITY, {VELOCITY_MIN}) must land \
         on `initial_velocity_min`; got {:?}",
        node.get_property("initial_velocity_min")
    );

    // -- 4. 🔑 PARAM_SCALE is `scale_amount_*`, not `scale_*` ----------------
    assert!(
        (as_f64(node.get_property("scale_amount_max"), "scale_amount_max") - SCALE_MAX).abs()
            < 1e-9,
        "set_param_max(PARAM_SCALE, {SCALE_MAX}) must land on \
         `scale_amount_max`. A stem table built by lower-casing the constant \
         name would write `scale_max`, which nothing reads; got {:?}",
        node.get_property("scale_amount_max")
    );

    // -- 5. `_min` and `_max` are distinguished ------------------------------
    assert_ne!(
        node.get_property("initial_velocity_max"),
        Variant::Float(VELOCITY_MIN),
        "set_param_MIN must not also write the `_max` side — both default to \
         0.0, so a suffix bug would be invisible without this"
    );
    assert_ne!(
        node.get_property("scale_amount_min"),
        Variant::Float(SCALE_MAX),
        "set_param_MAX must not also write the `_min` side"
    );

    // -- 6. the index decodes, rather than every param aliasing the first ----
    assert_ne!(
        node.get_property("angular_velocity_min"),
        Variant::Float(VELOCITY_MIN),
        "PARAM_INITIAL_LINEAR_VELOCITY is index 0 and PARAM_ANGULAR_VELOCITY is \
         1; writing both means the index is being ignored"
    );

    // -- 7. the getters answer the stored values -----------------------------
    assert!(
        (as_f64(script_var(ml.tree(), root, "vel_read"), "vel_read") - VELOCITY_MIN).abs() < 1e-9,
        "get_param_min(PARAM_INITIAL_LINEAR_VELOCITY) must answer the stored \
         {VELOCITY_MIN}, not Nil"
    );
    assert!(
        (as_f64(script_var(ml.tree(), root, "scale_read"), "scale_read") - SCALE_MAX).abs() < 1e-9,
        "get_param_max(PARAM_SCALE) must answer the stored {SCALE_MAX}"
    );
}
