//! pat-s3rrv.158 — `SkeletonModifier3D._process_modification` is dispatched
//! from the skeleton's modifier pass.
//!
//! Godot ground truth: a custom `SkeletonModifier3D` overrides
//! `_process_modification` and writes bone poses from it; the skeleton runs the
//! virtual during its modifier pass, gated on `active`.
//!
//! ⚠️ **The virtual was already DECLARED** — `class_db.rs:1742` registers it,
//! with a comment calling it *"the pose-update hook itself — the seam the
//! constraint/twist solvers will run from once they exist"*. Nothing called it,
//! so a script overriding it did nothing and reported nothing. Declaring an
//! override point the engine never invokes is the failure this bead's family is
//! about; this test exists because the call now happens.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, process_modifiers, process_modifiers_with_delta, set_bone_rest,
};
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records that the pass reached it.
const MODIFIER_GD: &str = "\
extends SkeletonModifier3D
var ran = 0

func _process_modification():
    self.ran += 1
";

/// The same script body without the override — the fall-through case.
const NO_OVERRIDE_GD: &str = "\
extends SkeletonModifier3D
var ran = 0
";

fn skeleton_with_child(child_class: &str, script: &str, active: bool) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");

    let b0 = add_bone(&mut tree, skel, "root");
    set_bone_rest(
        &mut tree,
        skel,
        b0,
        Transform3D {
            origin: Vector3::new(0.0, 1.0, 0.0),
            ..Transform3D::IDENTITY
        },
    );

    let mut node = Node::new("Custom", child_class);
    node.set_property("active", Variant::Bool(active));
    let child = tree.add_child(skel, node).expect("modifier added");
    tree.attach_script(
        child,
        Box::new(GDScriptNodeInstance::from_source(script, child).unwrap()),
    );
    (tree, child)
}

fn ran(tree: &SceneTree, node: NodeId) -> i64 {
    let v = tree
        .get_script(node)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == "ran")
                .map(|p| p.default_value)
        })
        .expect("the script exposes `ran`");
    match v {
        Variant::Int(n) => n,
        other => panic!("ran: {other:?}"),
    }
}

/// **The acceptance.** The override runs during the skeleton pass.
#[test]
fn a_custom_modifier_runs_its_process_modification() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", MODIFIER_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();

    assert_eq!(ran(&tree, child), 0, "nothing has run before the pass");
    process_modifiers(&mut tree, skel);
    assert_eq!(
        ran(&tree, child),
        1,
        "the skeleton's modifier pass must invoke `_process_modification` — \
         0 means the declared virtual is still an override point nothing calls"
    );
}

/// 🔑 `active = false` skips it, like every built-in modifier.
///
/// ⚠️ Without this the dispatch would run a disabled modifier's script anyway,
/// which is worse than not running it at all: the node reads as off in the
/// inspector while still writing poses every frame.
#[test]
fn an_inactive_modifier_does_not_run() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", MODIFIER_GD, false);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers(&mut tree, skel);
    assert_eq!(
        ran(&tree, child),
        0,
        "🔑 a modifier with `active = false` must be skipped entirely"
    );
}

/// 🔑 A script without the override falls through untouched.
///
/// ⚠️ This is the guard on the `continue` the dispatch uses. The override
/// REPLACES the built-in behaviour when present, so the engine must be able to
/// tell "the script ran" from "there was no override" — `call_script_method`
/// returns `Some(_)` only in the first case. If those were conflated, every
/// scripted modifier would swallow its own class's built-in handling.
#[test]
fn a_modifier_without_the_override_is_left_to_the_builtin_path() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", NO_OVERRIDE_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers(&mut tree, skel);
    assert_eq!(
        ran(&tree, child),
        0,
        "there is no override to run, and the pass must not invent one"
    );
}

/// 🔑 The hook belongs to SkeletonModifier3D, not to any child of a Skeleton3D.
///
/// ⚠️ A plain `Node3D` carrying the same method name must NOT be called. Godot
/// runs this virtual for modifiers; dispatching it on every skeleton child
/// would turn an unrelated `_process_modification` on some other node into a
/// per-frame callback nobody asked for.
#[test]
fn a_non_modifier_child_is_not_dispatched() {
    let (mut tree, child) = skeleton_with_child("Node3D", MODIFIER_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers(&mut tree, skel);
    assert_eq!(
        ran(&tree, child),
        0,
        "🔑 only SkeletonModifier3D-derived children get the hook"
    );
}

/// Overrides the CURRENT hook, which receives the frame delta.
const WITH_DELTA_GD: &str = "\
extends SkeletonModifier3D
var ran = 0
var got_delta = 0.0

func _process_modification_with_delta(delta):
    self.ran += 1
    self.got_delta = delta
";

/// Overrides BOTH hooks, to pin which one wins.
const BOTH_GD: &str = "\
extends SkeletonModifier3D
var ran = 0
var legacy_ran = 0

func _process_modification_with_delta(delta):
    self.ran += 1

func _process_modification():
    self.legacy_ran += 1
";

fn int_var(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_prop(tree, node, name) {
        Some(Variant::Int(n)) => n,
        other => panic!("{name}: {other:?}"),
    }
}

fn script_prop(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// The current hook runs and receives the frame delta.
#[test]
fn the_delta_hook_runs_and_receives_the_frame_delta() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", WITH_DELTA_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers_with_delta(&mut tree, skel, 0.25);

    assert_eq!(int_var(&tree, child, "ran"), 1, "the delta hook must run");
    assert_eq!(
        script_prop(&tree, child, "got_delta"),
        Some(Variant::Float(0.25)),
        "the frame delta must reach the script — 0.0 means the value was \
         dropped somewhere between MainLoop::step and the hook"
    );
}

/// 🔑 A script overriding BOTH gets only the delta hook.
///
/// ⚠️ Godot's base `_process_modification_with_delta` forwards to the legacy
/// `_process_modification`, so the delta form is the current entry point and
/// the older one is its fallback. Calling the legacy hook first — or calling
/// both — would run a deprecated path on every frame for any script that
/// implements the pair, and the two would fight over the same bones.
#[test]
fn the_delta_hook_wins_when_both_are_overridden() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", BOTH_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers_with_delta(&mut tree, skel, 0.5);

    assert_eq!(int_var(&tree, child, "ran"), 1, "the delta hook ran");
    assert_eq!(
        int_var(&tree, child, "legacy_ran"),
        0,
        "🔑 the legacy hook must NOT also run — Godot treats it as the \
         fallback, not a second callback"
    );
}

/// The legacy-only hook still runs through the delta-aware path.
///
/// ⚠️ The fallback is what keeps existing scripts working after this change.
/// Without it, adding the delta hook would silently stop every modifier that
/// implements only `_process_modification`.
#[test]
fn a_legacy_only_script_still_runs_through_the_delta_path() {
    let (mut tree, child) = skeleton_with_child("SkeletonModifier3D", MODIFIER_GD, true);
    let skel = tree.get_node(child).and_then(|n| n.parent()).unwrap();
    process_modifiers_with_delta(&mut tree, skel, 0.5);
    assert_eq!(
        int_var(&tree, child, "ran"),
        1,
        "a modifier overriding only the legacy hook must keep working"
    );
}
