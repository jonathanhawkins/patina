//! pat-a0w: Broadened notification and lifecycle parity coverage.
//!
//! Tests notification ordering for large trees (20+ nodes), late-added nodes,
//! and nodes that add children during lifecycle events.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;

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

fn all_notification_details(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.event_type == TraceEventType::Notification)
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

// ===========================================================================
// 1. Large tree (20+ nodes) — ENTER_TREE ordering
// ===========================================================================

#[test]
fn enter_tree_20_node_flat_tree_top_down() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();

    for i in 0..20 {
        let child = Node::new(&format!("C{i}"), "Node2D");
        tree.add_child(pid, child).unwrap();
    }

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, root);

    let paths = notification_paths(&tree, "ENTER_TREE");
    // Root first, then Parent, then 20 children
    assert_eq!(paths[0], "/root");
    assert_eq!(paths[1], "/root/Parent");
    assert_eq!(paths.len(), 22); // root + Parent + 20 children

    // All children should follow parent
    for path in &paths[2..] {
        assert!(path.starts_with("/root/Parent/C"));
    }
}

#[test]
fn ready_20_node_flat_tree_bottom_up() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();

    for i in 0..20 {
        let child = Node::new(&format!("C{i}"), "Node2D");
        tree.add_child(pid, child).unwrap();
    }

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, root);

    let ready_paths = notification_paths(&tree, "READY");
    // READY fires bottom-up: children first, then Parent, then root
    assert!(!ready_paths.is_empty());

    // Root should be last (or near last)
    assert_eq!(*ready_paths.last().unwrap(), "/root");

    // Parent should be second-to-last
    let parent_idx = ready_paths
        .iter()
        .position(|p| p == "/root/Parent")
        .unwrap();
    let root_idx = ready_paths.iter().position(|p| p == "/root").unwrap();
    assert!(parent_idx < root_idx, "Parent READY before root");
}

// ===========================================================================
// 2. Deep tree (chain of 20) — correct ordering
// ===========================================================================

#[test]
fn enter_tree_deep_chain_20_levels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut parent = root;
    for i in 0..20 {
        let node = Node::new(&format!("D{i}"), "Node2D");
        parent = tree.add_child(parent, node).unwrap();
    }

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, root);

    let paths = notification_paths(&tree, "ENTER_TREE");
    // Should be top-down: /root, /root/D0, /root/D0/D1, ...
    assert_eq!(paths[0], "/root");
    for i in 0..20 {
        assert!(
            paths[i + 1].ends_with(&format!("D{i}")),
            "path {} should end with D{i}",
            paths[i + 1]
        );
    }

    let ready_paths = notification_paths(&tree, "READY");
    // READY bottom-up: deepest first
    assert!(ready_paths[0].contains("D19"));
    assert_eq!(*ready_paths.last().unwrap(), "/root");
}

// ===========================================================================
// 3. Late-added nodes get PARENTED and ENTER_TREE
// ===========================================================================

#[test]
fn late_added_node_gets_parented() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Enter tree for root first
    LifecycleManager::enter_tree(&mut tree, root);

    tree.event_trace_mut().enable();

    // Add child after root is in tree
    let late_child = Node::new("LateChild", "Node2D");
    let _late_id = tree.add_child(root, late_child).unwrap();

    let details = all_notification_details(&tree);

    // Should see PARENTED for the late child
    let parented: Vec<_> = details.iter().filter(|(_, d)| d == "PARENTED").collect();
    assert_eq!(parented.len(), 1);
    assert_eq!(parented[0].0, "/root/LateChild");
}

#[test]
fn late_added_subtree_gets_notifications() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let existing = Node::new("Existing", "Node2D");
    let existing_id = tree.add_child(root, existing).unwrap();

    LifecycleManager::enter_tree(&mut tree, root);

    tree.event_trace_mut().enable();

    // Add subtree: Late -> LateSub
    let late = Node::new("Late", "Node2D");
    let late_id = tree.add_child(existing_id, late).unwrap();

    let late_sub = Node::new("LateSub", "Sprite2D");
    let _late_sub_id = tree.add_child(late_id, late_sub).unwrap();

    let parented_paths = notification_paths(&tree, "PARENTED");
    assert!(parented_paths.contains(&"/root/Existing/Late".to_string()));
    assert!(parented_paths.contains(&"/root/Existing/Late/LateSub".to_string()));
}

// ===========================================================================
// 4. EXIT_TREE ordering on large tree
// ===========================================================================

#[test]
fn exit_tree_20_nodes_bottom_up() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();

    for i in 0..20 {
        let child = Node::new(&format!("C{i}"), "Node2D");
        tree.add_child(pid, child).unwrap();
    }

    LifecycleManager::enter_tree(&mut tree, root);
    tree.event_trace_mut().clear();
    tree.event_trace_mut().enable();

    LifecycleManager::exit_tree(&mut tree, root);

    let exit_paths = notification_paths(&tree, "EXIT_TREE");
    assert!(!exit_paths.is_empty());

    // Root should exit last (bottom-up: children first, then parent, then root)
    assert_eq!(*exit_paths.last().unwrap(), "/root");
}

// ===========================================================================
// 5. CHILD_ORDER_CHANGED fires for each add_child
// ===========================================================================

#[test]
fn child_order_changed_fires_for_each_of_20_adds() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    for i in 0..20 {
        let child = Node::new(&format!("C{i}"), "Node2D");
        tree.add_child(root, child).unwrap();
    }

    let changed = notification_paths(&tree, "CHILD_ORDER_CHANGED");
    // Every add_child should fire CHILD_ORDER_CHANGED on the parent
    assert_eq!(changed.len(), 20, "should fire once per add_child");
    for path in &changed {
        assert_eq!(path, "/root");
    }
}

// ===========================================================================
// 6. Mixed lifecycle: add, enter, add more, enter those too
// ===========================================================================

#[test]
fn incremental_tree_building_lifecycle() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Phase 1: Add initial nodes
    let a = Node::new("A", "Node2D");
    let _a_id = tree.add_child(root, a).unwrap();

    // Enter tree for existing nodes
    LifecycleManager::enter_tree(&mut tree, root);

    tree.event_trace_mut().enable();

    // Phase 2: Add more nodes after tree is entered
    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(root, b).unwrap();

    let b_child = Node::new("BC", "Sprite2D");
    let _bc_id = tree.add_child(b_id, b_child).unwrap();

    let parented = notification_paths(&tree, "PARENTED");
    assert!(parented.contains(&"/root/B".to_string()));
    assert!(parented.contains(&"/root/B/BC".to_string()));
}

// ===========================================================================
// 7. Notification ordering: PARENTED before CHILD_ORDER_CHANGED
// ===========================================================================

#[test]
fn parented_fires_before_child_order_changed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.event_trace_mut().enable();

    let child = Node::new("Child", "Node2D");
    tree.add_child(root, child).unwrap();

    let details = all_notification_details(&tree);

    let parented_idx = details.iter().position(|(_, d)| d == "PARENTED").unwrap();
    let changed_idx = details
        .iter()
        .position(|(_, d)| d == "CHILD_ORDER_CHANGED")
        .unwrap();

    assert!(
        parented_idx < changed_idx,
        "PARENTED should fire before CHILD_ORDER_CHANGED"
    );
}
