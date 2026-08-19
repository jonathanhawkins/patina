//! pat-s3rrv.57: the `return` key reaches a script.
//!
//! `ClassDB.class_get_method_list` emitted `name`, `args`, `default_args` and
//! `flags` but no `return`, so a script could see that `class_exists` takes one
//! argument and not that it answers a bool. Godot's dict always carries the key,
//! shaped like one of the argument dicts.
//!
//! ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, not by
//! `MainLoop::step`.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// Godot's `Variant::Type` ordinals (`core/variant/variant.h`).
const TYPE_NIL: i64 = 0;
const TYPE_BOOL: i64 = 1;
const TYPE_DICTIONARY: i64 = 27;
const TYPE_ARRAY: i64 = 28;

/// Every var starts at -1 so "the script never ran" stays distinguishable from
/// "the answer was 0" — and TYPE_NIL *is* 0, which one assertion expects.
const PROBE_GD: &str = "\
extends Node
var bool_ret = -1
var dict_ret = -1
var list_ret = -1
var list_class = \"unset\"
var undeclared_ret = -1

func _ready():
    for m in ClassDB.class_get_method_list(\"ClassDB\"):
        if m[\"name\"] == \"class_exists\":
            self.bool_ret = m[\"return\"][\"type\"]
        if m[\"name\"] == \"class_get_signal\":
            self.dict_ret = m[\"return\"][\"type\"]
        if m[\"name\"] == \"class_get_method_list\":
            self.list_ret = m[\"return\"][\"type\"]
            self.list_class = m[\"return\"][\"class_name\"]
    for m in ClassDB.class_get_method_list(\"Node\"):
        if m[\"name\"] == \"_ready\":
            self.undeclared_ret = m[\"return\"][\"type\"]
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn run() -> (MainLoop, NodeId) {
    // register_all_classes, not the 2D/3D pair: ClassDB registers itself in
    // `register_root_window_chain`, which only the full registrar runs.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Host", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(PROBE_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, node)
}

#[test]
fn class_get_method_list_reports_the_return_type() {
    let (ml, node) = run();

    assert_eq!(
        script_var(&ml, node, "bool_ret"),
        Some(Variant::Int(TYPE_BOOL)),
        "class_exists answers a bool; -1 here means the script never reached \
         the `return` key at all"
    );
    assert_eq!(
        script_var(&ml, node, "dict_ret"),
        Some(Variant::Int(TYPE_DICTIONARY)),
        "class_get_signal answers a Dictionary — a second, different type, so \
         this cannot pass against an emitter that hardcodes one"
    );
}

/// A typed array reports ARRAY plus its element type, the same way an argument
/// dict reports an object's class.
#[test]
fn a_typed_array_return_carries_its_element_type() {
    let (ml, node) = run();

    assert_eq!(
        script_var(&ml, node, "list_ret"),
        Some(Variant::Int(TYPE_ARRAY))
    );
    assert_eq!(
        script_var(&ml, node, "list_class"),
        Some(Variant::String("Dictionary".to_string())),
        "`Dictionary[]` without the element type is indistinguishable from an \
         untyped Array"
    );
}

/// ⚠️ An undeclared return serialises as NIL — which is also what a void method
/// looks like. The key must still be present: a script doing
/// `m["return"]["type"]` must not raise on a method whose return nobody has
/// declared yet, which is most of the engine.
#[test]
fn an_undeclared_return_is_nil_and_still_present() {
    let (ml, node) = run();

    assert_eq!(
        script_var(&ml, node, "undeclared_ret"),
        Some(Variant::Int(TYPE_NIL)),
        "Node._ready declares no return; the key is present and NIL, not absent"
    );
}
