//! pat-mgixk.63 acceptance — deferred calls and group calls reach NATIVE node
//! methods, not just script-defined ones.
//!
//! Godot ground truth: `core/object/message_queue.cpp` flushes through
//! `Object::callp`, and `scene/main/scene_tree.cpp:350` `call_group_flagsp`
//! does the same. `Object::callp` tries the script instance and then the
//! ClassDB native method — so `call_group("enemies", "queue_free")` frees
//! script-less enemies, and `notify_group` reaches a member's `_notification`.
//!
//! ⚠️ EVERY FAILURE HERE WAS SILENT. `call_group_flags` ran
//! `if self.scripts.contains_key(&id) { .. }`, so a group member with no script
//! was skipped without an error — `get_tree().call_group("enemies",
//! "queue_free")` returned normally and freed nothing. `notify_group` wrote only
//! `node.receive_notification(..)`, which is an internal log nothing a game can
//! observe reads, so the member script's `_notification` never ran.
//!
//! 🔑 ONE CLAUSE HERE IS A REGRESSION GUARD, NOT NEW WORK, and saying so matters
//! for reading a failure. The bead describes `process_deferred_calls` as
//! script-only, but that is already fixed: it dispatches through the generic
//! `call_object_method` with no `scripts.contains_key` gate (pat-mgixk.9 removed
//! the gate, pat-mgixk.10 added the `add_child` arm it needed). So part (a)
//! below should pass before AND after this change; parts (b) and (c) are what
//! this bead fixes. If (a) fails, suspect the harness, not the group code.
//!
//! Entry point, per the bead's Wiring requirement: a `.tscn` instanced into a
//! real tree, a GDScript attached to its root, and `MainLoop::step` driving
//! `_physics_process` — the deferred call is queued inside that callback and
//! flushed by the same frame's deferred phase.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// 🔑 The three enemies carry NO script, which is the whole point: a scripted
/// group member already worked, so they are what distinguishes this test from
/// `scene_tree_group_broadcast`.
///
/// `Watcher` has a script so part (c) has somewhere for `_notification` to land,
/// and `Spare` is the node the deferred native `add_child` moves.
///
/// ⚠️ GROUP MEMBERSHIP IS ADDED BY THE DRIVER SCRIPT, NOT AUTHORED HERE, and
/// that is a deliberate workaround for a SEPARATE defect this test found: an
/// authored `groups=["enemies"]` is parsed (`packed_scene.rs:411`) and applied
/// with `node.add_to_group(..)` (`:2312, :2348, :2646` and five more), which
/// writes the NODE's own `HashSet` — but never `SceneTree::groups`, the index
/// that `nodes_in_group_ordered` (`scene_tree.rs:9779`) actually reads.
/// `SceneTree::add_to_group` (`:3537`) writes BOTH, which is why adding from a
/// script works and authoring does not. So a `.tscn` group is invisible to
/// `call_group`/`notify_group`/`get_nodes_in_group` regardless of this bead's
/// fix. Reported separately rather than fixed here: making authored groups live
/// would change behaviour across every scene that declares one.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Enemy1" type="Node2D" parent="."]

[node name="Enemy2" type="Node2D" parent="."]

[node name="Enemy3" type="Node2D" parent="."]

[node name="Watcher" type="Node2D" parent="."]

[node name="Spare" type="Node2D" parent="."]
"#;

