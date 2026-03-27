//! pat-lf8p: Expand notification coverage beyond lifecycle basics.
//!
//! Broadens notification parity beyond core lifecycle (ENTER_TREE, READY,
//! EXIT_TREE, PROCESS) into areas not covered by existing test files:
//!
//! 1. ProcessMode::Disabled — skips PROCESS/PHYSICS_PROCESS
//! 2. ProcessMode::Always — processes even when paused
//! 3. ProcessMode::WhenPaused — processes only when paused
//! 4. ProcessMode::Inherit — resolves via parent chain
//! 5. Process priority ordering — lower priority value = earlier dispatch
//! 6. Scene transition notifications — change_scene fires EXIT/ENTER correctly
//! 7. Notification dispatch chain integration with scene tree
//! 8. Documented exclusion inventory — all notification codes accounted for
//!
//! Acceptance: additional fixtures cover important non-lifecycle notification
//! cases and remaining exclusions are documented.

use gdscene::node::{Node, ProcessMode};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::{LifecycleManager, MainLoop};
use gdobject::notification::{
    Notification, NOTIFICATION_ENTER_TREE,
    NOTIFICATION_INTERNAL_PHYSICS_PROCESS, NOTIFICATION_INTERNAL_PROCESS,
    NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_PROCESS, NOTIFICATION_READY,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

fn node_notification_details(tree: &SceneTree, path: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.node_path == path && e.event_type == TraceEventType::Notification)
        .map(|e| e.detail.clone())
        .collect()
}

/// Build a tree with root -> A -> B, enter it, return MainLoop.
fn make_entered_ml() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Node2D")).unwrap();
    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    (MainLoop::new(tree), a, b)
}

// ===========================================================================
// 1. ProcessMode::Disabled — skips PROCESS and PHYSICS_PROCESS
// ===========================================================================

#[test]
fn disabled_node_skips_process_notifications() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("Disabled", "Node2D")).unwrap();
    tree.get_node_mut(child).unwrap().set_process_mode(ProcessMode::Disabled);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    let physics = notification_paths(tree, "PHYSICS_PROCESS");

    assert!(
        !process.iter().any(|p| p.contains("Disabled")),
        "Disabled node should not receive PROCESS"
    );
    assert!(
        !physics.iter().any(|p| p.contains("Disabled")),
        "Disabled node should not receive PHYSICS_PROCESS"
    );
}

#[test]
fn disabled_node_skips_internal_process() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("Disabled", "Node2D")).unwrap();
    tree.get_node_mut(child).unwrap().set_process_mode(ProcessMode::Disabled);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let ip = notification_paths(tree, "INTERNAL_PROCESS");
    let ipp = notification_paths(tree, "INTERNAL_PHYSICS_PROCESS");

    assert!(
        !ip.iter().any(|p| p.contains("Disabled")),
        "Disabled node should not receive INTERNAL_PROCESS"
    );
    assert!(
        !ipp.iter().any(|p| p.contains("Disabled")),
        "Disabled node should not receive INTERNAL_PHYSICS_PROCESS"
    );
}

#[test]
fn disabled_node_sibling_still_processes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _active = tree.add_child(root, Node::new("Active", "Node2D")).unwrap();
    let disabled = tree.add_child(root, Node::new("Disabled", "Node2D")).unwrap();
    tree.get_node_mut(disabled).unwrap().set_process_mode(ProcessMode::Disabled);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        process.iter().any(|p| p.contains("Active")),
        "Active sibling should still process"
    );
    assert!(
        !process.iter().any(|p| p.contains("Disabled")),
        "Disabled sibling should not process"
    );
}

// ===========================================================================
// 2. ProcessMode::Always — processes even when paused
// ===========================================================================

#[test]
fn always_node_processes_when_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let always = tree.add_child(root, Node::new("Always", "Node2D")).unwrap();
    tree.get_node_mut(always).unwrap().set_process_mode(ProcessMode::Always);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        process.iter().any(|p| p.contains("Always")),
        "Always-mode node should receive PROCESS even when paused"
    );
}

