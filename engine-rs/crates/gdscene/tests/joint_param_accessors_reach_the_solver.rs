//! pat-mgixk.31: the four Joint3D classes' indexed `set_param`/`get_param`,
//! plus HingeJoint3D's second family `set_flag`/`get_flag`.
//!
//! `set_param` is overloaded across **five** classes — Light3D and these four —
//! each with its own enum and, crucially, **its own property-name shape**. One
//! shared table cannot serve them, and that is what this file pins:
//!
//! | class | `PARAM_BIAS`-ish key | note |
//! |---|---|---|
//! | PinJoint3D | `params/bias` | prefixed |
//! | HingeJoint3D | `angular_limit/upper` | limit keys unsuffixed |
//! | SliderJoint3D | `angular_limit/upper_angle` | **same concept, `_angle` suffix** |
//! | ConeTwistJoint3D | `bias` | **bare, no prefix at all** |
//!
//! Lower-casing the constant name, or sharing one prefix across the joints, is
//! wrong for at least three of the four. Each assertion below therefore checks
//! both that the right key was written **and** that a neighbouring class's
//! spelling of the same concept was not.
//!
//! 🔑 Why SliderJoint3D's `*_motion/*` params are in here. ClassDB registers no
//! `PropertyInfo` for twelve of its twenty-two params, which looks like "unbacked
//! — writing them would be dead". It is not: `physics_server_3d.rs` reads **all
//! forty** joint keys through `float_prop_3d`/`truthy_prop_3d` when it syncs the
//! joint, supplying its own defaults. ClassDB registration governs
//! editor/reflection visibility, not whether a write is live. The real test for a
//! dead write is whether a consumer reads the key — so `PARAM_LINEAR_MOTION_SOFTNESS`
//! is asserted precisely because it is the unregistered-but-read case.
//!
//! No physics is stepped here, deliberately: this asserts the property contract
//! the solver reads, and a joint needs bodies and a world before its forces mean
//! anything. Adding either would introduce confounds that say nothing about
//! accessor dispatch.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Values picked to differ from every registered default they could land on
/// (`params/bias` 0.3, `angular_limit/upper` π/2, `angular_limit/upper_angle`
/// 0.0, `linear_limit/softness` 1.0, ConeTwist `bias` 0.3) and from each other,
/// so no assertion can be satisfied by an untouched property or a crossed wire.
const PIN: f64 = 0.3125;
const HINGE: f64 = 0.4375;
const SLIDER: f64 = 0.5625;
const MOTION: f64 = 0.6875;
const CONE: f64 = 0.8125;

const STAGES: [&str; 14] = [
    "$PinJ.set_param(PinJoint3D.PARAM_IMPULSE_CLAMP, …)",
    "$PinJ.get_param(PinJoint3D.PARAM_IMPULSE_CLAMP)",
    "$HingeJ.set_param(HingeJoint3D.PARAM_LIMIT_UPPER, …)",
    "$HingeJ.get_param(HingeJoint3D.PARAM_LIMIT_UPPER)",
    "$HingeJ.set_flag(HingeJoint3D.FLAG_USE_LIMIT, true)",
    "$HingeJ.get_flag(HingeJoint3D.FLAG_USE_LIMIT)",
    "$SliderJ.set_param(SliderJoint3D.PARAM_ANGULAR_LIMIT_UPPER, …)",
    "$SliderJ.get_param(SliderJoint3D.PARAM_ANGULAR_LIMIT_UPPER)",
    "$SliderJ.set_param(SliderJoint3D.PARAM_LINEAR_MOTION_SOFTNESS, …)",
    "$SliderJ.get_param(SliderJoint3D.PARAM_LINEAR_MOTION_SOFTNESS)",
    "$ConeJ.set_param(ConeTwistJoint3D.PARAM_BIAS, …)",
    "$ConeJ.get_param(ConeTwistJoint3D.PARAM_BIAS)",
    "$SliderJ.get_param(SliderJoint3D.PARAM_LINEAR_MOTION_DAMPING) (unwritten)",
    "$SliderJ.get_param(SliderJoint3D.PARAM_ANGULAR_MOTION_DAMPING) (unwritten)",
];

const SCRIPT: &str = "\
extends Node3D
var stage = 0
var pin_read = 0.0
var hinge_read = 0.0
var hinge_flag_read = false
var slider_read = 0.0
var motion_read = 0.0
var cone_read = 0.0
var unwritten_linear = -1.0
var unwritten_angular = -1.0
var ran = false

