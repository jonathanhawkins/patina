//! pat-gt59: Scene lifecycle parity coverage for packed-scene change transitions.
//!
//! Verifies enter_tree/ready/exit ordering when switching scenes through
//! packed-scene APIs. Acceptance: focused transition tests with explicit
//! ordering assertions.
//!
//! Since SceneTree does not yet expose high-level `change_scene_to_packed` /
//! `reload_current_scene` methods, these tests exercise the underlying
//! primitives (`add_packed_scene_to_tree`, `LifecycleManager::enter_tree`,
//! `LifecycleManager::exit_tree`, `remove_node`) to verify the lifecycle
//! contract that a future high-level API must uphold:
//!
//! 1. EXIT_TREE fires bottom-up for the departing scene
//! 2. ENTER_TREE fires top-down for the arriving scene
//! 3. READY fires bottom-up for the arriving scene
//! 4. All EXIT_TREE events precede all ENTER_TREE events during a transition
//! 5. Node counts are consistent after transitions
//! 6. Back-to-back transitions (A→B→C) maintain invariants
//! 7. Transitions from deep hierarchies to single nodes work correctly
//! 8. ENTER_TREE/READY counts match the incoming scene's node count
//! 9. EXIT_TREE counts match the departing scene's node count
//! 10. Re-instancing the same packed scene produces a fresh subtree

use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
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

fn lifecycle_sequence(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(e.detail.as_str(), "ENTER_TREE" | "READY" | "EXIT_TREE")
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

fn require_pos(paths: &[String], needle: &str) -> usize {
    paths
        .iter()
        .position(|p| p.ends_with(needle))
        .unwrap_or_else(|| panic!("{needle} missing from: {paths:?}"))
}

fn packed_scene_a() -> PackedScene {
    PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="SceneA" type="Node2D"]

[node name="ChildA1" type="Node2D" parent="."]

[node name="ChildA2" type="Node2D" parent="."]
"#,
    )
    .expect("valid tscn")
}

fn packed_scene_b() -> PackedScene {
    PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="SceneB" type="Node2D"]

[node name="ChildB1" type="Node2D" parent="."]

[node name="ChildB2" type="Node2D" parent="."]

[node name="GrandchildB" type="Node2D" parent="ChildB2"]
"#,
    )
    .expect("valid tscn")
}

fn packed_scene_c() -> PackedScene {
    PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="SceneC" type="Node"]

[node name="CC1" type="Node" parent="."]
"#,
    )
    .expect("valid tscn")
}

/// Simulates a scene transition: exits the old scene subtree, removes it,
/// instances the new packed scene, adds it to root, and enters the tree.
/// Returns the NodeId of the new scene root.
fn transition_to_packed(
    tree: &mut SceneTree,
    old_scene_root: Option<NodeId>,
    new_packed: &PackedScene,
) -> NodeId {
    let root = tree.root_id();

    // Exit and remove old scene if present.
    if let Some(old_id) = old_scene_root {
        LifecycleManager::exit_tree(tree, old_id);
        tree.remove_node(old_id).unwrap();
    }

    // Instance and add new scene.
    let new_id = add_packed_scene_to_tree(tree, root, new_packed).unwrap();
    LifecycleManager::enter_tree(tree, new_id);
    new_id
}

/// Builds a tree with packed scene A already loaded and lifecycle-entered.
/// Returns (tree, scene_a_root_id). Trace is cleared after setup.
fn tree_with_packed_scene_a() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let packed = packed_scene_a();
    let scene_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, scene_id);

    // Enable trace and clear so we only capture subsequent transition events.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    (tree, scene_id)
}

// ===========================================================================
// 1. EXIT_TREE fires bottom-up for the departing scene
// ===========================================================================

#[test]
fn exit_tree_bottom_up_during_transition() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // Exit scene A.
    LifecycleManager::exit_tree(&mut tree, scene_a_id);

    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        require_pos(&exits, "ChildA1") < require_pos(&exits, "SceneA"),
        "ChildA1 exits before SceneA: {exits:?}"
    );
    assert!(
        require_pos(&exits, "ChildA2") < require_pos(&exits, "SceneA"),
        "ChildA2 exits before SceneA: {exits:?}"
    );
}

// ===========================================================================
// 2. ENTER_TREE fires top-down for the arriving scene
// ===========================================================================

#[test]
fn enter_tree_top_down_during_transition() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // Transition A → B.
    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        require_pos(&enters, "SceneB") < require_pos(&enters, "ChildB1"),
        "SceneB enters before ChildB1: {enters:?}"
    );
    assert!(
        require_pos(&enters, "SceneB") < require_pos(&enters, "ChildB2"),
        "SceneB enters before ChildB2: {enters:?}"
    );
    assert!(
        require_pos(&enters, "ChildB2") < require_pos(&enters, "GrandchildB"),
        "ChildB2 enters before GrandchildB: {enters:?}"
    );
}

