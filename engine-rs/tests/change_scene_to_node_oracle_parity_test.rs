//! pat-cs9n: Oracle-backed parity test for SceneTree::change_scene_to_node().
//!
//! Loads the golden fixture `fixtures/golden/scenes/change_scene_to_node.json`
//! and verifies that Patina's lifecycle notification ordering matches the
//! Godot 4.6.1 oracle specification:
//!   - EXIT_TREE fires bottom-up on old scene (children before parent)
//!   - ENTER_TREE + READY fire on new scene root
//!   - Children added post-switch each receive ENTER_TREE then READY
//!   - All EXIT_TREE before any ENTER_TREE
//!   - Node already in tree is rejected
//!   - Packed scene source is cleared (reload fails)
//!
//! Acceptance: at least one oracle-backed fixture covers scene replacement by
//! node instance, lifecycle ordering is asserted, and the API is treated as
//! measured rather than implied.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;
use serde_json::Value;

// ===========================================================================
// Fixture loading
// ===========================================================================

fn load_fixture() -> Value {
    let path = format!(
        "{}/../fixtures/golden/scenes/change_scene_to_node.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load oracle fixture at {path}: {e}"));
    serde_json::from_str(&raw).expect("Failed to parse oracle fixture JSON")
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Extract lifecycle notification events from the trace.
fn lifecycle_sequence(tree: &SceneTree) -> Vec<(String, String)> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && matches!(
                    e.detail.as_str(),
                    "ENTER_TREE" | "READY" | "EXIT_TREE"
                )
        })
        .map(|e| (e.node_path.clone(), e.detail.clone()))
        .collect()
}

/// Build the initial scene from the fixture spec: root -> SceneA -> ChildA1, ChildA2.
fn build_initial_tree(fixture: &Value) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let initial = &fixture["initial_scene"];
    let scene_name = initial["name"].as_str().unwrap();
    let scene_class = initial["class"].as_str().unwrap();

    let scene_id = tree
        .add_child(root, Node::new(scene_name, scene_class))
        .unwrap();
    tree.set_current_scene(Some(scene_id));

    if let Some(children) = initial["children"].as_array() {
        for child in children {
            let name = child["name"].as_str().unwrap();
            let class = child["class"].as_str().unwrap();
            tree.add_child(scene_id, Node::new(name, class)).unwrap();
        }
    }

    // Clear trace so we only capture the transition.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree
}

// ===========================================================================
// 1. Oracle fixture loads correctly
// ===========================================================================

#[test]
fn oracle_fixture_loads_and_has_required_fields() {
    let fixture = load_fixture();

    assert_eq!(fixture["fixture_id"].as_str().unwrap(), "change_scene_to_node");
    assert_eq!(fixture["capture_type"].as_str().unwrap(), "scene_tree");
    assert_eq!(fixture["upstream_version"].as_str().unwrap(), "4.6.1-stable");
    assert!(fixture["upstream_commit"].as_str().is_some());
    assert!(fixture["generated_at"].as_str().is_some());
    assert!(fixture["expected_scene_switch_lifecycle"].is_object());
    assert!(fixture["initial_scene"].is_object());
    assert!(fixture["replacement_scene"].is_object());
    assert!(fixture["data"]["nodes"].as_array().is_some());
    assert!(fixture["validation_rules"].is_object());
}

// ===========================================================================
// 2. Scene switch lifecycle matches oracle
// ===========================================================================

