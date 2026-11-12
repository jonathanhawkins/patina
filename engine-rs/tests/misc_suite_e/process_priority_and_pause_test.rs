//! Tests for process priority (pat-15a) and pause mode (pat-518).

use gdobject::notification::{NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_PROCESS};
use gdscene::node::{Node, ProcessMode};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdscene::NodeId;

// ===========================================================================
// Helpers
// ===========================================================================

/// Creates a MainLoop, adds named children under root, returns (ml, child_ids).
fn make_loop_with_children(names: &[&str]) -> (MainLoop, Vec<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for &name in names {
        let child = Node::new(name, "Node");
        let id = tree.add_child(root, child).unwrap();
        ids.push(id);
    }
    (MainLoop::new(tree), ids)
}

/// Counts how many times a given notification appears in a node's log.
fn count_notif(
    ml: &MainLoop,
    node_id: NodeId,
    notif: gdobject::notification::Notification,
) -> usize {
    ml.tree()
        .get_node(node_id)
        .map(|n| {
            n.notification_log()
                .iter()
                .filter(|&&n2| n2 == notif)
                .count()
        })
        .unwrap_or(0)
}

// ===========================================================================
// Process Priority tests
// ===========================================================================

#[test]
fn default_process_priority_is_zero() {
    let node = Node::new("N", "Node");
    assert_eq!(node.process_priority(), 0);
}

#[test]
fn set_and_get_process_priority() {
    let mut node = Node::new("N", "Node");
    node.set_process_priority(42);
    assert_eq!(node.process_priority(), 42);
    node.set_process_priority(-5);
    assert_eq!(node.process_priority(), -5);
}

#[test]
fn lower_priority_processes_first() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    // A has priority 1, B has priority 0 -> B should process first.
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(1);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(0);

    ml.step(1.0 / 60.0);

    // B (priority 0) should have gotten PROCESS before A (priority 1).
    // Check the tree's process order method directly.
    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(
        pos_b < pos_a,
        "B (priority 0) should come before A (priority 1)"
    );
}

#[test]
fn negative_priority_processes_before_zero() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(0);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-1);

    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(
        pos_b < pos_a,
        "B (priority -1) should come before A (priority 0)"
    );
}

#[test]
fn same_priority_preserves_tree_order() {
    let (ml, ids) = make_loop_with_children(&["A", "B", "C"]);
    // All have default priority 0 — should be in tree order.
    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    let pos_c = order.iter().position(|&id| id == ids[2]).unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);
}

#[test]
fn priority_applies_to_physics_process() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(10);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-10);

    ml.step(1.0 / 60.0);

    // Both should get physics process notifications.
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS) > 0);
    assert!(count_notif(&ml, ids[1], NOTIFICATION_PHYSICS_PROCESS) > 0);

    // Verify ordering via process order.
    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(pos_b < pos_a, "B (priority -10) before A (priority 10)");
}

#[test]
fn priority_change_affects_next_frame() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(0);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(1);

    // Before change: A before B.
    let order1 = ml.tree().all_nodes_in_process_order();
    let pos_a1 = order1.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b1 = order1.iter().position(|&id| id == ids[1]).unwrap();
    assert!(pos_a1 < pos_b1);

    // Change B to -1.
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-1);

    // After change: B before A.
    let order2 = ml.tree().all_nodes_in_process_order();
    let pos_a2 = order2.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b2 = order2.iter().position(|&id| id == ids[1]).unwrap();
    assert!(
        pos_b2 < pos_a2,
        "B (now priority -1) should come before A (priority 0)"
    );
}

#[test]
fn mixed_priorities_across_tree_levels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // root -> parent -> grandchild
    //      -> sibling
    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let grandchild = Node::new("Grandchild", "Node");
    let grandchild_id = tree.add_child(parent_id, grandchild).unwrap();
    let sibling = Node::new("Sibling", "Node");
    let sibling_id = tree.add_child(root, sibling).unwrap();

    // Grandchild: -1, Sibling: 0, Parent: 1
    tree.get_node_mut(grandchild_id)
        .unwrap()
        .set_process_priority(-1);
    tree.get_node_mut(sibling_id)
        .unwrap()
        .set_process_priority(0);
    tree.get_node_mut(parent_id)
        .unwrap()
        .set_process_priority(1);

    let order = tree.all_nodes_in_process_order();
    let pos_gc = order.iter().position(|&id| id == grandchild_id).unwrap();
    let pos_sib = order.iter().position(|&id| id == sibling_id).unwrap();
    let pos_par = order.iter().position(|&id| id == parent_id).unwrap();

    assert!(pos_gc < pos_sib, "Grandchild (-1) before Sibling (0)");
    assert!(pos_sib < pos_par, "Sibling (0) before Parent (1)");
}

