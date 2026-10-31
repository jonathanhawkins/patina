//! pat-mgixk.64 acceptance — Godot's `Node` lifecycle signals actually fire.
//!
//! Godot ground truth (`scene/main/node.cpp`): `ready` after each node's
//! `_ready` (:331); `tree_entered` then the parent's `child_entered_tree(node)`
//! (:358-365); `tree_exiting` top-down BEFORE `NOTIFICATION_EXIT_TREE` and the
//! parent's `child_exiting_tree(node)` (:423-433); `tree_exited` after the node
//! is detached (:401); `child_order_changed` on the parent after
//! add/remove/move/reparent (:576, :1696, :1767); `renamed` on `set_name`
//! (:1462).
//!
//! ⚠️ ALL OF THESE FAILED SILENTLY, which is why this test records a log and
//! counts entries rather than asserting one flag. Every one is registered in our
//! ClassDB (`class_db.rs:1239-1250`), so `connect()` SUCCEEDED and returned no
//! error — and then nothing ever emitted them. Only `tree_entered` was emitted
//! (`lifecycle.rs:50`). A game connecting `tree_exited` or awaiting `ready` got
//! silence with nothing to diagnose.
//!
//! # Two spellings from the bead I deliberately did NOT use
//!
//! 🔑 **`await child.ready` would HANG rather than fail.** `ready` is emitted
//! inside `add_child` (the READY phase of `enter_tree`), so by the time a script
//! line after `add_child` runs, the signal has already passed and `await` parks
//! forever. A hanging test is worse than a missing clause, so `ready` is observed
//! with a connected handler — the same emission, just not the sugar.
//!
//! 🔑 **No `.new()`.** It is proven only for `AStar2D`
//! (`astar_handle_identity_diagnostic.rs`); whether `Node2D.new()` yields a
//! tree-insertable node is unverified. It is not needed: an AUTHORED child
//! reaches all seven signals through the exact operations the acceptance lists —
//! `move_child` (order), `set_name` (renamed), `remove_child`
//! (exiting/exiting_tree/exited), then re-`add_child` (entered_tree/ready).
//!
//! Entry point, per the bead's Wiring requirement: a `.tscn` instanced into a
//! real tree, a GDScript attached to its root, and `MainLoop::step` driving
//! `_physics_process`. Nothing here calls a lifecycle function directly.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// `Other` exists so `move_child(Victim, 0)` has a sibling to move ahead of — a
/// single-child parent would make the reorder indistinguishable from a no-op.
///
/// 🔑 THE SCRIPT LIVES ON `Driver`, A CHILD, NOT ON THE ROOT, and that is forced
/// rather than stylistic. The parent-side signals (`child_order_changed`,
/// `child_entered_tree`, `child_exiting_tree`) have to be connected ON THE
/// PARENT, and `self.connect(..)` does not work: `connect` has no arm in
/// `call_node_method`, and a dot call on `self` is rerouted to that same self
/// path (`call_object_method` short-circuits when the target is the current
/// node), so it raises. Only the cross-node form is dispatched. So the driver
/// sits beside its subject and reaches the parent with `get_parent()`, taking
/// the subject with `get_node("../Victim")` — the exact shape the green
/// `signal_value_emit_connect_await.rs:101` uses. ⚠️ `get_parent()` rather
/// than a bare `get_node("..")`: the green test proves a leading `../<name>`
/// segment resolves, but nothing proves a path that is only `..`.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Driver" type="Node2D" parent="."]

[node name="Victim" type="Node2D" parent="."]

[node name="Other" type="Node2D" parent="."]
"#;

/// Connects on frame 1 and acts on frame 2, so every connection exists before
/// the operation that should fire it. ⚠️ The handlers are NAMED methods, not
/// lambdas: a one-line GDScript lambda body is an expression, so
/// `func(): self.log = ...` does not parse.
const DRIVER: &str = "\
var phase = 0
var log = \"\"
var vid = null
var entered_arg_ok = false
var exiting_arg_ok = false
func _on_ready():
    self.log = self.log + \"ready;\"
func _on_tree_exiting():
    self.log = self.log + \"tree_exiting;\"
func _on_tree_exited():
    self.log = self.log + \"tree_exited;\"
func _on_renamed():
    self.log = self.log + \"renamed;\"
func _on_order():
    self.log = self.log + \"order;\"
func _on_child_entered(n):
    self.log = self.log + \"child_entered;\"
    self.entered_arg_ok = n == self.vid
func _on_child_exiting(n):
    self.log = self.log + \"child_exiting;\"
    self.exiting_arg_ok = n == self.vid
