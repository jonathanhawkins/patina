//! pat-a10w / pat-roo0 / pat-1074 / pat-n1cs: Scene lifecycle parity coverage for packed-scene change transitions.
//!
//! Verifies that ENTER_TREE / READY / EXIT_TREE ordering is correct when
//! switching scenes through `change_scene_to_node()` and
//! `change_scene_to_packed()`.
//!
//! Godot 4.6.1 contract:
//! - Old scene receives EXIT_TREE bottom-up (children before parent).
//! - New scene receives ENTER_TREE top-down (parent before children).
//! - All ENTER_TREE complete before any READY (within a single enter_tree call).
//! - READY fires bottom-up (children before parent).
//! - Old scene is fully exited before new scene enters.

use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
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

/// All lifecycle notification details in order.
fn lifecycle_sequence(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(
                    e.detail.as_str(),
                    "ENTER_TREE" | "READY" | "EXIT_TREE" | "PREDELETE"
                )
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

/// Find position of needle using `ends_with` to avoid substring false-matches
/// (e.g. "SceneA" matching "SceneA/ChildA1").
fn find_pos(paths: &[String], needle: &str) -> Option<usize> {
    paths.iter().position(|p| p.ends_with(needle))
}

fn require_pos(paths: &[String], needle: &str) -> usize {
    find_pos(paths, needle)
        .unwrap_or_else(|| panic!("{needle} missing from: {paths:?}"))
}

/// Build a scene tree with an initial scene:  root -> SceneA -> ChildA1, ChildA2.
fn tree_with_initial_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let scene_a = tree
        .add_child(root, Node::new("SceneA", "Node2D"))
        .unwrap();
    tree.add_child(scene_a, Node::new("ChildA1", "Node2D"))
        .unwrap();
    tree.add_child(scene_a, Node::new("ChildA2", "Node2D"))
        .unwrap();

    // Clear trace so we only capture transition events.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree
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

// ===========================================================================
// 1. change_scene_to_node: old scene EXIT_TREE is bottom-up
// ===========================================================================

#[test]
fn change_to_node_old_scene_exit_tree_bottom_up() {
    let mut tree = tree_with_initial_scene();

    tree.change_scene_to_node(Node::new("SceneB", "Node2D"))
        .unwrap();

    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.len() >= 3,
        "expected at least 3 EXIT_TREE events, got {exits:?}"
    );

    assert!(
        require_pos(&exits, "ChildA1") < require_pos(&exits, "/SceneA"),
        "ChildA1 EXIT before SceneA: {exits:?}"
    );
    assert!(
        require_pos(&exits, "ChildA2") < require_pos(&exits, "/SceneA"),
        "ChildA2 EXIT before SceneA: {exits:?}"
    );
}

// ===========================================================================
// 2. change_scene_to_packed: new scene ENTER_TREE is top-down
// ===========================================================================

#[test]
fn change_to_packed_new_scene_enter_tree_top_down() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        require_pos(&enters, "/SceneB") < require_pos(&enters, "ChildB1"),
        "SceneB ENTER before ChildB1: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/SceneB") < require_pos(&enters, "ChildB2"),
        "SceneB ENTER before ChildB2: {enters:?}"
    );
    assert!(
        require_pos(&enters, "ChildB2") < require_pos(&enters, "GrandchildB"),
        "ChildB2 ENTER before GrandchildB: {enters:?}"
    );
}

// ===========================================================================
// 3. change_scene_to_packed: READY is bottom-up for new scene
// ===========================================================================

#[test]
fn change_to_packed_new_scene_ready_bottom_up() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    let readys = notification_paths(&tree, "READY");
    assert!(
        require_pos(&readys, "GrandchildB") < require_pos(&readys, "/ChildB2"),
        "GrandchildB READY before ChildB2: {readys:?}"
    );
    assert!(
        require_pos(&readys, "ChildB1") < require_pos(&readys, "/SceneB"),
        "ChildB1 READY before SceneB: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/ChildB2") < require_pos(&readys, "/SceneB"),
        "ChildB2 READY before SceneB: {readys:?}"
    );
}

// ===========================================================================
// 4. change_scene_to_node: all EXIT_TREE before any ENTER_TREE
// ===========================================================================

#[test]
fn change_to_node_exit_completes_before_enter() {
    let mut tree = tree_with_initial_scene();

    tree.change_scene_to_node(Node::new("SceneB", "Node2D"))
        .unwrap();

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

// ===========================================================================
// 5. change_scene_to_packed: all ENTER_TREE before any READY (within subtree)
// ===========================================================================

#[test]
fn change_to_packed_enter_completes_before_ready() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // Filter to only new-scene events (exclude old scene's EXIT).
    let seq = lifecycle_sequence(&tree);
    let new_scene_events: Vec<_> = seq
        .iter()
        .filter(|(p, d)| {
            p.contains("SceneB") && (d == "ENTER_TREE" || d == "READY")
        })
        .collect();

    if let Some(last_enter) = new_scene_events
        .iter()
        .rposition(|(_, d)| d == "ENTER_TREE")
    {
        if let Some(first_ready) = new_scene_events
            .iter()
            .position(|(_, d)| d == "READY")
        {
            assert!(
                last_enter < first_ready,
                "All ENTER_TREE must complete before any READY.\n\
                 Lifecycle: {new_scene_events:?}"
            );
        }
    }
}

// ===========================================================================
// 6. change_scene_to_packed: full lifecycle ordering with multi-node scene
// ===========================================================================