func _ready():
    $PinJ.set_param(PinJoint3D.PARAM_IMPULSE_CLAMP, 0.3125)
    stage = 1
    pin_read = $PinJ.get_param(PinJoint3D.PARAM_IMPULSE_CLAMP)
    stage = 2
    $HingeJ.set_param(HingeJoint3D.PARAM_LIMIT_UPPER, 0.4375)
    stage = 3
    hinge_read = $HingeJ.get_param(HingeJoint3D.PARAM_LIMIT_UPPER)
    stage = 4
    $HingeJ.set_flag(HingeJoint3D.FLAG_USE_LIMIT, true)
    stage = 5
    hinge_flag_read = $HingeJ.get_flag(HingeJoint3D.FLAG_USE_LIMIT)
    stage = 6
    $SliderJ.set_param(SliderJoint3D.PARAM_ANGULAR_LIMIT_UPPER, 0.5625)
    stage = 7
    slider_read = $SliderJ.get_param(SliderJoint3D.PARAM_ANGULAR_LIMIT_UPPER)
    stage = 8
    $SliderJ.set_param(SliderJoint3D.PARAM_LINEAR_MOTION_SOFTNESS, 0.6875)
    stage = 9
    motion_read = $SliderJ.get_param(SliderJoint3D.PARAM_LINEAR_MOTION_SOFTNESS)
    stage = 10
    $ConeJ.set_param(ConeTwistJoint3D.PARAM_BIAS, 0.8125)
    stage = 11
    cone_read = $ConeJ.get_param(ConeTwistJoint3D.PARAM_BIAS)
    stage = 12
    unwritten_linear = $SliderJ.get_param(SliderJoint3D.PARAM_LINEAR_MOTION_DAMPING)
    stage = 13
    unwritten_angular = $SliderJ.get_param(SliderJoint3D.PARAM_ANGULAR_MOTION_DAMPING)
    stage = 14
    ran = true
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

/// Asserts a node property equals `want`, with the key named in the message.
fn prop_is(tree: &SceneTree, id: NodeId, key: &str, want: f64, why: &str) {
    let got = tree.get_node(id).expect("node exists").get_property(key);
    assert!(
        matches!(got, Variant::Float(f) if (f - want).abs() < 1e-9),
        "`{key}` should be {want}: {why}. Got {got:?}"
    );
}

/// Asserts a node property was NOT written with `rejected` — the guard against
/// a neighbouring class's spelling of the same concept.
fn prop_is_not(tree: &SceneTree, id: NodeId, key: &str, rejected: f64, why: &str) {
    let got = tree.get_node(id).expect("node exists").get_property(key);
    assert!(
        !matches!(got, Variant::Float(f) if (f - rejected).abs() < 1e-9),
        "`{key}` must NOT hold {rejected}: {why}. Got {got:?}"
    );
}

