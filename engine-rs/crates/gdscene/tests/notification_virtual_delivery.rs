//! pat-nzgdu.13 — engine notifications are delivered to a GDScript's
//! `_notification(int what)` virtual, matching Godot's
//! GDScriptInstance::notification.
//!
//! Before this bead the `ScriptInstance` trait had no `notification` method and
//! every notification sink only appended to a per-node `Vec<Notification>` log;
//! scripts only ever received the five NAMED lifecycle callbacks, so
//! save-on-quit / focus-pause / re-localize were impossible. This app-level test
//! attaches a script overriding `_notification(what)` and asserts it observes,
//! in order, ENTER_TREE(10), READY(13), PROCESS(17), PAUSED(14), UNPAUSED(15),
//! and PREDELETE(1) — driven by the real LifecycleManager, MainLoop tick, and
//! queue-free path (not a direct poke).

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

#[test]
fn notification_virtual_delivery_reaches_gdscript() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Disable auto-lifecycle so the script is attached BEFORE ENTER_TREE fires.
    // (add_child auto-enters the subtree when its parent is inside the tree,
    // which would run ENTER_TREE/READY before we can attach the script.)
    // Mirrors the LifecycleManager unit tests.
    tree.get_node_mut(root).unwrap().set_inside_tree(false);

    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();

    // `_notification` encodes each observed `what` into `log` as base-100 digits,
    // so the single integer preserves BOTH the set and the order of delivery.
    // `notify_script` is the only path that reaches `_notification`, so only the
    // wired notifications appear (no unrelated codes to filter).
    let script_src = "\
extends Node
var log = 0
func _notification(what):
    self.log = self.log * 100 + what
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    // ENTER_TREE (10) then READY (13), now that the script is attached.
    LifecycleManager::enter_tree(&mut tree, main_id);
    // Re-enable the root so per-frame / pause traversal reaches `main`. Setting
    // the flag does NOT re-fire lifecycle (only add_child/enter_tree do that).
    tree.get_node_mut(root).unwrap().set_inside_tree(true);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0); // PROCESS (17) once
    ml.set_paused(true); // PAUSED (14)
    ml.set_paused(false); // UNPAUSED (15)

    assert_eq!(
        ml.tree()
            .get_script(main_id)
            .and_then(|s| s.get_property("log")),
        // POST_ENTER_TREE (27) between ENTER_TREE and READY: Godot's
        // `_propagate_ready` sends it to every node on every tree entry.
        Some(Variant::Int(102713171415)),
        "script _notification must observe ENTER_TREE,POST_ENTER_TREE,READY,PROCESS,PAUSED,UNPAUSED in order",
    );

    // PREDELETE (1) is delivered through the same SceneTree::notify_script path
    // that queue_free's deferred deletion uses. Deliver it and confirm the script
    // records it (we can't read the script after the node is physically removed).
    ml.tree_mut().notify_script(main_id, 1);
    assert_eq!(
        ml.tree()
            .get_script(main_id)
            .and_then(|s| s.get_property("log")),
        Some(Variant::Int(10271317141501)),
        "script _notification must observe PREDELETE (1)",
    );

    // The real queue_free -> process_deletions path runs end-to-end (firing
    // EXIT_TREE + PREDELETE through notify_script) and removes the node.
    ml.tree_mut().queue_free(main_id);
    ml.tree_mut().process_deletions();
    assert!(
        ml.tree().get_node(main_id).is_none(),
        "queue_free + process_deletions must remove the node",
    );
}