#[test]
fn change_to_packed_full_lifecycle_ordering() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // EXIT_TREE: old scene, bottom-up.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(!exits.is_empty(), "should have EXIT_TREE events");
    assert!(
        require_pos(&exits, "ChildA1") < require_pos(&exits, "/SceneA"),
        "ChildA1 EXIT before SceneA: {exits:?}"
    );

    // Global invariant: all EXIT_TREE before first ENTER_TREE.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(
        last_exit < first_enter,
        "all EXIT before any ENTER: {seq:?}"
    );
}

// ===========================================================================
// 7. Sequential scene changes: A -> B -> C preserves invariants each time
// ===========================================================================

#[test]
fn sequential_scene_changes_preserve_lifecycle_invariants() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Add initial scene A with child.
    let sa = tree
        .add_child(root, Node::new("SceneA", "Node2D"))
        .unwrap();
    tree.add_child(sa, Node::new("CA1", "Node")).unwrap();

    // Transition A -> B (single node).
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_node(Node::new("SceneB", "Node2D"))
        .unwrap();

    // Verify A->B: exits before enters.
    let seq_ab = lifecycle_sequence(&tree);
    let last_exit_ab = seq_ab.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter_ab = seq_ab.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit_ab, first_enter_ab) {
        assert!(le < fe, "A->B: all EXIT before ENTER");
    }

    // Transition B -> C via packed scene.
    tree.event_trace_mut().clear();

    let tscn_c = r#"[gd_scene format=3]

[node name="SceneC" type="Node2D"]

[node name="CC1" type="Node" parent="."]
"#;
    let packed_c = PackedScene::from_tscn(tscn_c).unwrap();
    tree.change_scene_to_packed(&packed_c).unwrap();

    // Verify B->C: exits and enters present.
    let exits_bc = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits_bc.iter().any(|p| p.ends_with("SceneB")),
        "SceneB should exit: {exits_bc:?}"
    );

    let enters_bc = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters_bc.iter().any(|p| p.ends_with("SceneC")),
        "SceneC should enter: {enters_bc:?}"
    );

    let seq_bc = lifecycle_sequence(&tree);
    let last_exit_bc = seq_bc.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter_bc = seq_bc.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit_bc, first_enter_bc) {
        assert!(le < fe, "B->C: all EXIT before ENTER");
    }
}

// ===========================================================================
// 8. change_scene_to_node: empty tree (no current scene)
// ===========================================================================

#[test]
fn change_to_node_from_empty_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_node(Node::new("First", "Node2D"))
        .unwrap();

    // No EXIT_TREE should fire (nothing to remove).
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.is_empty(), "no EXIT_TREE for empty tree: {exits:?}");

    // ENTER_TREE and READY should fire for the new node.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.ends_with("First")),
        "First should get ENTER_TREE"
    );
    let readys = notification_paths(&tree, "READY");
    assert!(
        readys.iter().any(|p| p.ends_with("First")),
        "First should get READY"
    );
}

// ===========================================================================
// 9. change_scene_to_node rejects node already in tree
// ===========================================================================

#[test]
fn change_to_node_rejects_existing_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let existing = tree
        .add_child(root, Node::new("Existing", "Node"))
        .unwrap();
    let dup = tree.get_node(existing).unwrap().clone();
    let result = tree.change_scene_to_node(dup);
    assert!(result.is_err(), "should reject node already in tree");
}

// ===========================================================================
// 10. change_scene_to_packed: empty packed scene returns error
// ===========================================================================

#[test]
fn change_to_packed_empty_scene_errors() {
    let tscn = "[gd_scene format=3]\n";
    let packed = PackedScene::from_tscn(tscn);
    // Either parse or instance should error for an empty scene.
    if let Ok(packed) = packed {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        LifecycleManager::enter_tree(&mut tree, root);
        let result = tree.change_scene_to_packed(&packed);
        assert!(result.is_err(), "empty packed scene should error");
    }
}

// ===========================================================================
// 11. Deep hierarchy transition: 4-level old -> 4-level new
// ===========================================================================

#[test]
fn deep_hierarchy_transition_preserves_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Build 4-level old scene: OldRoot -> OldL1 -> OldL2 -> OldL3
    let old_root = tree
        .add_child(root, Node::new("OldRoot", "Node"))
        .unwrap();
    let old_l1 = tree
        .add_child(old_root, Node::new("OldL1", "Node"))
        .unwrap();
    let old_l2 = tree
        .add_child(old_l1, Node::new("OldL2", "Node"))
        .unwrap();
    tree.add_child(old_l2, Node::new("OldL3", "Node"))
        .unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // New 4-level scene via packed.
    let tscn = r#"[gd_scene format=3]

[node name="NewRoot" type="Node2D"]

[node name="NewL1" type="Node" parent="."]

[node name="NewL2" type="Node" parent="NewL1"]

[node name="NewL3" type="Node" parent="NewL1/NewL2"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    // EXIT_TREE: deepest first.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        require_pos(&exits, "OldL3") < require_pos(&exits, "/OldL2"),
        "OldL3 exits before OldL2: {exits:?}"
    );
    assert!(
        require_pos(&exits, "/OldL2") < require_pos(&exits, "/OldL1"),
        "OldL2 exits before OldL1: {exits:?}"
    );
    assert!(
        require_pos(&exits, "/OldL1") < require_pos(&exits, "/OldRoot"),
        "OldL1 exits before OldRoot: {exits:?}"
    );

    // ENTER_TREE: shallowest first.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        require_pos(&enters, "/NewRoot") < require_pos(&enters, "/NewL1"),
        "NewRoot enters before NewL1: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/NewL1") < require_pos(&enters, "/NewL2"),
        "NewL1 enters before NewL2: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/NewL2") < require_pos(&enters, "/NewL3"),
        "NewL2 enters before NewL3: {enters:?}"
    );

    // Global invariant: all exits before any enter.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(last_exit < first_enter, "all EXIT before any ENTER");
}

