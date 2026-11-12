//! pat-s3rrv.57: `ClassDB.class_get_property` / `class_set_property`.
//!
//! Both raised `UndefinedFunction` — and once ClassDB was registered as a class
//! it *declared* them in `class_get_method_list` while still refusing to run
//! them, so reflection and behaviour disagreed.
//!
//! These two are the odd pair in an otherwise class-name-only API: Godot's
//! signatures take an `Object`, so they read and write a LIVE object's
//! properties. That is what makes them useful — a save system or an inspector
//! walks `class_get_property_list(obj.get_class())` and then reads each name off
//! the instance without knowing any of them at author time.
//!
//! # Why this test lives in gdscene
//!
//! Reaching a live object needs `scene_access` to be `Some`, which the
//! scene-tree host installs. A gdscript-interop test would get `None` and see
//! the no-scene fallbacks instead of the real path.
//!
//! ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, not by
//! `MainLoop::step`.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// Every var starts as `-1` so that "the script never ran" (Int) is
/// distinguishable from "the call answered null" (Nil) — the two look identical
/// if the sentinel is itself null, and one of the assertions below expects Nil.
const PROBE_GD: &str = "\
extends Node
var visible_default = -1
var set_err = -1
var after_set = -1
var rotation_value = -1
var missing = -1

func _ready():
    var n = ClassDB.instantiate(\"Node2D\")
    add_child(n)
    self.visible_default = ClassDB.class_get_property(n, \"visible\")
    self.set_err = ClassDB.class_set_property(n, \"visible\", false)
    self.after_set = ClassDB.class_get_property(n, \"visible\")
    self.rotation_value = ClassDB.class_get_property(n, \"rotation\")
    self.missing = ClassDB.class_get_property(n, \"no_such_property_at_all\")
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Mirrors the call sequence of the green `engine_class_new_routes_through_classdb`
/// harness rather than inventing one.
fn run(src: &str) -> (MainLoop, NodeId) {
    // ClassDB is NOT populated implicitly; without this every query answers
    // false and the probe would construct nothing.
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Host", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, node)
}

/// **The acceptance case.** A read, a write, and a read back on a live node.
#[test]
fn class_get_and_set_property_round_trip_on_a_live_object() {
    let (ml, node) = run(PROBE_GD);

    // The value read first is the ClassDB default that `create_node` seeded —
    // so this also proves the getter reaches the real object rather than
    // answering out of the class table.
    assert_eq!(
        script_var(&ml, node, "visible_default"),
        Some(Variant::Bool(true)),
        "CanvasItem.visible defaults to true; Int(-1) here means the script \
         never got past `instantiate`"
    );

    assert_eq!(
        script_var(&ml, node, "set_err"),
        Some(Variant::Int(0)),
        "class_set_property returns Godot's Error, and OK is 0"
    );
    assert_eq!(
        script_var(&ml, node, "after_set"),
        Some(Variant::Bool(false)),
        "the write must be visible to the next read — a setter that silently \
         dropped the value would leave this true"
    );
}

/// A typed property other than a bool, so the pair is not passing on
/// `Variant::Bool` alone.
#[test]
fn a_float_property_keeps_its_type() {
    let (ml, node) = run(PROBE_GD);

    assert_eq!(
        script_var(&ml, node, "rotation_value"),
        Some(Variant::Float(0.0)),
        "Node2D.rotation is a float, and the getter must not stringify it"
    );
}

/// Godot answers null for a property the object does not have, rather than
/// raising.
///
/// ⚠️ The sentinel matters: `missing` starts as `Int(-1)`, so `Nil` here proves
/// the call ran AND answered null. Had it started as null, this assertion would
/// pass just as happily against a script that died on the line before.
#[test]
fn an_unknown_property_answers_null_without_raising() {
    let (ml, node) = run(PROBE_GD);

    assert_eq!(
        script_var(&ml, node, "missing"),
        Some(Variant::Nil),
        "an unknown property is null, not an error"
    );
}