#[test]
fn scene_switch_lifecycle_matches_oracle() {
    let fixture = load_fixture();
    let mut tree = build_initial_tree(&fixture);

    let repl = &fixture["replacement_scene"]["root"];
    let repl_name = repl["name"].as_str().unwrap();
    let repl_class = repl["class"].as_str().unwrap();

    tree.change_scene_to_node(Node::new(repl_name, repl_class))
        .unwrap();

    let actual = lifecycle_sequence(&tree);
    let expected: Vec<(String, String)> = fixture["expected_scene_switch_lifecycle"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["node_path"].as_str().unwrap().to_string(),
                e["notification"].as_str().unwrap().to_string(),
            )
        })
        .collect();

    assert_eq!(
        actual.len(),
        expected.len(),
        "lifecycle event count mismatch:\n  actual:   {actual:?}\n  expected: {expected:?}"
    );

    for (i, (actual_entry, expected_entry)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(
            actual_entry, expected_entry,
            "lifecycle event #{i} mismatch:\n  actual:   {actual_entry:?}\n  expected: {expected_entry:?}\n  full actual: {actual:?}"
        );
    }
}

// ===========================================================================
// 3. EXIT_TREE phase completes before ENTER_TREE (oracle-driven)
// ===========================================================================

#[test]
fn all_exit_tree_before_any_enter_tree_oracle() {
    let fixture = load_fixture();
    let events = fixture["expected_scene_switch_lifecycle"]["events"]
        .as_array()
        .unwrap();

    let last_exit = events
        .iter()
        .rposition(|e| e["notification"].as_str().unwrap() == "EXIT_TREE")
        .expect("fixture must have EXIT_TREE events");
    let first_enter = events
        .iter()
        .position(|e| e["notification"].as_str().unwrap() == "ENTER_TREE")
        .expect("fixture must have ENTER_TREE events");

    assert!(
        last_exit < first_enter,
        "oracle: all EXIT_TREE ({last_exit}) before first ENTER_TREE ({first_enter})"
    );

    // Verify Patina matches.
    let mut tree = build_initial_tree(&fixture);
    let repl = &fixture["replacement_scene"]["root"];
    tree.change_scene_to_node(Node::new(
        repl["name"].as_str().unwrap(),
        repl["class"].as_str().unwrap(),
    ))
    .unwrap();

    let actual = lifecycle_sequence(&tree);
    let actual_last_exit = actual
        .iter()
        .rposition(|(_, n)| n == "EXIT_TREE")
        .expect("must have EXIT_TREE");
    let actual_first_enter = actual
        .iter()
        .position(|(_, n)| n == "ENTER_TREE")
        .expect("must have ENTER_TREE");

    assert!(
        actual_last_exit < actual_first_enter,
        "Patina: all EXIT_TREE must precede ENTER_TREE. actual={actual:?}"
    );
}

// ===========================================================================
// 4. EXIT_TREE is bottom-up (children before parent) per oracle
// ===========================================================================

#[test]
fn exit_tree_bottom_up_oracle() {
    let fixture = load_fixture();
    let events = fixture["expected_scene_switch_lifecycle"]["events"]
        .as_array()
        .unwrap();

    let exits: Vec<&str> = events
        .iter()
        .filter(|e| e["notification"].as_str().unwrap() == "EXIT_TREE")
        .map(|e| e["node_path"].as_str().unwrap())
        .collect();

    // Oracle says children exit before parent.
    assert!(exits.len() >= 3, "need at least 3 EXIT_TREE events");
    // ChildA1 and ChildA2 must appear before SceneA.
    let child_a1_pos = exits.iter().position(|p| p.ends_with("ChildA1")).unwrap();
    let child_a2_pos = exits.iter().position(|p| p.ends_with("ChildA2")).unwrap();
    let scene_a_pos = exits.iter().position(|p| p.ends_with("/SceneA")).unwrap();
    assert!(child_a1_pos < scene_a_pos, "ChildA1 EXIT before SceneA");
    assert!(child_a2_pos < scene_a_pos, "ChildA2 EXIT before SceneA");

    // Verify Patina matches.
    let mut tree = build_initial_tree(&fixture);
    let repl = &fixture["replacement_scene"]["root"];
    tree.change_scene_to_node(Node::new(
        repl["name"].as_str().unwrap(),
        repl["class"].as_str().unwrap(),
    ))
    .unwrap();

    let actual_exits: Vec<String> = lifecycle_sequence(&tree)
        .into_iter()
        .filter(|(_, n)| n == "EXIT_TREE")
        .map(|(p, _)| p)
        .collect();

    let a1 = actual_exits.iter().position(|p| p.ends_with("ChildA1")).unwrap();
    let a2 = actual_exits.iter().position(|p| p.ends_with("ChildA2")).unwrap();
    let sa = actual_exits.iter().position(|p| p.ends_with("/SceneA")).unwrap();
    assert!(a1 < sa, "Patina: ChildA1 EXIT before SceneA. actual={actual_exits:?}");
    assert!(a2 < sa, "Patina: ChildA2 EXIT before SceneA. actual={actual_exits:?}");
}

