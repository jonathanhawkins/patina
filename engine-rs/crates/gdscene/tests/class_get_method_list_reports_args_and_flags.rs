//! pat-s3rrv.57 (merged finding 6, end to end): `ClassDB.class_get_method_list`
//! must report each method's arguments and flags to a script.
//!
//! The emitter hardcoded `args = []` and `flags = 0` for every method in the
//! engine, behind a comment admitting the data was "genuinely absent" —
//! `MethodInfo` carried only a name and an arity, so ClassDB could not tell a
//! virtual method from a concrete one (278 of 1413 declared pairs; `Node` and
//! `Control` declared none).
//!
//! A tool reading this — a plugin listing overridable virtuals, a documentation
//! generator, an autocomplete provider — got a list of names and nothing it
//! could act on. This drives the whole path: registration → `MethodInfo` →
//! `class_get_method_list` → a GDScript dictionary.

use gdobject::class_db::{ClassRegistration, MethodInfo, PropertyInfo};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// Godot's `MethodFlags` (`core/object/object.h:224-233`).
const METHOD_FLAG_NORMAL: i64 = 1;
const METHOD_FLAG_VIRTUAL: i64 = 8;

/// ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, not by
/// `MainLoop::step`. Without the explicit call every var keeps its declared
/// value and the failures read as "ClassDB returned nothing".
const PROBE_GD: &str = "\
extends Node
var virt_flags = -1
var concrete_flags = -1
var virt_arg_count = -1
var virt_arg_name = \"unset\"
var virt_arg_type = -1
var method_count = -1

func _ready():
    var list = ClassDB.class_get_method_list(\"MethodFlagProbe\")
    self.method_count = list.size()
    for m in list:
        if m[\"name\"] == \"_process\":
            self.virt_flags = m[\"flags\"]
            var args = m[\"args\"]
            self.virt_arg_count = args.size()
            if args.size() > 0:
                self.virt_arg_name = args[0][\"name\"]
                self.virt_arg_type = args[0][\"type\"]
        if m[\"name\"] == \"start\":
            self.concrete_flags = m[\"flags\"]
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(ml: &MainLoop, node: NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Some(Variant::Int(v)) => v,
        other => panic!("`{name}` must be an Int, got {other:?}"),
    }
}

fn string_var(ml: &MainLoop, node: NodeId, name: &str) -> String {
    match script_var(ml, node, name) {
        Some(Variant::String(v)) => v,
        other => panic!("`{name}` must be a String, got {other:?}"),
    }
}

/// Registers a probe class carrying one virtual method with a named, typed
/// argument and one plain concrete method, then runs `src` against it.
///
/// A probe rather than a real class because no engine registration supplies
/// per-argument detail yet — all 1859 of them go through
/// `MethodInfo::new(name, arity)`. The probe is what proves the pipeline can
/// carry the detail once a registration does.
fn run(src: &str) -> (MainLoop, NodeId) {
    gdobject::class_db::register_class(
        ClassRegistration::new("MethodFlagProbe")
            .method(
                MethodInfo::new("_process", 1)
                    .virtual_method()
                    .with_arguments(vec![PropertyInfo::new("delta", Variant::Float(0.0))]),
            )
            .method(MethodInfo::new("start", 0)),
    );

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

#[test]
fn class_get_method_list_reports_args_and_flags() {
    let (ml, node) = run(PROBE_GD);

    assert_eq!(
        int_var(&ml, node, "method_count"),
        2,
        "the probe declares exactly two methods — if this is 0 the script never \
         reached ClassDB and every assertion below would pass vacuously"
    );

    // -- flags ---------------------------------------------------------------
    //
    // The pair is what discriminates: the old emitter returned 0 for BOTH, so
    // asserting only the virtual would have passed against a decoder that
    // hardcodes 9 just as happily as against a real one.
    assert_eq!(
        int_var(&ml, node, "virt_flags"),
        METHOD_FLAG_NORMAL | METHOD_FLAG_VIRTUAL,
        "a virtual method reports NORMAL|VIRTUAL (9)"
    );
    assert_eq!(
        int_var(&ml, node, "concrete_flags"),
        METHOD_FLAG_NORMAL,
        "a concrete method reports NORMAL (1) — never 0, which is what the \
         hardcoded emitter used to return for everything"
    );

    // -- arguments -----------------------------------------------------------
    assert_eq!(
        int_var(&ml, node, "virt_arg_count"),
        1,
        "`args` must carry the declared argument, not the empty array the \
         emitter used to hardcode"
    );
    assert_eq!(
        string_var(&ml, node, "virt_arg_name"),
        "delta",
        "each argument entry is a property dictionary with a `name`"
    );
    // Godot's Variant type code for FLOAT is 3, and the argument dictionaries
    // use the same shape `class_get_property_list` emits, so a caller can read
    // either with one code path.
    assert_eq!(
        int_var(&ml, node, "virt_arg_type"),
        3,
        "the argument's `type` comes from its PropertyInfo default, so the type \
         and the default can never disagree"
    );
}