// ===========================================================================
// 3. READY fires bottom-up for the arriving scene
// ===========================================================================

#[test]
fn ready_bottom_up_during_transition() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    let readys = notification_paths(&tree, "READY");
    assert!(
        require_pos(&readys, "GrandchildB") < require_pos(&readys, "ChildB2"),
        "GrandchildB ready before ChildB2: {readys:?}"
    );
    assert!(
        require_pos(&readys, "ChildB1") < require_pos(&readys, "SceneB"),
        "ChildB1 ready before SceneB: {readys:?}"
    );
    assert!(
        require_pos(&readys, "ChildB2") < require_pos(&readys, "SceneB"),
        "ChildB2 ready before SceneB: {readys:?}"
    );
}

// ===========================================================================
// 4. All EXIT_TREE events precede all ENTER_TREE during transition
// ===========================================================================

#[test]
fn all_exits_before_all_enters_during_transition() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    let seq = lifecycle_sequence(&tree);
    let last_exit = seq
        .iter()
        .rposition(|(_, d)| d == "EXIT_TREE")
        .expect("should have EXIT_TREE events");
    let first_enter = seq
        .iter()
        .position(|(_, d)| d == "ENTER_TREE")
        .expect("should have ENTER_TREE events");

    assert!(
        last_exit < first_enter,
        "All EXIT_TREE must complete before first ENTER_TREE.\nSequence: {seq:?}"
    );
}

#[test]
fn all_exits_before_all_enters_a_to_c() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    let packed_c = packed_scene_c();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_c);

    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(last_exit < first_enter, "EXIT before ENTER: {seq:?}");
}

// ===========================================================================
// 5. Node counts are consistent after transitions
// ===========================================================================

#[test]
fn node_count_correct_after_packed_transition() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // SceneA has 3 nodes + root = 4.
    assert_eq!(tree.node_count(), 4);

    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    // SceneB has 4 nodes (SceneB, ChildB1, ChildB2, GrandchildB) + root = 5.
    assert_eq!(tree.node_count(), 5);
}

#[test]
fn node_count_after_transition_to_smaller_scene() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();
    assert_eq!(tree.node_count(), 4);

    let packed_c = packed_scene_c();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_c);

    // SceneC has 2 nodes + root = 3.
    assert_eq!(tree.node_count(), 3);
}

#[test]
fn node_count_one_after_exit_and_remove() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    LifecycleManager::exit_tree(&mut tree, scene_a_id);
    tree.remove_node(scene_a_id).unwrap();

    // Only root remains.
    assert_eq!(tree.node_count(), 1);
}

// ===========================================================================
// 6. Back-to-back transitions (A → B → C) maintain invariants
// ===========================================================================

#[test]
fn back_to_back_packed_transitions_preserve_invariants() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // A → B
    let packed_b = packed_scene_b();
    let scene_b_id = transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    tree.event_trace_mut().clear();

    // B → C
    let packed_c = packed_scene_c();
    transition_to_packed(&mut tree, Some(scene_b_id), &packed_c);

    // Verify B exited.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.iter().any(|p| p.ends_with("SceneB")),
        "SceneB should exit: {exits:?}"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("GrandchildB")),
        "GrandchildB should exit: {exits:?}"
    );

    // Verify C entered.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.ends_with("SceneC")),
        "SceneC should enter: {enters:?}"
    );

    // Verify EXIT before ENTER.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(last_exit < first_enter, "EXIT before ENTER: {seq:?}");

    // Final node count: root + SceneC + CC1 = 3.
    assert_eq!(tree.node_count(), 3);
}

#[test]
fn three_consecutive_transitions() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // A → B → C → A
    let packed_b = packed_scene_b();
    let scene_b_id = transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    let packed_c = packed_scene_c();
    let scene_c_id = transition_to_packed(&mut tree, Some(scene_b_id), &packed_c);

    let packed_a2 = packed_scene_a();
    transition_to_packed(&mut tree, Some(scene_c_id), &packed_a2);

    // Back to scene A structure: root + SceneA + ChildA1 + ChildA2 = 4.
    assert_eq!(tree.node_count(), 4);
}

// ===========================================================================
// 7. Transition from deep hierarchy to single node
// ===========================================================================