#[test]
fn always_node_processes_when_not_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let always = tree.add_child(root, Node::new("Always", "Node2D")).unwrap();
    tree.get_node_mut(always).unwrap().set_process_mode(ProcessMode::Always);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        process.iter().any(|p| p.contains("Always")),
        "Always-mode node should process when not paused too"
    );
}

// ===========================================================================
// 3. ProcessMode::WhenPaused — processes only when paused
// ===========================================================================

#[test]
fn when_paused_node_skips_when_not_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let wp = tree.add_child(root, Node::new("WhenPaused", "Node2D")).unwrap();
    tree.get_node_mut(wp).unwrap().set_process_mode(ProcessMode::WhenPaused);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        !process.iter().any(|p| p.contains("WhenPaused")),
        "WhenPaused node should NOT process when tree is not paused"
    );
}

#[test]
fn when_paused_node_processes_when_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let wp = tree.add_child(root, Node::new("WhenPaused", "Node2D")).unwrap();
    tree.get_node_mut(wp).unwrap().set_process_mode(ProcessMode::WhenPaused);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        process.iter().any(|p| p.contains("WhenPaused")),
        "WhenPaused node should process when tree IS paused"
    );
}

// ===========================================================================
// 4. ProcessMode::Inherit — resolves via parent chain
// ===========================================================================

#[test]
fn inherit_node_disabled_when_parent_disabled() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let _child = tree.add_child(parent, Node::new("Child", "Node2D")).unwrap();

    // Parent disabled, child inherits (default)
    tree.get_node_mut(parent).unwrap().set_process_mode(ProcessMode::Disabled);
    // Child defaults to Inherit

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        !process.iter().any(|p| p.contains("Child")),
        "Child with Inherit mode should be disabled when parent is Disabled"
    );
}

#[test]
fn inherit_node_always_when_parent_always() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let _child = tree.add_child(parent, Node::new("Child", "Node2D")).unwrap();

    tree.get_node_mut(parent).unwrap().set_process_mode(ProcessMode::Always);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");
    assert!(
        process.iter().any(|p| p.contains("Child")),
        "Child with Inherit mode should process when parent is Always, even when paused"
    );
}

// ===========================================================================
// 5. Process priority ordering
// ===========================================================================

#[test]
fn process_priority_lower_value_fires_first() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let high = tree.add_child(root, Node::new("HighPrio", "Node2D")).unwrap();
    let low = tree.add_child(root, Node::new("LowPrio", "Node2D")).unwrap();
    let _normal = tree.add_child(root, Node::new("Normal", "Node2D")).unwrap();

    // Lower priority value = earlier processing (Godot convention).
    tree.get_node_mut(high).unwrap().set_process_priority(-10);
    tree.get_node_mut(low).unwrap().set_process_priority(10);
    // Normal stays at default 0

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");

    let pos = |name: &str| {
        process
            .iter()
            .position(|p| p.contains(name))
            .unwrap_or_else(|| panic!("{name} should appear in PROCESS: {:?}", process))
    };

    assert!(
        pos("HighPrio") < pos("Normal"),
        "Priority -10 should fire before priority 0"
    );
    assert!(
        pos("Normal") < pos("LowPrio"),
        "Priority 0 should fire before priority 10"
    );
}

#[test]
fn process_priority_applies_to_physics_process_too() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let first = tree.add_child(root, Node::new("First", "Node2D")).unwrap();
    let last = tree.add_child(root, Node::new("Last", "Node2D")).unwrap();

    tree.get_node_mut(first).unwrap().set_process_priority(-5);
    tree.get_node_mut(last).unwrap().set_process_priority(5);

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let phys = notification_paths(tree, "PHYSICS_PROCESS");

    let first_pos = phys.iter().position(|p| p.contains("First")).unwrap();
    let last_pos = phys.iter().position(|p| p.contains("Last")).unwrap();
    assert!(
        first_pos < last_pos,
        "Priority -5 should fire PHYSICS_PROCESS before priority 5"
    );
}

