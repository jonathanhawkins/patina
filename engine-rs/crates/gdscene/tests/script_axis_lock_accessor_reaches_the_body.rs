
//! pat-mgixk.31 (increment 3): a GDScript call to
//! `PhysicsBody3D.set_axis_lock(axis, lock)` reaches the property the physics
//! solver reads, and `PhysicsServer3D.BodyAxis` is nameable from a script.
//!
//! `rigidbody3d_axis_locks` (pat-s3rrv.105) already proves the six
//! `axis_lock_*` **properties** freeze a body when authored from Rust. This is
//! the other half: the *script-callable* accessor. Two things blocked it.
//!
//! 1. All six properties share ONE `get_axis_lock`/`set_axis_lock` pair, so the
//!    generic reverse-lookup bridge resolves with `next_back()` and would
//!    answer a 0-arg `get_axis_lock()` with `axis_lock_angular_z` — and a
//!    2-arg `set_axis_lock` by writing `args[0]`, the AXIS FLAG, as the value.
//!    Indexed handling has to come first.
//! 2. `PhysicsServer3D` was registered with **zero** constants, so
//!    `PhysicsServer3D.BODY_AXIS_LINEAR_Y` was an `UndefinedVariable` and no
//!    script could name the axis however the call dispatched.
//!
//! ⚠️ The flags are **powers of two** (1, 2, 4, 8, 16, 32), not `0..5`. A
//! positional table would put `LINEAR_Y` in `linear_z`'s slot and shift every
//! axis after it — which is why the "a different axis is still unlocked"
//! assertion matters more than the headline one.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Where the body starts. Free fall is the observable, so this has to be high
/// enough that 60 ticks of gravity move the control measurably.
const START_Y: f32 = 5.0;
const TICKS: usize = 60;

/// Each statement bumps `stage`, so an abort names the call that raised rather
/// than leaving the physics assertions to fail for an unrelated-looking reason.
const STAGES: [&str; 3] = [
    "$Body.set_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_Y, true)",
    "$Body.get_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_Y)",
    "$Body.get_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_X)",
];

const SCRIPT: &str = "\
extends Node3D
var stage = 0
var locked_y = false
var locked_x = true
var ran = false

func _ready():
    $Body.set_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_Y, true)
    stage = 1
    locked_y = $Body.get_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_Y)
    stage = 2
    locked_x = $Body.get_axis_lock(PhysicsServer3D.BODY_AXIS_LINEAR_X)
    stage = 3
    ran = true
";

/// A world containing exactly ONE falling `RigidBody3D`, optionally with a
/// script on its parent.
///
/// No floor: this is about integration, not contact, and a floor would stop the
/// control case for a reason unrelated to the flag under test.
///
/// ⚠️ **One body per tree, and that is the whole point of this helper.** My
/// first version put the control and the locked body in the *same* tree at the
/// same position — two radius-0.5 spheres interpenetrating. Depenetration then
/// pushed the locked body UP by 0.4986, almost exactly the radius, while the
/// control fell normally. The lock was working perfectly; the fixture was
/// measuring contact resolution, and the failure read as "the script's write
/// never reached the property". `rigidbody3d_axis_locks` builds a fresh tree per
/// case for exactly this reason.
fn world(script: Option<&str>) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let body = tree
        .add_child(root, Node::new("Body", "RigidBody3D"))
        .unwrap();
    // Set through `node3d` for the same reason `y_of` reads through it.
    gdscene::node3d::set_position(&mut tree, body, Vector3::new(0.0, START_Y, 0.0));
    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_string()));
    shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(body, shape).unwrap();

    if let Some(src) = script {
        let instance = GDScriptNodeInstance::from_source(src, root).expect("script parses");
        tree.attach_script(root, Box::new(instance));
    }

    (MainLoop::new(tree), root, body)
}

/// ⚠️ Read through `node3d::get_position`, NOT `get_property("position")`. A
/// Node3D carries a transform and `node3d` reads that when one exists, ignoring
/// the raw property; the solver publishes back with `node3d::set_position`, so
/// the raw property is stale and a test reading it would report "the body never
/// moved" against a perfectly working engine.
fn y_of(ml: &MainLoop, id: NodeId) -> f32 {
    gdscene::node3d::get_position(ml.tree(), id).y
}

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