/// The driver. Queues a deferred NATIVE call during `_physics_process`, then
/// makes both group calls. Guarded by `done` so a second `step` does not re-run
/// it (the enemies are gone by then, and re-queueing would muddy part (a)).
const DRIVER: &str = "\
var done = false
func _physics_process(delta):
    if self.done:
        return
    # Group membership first, via the tree-level add_to_group that maintains
    # BOTH stores. The enemies themselves stay script-less: it is the DRIVER
    # that has a script, which is exactly the acceptance's shape.
    $Enemy1.add_to_group(\"enemies\")
    $Enemy2.add_to_group(\"enemies\")
    $Enemy3.add_to_group(\"enemies\")
    $Watcher.add_to_group(\"watchers\")
    # (a) a deferred NATIVE method, queued from inside _physics_process.
    var spare = $Spare
    self.remove_child(spare)
    $Watcher.call_deferred(\"add_child\", spare)
    # (b) a native method on three SCRIPT-LESS group members.
    get_tree().call_group(\"enemies\", \"queue_free\")
    # (c) a notification that has to reach a member script's _notification.
    get_tree().notify_group(\"watchers\", 9999)
    self.done = true
";

/// ⚠️ Guarded on the exact code. `_notification` also receives the engine's own
/// lifecycle notifications (entered tree, ready, ...), so an unguarded handler
/// would record whichever arrived last and prove nothing about `notify_group`.
const WATCHER: &str = "\
var got = -1
var hits = 0
func _notification(what):
    if what == 9999:
        self.got = what
        self.hits += 1
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
fn deferred_and_group_calls_reach_native_methods() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let watcher = tree
        .find_child(scene_root, "Watcher", true)
        .expect("Watcher authored");

    // Fixture preconditions, asserted BEFORE the step so a later failure cannot
    // be the scene having loaded differently than this test assumes.
    //
    // 🔑 The script-less check is the one that matters: a group member WITH a
    // script already worked before this bead, so a script on any enemy would
    // let the test pass without the fix. Group membership itself is added by
    // the driver (see SCENE), so it is asserted after the step instead.
    for name in ["Enemy1", "Enemy2", "Enemy3"] {
        let id = tree
            .find_child(scene_root, name, true)
            .unwrap_or_else(|| panic!("fixture: {name} must be authored"));
        assert!(
            tree.get_script(id).is_none(),
            "fixture: {name} must have NO script — a scripted member already \
             worked before this bead, so a script here would make the test pass \
             without the fix"
        );
    }

    tree.attach_script(
        watcher,
        Box::new(GDScriptNodeInstance::from_source(WATCHER, watcher).expect("watcher parses")),
    );
    tree.attach_script(
        scene_root,
        Box::new(GDScriptNodeInstance::from_source(DRIVER, scene_root).expect("driver parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(ml.tree(), scene_root, "done"),
        Variant::Bool(true),
        "`_physics_process` must run to completion — if it raised part-way, \
         every assertion below is reading an initialiser"
    );

    // The driver really did build the group — otherwise every group call below
    // would have run over an empty member list. ⚠️ Checked AFTER the step
    // because the driver populates it; the enemies are freed by part (b) below,
    // so this reads the Watcher group, which nothing frees.
    assert_eq!(
        ml.tree().get_nodes_in_group("watchers").len(),
        1,
        "the driver's add_to_group must have reached SceneTree's group index, or \
         the group calls had no members and (b)/(c) prove nothing"
    );

    // -- (a) the deferred NATIVE call landed in the same frame ---------------
    //
    // Regression guard (see the module note): this path already worked.
    assert!(
        child_names(ml.tree(), watcher).contains(&"Spare".to_string()),
        "call_deferred(\"add_child\", spare) must have been flushed by the end \
         of the step, so Spare is now Watcher's child; Watcher's children are \
         {:?}",
        child_names(ml.tree(), watcher)
    );

    // -- (c) the notification reached a member's SCRIPT ---------------------
    assert_eq!(
        script_var(ml.tree(), watcher, "got"),
        Variant::Int(9999),
        "notify_group(\"watchers\", 9999) must reach the member script's \
         _notification(9999). -1 means the notification only went into the \
         node's internal log, which is the old behaviour"
    );
    assert_eq!(
        script_var(ml.tree(), watcher, "hits"),
        Variant::Int(1),
        "and exactly once — a notification delivered twice means the group walk \
         and the per-node path both fired"
    );

    // -- (b) the native method ran on all three SCRIPT-LESS members ---------
    //
    // Stepped again before asserting: `queue_free` enqueues into
    // `pending_deletions`, and the acceptance says the nodes are gone "after
    // the frame" rather than at a specific point inside it. A second step
    // cannot mask a failure — the driver is guarded by `done`, so nothing is
    // re-queued and an unfreed enemy stays unfreed.
    ml.step(1.0 / 60.0);

    for name in ["Enemy1", "Enemy2", "Enemy3"] {
        assert!(
            ml.tree().find_child(scene_root, name, true).is_none(),
            "call_group(\"enemies\", \"queue_free\") must free {name}: a native \
             method has to reach a group member that has no script. Still \
             present means call_group took its script-only branch and skipped \
             it. Root's children are {:?}",
            child_names(ml.tree(), scene_root)
        );
    }
    assert!(
        ml.tree().get_nodes_in_group("enemies").is_empty(),
        "and the group itself must be empty once its members are freed"
    );
    // The group call must not have been a blunt instrument: Watcher is in a
    // DIFFERENT group and must survive, or "all three freed" could just mean
    // everything was freed.
    assert!(
        ml.tree().get_node(watcher).is_some(),
        "Watcher is in `watchers`, not `enemies`, so it must survive the group \
         call — if it did not, the group filter is being ignored"
    );
}