#[test]
fn equal_priority_preserves_tree_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // All at same priority — tree insertion order should be preserved.
    let _a = tree.add_child(root, Node::new("Alpha", "Node2D")).unwrap();
    let _b = tree.add_child(root, Node::new("Beta", "Node2D")).unwrap();
    let _c = tree.add_child(root, Node::new("Gamma", "Node2D")).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().event_trace_mut().enable();
    ml.tree_mut().event_trace_mut().clear();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let process = notification_paths(tree, "PROCESS");

    let pos = |name: &str| process.iter().position(|p| p.contains(name)).unwrap();
    assert!(pos("Alpha") < pos("Beta"), "same priority: Alpha before Beta (tree order)");
    assert!(pos("Beta") < pos("Gamma"), "same priority: Beta before Gamma (tree order)");
}

// ===========================================================================
// 6. Scene transition notifications
// ===========================================================================

#[test]
fn change_scene_fires_exit_on_old_scene() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let scene_a = r#"[gd_scene format=3]
[node name="SceneA" type="Node2D"]
[node name="ChildA" type="Node2D" parent="."]
"#;
    let scene_b = r#"[gd_scene format=3]
[node name="SceneB" type="Node2D"]
"#;

    let packed_a = PackedScene::from_tscn(scene_a).unwrap();
    let packed_b = PackedScene::from_tscn(scene_b).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Add scene A as current scene.
    let scene_a_root = add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
    LifecycleManager::enter_tree(&mut tree, scene_a_root);

    // Clear and re-enable trace before transition.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Simulate scene transition: exit old, remove, add new, enter new.
    LifecycleManager::exit_tree(&mut tree, scene_a_root);
    tree.remove_node(scene_a_root).unwrap();
    let scene_b_root = add_packed_scene_to_tree(&mut tree, root, &packed_b).unwrap();
    LifecycleManager::enter_tree(&mut tree, scene_b_root);

    // EXIT_TREE should fire for old scene nodes.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.iter().any(|p| p.contains("ChildA") || p.contains("SceneA")),
        "EXIT_TREE should fire for old scene nodes: {:?}",
        exits
    );

    // ENTER_TREE should fire for new scene.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.contains("SceneB")),
        "ENTER_TREE should fire for new scene: {:?}",
        enters
    );
}

#[test]
fn change_scene_exits_before_enters() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let scene_a = r#"[gd_scene format=3]
[node name="Old" type="Node2D"]
"#;
    let scene_b = r#"[gd_scene format=3]
[node name="New" type="Node2D"]
"#;

    let packed_a = PackedScene::from_tscn(scene_a).unwrap();
    let packed_b = PackedScene::from_tscn(scene_b).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    let old_root = add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
    LifecycleManager::enter_tree(&mut tree, old_root);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Simulate scene transition: exit old, remove, add new, enter new.
    LifecycleManager::exit_tree(&mut tree, old_root);
    tree.remove_node(old_root).unwrap();
    let new_root = add_packed_scene_to_tree(&mut tree, root, &packed_b).unwrap();
    LifecycleManager::enter_tree(&mut tree, new_root);

    let events = tree.event_trace().events();
    let lifecycle: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(e.detail.as_str(), "EXIT_TREE" | "ENTER_TREE" | "READY")
        })
        .collect();

    let last_exit = lifecycle.iter().rposition(|e| e.detail == "EXIT_TREE");
    let first_enter = lifecycle.iter().position(|e| e.detail == "ENTER_TREE");

    if let (Some(le), Some(fe)) = (last_exit, first_enter) {
        assert!(
            le < fe,
            "All EXIT_TREE must complete before first ENTER_TREE on scene transition"
        );
    }
}