// ===========================================================================
// 12. Node state (is_inside_tree / is_ready) after change_scene_to_packed
// ===========================================================================

#[test]
fn change_to_packed_new_nodes_are_inside_tree_and_ready() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // All new scene nodes should be inside tree and ready.
    let root = tree.root_id();
    let root_children: Vec<_> = tree
        .get_node(root)
        .unwrap()
        .children()
        .to_vec();
    assert!(!root_children.is_empty(), "root should have children after transition");

    // Walk all nodes in tree (except root) and verify state.
    fn check_subtree(tree: &SceneTree, id: gdscene::node::NodeId) {
        let node = tree.get_node(id).expect("node should exist");
        assert!(
            node.is_inside_tree(),
            "{} should be inside tree",
            node.name()
        );
        assert!(node.is_ready(), "{} should be ready", node.name());
        for &child_id in node.children() {
            check_subtree(tree, child_id);
        }
    }
    for &child_id in &root_children {
        check_subtree(&tree, child_id);
    }
}

// ===========================================================================
// 13. Old scene nodes are removed from tree after transition
// ===========================================================================

#[test]
fn change_to_packed_old_scene_nodes_removed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let old_root = tree
        .add_child(root, Node::new("OldScene", "Node2D"))
        .unwrap();
    let old_child = tree
        .add_child(old_root, Node::new("OldChild", "Node2D"))
        .unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed = packed_scene_b();
    tree.change_scene_to_packed(&packed).unwrap();

    // Old nodes should be gone from the arena.
    assert!(
        tree.get_node(old_root).is_none(),
        "OldScene should be removed from tree"
    );
    assert!(
        tree.get_node(old_child).is_none(),
        "OldChild should be removed from tree"
    );
}

// ===========================================================================
// 14. change_scene_to_node with multi-child new scene built manually
// ===========================================================================

#[test]
fn change_to_node_then_add_children_lifecycle() {
    let mut tree = tree_with_initial_scene();

    // Transition to new scene (single node first, then add children).
    let new_root_id = tree
        .change_scene_to_node(Node::new("NewScene", "Node2D"))
        .unwrap();

    // Clear trace to isolate child-add lifecycle events.
    tree.event_trace_mut().clear();

    let child1 = tree
        .add_child(new_root_id, Node::new("NewChild1", "Node"))
        .unwrap();
    let child2 = tree
        .add_child(new_root_id, Node::new("NewChild2", "Node"))
        .unwrap();

    // Children added after scene change should also be inside tree and ready.
    assert!(tree.get_node(child1).unwrap().is_inside_tree());
    assert!(tree.get_node(child1).unwrap().is_ready());
    assert!(tree.get_node(child2).unwrap().is_inside_tree());
    assert!(tree.get_node(child2).unwrap().is_ready());

    // Each child should have gotten ENTER_TREE then READY.
    let enters = notification_paths(&tree, "ENTER_TREE");
    let readys = notification_paths(&tree, "READY");
    assert!(
        enters.iter().any(|p| p.ends_with("NewChild1")),
        "NewChild1 ENTER_TREE: {enters:?}"
    );
    assert!(
        readys.iter().any(|p| p.ends_with("NewChild1")),
        "NewChild1 READY: {readys:?}"
    );
}

// ===========================================================================
// 15. Root node persists and stays in-tree across transitions
// ===========================================================================

#[test]
fn root_node_persists_across_transitions() {
    let mut tree = tree_with_initial_scene();
    let root = tree.root_id();
    let packed = packed_scene_b();

    // Root should be in-tree before transition.
    assert!(tree.get_node(root).unwrap().is_inside_tree());

    tree.change_scene_to_packed(&packed).unwrap();

    // Root should still exist and be in-tree after transition.
    assert!(tree.get_node(root).is_some(), "root must survive transition");
    assert!(
        tree.get_node(root).unwrap().is_inside_tree(),
        "root must stay inside tree"
    );

    // Root should not appear in EXIT_TREE events.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        !exits.iter().any(|p| p == "/root"),
        "root should NOT receive EXIT_TREE: {exits:?}"
    );
}

// ===========================================================================
// 16. Wide branching: scene with many siblings at same depth
// ===========================================================================

#[test]
fn wide_scene_transition_lifecycle_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Old scene: parent with 5 siblings.
    let old_parent = tree
        .add_child(root, Node::new("OldParent", "Node2D"))
        .unwrap();
    for i in 0..5 {
        tree.add_child(old_parent, Node::new(&format!("OldSib{i}"), "Node"))
            .unwrap();
    }

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // New scene: parent with 5 siblings.
    let tscn = r#"[gd_scene format=3]

[node name="NewParent" type="Node2D"]

[node name="NewSib0" type="Node" parent="."]

[node name="NewSib1" type="Node" parent="."]

[node name="NewSib2" type="Node" parent="."]

[node name="NewSib3" type="Node" parent="."]