#[test]
fn joint_param_accessors_reach_the_solver() {
    // ⚠️ Needed HERE: nextest runs each `#[test]` in its own process. Both the
    // `<Joint>.PARAM_*` constant lookups and every dispatch gate's
    // `is_parent_class` read this registry, and `is_parent_class` answers FALSE
    // against an empty one — so all four tables would decline and the script
    // would raise on its first statement.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    let pin = tree
        .add_child(root, Node::new("PinJ", "PinJoint3D"))
        .unwrap();
    let hinge = tree
        .add_child(root, Node::new("HingeJ", "HingeJoint3D"))
        .unwrap();
    let slider = tree
        .add_child(root, Node::new("SliderJ", "SliderJoint3D"))
        .unwrap();
    let cone = tree
        .add_child(root, Node::new("ConeJ", "ConeTwistJoint3D"))
        .unwrap();

    let script = GDScriptNodeInstance::from_source(SCRIPT, root).expect("script parses");
    tree.attach_script(root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(root);

    // -- 0. did `_ready` finish? --------------------------------------------
    let stage = match script_var(ml.tree(), root, "stage") {
        Variant::Int(i) => i as usize,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        script_var(ml.tree(), root, "ran"),
        Variant::Bool(true),
        "`_ready` aborted after stage {stage} ({}), so the call that raised is \
         `{}`. An UndefinedVariable on a <Joint>.PARAM_* constant looks the same \
         from here as an undispatched set_param",
        STAGES
            .get(stage.saturating_sub(1))
            .copied()
            .unwrap_or("(none yet)"),
        STAGES.get(stage).copied().unwrap_or("(unknown)"),
    );

    let t = ml.tree();

    // -- 1. PinJoint3D: prefixed keys, and the index decodes ----------------
    prop_is(
        t,
        pin,
        "params/impulse_clamp",
        PIN,
        "PARAM_IMPULSE_CLAMP is index 2 on PinJoint3D",
    );
    prop_is_not(
        t,
        pin,
        "params/bias",
        PIN,
        "index 2 must not write index 0's property; equal means the index is ignored",
    );

    // -- 2. HingeJoint3D: `angular_limit/upper`, NOT the slider spelling ----
    prop_is(
        t,
        hinge,
        "angular_limit/upper",
        HINGE,
        "HingeJoint3D's limit keys carry no `_angle` suffix",
    );
    prop_is_not(
        t,
        hinge,
        "angular_limit/upper_angle",
        HINGE,
        "that is SliderJoint3D's spelling; writing it here means one shared \
         table is serving both classes",
    );

    // -- 3. 🔑 HingeJoint3D's SECOND family: set_flag -----------------------
    assert_eq!(
        t.get_node(hinge)
            .expect("hinge")
            .get_property("angular_limit/enable"),
        Variant::Bool(true),
        "set_flag(FLAG_USE_LIMIT, true) must write `angular_limit/enable`. It \
         defaults to FALSE, so this cannot pass on an untouched property — and \
         a param-only table leaves this property unreachable from a script"
    );
    // ⚠️ `assert_ne!(.., Bool(true))`, NOT `assert_eq!(.., Bool(false))`. An
    // unwritten node property reads as **`Nil`**, not as its registered
    // default — the ClassDB default is applied by the accessor getter
    // (`joint_property_or_default`), never by `node.get_property()`. Asserting
    // equality with `false` here failed on `Nil` and said nothing about the
    // flag decoding, which is the thing this guards.
    assert_ne!(
        t.get_node(hinge)
            .expect("hinge")
            .get_property("motor/enable"),
        Variant::Bool(true),
        "FLAG_USE_LIMIT is 0 and FLAG_ENABLE_MOTOR is 1; both set means the \
         flag argument is being ignored"
    );
    assert_eq!(
        script_var(ml.tree(), root, "hinge_flag_read"),
        Variant::Bool(true),
        "get_flag(FLAG_USE_LIMIT) must report the flag just set",
    );

    // -- 4. SliderJoint3D: the `_angle` spelling, and NOT the hinge one -----
    prop_is(
        t,
        slider,
        "angular_limit/upper_angle",
        SLIDER,
        "PARAM_ANGULAR_LIMIT_UPPER is index 11 and spells the key with `_angle`",
    );
    prop_is_not(
        t,
        slider,
        "angular_limit/upper",
        SLIDER,
        "that is HingeJoint3D's spelling",
    );

    // -- 5. 🔑 the unregistered-but-READ slider param -----------------------
    //
    // ClassDB registers no PropertyInfo for this key, which is why a
    // registration-based notion of "backed" would have excluded it. The solver
    // reads it anyway (`float_prop_3d(tree, id, "linear_motion/softness", 1.0)`
    // in physics_server_3d.rs), so the write is live and must land.
    prop_is(
        t,
        slider,
        "linear_motion/softness",
        MOTION,
        "PARAM_LINEAR_MOTION_SOFTNESS is index 5; the solver reads this key even \
         though ClassDB registers no PropertyInfo for it",
    );
    prop_is_not(
        t,
        slider,
        "linear_limit/softness",
        MOTION,
        "index 5 is the MOTION softness, index 2 is the LIMIT softness",
    );

    // -- 6. ConeTwistJoint3D: BARE keys -------------------------------------
    prop_is(
        t,
        cone,
        "bias",
        CONE,
        "ConeTwistJoint3D's properties have no prefix at all",
    );
    prop_is_not(
        t,
        cone,
        "params/bias",
        CONE,
        "`params/bias` is PinJoint3D's spelling; writing it here means the \
         prefix was assumed rather than transcribed per class",
    );

    // -- 7. every getter answered the stored value, not Nil ------------------
    for (name, want) in [
        ("pin_read", PIN),
        ("hinge_read", HINGE),
        ("slider_read", SLIDER),
        ("motion_read", MOTION),
        ("cone_read", CONE),
    ] {
        let got = as_f64(script_var(ml.tree(), root, name), name);
        assert!(
            (got - want).abs() < 1e-9,
            "`{name}` should read back {want}, got {got} — a getter answering \
             Nil reads as `no such property` to the interpreter"
        );
    }

    // -- 8. 🔑 an UNWRITTEN motion param answers the SOLVER's default, not Nil
    //
    // These twelve `*_motion/*` / `*_ortho/*` params had no `PropertyInfo` at
    // all, which made them look unbacked. They were always live — the solver
    // reads every one through `float_prop_3d` — but with nothing registered
    // there was no default to fall back to, so `get_param` on an unwritten one
    // answered `Nil` while the solver used its own hardcoded value. Two
    // different answers for one parameter.
    //
    // ⚠️ Asserted as a PAIR, and that is the point: linear is **0.0** while
    // angular is **1.0** (Godot's own asymmetry, which the solver comments on
    // at `physics_server_3d.rs:1469`). A single shared fallback constant — the
    // obvious way to paper over the `Nil` — satisfies exactly one of these.
    //
    // `as_f64` panics on `Nil`, so the pre-registration behaviour fails here
    // loudly rather than comparing equal to anything.
    let unwritten_linear = as_f64(
        script_var(ml.tree(), root, "unwritten_linear"),
        "unwritten_linear",
    );
    assert!(
        (unwritten_linear - 0.0).abs() < 1e-9,
        "get_param(PARAM_LINEAR_MOTION_DAMPING) on an unwritten property must \
         answer the registered default 0.0 — the same value \
         `float_prop_3d(tree, id, \"linear_motion/damping\", 0.0)` gives the \
         solver. Got {unwritten_linear}"
    );
    let unwritten_angular = as_f64(
        script_var(ml.tree(), root, "unwritten_angular"),
        "unwritten_angular",
    );
    assert!(
        (unwritten_angular - 1.0).abs() < 1e-9,
        "get_param(PARAM_ANGULAR_MOTION_DAMPING) must answer 1.0, NOT the 0.0 \
         its linear counterpart answers. Equal to 0.0 means one shared fallback \
         is standing in for twelve per-key defaults. Got {unwritten_angular}"
    );
}