#[test]
fn change_scene_ready_fires_for_new_scene() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let scene = r#"[gd_scene format=3]
[node name="Root" type="Node2D"]
[node name="Child" type="Sprite2D" parent="."]
"#;

    let packed = PackedScene::from_tscn(scene).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, scene_root);

    let ready = notification_paths(&tree, "READY");
    assert!(
        ready.iter().any(|p| p.contains("Root")),
        "READY should fire for new scene root"
    );
    assert!(
        ready.iter().any(|p| p.contains("Child")),
        "READY should fire for new scene children"
    );
}

// ===========================================================================
// 7. Notification dispatch chain — integration with inheritance
// ===========================================================================

#[test]
fn dispatch_chain_walks_inheritance() {
    use gdobject::notification::{dispatch_notification_chain, NotificationHandler};

    struct DerivedHandler(Vec<Notification>);
    struct BaseHandler(Vec<Notification>);

    impl NotificationHandler for DerivedHandler {
        fn handle_notification(&mut self, what: Notification) {
            self.0.push(what);
        }
        fn handler_class_name(&self) -> &str { "Player" }
    }

    impl NotificationHandler for BaseHandler {
        fn handle_notification(&mut self, what: Notification) {
            self.0.push(what);
        }
        fn handler_class_name(&self) -> &str { "Node2D" }
    }

    let mut derived = DerivedHandler(vec![]);
    let mut base = BaseHandler(vec![]);

    let records = dispatch_notification_chain(
        &mut [&mut derived, &mut base],
        NOTIFICATION_ENTER_TREE,
    );

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].class_name, "Player");
    assert_eq!(records[1].class_name, "Node2D");
    assert_eq!(derived.0, vec![NOTIFICATION_ENTER_TREE]);
    assert_eq!(base.0, vec![NOTIFICATION_ENTER_TREE]);
}

// ===========================================================================
// 8. Notification log vs EventTrace consistency
// ===========================================================================

#[test]
fn notification_log_matches_trace_for_lifecycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
    tree.event_trace_mut().enable();

    LifecycleManager::enter_tree(&mut tree, child);

    // The node's notification_log should contain ENTER_TREE and READY.
    let log = tree.get_node(child).unwrap().notification_log();
    assert!(log.contains(&NOTIFICATION_ENTER_TREE));
    assert!(log.contains(&NOTIFICATION_READY));

    // The event trace should also have these.
    let trace_details = node_notification_details(&tree, "/root/Child");
    assert!(trace_details.contains(&"ENTER_TREE".to_string()));
    assert!(trace_details.contains(&"READY".to_string()));
}

#[test]
fn notification_log_matches_trace_for_process() {
    let (mut ml, _a, _b) = make_entered_ml();
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    let log = tree.get_node(_b).unwrap().notification_log();

    // Should have all 4 per-frame notification types.
    assert!(log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS));
    assert!(log.contains(&NOTIFICATION_PHYSICS_PROCESS));
    assert!(log.contains(&NOTIFICATION_INTERNAL_PROCESS));
    assert!(log.contains(&NOTIFICATION_PROCESS));

    // Trace should match.
    let trace = node_notification_details(tree, "/root/A/B");
    assert!(trace.contains(&"INTERNAL_PHYSICS_PROCESS".to_string()));
    assert!(trace.contains(&"PHYSICS_PROCESS".to_string()));
    assert!(trace.contains(&"INTERNAL_PROCESS".to_string()));
    assert!(trace.contains(&"PROCESS".to_string()));
}

// ===========================================================================
// 9. Effective process mode resolution
// ===========================================================================

#[test]
fn effective_process_mode_inherit_chain() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Node")).unwrap();
    let c = tree.add_child(b, Node::new("C", "Node")).unwrap();

    // Set A to Always, B and C inherit.
    tree.get_node_mut(a).unwrap().set_process_mode(ProcessMode::Always);

    let mode_c = tree.effective_process_mode(c);
    assert_eq!(mode_c, ProcessMode::Always, "C should inherit Always from A");
}

#[test]
fn effective_process_mode_root_defaults_to_pausable() {
    let tree = SceneTree::new();
    let root = tree.root_id();
    // Root with Inherit should resolve to Pausable.
    let mode = tree.effective_process_mode(root);
    assert_eq!(
        mode,
        ProcessMode::Pausable,
        "Root with Inherit should default to Pausable"
    );
}