[node name="NewSib4" type="Node" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    // All 5 old siblings should exit before parent.
    let exits = notification_paths(&tree, "EXIT_TREE");
    let parent_exit_pos = require_pos(&exits, "/OldParent");
    for i in 0..5 {
        let sib_pos = require_pos(&exits, &format!("OldSib{i}"));
        assert!(
            sib_pos < parent_exit_pos,
            "OldSib{i} should exit before OldParent: {exits:?}"
        );
    }

    // NewParent should enter before all new siblings.
    let enters = notification_paths(&tree, "ENTER_TREE");
    let new_parent_enter = require_pos(&enters, "/NewParent");
    for i in 0..5 {
        let sib_enter = require_pos(&enters, &format!("NewSib{i}"));
        assert!(
            new_parent_enter < sib_enter,
            "NewParent should enter before NewSib{i}: {enters:?}"
        );
    }

    // All new siblings READY before parent.
    let readys = notification_paths(&tree, "READY");
    let new_parent_ready = require_pos(&readys, "/NewParent");
    for i in 0..5 {
        let sib_ready = require_pos(&readys, &format!("NewSib{i}"));
        assert!(
            sib_ready < new_parent_ready,
            "NewSib{i} should be READY before NewParent: {readys:?}"
        );
    }

    // Global invariant.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(last_exit < first_enter, "all EXIT before any ENTER");
}

// ===========================================================================
// 17. Mixed API transitions: packed -> node -> packed
// ===========================================================================

#[test]
fn mixed_api_transitions_preserve_invariants() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Start with packed scene.
    let packed_a = packed_scene_b(); // SceneB with children
    tree.change_scene_to_packed(&packed_a).unwrap();

    // Transition via node API.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_node(Node::new("MiddleScene", "Node2D"))
        .unwrap();

    let seq1 = lifecycle_sequence(&tree);
    let last_exit1 = seq1.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter1 = seq1.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit1, first_enter1) {
        assert!(le < fe, "packed->node: all EXIT before ENTER: {seq1:?}");
    }

    // Transition back via packed API.
    tree.event_trace_mut().clear();

    let tscn = r#"[gd_scene format=3]

[node name="FinalScene" type="Node2D"]

[node name="FinalChild" type="Node" parent="."]
"#;
    let packed_c = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed_c).unwrap();

    let seq2 = lifecycle_sequence(&tree);
    let last_exit2 = seq2.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter2 = seq2.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit2, first_enter2) {
        assert!(le < fe, "node->packed: all EXIT before ENTER: {seq2:?}");
    }

    // Final state: FinalScene and FinalChild in tree.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(enters.iter().any(|p| p.ends_with("FinalScene")));
    assert!(enters.iter().any(|p| p.ends_with("FinalChild")));
}

// ===========================================================================
// 18. Node count correctness after transition
// ===========================================================================

#[test]
fn node_count_correct_after_transition() {
    let mut tree = tree_with_initial_scene();

    // Before: root + SceneA + ChildA1 + ChildA2 = 4 nodes.
    assert_eq!(tree.node_count(), 4);

    let packed = packed_scene_b();
    tree.change_scene_to_packed(&packed).unwrap();

    // After: root + SceneB + ChildB1 + ChildB2 + GrandchildB = 5 nodes.
    assert_eq!(
        tree.node_count(),
        5,
        "old scene removed, new scene added"
    );
}

// ===========================================================================
// 19. EXIT_TREE count matches old scene node count exactly
// ===========================================================================

#[test]
fn change_to_packed_exit_count_matches_old_scene() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    // Old scene: SceneA + ChildA1 + ChildA2 = 3 nodes.
    tree.change_scene_to_packed(&packed).unwrap();

    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(
        exits.len(),
        3,
        "EXIT_TREE should fire for exactly 3 old-scene nodes: {exits:?}"
    );

    // New scene: SceneB + ChildB1 + ChildB2 + GrandchildB = 4 nodes.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(
        enters.len(),
        4,
        "ENTER_TREE should fire for exactly 4 new-scene nodes: {enters:?}"
    );

    let readys = notification_paths(&tree, "READY");
    assert_eq!(
        readys.len(),
        4,
        "READY should fire for exactly 4 new-scene nodes: {readys:?}"
    );
}

// ===========================================================================
// 20. Multiple direct children of root all removed during transition
// ===========================================================================

#[test]
fn multiple_root_children_all_removed_on_transition() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Add three independent scenes as direct children of root.
    let s1 = tree
        .add_child(root, Node::new("Scene1", "Node2D"))
        .unwrap();
    tree.add_child(s1, Node::new("S1Child", "Node")).unwrap();

    let s2 = tree
        .add_child(root, Node::new("Scene2", "Node2D"))
        .unwrap();
    tree.add_child(s2, Node::new("S2Child", "Node")).unwrap();

    tree.add_child(root, Node::new("Scene3", "Node2D"))
        .unwrap();

    // 1 root + 3 scenes + 2 children = 6
    assert_eq!(tree.node_count(), 6);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_node(Node::new("Replacement", "Node2D"))
        .unwrap();

    // All old nodes removed: only root + Replacement remain.
    assert_eq!(tree.node_count(), 2, "only root + new scene should remain");

    // EXIT_TREE should fire for all 5 old nodes.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.iter().any(|p| p.ends_with("Scene1")),
        "Scene1 should exit: {exits:?}"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("Scene2")),
        "Scene2 should exit: {exits:?}"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("Scene3")),
        "Scene3 should exit: {exits:?}"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("S1Child")),
        "S1Child should exit: {exits:?}"
    );
    assert!(
        exits.iter().any(|p| p.ends_with("S2Child")),
        "S2Child should exit: {exits:?}"
    );
}

