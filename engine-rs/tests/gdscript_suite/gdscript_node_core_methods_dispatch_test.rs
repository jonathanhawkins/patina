//! pat-mgixk.6: `Node`/`Object` core methods reach a dispatch arm.
//!
//! `get_child`, `get_node_or_null`, `remove_child`, `is_inside_tree`,
//! `set_process`, `is_processing`, `set_physics_process`,
//! `is_physics_processing` and `Object.notification` are all registered in
//! ClassDB (`gdobject/src/class_db.rs`) and had no dispatch arm in any table.
//! That is the worst combination: `node_has_method` answers *true* off the
//! registration, so `Interpreter::call_object_method` takes its `responds`
//! branch and hands the script `Variant::Nil`. `$X.get_child(0)` read as null
//! and `other.set_process(false)` did nothing at all — with no script error to
//! notice it by.
//!
//! Entry point, per the bead's wiring requirement: a `.tscn` parsed by
//! `PackedScene`, instanced into a real `SceneTree`, with GDScript attached and
//! driven by `MainLoop::step`. Every assertion is on what the scene *observes* —
//! script variables the callbacks wrote, the tree's own child list, and B's
//! notification log — never on the new functions called directly.
//!
//! The `_process` half is deliberately a before/after measurement rather than a
//! bare "ran 0 times": the count has to be non-zero BEFORE `set_process(false)`,
//! or a frozen counter would prove nothing about the toggle.

use gdobject::notification::NOTIFICATION_EXIT_TREE;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{LifecycleManager, NodeId, SceneTree};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="A" type="Node" parent="."]

[node name="B" type="Node" parent="."]
"#;

/// Every call here is the BARE spelling (`get_child(0)`) or the `other.m()`
/// spelling (`a.get_node_or_null(..)`) — the two paths that both fall back to
/// `SceneAccess::call_node_method`, so one arm has to serve both.
const ROOT_SCRIPT: &str = r#"extends Node

var process_calls = 0
var first_child_name = "<unset>"
var last_child_name = "<unset>"
var out_of_range_is_null = false
var missing_is_null = false
var b_was_inside_tree = false
var processing_before_disable = false
var processing_after_disable = true
var physics_processing_after_disable = true
var child_count_after_detach = -1
var b_inside_tree_after_detach = true

func _ready():
    first_child_name = get_child(0).get_name()
    last_child_name = get_child(-1).get_name()
    out_of_range_is_null = get_child(7) == null
    var a = get_node("A")
    missing_is_null = a.get_node_or_null("Nope") == null
    b_was_inside_tree = get_node("B").is_inside_tree()
    a.notification(9001)
    processing_before_disable = is_processing()

func _process(_delta):
    process_calls += 1

func disable_processing():
    set_process(false)
    set_physics_process(false)
    processing_after_disable = is_processing()
    physics_processing_after_disable = is_physics_processing()

func detach_b():
    var b = get_node("B")
    remove_child(b)
    child_count_after_detach = get_child_count()
    b_inside_tree_after_detach = b.is_inside_tree()
"#;

/// Lives on a SEPARATE node from the driver on purpose: a script is checked out
/// of the arena for the duration of its own call, so `notification()` aimed at
/// the running script could never reach its own `_notification`.
const CHILD_A_SCRIPT: &str = r#"extends Node

var saw_custom_notification = false

func _notification(what):
    if what == 9001:
        saw_custom_notification = true
"#;

struct Fixture {
    main_loop: MainLoop,
    root: NodeId,
    child_a: NodeId,
    child_b: NodeId,
    scene_root: NodeId,
}

fn build() -> Fixture {
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene must parse");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // The tree root has to be inside the tree BEFORE the scene is instanced, or
    // `add_child` never propagates `enter_tree` and `is_inside_tree()` would
    // report false for a reason that has nothing to do with dispatch.
    LifecycleManager::enter_tree(&mut tree, root);
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("fixture scene must instance");

    let find = |tree: &SceneTree, name: &str| -> NodeId {
        tree.all_nodes_in_tree_order()
            .into_iter()
            .find(|&id| tree.get_node(id).is_some_and(|n| n.name() == name))
            .unwrap_or_else(|| panic!("node {name} must exist in the fixture scene"))
    };
    let scene_root = find(&tree, "Root");
    let child_a = find(&tree, "A");
    let child_b = find(&tree, "B");

    let root_script =
        GDScriptNodeInstance::from_source(ROOT_SCRIPT, scene_root).expect("root script must parse");
    tree.attach_script(scene_root, Box::new(root_script));
    let a_script =
        GDScriptNodeInstance::from_source(CHILD_A_SCRIPT, child_a).expect("A script must parse");
    tree.attach_script(child_a, Box::new(a_script));

    for id in tree.all_nodes_in_tree_order() {
        tree.process_script_enter_tree(id);
    }
    for id in tree.all_nodes_in_tree_order() {
        if tree.has_script(id) {
            tree.process_script_ready(id);
        }
    }

    Fixture {
        main_loop: MainLoop::new(tree),
        root,
        child_a,
        child_b,
        scene_root,
    }
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .unwrap_or_else(|| panic!("node must have a script attached"))
        .get_property(name)
        .unwrap_or_else(|| panic!("script var {name} must exist"))
}

