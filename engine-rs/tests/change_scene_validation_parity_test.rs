//! pat-nn5n / pat-aeh8: Validation semantics and failure modes for
//! `change_scene_to_packed` and `change_scene_to_node`.
//!
//! These tests cover edge cases and error paths not already covered by the
//! lifecycle ordering tests in `packed_scene_change_lifecycle_parity_test.rs`.
//!
//! Godot 4.x contract references:
//! - `change_scene_to_packed(null)` is a runtime error in Godot
//! - `change_scene_to_packed` on an empty tree (no current scene) succeeds
//! - Calling `change_scene_to_packed` twice in rapid succession replaces correctly
//! - Root node identity is preserved across scene changes
//! - Node properties from tscn are preserved after scene change

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

fn make_tree() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree
}

fn make_tree_with_scene() -> SceneTree {
    let mut tree = make_tree();
    let root = tree.root_id();
    let scene = tree
        .add_child(root, Node::new("SceneA", "Node2D"))
        .unwrap();
    tree.add_child(scene, Node::new("Child1", "Sprite2D"))
        .unwrap();
    tree.add_child(scene, Node::new("Child2", "CollisionShape2D"))
        .unwrap();
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();
    tree
}

fn simple_packed(name: &str) -> PackedScene {
    let tscn = format!(
        "[gd_scene format=3]\n\n[node name=\"{name}\" type=\"Node2D\"]\n"
    );
    PackedScene::from_tscn(&tscn).expect("valid tscn")
}

fn packed_with_children() -> PackedScene {
    PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="NewScene" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Body" type="CharacterBody2D" parent="."]

[node name="Shape" type="CollisionShape2D" parent="Body"]
"#,
    )
    .expect("valid tscn")
}

// ===========================================================================
// 1. change_scene_to_packed from empty tree (no current scene) succeeds
// ===========================================================================

#[test]
fn change_to_packed_from_empty_tree() {
    let mut tree = make_tree();
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed = simple_packed("FirstScene");
    tree.change_scene_to_packed(&packed).unwrap();

    // No EXIT_TREE should fire (nothing to remove).
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.is_empty(), "no EXIT_TREE for empty tree: {exits:?}");

    // ENTER_TREE and READY should fire for the new scene.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.ends_with("FirstScene")),
        "FirstScene should get ENTER_TREE: {enters:?}"
    );
    let readys = notification_paths(&tree, "READY");
    assert!(
        readys.iter().any(|p| p.ends_with("FirstScene")),
        "FirstScene should get READY: {readys:?}"
    );
}

// ===========================================================================
// 2. change_scene_to_packed with multi-child scene from empty tree
// ===========================================================================

#[test]
fn change_to_packed_multi_child_from_empty_tree() {
    let mut tree = make_tree();
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed = packed_with_children();
    tree.change_scene_to_packed(&packed).unwrap();

    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.len() >= 4,
        "expected at least 4 ENTER_TREE events (root + 3 children), got {enters:?}"
    );

    // Verify tree structure: root should have NewScene as child.
    let root = tree.root_id();
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 1, "root should have exactly 1 child");
}

// ===========================================================================
// 3. Double scene change: A -> B -> C in rapid succession
// ===========================================================================

#[test]
fn double_scene_change_packed_replaces_correctly() {
    let mut tree = make_tree_with_scene();

    // First change: to packed SceneB.
    let packed_b = simple_packed("SceneB");
    tree.change_scene_to_packed(&packed_b).unwrap();

    tree.event_trace_mut().clear();

    // Second change: to packed SceneC.
    let packed_c = simple_packed("SceneC");
    tree.change_scene_to_packed(&packed_c).unwrap();

    // SceneB should have exited.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.iter().any(|p| p.ends_with("SceneB")),
        "SceneB should exit: {exits:?}"
    );

    // SceneC should have entered.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.ends_with("SceneC")),
        "SceneC should enter: {enters:?}"
    );

    // Only SceneC should remain under root.
    let root = tree.root_id();
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 1);
}

// ===========================================================================
// 4. Root node identity is preserved across scene changes
// ===========================================================================

#[test]
fn root_identity_preserved_across_scene_changes() {
    let mut tree = make_tree_with_scene();
    let root_before = tree.root_id();

    tree.change_scene_to_packed(&simple_packed("SceneB")).unwrap();

    let root_after = tree.root_id();
    assert_eq!(
        root_before, root_after,
        "root node ID should not change after scene change"
    );

    let root_node = tree.get_node(root_after).unwrap();
    assert_eq!(root_node.name(), "root", "root name should stay 'root'");
}