// ===========================================================================
// 21. Transition to scene with identical node names — no stale references
// ===========================================================================

#[test]
fn transition_to_same_named_scene_no_stale_refs() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Old scene: "Player" with child "Sprite".
    let old_player = tree
        .add_child(root, Node::new("Player", "Node2D"))
        .unwrap();
    let old_sprite = tree
        .add_child(old_player, Node::new("Sprite", "Sprite2D"))
        .unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // New scene with same names but different types.
    let tscn = r#"[gd_scene format=3]

[node name="Player" type="CharacterBody2D"]

[node name="Sprite" type="AnimatedSprite2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    // Old node IDs should be gone.
    assert!(
        tree.get_node(old_player).is_none(),
        "old Player ID should be invalid"
    );
    assert!(
        tree.get_node(old_sprite).is_none(),
        "old Sprite ID should be invalid"
    );

    // New nodes reachable by path with correct types.
    let new_player = tree.get_node_by_path("/root/Player").unwrap();
    assert_eq!(
        tree.get_node(new_player).unwrap().class_name(),
        "CharacterBody2D",
        "new Player should be CharacterBody2D"
    );
    let new_sprite = tree.get_node_by_path("/root/Player/Sprite").unwrap();
    assert_eq!(
        tree.get_node(new_sprite).unwrap().class_name(),
        "AnimatedSprite2D",
        "new Sprite should be AnimatedSprite2D"
    );

    // Lifecycle ordering still correct.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(
        last_exit < first_enter,
        "all EXIT before any ENTER even with same names: {seq:?}"
    );
}

// ===========================================================================
// 22. Packed-to-packed transition preserves lifecycle ordering (pat-1074)
// ===========================================================================

#[test]
fn packed_to_packed_transition_lifecycle_ordering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Start with a packed scene A.
    let tscn_a = r#"[gd_scene format=3]

[node name="PackedA" type="Node2D"]

[node name="PA_Child1" type="Node" parent="."]

[node name="PA_Child2" type="Node" parent="."]
"#;
    let packed_a = PackedScene::from_tscn(tscn_a).unwrap();
    tree.change_scene_to_packed(&packed_a).unwrap();

    // Enable trace and clear, then transition to packed scene B.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed_b = packed_scene_b(); // SceneB with ChildB1, ChildB2, GrandchildB
    tree.change_scene_to_packed(&packed_b).unwrap();

    // EXIT_TREE: old packed scene exits bottom-up.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(exits.len(), 3, "3 old nodes exit: {exits:?}");
    assert!(
        require_pos(&exits, "PA_Child1") < require_pos(&exits, "/PackedA"),
        "PA_Child1 exits before PackedA: {exits:?}"
    );
    assert!(
        require_pos(&exits, "PA_Child2") < require_pos(&exits, "/PackedA"),
        "PA_Child2 exits before PackedA: {exits:?}"
    );

    // ENTER_TREE: new packed scene enters top-down.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters.len(), 4, "4 new nodes enter: {enters:?}");
    assert!(
        require_pos(&enters, "/SceneB") < require_pos(&enters, "ChildB1"),
        "SceneB enters before ChildB1: {enters:?}"
    );

    // READY: new packed scene readies bottom-up.
    let readys = notification_paths(&tree, "READY");
    assert!(
        require_pos(&readys, "GrandchildB") < require_pos(&readys, "/ChildB2"),
        "GrandchildB ready before ChildB2: {readys:?}"
    );

    // Global invariant: all exits before any enters.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(last_exit < first_enter, "packed->packed: all EXIT before ENTER");
}

// ===========================================================================
// 23. UNPARENTED fires for old scene root during change_scene_to_packed (pat-1074)
// ===========================================================================

#[test]
fn change_to_packed_fires_unparented_for_old_root() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // The old scene root (SceneA) should receive UNPARENTED when detached from root.
    let unparented: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == "UNPARENTED" && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect();

    assert!(
        unparented.iter().any(|p| p.ends_with("SceneA")),
        "SceneA should receive UNPARENTED: {unparented:?}"
    );
}

// ===========================================================================
// 24. EXIT_TREE fires before UNPARENTED during scene change (pat-1074)
// ===========================================================================

#[test]
fn exit_tree_before_unparented_during_transition() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // EXIT_TREE for the old scene root should come before UNPARENTED.
    let all_events: Vec<(String, String)> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "EXIT_TREE" || e.detail == "UNPARENTED")
                && e.node_path.contains("SceneA")
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect();

    let exit_pos = all_events
        .iter()
        .position(|(p, d)| p.ends_with("SceneA") && d == "EXIT_TREE");
    let unparented_pos = all_events
        .iter()
        .position(|(p, d)| p.ends_with("SceneA") && d == "UNPARENTED");

    if let (Some(ep), Some(up)) = (exit_pos, unparented_pos) {
        assert!(
            ep < up,
            "EXIT_TREE should fire before UNPARENTED for SceneA: {all_events:?}"
        );
    }
}

// ===========================================================================
// 25. Group membership cleaned up after packed scene transition (pat-1074)
// ===========================================================================

#[test]
fn group_membership_cleaned_after_packed_transition() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Old scene with group membership.
    let tscn_old = r#"[gd_scene format=3]

[node name="OldGrouped" type="Node2D" groups=["enemies", "targetable"]]