#[test]
fn deep_to_single_node_transition() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build 4-level deep hierarchy manually.
    let l0 = tree.add_child(root, Node::new("Deep0", "Node")).unwrap();
    let l1 = tree.add_child(l0, Node::new("Deep1", "Node")).unwrap();
    let l2 = tree.add_child(l1, Node::new("Deep2", "Node")).unwrap();
    tree.add_child(l2, Node::new("Deep3", "Node")).unwrap();

    LifecycleManager::enter_tree(&mut tree, l0);
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Exit deep hierarchy.
    LifecycleManager::exit_tree(&mut tree, l0);

    // All 4 deep nodes should exit bottom-up.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        require_pos(&exits, "Deep3") < require_pos(&exits, "Deep2"),
        "Deep3 exits before Deep2: {exits:?}"
    );
    assert!(
        require_pos(&exits, "Deep2") < require_pos(&exits, "Deep1"),
        "Deep2 exits before Deep1: {exits:?}"
    );
    assert!(
        require_pos(&exits, "Deep1") < require_pos(&exits, "Deep0"),
        "Deep1 exits before Deep0: {exits:?}"
    );

    // Remove deep hierarchy.
    tree.remove_node(l0).unwrap();
    tree.event_trace_mut().clear();

    // Add single flat node.
    let flat = tree.add_child(root, Node::new("Flat", "Node2D")).unwrap();
    LifecycleManager::enter_tree(&mut tree, flat);

    // Only root + Flat remain.
    assert_eq!(tree.node_count(), 2);

    // Verify single node got ENTER_TREE and READY.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters.len(), 1, "single node enter: {enters:?}");
    let readys = notification_paths(&tree, "READY");
    assert_eq!(readys.len(), 1, "single node ready: {readys:?}");
}

// ===========================================================================
// 8. ENTER_TREE / READY counts match incoming scene's node count
// ===========================================================================

#[test]
fn enter_and_ready_counts_match_new_scene_size() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    // SceneB has 4 nodes: SceneB, ChildB1, ChildB2, GrandchildB.
    let enters = notification_paths(&tree, "ENTER_TREE");
    let new_enters: Vec<_> = enters
        .iter()
        .filter(|p| p.contains("SceneB") || p.contains("ChildB") || p.contains("GrandchildB"))
        .collect();
    assert_eq!(
        new_enters.len(),
        4,
        "expected 4 ENTER_TREE events for SceneB hierarchy, got {new_enters:?}"
    );

    let readys = notification_paths(&tree, "READY");
    let new_readys: Vec<_> = readys
        .iter()
        .filter(|p| p.contains("SceneB") || p.contains("ChildB") || p.contains("GrandchildB"))
        .collect();
    assert_eq!(
        new_readys.len(),
        4,
        "expected 4 READY events for SceneB hierarchy, got {new_readys:?}"
    );
}

// ===========================================================================
// 9. EXIT_TREE counts match departing scene's node count
// ===========================================================================

#[test]
fn exit_count_matches_old_scene_size() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    LifecycleManager::exit_tree(&mut tree, scene_a_id);

    // SceneA had 3 nodes: SceneA, ChildA1, ChildA2.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        exits.len(),
        3,
        "expected 3 EXIT_TREE events for SceneA hierarchy, got {exits:?}"
    );
}

#[test]
fn exit_count_matches_deep_scene() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // First transition to scene B (4 nodes).
    let packed_b = packed_scene_b();
    let scene_b_id = transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    tree.event_trace_mut().clear();

    // Exit scene B.
    LifecycleManager::exit_tree(&mut tree, scene_b_id);

    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        exits.len(),
        4,
        "expected 4 EXIT_TREE events for SceneB hierarchy, got {exits:?}"
    );
}

// ===========================================================================
// 10. Re-instancing the same packed scene produces a fresh subtree
// ===========================================================================

#[test]
fn reinstancing_same_scene_produces_fresh_nodes() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    // Exit and remove scene A.
    LifecycleManager::exit_tree(&mut tree, scene_a_id);
    tree.remove_node(scene_a_id).unwrap();

    tree.event_trace_mut().clear();

    // Re-instance scene A.
    let root = tree.root_id();
    let packed = packed_scene_a();
    let new_scene_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, new_scene_id);

    // New scene root must have a different NodeId.
    assert_ne!(
        scene_a_id, new_scene_id,
        "re-instanced scene must have a new NodeId"
    );

    // Same structure: root + SceneA + ChildA1 + ChildA2 = 4.
    assert_eq!(tree.node_count(), 4);

    // Lifecycle fired for the new instance.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters.len(), 3, "3 ENTER_TREE for fresh SceneA: {enters:?}");
    let readys = notification_paths(&tree, "READY");
    assert_eq!(readys.len(), 3, "3 READY for fresh SceneA: {readys:?}");
}