// ===========================================================================
// 5. Phase annotations in fixture are consistent
// ===========================================================================

#[test]
fn oracle_phases_consistent() {
    let fixture = load_fixture();
    let events = fixture["expected_scene_switch_lifecycle"]["events"]
        .as_array()
        .unwrap();

    for entry in events {
        let notif = entry["notification"].as_str().unwrap();
        let phase = entry["phase"].as_str().unwrap();

        match notif {
            "EXIT_TREE" => assert_eq!(phase, "old_scene_exit"),
            "ENTER_TREE" => assert_eq!(phase, "new_scene_enter"),
            "READY" => assert_eq!(phase, "new_scene_ready"),
            other => panic!("unexpected notification: {other}"),
        }
    }
}

// ===========================================================================
// 6. current_scene set to new root (oracle validation_rules)
// ===========================================================================

#[test]
fn current_scene_set_to_new_root_oracle() {
    let fixture = load_fixture();
    assert!(fixture["validation_rules"]["current_scene_set_to_new_root"].as_bool().unwrap());

    let mut tree = build_initial_tree(&fixture);
    let repl = &fixture["replacement_scene"]["root"];
    let new_root_id = tree
        .change_scene_to_node(Node::new(
            repl["name"].as_str().unwrap(),
            repl["class"].as_str().unwrap(),
        ))
        .unwrap();

    assert_eq!(
        tree.current_scene(),
        Some(new_root_id),
        "current_scene must be new root after change_scene_to_node"
    );
}

// ===========================================================================
// 7. Reject node already in tree (oracle validation_rules)
// ===========================================================================

#[test]
fn reject_node_already_in_tree_oracle() {
    let fixture = load_fixture();
    assert!(fixture["validation_rules"]["reject_node_already_in_tree"].as_bool().unwrap());

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let existing = tree
        .add_child(root, Node::new("Existing", "Node2D"))
        .unwrap();

    let dup = tree.get_node(existing).unwrap().clone();
    let result = tree.change_scene_to_node(dup);
    assert!(
        result.is_err(),
        "change_scene_to_node must reject node already in the tree"
    );
}

// ===========================================================================
// 8. Reload errors after change_scene_to_node (oracle validation_rules)
// ===========================================================================

#[test]
fn reload_errors_after_change_to_node_oracle() {
    let fixture = load_fixture();
    assert!(fixture["validation_rules"]["reload_after_change_to_node_errors"].as_bool().unwrap());

    let mut tree = build_initial_tree(&fixture);
    let repl = &fixture["replacement_scene"]["root"];
    tree.change_scene_to_node(Node::new(
        repl["name"].as_str().unwrap(),
        repl["class"].as_str().unwrap(),
    ))
    .unwrap();

    let result = tree.reload_current_scene();
    assert!(
        result.is_err(),
        "reload must error after change_scene_to_node (no packed source)"
    );
}

// ===========================================================================
// 9. Packed scene source cleared (oracle validation_rules)
// ===========================================================================

#[test]
fn packed_source_cleared_after_change_to_node_oracle() {
    let fixture = load_fixture();
    assert!(fixture["validation_rules"]["clears_packed_scene_source"].as_bool().unwrap());

    let mut tree = build_initial_tree(&fixture);
    let repl = &fixture["replacement_scene"]["root"];
    tree.change_scene_to_node(Node::new(
        repl["name"].as_str().unwrap(),
        repl["class"].as_str().unwrap(),
    ))
    .unwrap();

    assert!(
        tree.reload_current_scene().is_err(),
        "packed source must be cleared after change_scene_to_node"
    );
}