[node name="OldChild" type="Node" parent="." groups=["enemies"]]
"#;
    let packed_old = PackedScene::from_tscn(tscn_old).unwrap();
    tree.change_scene_to_packed(&packed_old).unwrap();

    // Verify groups populated.
    assert_eq!(
        tree.get_nodes_in_group("enemies").len(),
        2,
        "both nodes in enemies group before transition"
    );
    assert_eq!(
        tree.get_nodes_in_group("targetable").len(),
        1,
        "one node in targetable group before transition"
    );

    // Transition to a new scene with no groups.
    tree.change_scene_to_packed(&packed_scene_b()).unwrap();

    // Old groups should be empty now.
    assert_eq!(
        tree.get_nodes_in_group("enemies").len(),
        0,
        "enemies group should be empty after transition"
    );
    assert_eq!(
        tree.get_nodes_in_group("targetable").len(),
        0,
        "targetable group should be empty after transition"
    );
}

// ===========================================================================
// 26. Sequential packed-to-packed transitions: state correctness (pat-1074)
// ===========================================================================

#[test]
fn sequential_packed_transitions_state_correctness() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let scenes = [
        r#"[gd_scene format=3]
[node name="S1" type="Node2D"]
[node name="S1C" type="Node" parent="."]
"#,
        r#"[gd_scene format=3]
[node name="S2" type="Node2D"]
[node name="S2C1" type="Node" parent="."]
[node name="S2C2" type="Node" parent="."]
"#,
        r#"[gd_scene format=3]
[node name="S3" type="Node2D"]
"#,
    ];

    for (i, tscn) in scenes.iter().enumerate() {
        let packed = PackedScene::from_tscn(tscn).unwrap();
        tree.event_trace_mut().clear();
        tree.change_scene_to_packed(&packed).unwrap();

        // After each transition, verify all new nodes are in-tree and ready.
        let root_children: Vec<_> = tree.get_node(root).unwrap().children().to_vec();
        assert!(
            !root_children.is_empty(),
            "scene {i} should have children under root"
        );
        for &cid in &root_children {
            let node = tree.get_node(cid).unwrap();
            assert!(
                node.is_inside_tree(),
                "scene {i}: {} should be inside tree",
                node.name()
            );
            assert!(
                node.is_ready(),
                "scene {i}: {} should be ready",
                node.name()
            );
        }

        // Verify lifecycle ordering for each transition.
        let seq = lifecycle_sequence(&tree);
        let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE");
        let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE");
        if let (Some(le), Some(fe)) = (last_exit, first_enter) {
            assert!(le < fe, "scene {i}: all EXIT before ENTER");
        }
    }

    // Final state: only root + S3 = 2 nodes.
    assert_eq!(tree.node_count(), 2, "only root + S3 after 3 transitions");
}

// ===========================================================================
// 27. Packed transition with deep nesting: grandchild READY before child (pat-1074)
// ===========================================================================

#[test]
fn packed_transition_deep_nesting_ready_ordering() {
    let mut tree = tree_with_initial_scene();

    let tscn = r#"[gd_scene format=3]

[node name="Deep" type="Node2D"]

[node name="L1" type="Node" parent="."]

[node name="L2" type="Node" parent="L1"]

[node name="L3" type="Node" parent="L1/L2"]

[node name="L4" type="Node" parent="L1/L2/L3"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    let readys = notification_paths(&tree, "READY");

    // READY fires bottom-up: L4 < L3 < L2 < L1 < Deep.
    assert!(
        require_pos(&readys, "L4") < require_pos(&readys, "/L3"),
        "L4 READY before L3: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/L3") < require_pos(&readys, "/L2"),
        "L3 READY before L2: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/L2") < require_pos(&readys, "/L1"),
        "L2 READY before L1: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/L1") < require_pos(&readys, "/Deep"),
        "L1 READY before Deep: {readys:?}"
    );

    // ENTER_TREE fires top-down: Deep < L1 < L2 < L3 < L4.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        require_pos(&enters, "/Deep") < require_pos(&enters, "/L1"),
        "Deep ENTER before L1: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/L1") < require_pos(&enters, "/L2"),
        "L1 ENTER before L2: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/L2") < require_pos(&enters, "/L3"),
        "L2 ENTER before L3: {enters:?}"
    );
    assert!(
        require_pos(&enters, "/L3") < require_pos(&enters, "L4"),
        "L3 ENTER before L4: {enters:?}"
    );
}

// ===========================================================================
// 28. current_scene tracks correctly through transitions (pat-n1cs)
// ===========================================================================

#[test]
fn current_scene_tracks_through_transitions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Initially no current scene.
    assert!(
        tree.current_scene().is_none(),
        "no current scene initially"
    );

    // After change_scene_to_node, current_scene should be set.
    let scene_a = tree
        .change_scene_to_node(Node::new("SceneA", "Node2D"))
        .unwrap();
    assert_eq!(
        tree.current_scene(),
        Some(scene_a),
        "current_scene should be SceneA after change_scene_to_node"
    );

    // After change_scene_to_packed, current_scene should update.
    let packed = packed_scene_b();
    let scene_b = tree.change_scene_to_packed(&packed).unwrap();
    assert_eq!(
        tree.current_scene(),
        Some(scene_b),
        "current_scene should be SceneB after change_scene_to_packed"
    );

    // Old scene_a ID should be invalid.
    assert!(
        tree.get_node(scene_a).is_none(),
        "old SceneA ID should be invalid after transition"
    );
}

// ===========================================================================
// 29. unload_current_scene fires EXIT_TREE bottom-up (pat-n1cs)
// ===========================================================================

#[test]
fn unload_current_scene_exit_tree_bottom_up() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let tscn = r#"[gd_scene format=3]