#[test]
fn extreme_priority_values() {
    let (mut ml, ids) = make_loop_with_children(&["Min", "Max"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(i32::MIN);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(i32::MAX);

    let order = ml.tree().all_nodes_in_process_order();
    let pos_min = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_max = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(pos_min < pos_max, "i32::MIN should process before i32::MAX");

    // Verify they both still get processed.
    ml.step(1.0 / 60.0);
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
    assert!(count_notif(&ml, ids[1], NOTIFICATION_PROCESS) > 0);
}

// ===========================================================================
// Pause Mode tests
// ===========================================================================

#[test]
fn default_process_mode_is_inherit() {
    let node = Node::new("N", "Node");
    assert_eq!(node.process_mode(), ProcessMode::Inherit);
}

#[test]
fn set_and_get_process_mode() {
    let mut node = Node::new("N", "Node");
    node.set_process_mode(ProcessMode::Always);
    assert_eq!(node.process_mode(), ProcessMode::Always);
    node.set_process_mode(ProcessMode::Disabled);
    assert_eq!(node.process_mode(), ProcessMode::Disabled);
}

#[test]
fn pausable_node_stops_when_tree_paused() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Pausable);
    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS), 0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS), 0);
}

#[test]
fn always_node_processes_when_paused() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS) > 0);
}

#[test]
fn when_paused_node_only_processes_when_paused() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::WhenPaused);

    // Not paused -> should NOT process.
    ml.step(1.0 / 60.0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS), 0);

    // Paused -> should process.
    ml.set_paused(true);
    ml.step(1.0 / 60.0);
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
}

#[test]
fn disabled_node_never_processes() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);

    // Not paused.
    ml.step(1.0 / 60.0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS), 0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS), 0);

    // Paused.
    ml.set_paused(true);
    ml.step(1.0 / 60.0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS), 0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS), 0);
}

#[test]
fn inherit_uses_parent_mode() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("Child", "Node");
    let child_id = tree.add_child(parent_id, child).unwrap();

    // Parent = Always, Child = Inherit -> effective = Always.
    tree.get_node_mut(parent_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);

    let mut ml = MainLoop::new(tree);
    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    // Child inherits Always from parent -> processes even when paused.
    assert!(count_notif(&ml, child_id, NOTIFICATION_PROCESS) > 0);
}

#[test]
fn inherit_chain_resolves_through_ancestors() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = Node::new("A", "Node");
    let a_id = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "Node");
    let b_id = tree.add_child(a_id, b).unwrap();
    let c = Node::new("C", "Node");
    let c_id = tree.add_child(b_id, c).unwrap();

    // A = Always, B = Inherit, C = Inherit -> C's effective = Always.
    tree.get_node_mut(a_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);

    assert_eq!(tree.effective_process_mode(c_id), ProcessMode::Always);
    assert_eq!(tree.effective_process_mode(b_id), ProcessMode::Always);
}

#[test]
fn root_inherit_defaults_to_pausable() {
    let tree = SceneTree::new();
    let root = tree.root_id();
    // Root has Inherit (default) -> should resolve as Pausable.
    assert_eq!(tree.effective_process_mode(root), ProcessMode::Pausable);
}

#[test]
fn unpause_resumes_pausable_nodes() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Pausable);

    ml.set_paused(true);
    ml.step(1.0 / 60.0);
    assert_eq!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS), 0);

    ml.set_paused(false);
    ml.step(1.0 / 60.0);
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
}

#[test]
fn when_paused_stops_after_unpause() {
    let (mut ml, ids) = make_loop_with_children(&["Child"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::WhenPaused);

    ml.set_paused(true);
    ml.step(1.0 / 60.0);
    let count_paused = count_notif(&ml, ids[0], NOTIFICATION_PROCESS);
    assert!(
        count_paused > 0,
        "WhenPaused node should process when paused"
    );

    ml.set_paused(false);
    ml.step(1.0 / 60.0);
    // Count should not increase after unpausing.
    let count_after = count_notif(&ml, ids[0], NOTIFICATION_PROCESS);
    assert_eq!(
        count_after, count_paused,
        "WhenPaused node should stop after unpause"
    );
}

#[test]
fn pause_mode_affects_physics_process() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_mode(ProcessMode::Pausable);

    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    // Always node gets physics.
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PHYSICS_PROCESS) > 0);
    // Pausable node does not.
    assert_eq!(count_notif(&ml, ids[1], NOTIFICATION_PHYSICS_PROCESS), 0);
}