#[test]
fn script_axis_lock_accessor_reaches_the_body() {
    // ⚠️ Mandatory, and needed HERE because nextest runs each `#[test]` in its
    // own process. Two separate things read this registry: the
    // `PhysicsServer3D.BODY_AXIS_*` constant lookup, and the dispatch gate's
    // `is_parent_class("RigidBody3D", "PhysicsBody3D")` — which answers FALSE
    // against an empty registry, so the accessor would decline and the script
    // would raise.
    gdobject::class_db::register_all_classes();

    // -- the control, FIRST: if gravity were not reaching the solver, the
    //    locked body would also be motionless and would "pass" while proving
    //    nothing ------------------------------------------------------------
    let (mut control, _, free) = world(None);
    for _ in 0..TICKS {
        control.step(1.0 / 60.0);
    }
    let free_y = y_of(&control, free);
    assert!(
        free_y < START_Y - 0.5,
        "the unlocked control must FALL, or every assertion below is vacuous: \
         y={free_y} after {TICKS} ticks from {START_Y}"
    );

    // -- the same world, with a script that freezes Y -----------------------
    let (mut ml, root, locked) = world(Some(SCRIPT));
    ml.tree_mut().process_script_ready(root);

    let stage = match script_var(ml.tree(), root, "stage") {
        Variant::Int(i) => i as usize,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        script_var(ml.tree(), root, "ran"),
        Variant::Bool(true),
        "`_ready` aborted after stage {stage} ({}), so the call that raised is \
         `{}`. An UndefinedVariable on PhysicsServer3D.BODY_AXIS_* looks the \
         same from here as an undispatched set_axis_lock",
        STAGES
            .get(stage.saturating_sub(1))
            .copied()
            .unwrap_or("(none yet)"),
        STAGES.get(stage).copied().unwrap_or("(unknown)"),
    );

    for _ in 0..TICKS {
        ml.step(1.0 / 60.0);
    }

    // -- 🔑 the script-locked body did not move on Y -------------------------
    //
    // EXACTLY unmoved, not near-zero: the lock is applied to velocity before
    // integration, and a tolerance would accept the broken ordering where a
    // "locked" axis creeps one step per frame (~2.7mm/frame under default
    // gravity, which reads as solver noise rather than a bug).
    let locked_y_pos = y_of(&ml, locked);
    assert_eq!(
        locked_y_pos, START_Y,
        "set_axis_lock(BODY_AXIS_LINEAR_Y, true) called from GDScript must \
         freeze the body on Y. The control fell to {free_y}, so gravity ran. \
         A value NEAR {START_Y} but above it (≈ +0.5, the sphere radius) means \
         something is touching this body and the fixture has a contact in it; \
         a value near the control's means the script's write never reached \
         `axis_lock_linear_y`, which physics_server_3d.rs reads every tick"
    );

    // -- the getter answers the stored value ---------------------------------
    assert_eq!(
        script_var(ml.tree(), root, "locked_y"),
        Variant::Bool(true),
        "get_axis_lock(BODY_AXIS_LINEAR_Y) must report the lock just set",
    );

    // -- 🔑 a DIFFERENT axis is still unlocked -------------------------------
    //
    // This is what pins the flag decoding. The flags are 1/2/4/8/16/32, so a
    // positional 0..5 table, or an implementation that ignores the argument and
    // locks everything, satisfies the assertions above and fails only here.
    assert_eq!(
        script_var(ml.tree(), root, "locked_x"),
        Variant::Bool(false),
        "get_axis_lock(BODY_AXIS_LINEAR_X) must still be FALSE — locking Y must \
         not lock X. Equal to `true` means the axis argument is being ignored",
    );

    // -- ...and it landed on the right property, not merely round-tripped ----
    //
    // The six properties are the contract with the solver. Asserting only
    // through the getter would pass even if both directions shared one slot
    // nothing else reads.
    let node = ml.tree().get_node(locked).expect("the Body node");
    assert_eq!(
        node.get_property("axis_lock_linear_y"),
        Variant::Bool(true),
        "the write must land on `axis_lock_linear_y` itself"
    );
    assert_ne!(
        node.get_property("axis_lock_linear_x"),
        Variant::Bool(true),
        "`axis_lock_linear_x` must NOT have been written — flag 2 is Y, and a \
         positional table would have put it in X's or Z's slot"
    );
    assert_ne!(
        node.get_property("axis_lock_angular_z"),
        Variant::Bool(true),
        "`axis_lock_angular_z` must NOT have been written — it is what the \
         generic reverse lookup's `next_back()` would reach for the shared \
         `set_axis_lock` name, so this catches the indexed case being skipped"
    );
}