#[test]
fn reinstancing_same_scene_twice_produces_unique_ids() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let packed = packed_scene_a();

    // Instance 1.
    let id1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, id1);
    LifecycleManager::exit_tree(&mut tree, id1);
    tree.remove_node(id1).unwrap();

    // Instance 2.
    let id2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, id2);
    LifecycleManager::exit_tree(&mut tree, id2);
    tree.remove_node(id2).unwrap();

    // Instance 3.
    let id3 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    LifecycleManager::enter_tree(&mut tree, id3);

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
    assert_eq!(tree.node_count(), 4); // root + SceneA + 2 children
}

// ===========================================================================
// 11. Multiple root children: transition replaces all
// ===========================================================================

#[test]
fn transition_replaces_all_root_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add multiple top-level children.
    let top_a = tree.add_child(root, Node::new("TopA", "Node")).unwrap();
    let top_b = tree.add_child(root, Node::new("TopB", "Node")).unwrap();
    let top_c = tree.add_child(root, Node::new("TopC", "Node")).unwrap();

    LifecycleManager::enter_tree(&mut tree, top_a);
    LifecycleManager::enter_tree(&mut tree, top_b);
    LifecycleManager::enter_tree(&mut tree, top_c);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Exit all three, then remove them.
    LifecycleManager::exit_tree(&mut tree, top_a);
    LifecycleManager::exit_tree(&mut tree, top_b);
    LifecycleManager::exit_tree(&mut tree, top_c);
    tree.remove_node(top_a).unwrap();
    tree.remove_node(top_b).unwrap();
    tree.remove_node(top_c).unwrap();

    // All three old children should have exited.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.iter().any(|p| p.ends_with("TopA")),
        "TopA should exit"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("TopB")),
        "TopB should exit"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("TopC")),
        "TopC should exit"
    );

    // Add new scene.
    tree.event_trace_mut().clear();
    let packed_c = packed_scene_c();
    let new_id = add_packed_scene_to_tree(&mut tree, root, &packed_c).unwrap();
    LifecycleManager::enter_tree(&mut tree, new_id);

    // Only root + SceneC + CC1 remain.
    assert_eq!(tree.node_count(), 3);
}

// ===========================================================================
// 12. Transition from empty root to packed scene
// ===========================================================================

#[test]
fn transition_from_empty_root_to_packed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _ = root; // suppress unused warning

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // No old scene — direct add.
    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, None, &packed_b);

    // No EXIT_TREE should fire (tree was empty).
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.is_empty(), "no EXIT_TREE from empty root: {exits:?}");

    // ENTER_TREE should fire for all new nodes.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters.len(), 4, "4 ENTER_TREE for SceneB: {enters:?}");

    // SceneB has 4 nodes + root = 5.
    assert_eq!(tree.node_count(), 5);
}

// ===========================================================================
// 13. Full lifecycle sequence invariants during A → B
// ===========================================================================

#[test]
fn full_transition_sequence_invariants() {
    let (mut tree, scene_a_id) = tree_with_packed_scene_a();

    let packed_b = packed_scene_b();
    transition_to_packed(&mut tree, Some(scene_a_id), &packed_b);

    let seq = lifecycle_sequence(&tree);

    // Count events by type.
    let exit_count = seq.iter().filter(|(_, d)| d == "EXIT_TREE").count();
    let enter_count = seq.iter().filter(|(_, d)| d == "ENTER_TREE").count();
    let ready_count = seq.iter().filter(|(_, d)| d == "READY").count();

    // SceneA has 3 nodes, SceneB has 4 nodes.
    assert_eq!(exit_count, 3, "3 EXIT_TREE for SceneA");
    assert_eq!(enter_count, 4, "4 ENTER_TREE for SceneB");
    assert_eq!(ready_count, 4, "4 READY for SceneB");

    // Order: all exits, then all enters, then... actually READY is interleaved
    // with ENTER_TREE (they happen during the same enter_tree call).
    // The key invariant: all EXIT_TREE before first ENTER_TREE.
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(
        last_exit < first_enter,
        "all exits before first enter: {seq:?}"
    );

    // Within ENTER_TREE: top-down (SceneB before children).
    let enter_paths: Vec<_> = seq
        .iter()
        .filter(|(_, d)| d == "ENTER_TREE")
        .map(|(p, _)| p.clone())
        .collect();
    assert!(
        require_pos(&enter_paths, "SceneB") < require_pos(&enter_paths, "ChildB1"),
        "SceneB enters before ChildB1"
    );

    // Within READY: bottom-up (GrandchildB before ChildB2 before SceneB).
    let ready_paths: Vec<_> = seq
        .iter()
        .filter(|(_, d)| d == "READY")
        .map(|(p, _)| p.clone())
        .collect();
    assert!(
        require_pos(&ready_paths, "GrandchildB") < require_pos(&ready_paths, "SceneB"),
        "GrandchildB ready before SceneB"
    );
}