// ===========================================================================
// 5. change_scene_to_packed preserves node properties from tscn
// ===========================================================================

#[test]
fn change_to_packed_preserves_properties() {
    let tscn = r#"[gd_scene format=3]

[node name="Player" type="CharacterBody2D"]
speed = 200.0
jump_force = -400.0

[node name="Sprite" type="Sprite2D" parent="."]
visible = false
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    // Find the Player node by path.
    let player_id = tree.get_node_by_path("/root/Player");
    assert!(player_id.is_some(), "Player node should exist in tree");

    let player = tree.get_node(player_id.unwrap()).unwrap();
    assert_eq!(
        player.get_property("speed"),
        gdvariant::Variant::Float(200.0),
        "speed property should be preserved"
    );
}

// ===========================================================================
// 6. change_scene_to_packed with invalid parent path errors
// ===========================================================================

#[test]
fn change_to_packed_invalid_parent_path_errors() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Orphan" type="Node" parent="NonExistent"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    let result = tree.change_scene_to_packed(&packed);
    assert!(
        result.is_err(),
        "should error when tscn has invalid parent path"
    );
}

// ===========================================================================
// 7. change_scene_to_node with multi-child subtree
// ===========================================================================

#[test]
fn change_to_node_with_children_manual() {
    let mut tree = make_tree_with_scene();

    // Build a multi-child node manually: NewRoot with two children.
    let new_root = Node::new("NewRoot", "Node2D");
    let new_root_id = tree.change_scene_to_node(new_root).unwrap();

    // Verify old scene is gone and new scene is under root.
    let root = tree.root_id();
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 1);
    assert_eq!(root_node.children()[0], new_root_id);

    // Add children to new scene after change.
    tree.add_child(new_root_id, Node::new("ChildX", "Sprite2D"))
        .unwrap();
    tree.add_child(new_root_id, Node::new("ChildY", "Area2D"))
        .unwrap();

    let new_root_node = tree.get_node(new_root_id).unwrap();
    assert_eq!(new_root_node.children().len(), 2);
}

// ===========================================================================
// 8. change_scene_to_node: old scene children are fully removed
// ===========================================================================

#[test]
fn change_to_node_old_children_fully_removed() {
    let mut tree = make_tree_with_scene();

    // Count nodes before (root + SceneA + Child1 + Child2 = 4).
    let count_before = tree.node_count();
    assert_eq!(count_before, 4, "should have 4 nodes before change");

    tree.change_scene_to_node(Node::new("NewScene", "Node")).unwrap();

    // After: root + NewScene = 2.
    let count_after = tree.node_count();
    assert_eq!(count_after, 2, "should have 2 nodes after change");
}

// ===========================================================================
// 9. change_scene_to_packed: old scene with deep hierarchy is fully removed
// ===========================================================================

#[test]
fn change_to_packed_old_deep_hierarchy_fully_removed() {
    let mut tree = make_tree();
    let root = tree.root_id();

    // Build deep old scene: A -> B -> C -> D.
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Node")).unwrap();
    let c = tree.add_child(b, Node::new("C", "Node")).unwrap();
    tree.add_child(c, Node::new("D", "Node")).unwrap();

    assert_eq!(tree.node_count(), 5, "root + 4 deep nodes");

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed = simple_packed("Fresh");
    tree.change_scene_to_packed(&packed).unwrap();

    // Only root + Fresh should remain.
    assert_eq!(tree.node_count(), 2, "root + Fresh after change");

    // All old nodes should have received EXIT_TREE.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.len() >= 4, "all 4 old nodes should exit: {exits:?}");
}

// ===========================================================================
// 10. change_scene_to_node then change_scene_to_packed interleaved
// ===========================================================================

#[test]
fn interleaved_node_and_packed_changes() {
    let mut tree = make_tree();

    // Start with node-based scene.
    tree.change_scene_to_node(Node::new("NodeScene", "Node2D")).unwrap();
    assert_eq!(tree.node_count(), 2);

    // Switch to packed scene.
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_packed(&packed_with_children()).unwrap();

    // Verify lifecycle ordering: all exits before enters.
    let seq = lifecycle_sequence(&tree);
    let last_exit = seq.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter = seq.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit, first_enter) {
        assert!(le < fe, "all EXIT before any ENTER: {seq:?}");
    }

    // Switch back to a node-based scene.
    tree.event_trace_mut().clear();

    tree.change_scene_to_node(Node::new("BackToNode", "Control")).unwrap();

    let seq2 = lifecycle_sequence(&tree);
    let last_exit2 = seq2.iter().rposition(|(_, d)| d == "EXIT_TREE");
    let first_enter2 = seq2.iter().position(|(_, d)| d == "ENTER_TREE");
    if let (Some(le), Some(fe)) = (last_exit2, first_enter2) {
        assert!(le < fe, "all EXIT before any ENTER (second switch): {seq2:?}");
    }

    assert_eq!(tree.node_count(), 2, "root + BackToNode");
}