func _physics_process(delta):
    if self.phase == 0:
        var p = get_parent()
        var v = get_node(\"../Victim\")
        self.vid = v
        v.connect(\"ready\", _on_ready)
        v.connect(\"tree_exiting\", _on_tree_exiting)
        v.connect(\"tree_exited\", _on_tree_exited)
        v.connect(\"renamed\", _on_renamed)
        p.connect(\"child_order_changed\", _on_order)
        p.connect(\"child_entered_tree\", _on_child_entered)
        p.connect(\"child_exiting_tree\", _on_child_exiting)
        self.phase = 1
        return
    if self.phase == 1:
        var p = get_parent()
        var v = self.vid
        p.move_child(v, 0)
        v.set_name(\"Renamed\")
        p.remove_child(v)
        p.add_child(v)
        self.phase = 2
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

fn child_names(tree: &SceneTree, parent: NodeId) -> Vec<String> {
    tree.get_node(parent)
        .expect("parent exists")
        .children()
        .iter()
        .filter_map(|id| tree.get_node(*id).map(|n| n.name().to_string()))
        .collect()
}

#[test]
fn node_lifecycle_signal_emission_test() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    assert_eq!(
        child_names(&tree, scene_root),
        vec![
            "Driver".to_string(),
            "Victim".to_string(),
            "Other".to_string()
        ],
        "fixture: the root must start with Driver, Victim, Other, so \
         move_child(Victim, 0) has somewhere to move from"
    );

    let driver = tree
        .find_child(scene_root, "Driver", true)
        .expect("Driver authored");
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(DRIVER, driver).expect("driver parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0); // frame 1: connect
    ml.step(1.0 / 60.0); // frame 2: act

    assert_eq!(
        script_var(ml.tree(), driver, "phase"),
        Variant::Int(2),
        "both frames of `_physics_process` must run to completion — a call \
         raising part-way through leaves the log short and every count below \
         misleading"
    );

    // ⚠️ Split on `;` and compare WHOLE tokens. A `contains` test would let
    // `tree_exiting` satisfy a search for `tree_exit`, and `child_entered`
    // overlap `entered` — the two pairs here are deliberately similar.
    let raw = match script_var(ml.tree(), driver, "log") {
        Variant::String(s) => s,
        other => panic!("`log` must be a String, got {other:?}"),
    };
    let seen: Vec<&str> = raw.split(';').filter(|s| !s.is_empty()).collect();
    let count = |name: &str| seen.iter().filter(|s| **s == name).count();

    // -- each signal fired, exactly as often as the script caused it ---------
    for (name, want, why) in [
        (
            "ready",
            1usize,
            "`ready` must fire when the re-added child runs _ready (node.cpp:331). \
             0 is the old behaviour: registered in ClassDB, never emitted, so \
             connect() succeeded silently",
        ),
        (
            "renamed",
            1,
            "`renamed` must fire on set_name (node.cpp:1462). 0 also means the \
             rename itself never reached SceneTree::set_node_name, which is the \
             only place Node::set_name is called from",
        ),
        (
            "tree_exiting",
            1,
            "`tree_exiting` must fire when the child is removed (node.cpp:423)",
        ),
        (
            "tree_exited",
            1,
            "`tree_exited` must fire after the child is detached (node.cpp:401)",
        ),
        (
            "child_entered",
            1,
            "the parent must emit `child_entered_tree` when the child is \
             re-added (node.cpp:365)",
        ),
        (
            "child_exiting",
            1,
            "the parent must emit `child_exiting_tree` when the child is \
             removed (node.cpp:433)",
        ),
    ] {
        assert_eq!(count(name), want, "{why}. Full log: {seen:?}");
    }

    // 🔑 Three, not one: `child_order_changed` has to fire at EVERY site that
    // changes the child list, and this script changes it three times —
    // move_child, remove_child, add_child. Wiring only one site is exactly how a
    // listener ends up firing for an add but not a move, and a `>= 1` assertion
    // would not catch that.
    assert_eq!(
        count("order"),
        3,
        "`child_order_changed` must fire for move_child, remove_child AND \
         add_child — three times. Full log: {seen:?}"
    );

    // -- Godot's ordering ---------------------------------------------------
    let at = |name: &str| {
        seen.iter()
            .position(|s| *s == name)
            .unwrap_or_else(|| panic!("`{name}` never fired; log {seen:?}"))
    };
    assert!(
        at("tree_exiting") < at("tree_exited"),
        "`tree_exiting` is emitted BEFORE the exit notifications and \
         `tree_exited` only after detachment, so this order is the signals' \
         whole meaning: a tree_exiting listener may still walk the subtree, a \
         tree_exited one may not. Log: {seen:?}"
    );
    assert!(
        at("child_exiting") < at("child_entered"),
        "the child leaves before it is re-added; a reversed order would mean the \
         two parent-side signals are crossed. Log: {seen:?}"
    );

    // -- the signals carried the child --------------------------------------
    assert_eq!(
        script_var(ml.tree(), driver, "entered_arg_ok"),
        Variant::Bool(true),
        "`child_entered_tree` must carry the node that entered — the acceptance \
         names this explicitly. A bare emit with no argument reads as null here"
    );
    assert_eq!(
        script_var(ml.tree(), driver, "exiting_arg_ok"),
        Variant::Bool(true),
        "`child_exiting_tree` must carry the node that is leaving"
    );

    // -- and the ENGINE really did what the signals announced ---------------
    //
    // Script vars record what fired; these record what happened. An emit with no
    // underlying mutation passes everything above and fails here.
    assert!(
        ml.tree().find_child(scene_root, "Renamed", true).is_some(),
        "set_name must actually rename the node, not just emit: no child called \
         `Renamed` exists. Root's children are {:?}",
        child_names(ml.tree(), scene_root)
    );
    assert!(
        child_names(ml.tree(), scene_root).contains(&"Renamed".to_string()),
        "the removed child must be back under the root after add_child; root's \
         children are {:?}",
        child_names(ml.tree(), scene_root)
    );
}