#[test]
fn always_node_with_priority() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(10);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-5);

    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    // Both Always nodes process when paused.
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
    assert!(count_notif(&ml, ids[1], NOTIFICATION_PROCESS) > 0);

    // Priority ordering: B (-5) before A (10).
    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(
        pos_b < pos_a,
        "B (priority -5) should come before A (priority 10)"
    );
}

// ===========================================================================
// Notification ordering with process_priority and pause mode (pat-a0w)
// ===========================================================================

/// Process priority ordering: lower priority nodes receive PROCESS notification first.
#[test]
fn notification_order_respects_process_priority() {
    let (mut ml, ids) = make_loop_with_children(&["High", "Low"]);
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(10);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-5);

    ml.step(1.0 / 60.0);

    // Both should receive PROCESS
    assert!(count_notif(&ml, ids[0], NOTIFICATION_PROCESS) > 0);
    assert!(count_notif(&ml, ids[1], NOTIFICATION_PROCESS) > 0);

    // Low (priority -5) should be processed before High (priority 10)
    let order = ml.tree().all_nodes_in_process_order();
    let pos_high = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_low = order.iter().position(|&id| id == ids[1]).unwrap();
    assert!(
        pos_low < pos_high,
        "Low priority (-5) should process before High priority (10)"
    );
}

/// Pause mode WhenPaused combined with process priority: priority order still respected.
#[test]
fn when_paused_respects_priority_ordering() {
    let (mut ml, ids) = make_loop_with_children(&["A", "B", "C"]);
    // All WhenPaused, different priorities
    for &id in &ids {
        ml.tree_mut()
            .get_node_mut(id)
            .unwrap()
            .set_process_mode(ProcessMode::WhenPaused);
    }
    ml.tree_mut()
        .get_node_mut(ids[0])
        .unwrap()
        .set_process_priority(5);
    ml.tree_mut()
        .get_node_mut(ids[1])
        .unwrap()
        .set_process_priority(-1);
    ml.tree_mut()
        .get_node_mut(ids[2])
        .unwrap()
        .set_process_priority(0);

    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    // All should process when paused
    for &id in &ids {
        assert!(
            count_notif(&ml, id, NOTIFICATION_PROCESS) > 0,
            "WhenPaused node should process when paused"
        );
    }

    // Priority order: B(-1) < C(0) < A(5)
    let order = ml.tree().all_nodes_in_process_order();
    let pos_a = order.iter().position(|&id| id == ids[0]).unwrap();
    let pos_b = order.iter().position(|&id| id == ids[1]).unwrap();
    let pos_c = order.iter().position(|&id| id == ids[2]).unwrap();
    assert!(pos_b < pos_c, "B (-1) before C (0)");
    assert!(pos_c < pos_a, "C (0) before A (5)");
}

/// Disabled subtree: parent Disabled blocks inherited children from processing.
#[test]
fn disabled_parent_blocks_inherited_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("Child", "Node");
    let child_id = tree.add_child(parent_id, child).unwrap();
    let grandchild = Node::new("Grandchild", "Node");
    let grandchild_id = tree.add_child(child_id, grandchild).unwrap();

    // Parent disabled, children inherit
    tree.get_node_mut(parent_id)
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // No node in the disabled subtree should process
    assert_eq!(count_notif(&ml, parent_id, NOTIFICATION_PROCESS), 0);
    assert_eq!(count_notif(&ml, child_id, NOTIFICATION_PROCESS), 0);
    assert_eq!(count_notif(&ml, grandchild_id, NOTIFICATION_PROCESS), 0);
}

/// Always child under Pausable parent still processes when paused.
#[test]
fn always_child_under_pausable_parent_processes_when_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node");
    let parent_id = tree.add_child(root, parent).unwrap();
    let child = Node::new("AlwaysChild", "Node");
    let child_id = tree.add_child(parent_id, child).unwrap();

    tree.get_node_mut(parent_id)
        .unwrap()
        .set_process_mode(ProcessMode::Pausable);
    tree.get_node_mut(child_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);

    let mut ml = MainLoop::new(tree);
    ml.set_paused(true);
    ml.step(1.0 / 60.0);

    // Parent should be paused
    assert_eq!(count_notif(&ml, parent_id, NOTIFICATION_PROCESS), 0);
    // Always child should still process
    assert!(count_notif(&ml, child_id, NOTIFICATION_PROCESS) > 0);
}