// ===========================================================================
// 11. change_scene_to_packed: scene with connections parses without error
// ===========================================================================

#[test]
fn change_to_packed_with_connections_succeeds() {
    let tscn = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let ui = tree.get_node_by_path("/root/UI");
    assert!(ui.is_some(), "UI node should exist");
    let button = tree.get_node_by_path("/root/UI/Button");
    assert!(button.is_some(), "Button node should exist");
}

// ===========================================================================
// 12. change_scene_to_packed: first node with parent attribute errors
// ===========================================================================

#[test]
fn change_to_packed_root_with_parent_errors() {
    // tscn where the first node has a parent attribute (malformed).
    let tscn = r#"[gd_scene format=3]

[node name="NotRoot" type="Node" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn);
    // Should error either at parse or at instance time.
    if let Ok(packed) = packed {
        let mut tree = make_tree();
        let result = tree.change_scene_to_packed(&packed);
        assert!(result.is_err(), "root node with parent= should error");
    }
}

// ===========================================================================
// 13. change_scene_to_node: same name as old root is fine
// ===========================================================================

#[test]
fn change_to_node_same_name_as_old_scene() {
    let mut tree = make_tree();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    // Change to a new node with the same name "Scene".
    tree.change_scene_to_node(Node::new("Scene", "Node2D")).unwrap();

    // Should succeed — old "Scene" was removed before new one added.
    let enters = notification_paths(&tree, "ENTER_TREE");
    assert!(
        enters.iter().any(|p| p.ends_with("Scene")),
        "new Scene should enter: {enters:?}"
    );
    assert_eq!(tree.node_count(), 2, "root + new Scene");
}

// ===========================================================================
// 14. change_scene_to_packed: scene with groups preserves group membership
// ===========================================================================

#[test]
fn change_to_packed_preserves_groups() {
    let tscn = r#"[gd_scene format=3]

[node name="Enemy" type="CharacterBody2D" groups=["enemies", "damageable"]]

[node name="Hitbox" type="Area2D" parent="." groups=["hitboxes"]]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let enemies = tree.get_nodes_in_group("enemies");
    assert_eq!(enemies.len(), 1, "should have 1 node in 'enemies' group");

    let hitboxes = tree.get_nodes_in_group("hitboxes");
    assert_eq!(hitboxes.len(), 1, "should have 1 node in 'hitboxes' group");

    let damageable = tree.get_nodes_in_group("damageable");
    assert_eq!(damageable.len(), 1, "should have 1 node in 'damageable' group");
}

// ===========================================================================
// 15. current_scene is None on a fresh tree
// ===========================================================================

#[test]
fn current_scene_none_on_fresh_tree() {
    let tree = make_tree();
    assert!(
        tree.current_scene().is_none(),
        "fresh tree should have no current_scene"
    );
}

// ===========================================================================
// 16. change_scene_to_packed sets current_scene
// ===========================================================================

#[test]
fn change_to_packed_sets_current_scene() {
    let mut tree = make_tree();
    let packed = simple_packed("MyScene");
    let new_id = tree.change_scene_to_packed(&packed).unwrap();

    assert_eq!(
        tree.current_scene(),
        Some(new_id),
        "current_scene should track the newly instanced scene root"
    );
}

// ===========================================================================
// 17. change_scene_to_node sets current_scene
// ===========================================================================

#[test]
fn change_to_node_sets_current_scene() {
    let mut tree = make_tree();
    let new_id = tree
        .change_scene_to_node(Node::new("ManualScene", "Node2D"))
        .unwrap();

    assert_eq!(
        tree.current_scene(),
        Some(new_id),
        "current_scene should track the manually added scene"
    );
}

// ===========================================================================
// 18. sequential scene changes update current_scene each time
// ===========================================================================