[node name="Unloadable" type="Node2D"]

[node name="Child1" type="Node" parent="."]

[node name="Child2" type="Node" parent="."]

[node name="Grandchild" type="Node" parent="Child2"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.unload_current_scene().unwrap();

    // EXIT_TREE should fire bottom-up.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert_eq!(exits.len(), 4, "all 4 scene nodes should exit: {exits:?}");

    assert!(
        require_pos(&exits, "Grandchild") < require_pos(&exits, "/Child2"),
        "Grandchild exits before Child2: {exits:?}"
    );
    assert!(
        require_pos(&exits, "/Child2") < require_pos(&exits, "/Unloadable"),
        "Child2 exits before Unloadable: {exits:?}"
    );
    assert!(
        require_pos(&exits, "Child1") < require_pos(&exits, "/Unloadable"),
        "Child1 exits before Unloadable: {exits:?}"
    );

    // No ENTER_TREE or READY should fire.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(enters.is_empty(), "no ENTER_TREE after unload: {enters:?}");
    let readys = notification_paths(&tree, "READY");
    assert!(readys.is_empty(), "no READY after unload: {readys:?}");

    // current_scene should be None.
    assert!(
        tree.current_scene().is_none(),
        "current_scene should be None after unload"
    );

    // Only root remains.
    assert_eq!(tree.node_count(), 1, "only root after unload");
}

// ===========================================================================
// 30. Self-replacement: same packed scene instanced twice (pat-n1cs)
// ===========================================================================

#[test]
fn self_replacement_same_packed_scene_twice() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let packed = packed_scene_b();

    // First load.
    let first_id = tree.change_scene_to_packed(&packed).unwrap();
    assert_eq!(tree.node_count(), 5); // root + SceneB + ChildB1 + ChildB2 + GrandchildB

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Replace with same packed scene.
    let second_id = tree.change_scene_to_packed(&packed).unwrap();

    // IDs should differ (fresh instances).
    assert_ne!(first_id, second_id, "new instance should have different ID");

    // Old IDs should be invalid.
    assert!(
        tree.get_node(first_id).is_none(),
        "first instance should be removed"
    );

    // Node count should be the same as before.
    assert_eq!(tree.node_count(), 5, "same structure after self-replacement");

    // Lifecycle ordering still correct.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE").unwrap();
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE").unwrap();
    assert!(
        last_exit < first_enter,
        "self-replacement: all EXIT before ENTER: {seq:?}"
    );

    // EXIT count = 4 (old SceneB tree), ENTER count = 4 (new SceneB tree).
    let exits = notification_paths(&tree, "EXIT_TREE");
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(exits.len(), 4, "4 old nodes exit: {exits:?}");
    assert_eq!(enters.len(), 4, "4 new nodes enter: {enters:?}");
}

// ===========================================================================
// 31. Rapid sequential transitions: no accumulated state leaks (pat-n1cs)
// ===========================================================================

#[test]
fn rapid_sequential_transitions_no_state_leaks() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let scenes: Vec<&str> = vec![
        r#"[gd_scene format=3]
[node name="R1" type="Node2D"]
[node name="R1C" type="Node" parent="."]
"#,
        r#"[gd_scene format=3]
[node name="R2" type="Node2D"]
[node name="R2C1" type="Node" parent="."]
[node name="R2C2" type="Node" parent="."]
"#,
        r#"[gd_scene format=3]
[node name="R3" type="Node"]
"#,
        r#"[gd_scene format=3]
[node name="R4" type="Node2D"]
[node name="R4C" type="Node" parent="."]
[node name="R4G" type="Node" parent="R4C"]
"#,
        r#"[gd_scene format=3]
[node name="R5" type="Node2D"]
"#,
    ];

    for (i, tscn) in scenes.iter().enumerate() {
        let packed = PackedScene::from_tscn(tscn).unwrap();
        tree.event_trace_mut().enable();
        tree.event_trace_mut().clear();
        tree.change_scene_to_packed(&packed).unwrap();

        // Each transition: all EXIT before any ENTER.
        let seq = lifecycle_sequence(&tree);
        let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE");
        let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE");
        if let (Some(le), Some(fe)) = (last_exit, first_enter) {
            assert!(le < fe, "transition {i}: all EXIT before ENTER: {seq:?}");
        }

        // current_scene should be set after each transition.
        assert!(
            tree.current_scene().is_some(),
            "transition {i}: current_scene should be set"
        );
    }

    // Final state: root + R5 = 2 nodes.
    assert_eq!(tree.node_count(), 2, "only root + R5 after 5 transitions");

    // Final scene node should be valid.
    let final_id = tree.current_scene().unwrap();
    let final_node = tree.get_node(final_id).unwrap();
    assert_eq!(final_node.name(), "R5");
    assert!(final_node.is_inside_tree());
    assert!(final_node.is_ready());
}

// ===========================================================================
// 32. PARENTED notification fires for new scene root (pat-n1cs)
// ===========================================================================

#[test]
fn parented_fires_for_new_scene_root_during_transition() {
    let mut tree = tree_with_initial_scene();
    let packed = packed_scene_b();

    tree.change_scene_to_packed(&packed).unwrap();

    // New scene root (SceneB) should receive PARENTED when added under root.
    let parented: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == "PARENTED" && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect();

    assert!(
        parented.iter().any(|p| p.ends_with("SceneB")),
        "SceneB should receive PARENTED: {parented:?}"
    );
}

// ===========================================================================
// 33. Transition from unloaded state: load after unload (pat-n1cs)
// ===========================================================================