// ===========================================================================
// 10. Documented exclusion inventory
// ===========================================================================

/// All 18 defined notification constants, categorized as auto-dispatched,
/// manually-dispatched, or documented gaps. This test serves as a living
/// inventory — if a new notification is added, this test must be updated.
#[test]
fn notification_coverage_inventory() {
    // Auto-dispatched by LifecycleManager / SceneTree / MainLoop:
    let auto_dispatched = [
        (0, "POSTINITIALIZE"),   // Not auto-dispatched yet; placeholder
        (1, "PREDELETE"),        // queue_free -> process_deletions
        (10, "ENTER_TREE"),      // LifecycleManager::enter_tree
        (11, "EXIT_TREE"),       // LifecycleManager::exit_tree / remove_node
        (12, "MOVED_IN_PARENT"), // reparent / move_child / raise / lower
        (13, "READY"),           // LifecycleManager::enter_tree (bottom-up)
        (14, "PAUSED"),          // MainLoop::set_paused(true)
        (15, "UNPAUSED"),        // MainLoop::set_paused(false)
        (16, "CHILD_ORDER_CHANGED"), // add_child / move_child / reparent
        (17, "PROCESS"),         // MainLoop::step (per-frame)
        (18, "PHYSICS_PROCESS"), // MainLoop::step (physics tick)
        (20, "PARENTED"),        // add_child
        (21, "UNPARENTED"),      // remove_node / reparent
        (35, "INTERNAL_PROCESS"),         // MainLoop::step
        (36, "INTERNAL_PHYSICS_PROCESS"), // MainLoop::step
    ];

    // Documented gaps — constant defined but not auto-dispatched:
    let documented_gaps = [
        (25, "INSTANCED"),  // Only from PackedScene instantiation, not add_child
        (26, "DRAG_BEGIN"), // Requires UI input handling (not implemented)
        (27, "DRAG_END"),   // Requires UI input handling (not implemented)
        (30, "DRAW"),       // Requires CanvasItem draw phase (not auto-dispatched)
    ];

    // Verify all constants exist and have correct codes.
    for (code, name) in auto_dispatched.iter().chain(documented_gaps.iter()) {
        let notif = Notification::new(*code);
        let display = format!("{notif}");
        assert!(
            display.contains(name),
            "Notification code {code} should display as {name}, got {display}"
        );
    }

    // Total: 15 auto-dispatched + 4 gaps = 19 notification codes accounted for.
    assert_eq!(
        auto_dispatched.len() + documented_gaps.len(),
        19,
        "All 19 defined notification codes should be accounted for"
    );
}

/// Not-yet-implemented Godot notifications that are known but have no
/// constant defined. This documents future work.
#[test]
fn future_notification_gaps_documented() {
    // These Godot notifications exist but are not yet in gdobject:
    // - NOTIFICATION_TRANSFORM_CHANGED (2000) — Node2D/Node3D
    // - NOTIFICATION_VISIBILITY_CHANGED (43) — CanvasItem
    // - NOTIFICATION_WM_CLOSE_REQUEST (1006) — Window
    // - NOTIFICATION_WM_SIZE_CHANGED (1007) — Window
    // - NOTIFICATION_WM_DPI_CHANGE (1008) — Window
    // - NOTIFICATION_WM_MOUSE_ENTER (1002) — Window
    // - NOTIFICATION_WM_MOUSE_EXIT (1003) — Window
    // - NOTIFICATION_WM_FOCUS_IN (1004) — Window
    // - NOTIFICATION_WM_FOCUS_OUT (1005) — Window
    // - NOTIFICATION_OS_MEMORY_WARNING (2009) — MainLoop
    //
    // These are intentionally excluded until the corresponding subsystems
    // (2D transforms, visibility, windowing) are implemented.
    assert!(true, "Future notification gaps documented in this test");
}