#[test]
fn sequential_changes_update_current_scene() {
    let mut tree = make_tree();

    let id_a = tree
        .change_scene_to_node(Node::new("SceneA", "Node"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(id_a));

    let id_b = tree
        .change_scene_to_packed(&simple_packed("SceneB"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(id_b));
    assert_ne!(id_a, id_b, "different scenes should have different IDs");

    let id_c = tree
        .change_scene_to_node(Node::new("SceneC", "Control"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(id_c));
}

// ===========================================================================
// 19. unload_current_scene removes scene and clears current_scene
// ===========================================================================

#[test]
fn unload_current_scene_removes_and_clears() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed_with_children()).unwrap();

    assert!(tree.current_scene().is_some());
    // root + NewScene + Sprite + Body + Shape = 5 nodes
    assert_eq!(tree.node_count(), 5);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.unload_current_scene().unwrap();

    assert!(
        tree.current_scene().is_none(),
        "current_scene should be None after unload"
    );
    assert_eq!(tree.node_count(), 1, "only root should remain");

    // EXIT_TREE should have fired for the removed nodes.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(
        exits.len() >= 4,
        "all scene nodes should receive EXIT_TREE: {exits:?}"
    );
}

// ===========================================================================
// 20. unload_current_scene on empty tree is a no-op
// ===========================================================================

#[test]
fn unload_current_scene_empty_is_noop() {
    let mut tree = make_tree();
    assert!(tree.current_scene().is_none());

    tree.unload_current_scene().unwrap();

    assert!(tree.current_scene().is_none());
    assert_eq!(tree.node_count(), 1, "root still present");
}

// ===========================================================================
// 21. unload then reload: current_scene tracks correctly
// ===========================================================================

#[test]
fn unload_then_reload_tracks_current_scene() {
    let mut tree = make_tree();

    let id_a = tree
        .change_scene_to_packed(&simple_packed("SceneA"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(id_a));

    tree.unload_current_scene().unwrap();
    assert!(tree.current_scene().is_none());

    let id_b = tree
        .change_scene_to_node(Node::new("SceneB", "Node"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(id_b));
}

// ===========================================================================
// 22. current_scene cleared when scene removed via remove_node
// ===========================================================================

#[test]
fn current_scene_cleared_on_direct_remove() {
    let mut tree = make_tree();
    let scene_id = tree
        .change_scene_to_node(Node::new("DirectRemove", "Node2D"))
        .unwrap();
    assert_eq!(tree.current_scene(), Some(scene_id));

    tree.remove_node(scene_id).unwrap();
    assert!(
        tree.current_scene().is_none(),
        "current_scene should be cleared when the scene node is removed directly"
    );
}

// ===========================================================================
// 23. unload_current_scene fires EXIT_TREE bottom-up (children before parent)
// ===========================================================================

#[test]
fn unload_fires_exit_tree_bottom_up() {
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed_with_children()).unwrap();

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.unload_current_scene().unwrap();

    let exits = notification_paths(&tree, "EXIT_TREE");
    // Shape should exit before Body (child before parent).
    let shape_pos = exits.iter().position(|p| p.ends_with("Shape"));
    let body_pos = exits.iter().position(|p| p.ends_with("Body"));
    if let (Some(sp), Some(bp)) = (shape_pos, body_pos) {
        assert!(
            sp < bp,
            "Shape should EXIT_TREE before Body (bottom-up): {exits:?}"
        );
    }

    // Body and Sprite should exit before NewScene.
    let scene_pos = exits.iter().position(|p| p.ends_with("NewScene"));
    if let (Some(bp), Some(scp)) = (body_pos, scene_pos) {
        assert!(
            bp < scp,
            "Body should EXIT_TREE before NewScene: {exits:?}"
        );
    }
}

// ===========================================================================
// 24. old scene groups are cleaned up after change_scene_to_packed
// ===========================================================================

#[test]
fn old_scene_groups_cleaned_after_change() {
    let tscn_a = r#"[gd_scene format=3]

[node name="OldScene" type="Node2D" groups=["enemies"]]

[node name="Child" type="Sprite2D" parent="." groups=["enemies", "visible"]]
"#;
    let packed_a = PackedScene::from_tscn(tscn_a).unwrap();

    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed_a).unwrap();
    assert_eq!(tree.get_nodes_in_group("enemies").len(), 2);
    assert_eq!(tree.get_nodes_in_group("visible").len(), 1);

    // Replace with a scene that has no groups.
    tree.change_scene_to_packed(&simple_packed("Clean")).unwrap();

    assert_eq!(
        tree.get_nodes_in_group("enemies").len(),
        0,
        "old scene groups should be cleaned up"
    );
    assert_eq!(
        tree.get_nodes_in_group("visible").len(),
        0,
        "old scene groups should be cleaned up"
    );
}

// ===========================================================================
// 25. set_current_scene allows manual override
// ===========================================================================

#[test]
fn set_current_scene_manual_override() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("ManualChild", "Node")).unwrap();

    assert!(tree.current_scene().is_none());
    tree.set_current_scene(Some(child));
    assert_eq!(tree.current_scene(), Some(child));

    tree.set_current_scene(None);
    assert!(tree.current_scene().is_none());
}