// ===========================================================================
// 10. Post-switch children receive lifecycle events
// ===========================================================================

#[test]
fn post_switch_children_lifecycle_oracle() {
    let fixture = load_fixture();
    let mut tree = build_initial_tree(&fixture);

    let repl = &fixture["replacement_scene"]["root"];
    let new_root_id = tree
        .change_scene_to_node(Node::new(
            repl["name"].as_str().unwrap(),
            repl["class"].as_str().unwrap(),
        ))
        .unwrap();

    // Clear trace to isolate child-add events.
    tree.event_trace_mut().clear();

    // Add children per fixture spec.
    let children_spec = fixture["replacement_scene"]["children"].as_array().unwrap();
    for child_spec in children_spec {
        let name = child_spec["name"].as_str().unwrap();
        let class = child_spec["class"].as_str().unwrap();
        let child_id = tree
            .add_child(new_root_id, Node::new(name, class))
            .unwrap();

        // Add grandchildren if specified.
        if let Some(grandchildren) = child_spec["children"].as_array() {
            for gc_spec in grandchildren {
                let gc_name = gc_spec["name"].as_str().unwrap();
                let gc_class = gc_spec["class"].as_str().unwrap();
                tree.add_child(child_id, Node::new(gc_name, gc_class))
                    .unwrap();
            }
        }
    }

    // Verify every child added is inside tree and ready.
    let expected_children = fixture["expected_post_switch_child_lifecycle"]["child_events"]
        .as_array()
        .unwrap();

    let actual = lifecycle_sequence(&tree);

    // Every expected child should have both ENTER_TREE and READY in the trace.
    let expected_paths: Vec<&str> = expected_children
        .iter()
        .filter(|e| e["notification"].as_str().unwrap() == "ENTER_TREE")
        .map(|e| e["node_path"].as_str().unwrap())
        .collect();

    for path in &expected_paths {
        let has_enter = actual
            .iter()
            .any(|(p, n)| p.ends_with(path.rsplit('/').next().unwrap()) && n == "ENTER_TREE");
        let has_ready = actual
            .iter()
            .any(|(p, n)| p.ends_with(path.rsplit('/').next().unwrap()) && n == "READY");

        assert!(has_enter, "child {path} must have ENTER_TREE in trace. actual={actual:?}");
        assert!(has_ready, "child {path} must have READY in trace. actual={actual:?}");
    }
}

// ===========================================================================
// 11. Old scene fully removed from tree
// ===========================================================================

#[test]
fn old_scene_fully_removed_oracle() {
    let fixture = load_fixture();
    let mut tree = build_initial_tree(&fixture);

    let repl = &fixture["replacement_scene"]["root"];
    tree.change_scene_to_node(Node::new(
        repl["name"].as_str().unwrap(),
        repl["class"].as_str().unwrap(),
    ))
    .unwrap();

    let root = tree.root_id();
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(
        root_node.children().len(),
        1,
        "root should have exactly 1 child after scene change"
    );
}

// ===========================================================================
// 12. New scene root has correct name/class from fixture
// ===========================================================================

#[test]
fn new_scene_root_matches_fixture() {
    let fixture = load_fixture();
    let mut tree = build_initial_tree(&fixture);

    let repl = &fixture["replacement_scene"]["root"];
    let expected_name = repl["name"].as_str().unwrap();
    let expected_class = repl["class"].as_str().unwrap();

    let new_root_id = tree
        .change_scene_to_node(Node::new(expected_name, expected_class))
        .unwrap();

    let new_root = tree.get_node(new_root_id).unwrap();
    assert_eq!(new_root.name(), expected_name);
    assert_eq!(new_root.class_name(), expected_class);
}