#[test]
fn gdscript_node_core_methods_dispatch() {
    let mut fx = build();

    // -- get_child / get_node_or_null / is_inside_tree, written in `_ready` ---

    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "first_child_name"),
        Variant::String("A".into()),
        "get_child(0) must return the first child, not the silent Nil placeholder",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "last_child_name"),
        Variant::String("B".into()),
        "get_child(-1) must count back from the end, as Godot's Node::get_child does",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "out_of_range_is_null"),
        Variant::Bool(true),
        "an out-of-range get_child index must read as null",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "missing_is_null"),
        Variant::Bool(true),
        "get_node_or_null on a path that does not resolve must read as null",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "b_was_inside_tree"),
        Variant::Bool(true),
        "is_inside_tree() on a node that IS in the tree must be true, not Nil",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "processing_before_disable"),
        Variant::Bool(true),
        "a fresh node processes by default, so is_processing() must read true here",
    );

    // -- Object.notification reached the other node's `_notification` ---------

    assert_eq!(
        script_var(&fx.main_loop, fx.child_a, "saw_custom_notification"),
        Variant::Bool(true),
        "a.notification(9001) must reach A's _notification virtual",
    );

    // -- set_process(false) actually stops `_process` under MainLoop ---------

    for _ in 0..3 {
        fx.main_loop.step(DT);
    }
    let calls_while_enabled = match script_var(&fx.main_loop, fx.scene_root, "process_calls") {
        Variant::Int(n) => n,
        other => panic!("process_calls must be an int, got {other:?}"),
    };
    // ⚠️ The control. Without this the frozen-counter assertion below would pass
    // just as happily on a scene whose `_process` never ran at all.
    assert!(
        calls_while_enabled > 0,
        "_process must be running before the toggle, or the next assertion proves nothing",
    );

    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.scene_root, "disable_processing", &[]);

    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "processing_after_disable"),
        Variant::Bool(false),
        "is_processing() must read back the set_process(false) that just ran",
    );
    assert_eq!(
        script_var(
            &fx.main_loop,
            fx.scene_root,
            "physics_processing_after_disable"
        ),
        Variant::Bool(false),
        "is_physics_processing() must read back set_physics_process(false)",
    );

    for _ in 0..3 {
        fx.main_loop.step(DT);
    }
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "process_calls"),
        Variant::Int(calls_while_enabled),
        "set_process(false) must stop _process in MainLoop::step, not merely set a flag",
    );

    // -- remove_child detaches, fires exit_tree, and leaves the node alive ---

    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.scene_root, "detach_b", &[]);

    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "child_count_after_detach"),
        Variant::Int(1),
        "after remove_child(B) the scene root must have A as its only child",
    );
    assert_eq!(
        script_var(&fx.main_loop, fx.scene_root, "b_inside_tree_after_detach"),
        Variant::Bool(false),
        "a detached node must report is_inside_tree() == false",
    );

    let tree = fx.main_loop.tree();
    assert_eq!(
        tree.get_node(fx.scene_root)
            .expect("scene root must still exist")
            .children(),
        &[fx.child_a],
        "the tree's own child list must reflect the detach",
    );
    // Godot's `remove_child` hands back an ORPHAN the caller still owns — the
    // standard move-a-node idiom is remove_child + add_child. Freeing it here
    // (which `SceneTree::remove_node` would have done) turns every one of those
    // into a use-after-free.
    let b = tree
        .get_node(fx.child_b)
        .expect("remove_child must detach B, not destroy it");
    assert!(
        b.notification_log().contains(&NOTIFICATION_EXIT_TREE),
        "B must have received NOTIFICATION_EXIT_TREE on detach, got {:?}",
        b.notification_log(),
    );
    assert_eq!(
        tree.get_node(fx.root)
            .expect("tree root must still exist")
            .children(),
        &[fx.scene_root],
        "detaching B must not disturb the scene root's own parentage",
    );
}