#[test]
fn transition_from_unloaded_state() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Load a scene, then unload it.
    let packed_a = packed_scene_b();
    tree.change_scene_to_packed(&packed_a).unwrap();
    tree.unload_current_scene().unwrap();

    assert!(tree.current_scene().is_none());
    assert_eq!(tree.node_count(), 1, "only root after unload");

    // Now load a new scene into the empty tree.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let tscn = r#"[gd_scene format=3]

[node name="AfterUnload" type="Node2D"]

[node name="AUChild" type="Node" parent="."]
"#;
    let packed_b = PackedScene::from_tscn(tscn).unwrap();
    let new_id = tree.change_scene_to_packed(&packed_b).unwrap();

    // No EXIT_TREE events (nothing to exit).
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.is_empty(),
        "no EXIT_TREE from empty tree: {exits:?}"
    );

    // ENTER_TREE fires top-down.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert_eq!(enters.len(), 2, "2 new nodes enter: {enters:?}");
    assert!(
        require_pos(&enters, "/AfterUnload") < require_pos(&enters, "AUChild"),
        "AfterUnload enters before AUChild: {enters:?}"
    );

    // READY fires bottom-up.
    let readys = notification_paths(&tree, "READY");
    assert!(
        require_pos(&readys, "AUChild") < require_pos(&readys, "/AfterUnload"),
        "AUChild ready before AfterUnload: {readys:?}"
    );

    // current_scene set correctly.
    assert_eq!(tree.current_scene(), Some(new_id));
    assert_eq!(tree.node_count(), 3, "root + AfterUnload + AUChild");
}

// ===========================================================================
// 34. ENTER_TREE ordering with mixed depths in new scene (pat-n1cs)
//     Verifies breadth vs depth ordering: Godot uses depth-first top-down
// ===========================================================================

#[test]
fn enter_tree_depth_first_with_mixed_depths() {
    let mut tree = tree_with_initial_scene();

    // Scene with a mix of shallow and deep branches:
    //   MixRoot
    //   ├── ShallowA
    //   ├── DeepB
    //   │   └── DeepB1
    //   │       └── DeepB2
    //   └── ShallowC
    let tscn = r#"[gd_scene format=3]

[node name="MixRoot" type="Node2D"]

[node name="ShallowA" type="Node" parent="."]

[node name="DeepB" type="Node" parent="."]

[node name="DeepB1" type="Node" parent="DeepB"]

[node name="DeepB2" type="Node" parent="DeepB/DeepB1"]

[node name="ShallowC" type="Node" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    let enters = notification_paths(&tree, "ENTER_TREE");

    // MixRoot first.
    assert!(
        require_pos(&enters, "/MixRoot") < require_pos(&enters, "ShallowA"),
        "MixRoot before ShallowA: {enters:?}"
    );

    // Depth-first: after ShallowA, DeepB branch fully explored before ShallowC.
    assert!(
        require_pos(&enters, "ShallowA") < require_pos(&enters, "/DeepB"),
        "ShallowA before DeepB (sibling order): {enters:?}"
    );
    assert!(
        require_pos(&enters, "/DeepB") < require_pos(&enters, "DeepB1"),
        "DeepB before DeepB1: {enters:?}"
    );
    assert!(
        require_pos(&enters, "DeepB1") < require_pos(&enters, "DeepB2"),
        "DeepB1 before DeepB2: {enters:?}"
    );
    assert!(
        require_pos(&enters, "DeepB2") < require_pos(&enters, "ShallowC"),
        "DeepB2 before ShallowC (depth-first): {enters:?}"
    );

    // READY fires in reverse depth-first order (bottom-up).
    let readys = notification_paths(&tree, "READY");
    assert!(
        require_pos(&readys, "DeepB2") < require_pos(&readys, "/DeepB1"),
        "DeepB2 READY before DeepB1: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/DeepB1") < require_pos(&readys, "/DeepB"),
        "DeepB1 READY before DeepB: {readys:?}"
    );
    assert!(
        require_pos(&readys, "/DeepB") < require_pos(&readys, "/MixRoot"),
        "DeepB READY before MixRoot: {readys:?}"
    );
    assert!(
        require_pos(&readys, "ShallowC") < require_pos(&readys, "/MixRoot"),
        "ShallowC READY before MixRoot: {readys:?}"
    );
}

// ===========================================================================
// 35. Transition preserves root node state flags (pat-n1cs)
// ===========================================================================

#[test]
fn root_node_state_flags_preserved_across_transitions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Root should be inside_tree and ready.
    assert!(tree.get_node(root).unwrap().is_inside_tree());
    assert!(tree.get_node(root).unwrap().is_ready());

    // Multiple transitions.
    for i in 0..3 {
        let tscn = format!(
            "[gd_scene format=3]\n[node name=\"Scene{i}\" type=\"Node2D\"]\n"
        );
        let packed = PackedScene::from_tscn(&tscn).unwrap();
        tree.change_scene_to_packed(&packed).unwrap();

        // Root remains inside_tree and ready after each transition.
        assert!(
            tree.get_node(root).unwrap().is_inside_tree(),
            "root inside_tree after transition {i}"
        );
        assert!(
            tree.get_node(root).unwrap().is_ready(),
            "root is_ready after transition {i}"
        );
    }

    // Unload and verify root still stable.
    tree.unload_current_scene().unwrap();
    assert!(tree.get_node(root).unwrap().is_inside_tree());
    assert!(tree.get_node(root).unwrap().is_ready());
}
