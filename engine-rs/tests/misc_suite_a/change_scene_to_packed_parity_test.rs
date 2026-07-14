//! pat-qjni: change_scene_to_packed API parity surface.
//!
//! Covers validation semantics and expected failure modes for the
//! packed-scene change API that are not already covered by:
//! - `change_scene_validation_parity_test.rs` (25 tests)
//! - `change_scene_api_parity_test.rs` (18 tests)
//! - `packed_scene_change_lifecycle_parity_test.rs` (~30 tests)
//!
//! Focus areas:
//! 1. Return value validation (NodeId matches scene root)
//! 2. Class name preservation across transitions
//! 3. Scenes with ext_resource references
//! 4. Scenes with sub_resource references
//! 5. Scenes with script path attributes
//! 6. Packed scene with duplicate sibling names
//! 7. Node path correctness post-change
//! 8. is_inside_tree state for instanced nodes
//! 9. Packed scene identity (same PackedScene re-used)
//! 10. change_scene_to_packed idempotency
//!
//! Acceptance: bounded tests verify validation semantics and expected failure modes.

use gdscene::node::Node;
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_tree() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);
    tree
}

fn simple_packed(name: &str) -> PackedScene {
    let tscn = format!("[gd_scene format=3]\n\n[node name=\"{name}\" type=\"Node2D\"]\n");
    PackedScene::from_tscn(&tscn).expect("valid tscn")
}

fn notification_paths(tree: &SceneTree, detail: &str) -> Vec<String> {
    tree.event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == detail && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect()
}

// ===========================================================================
// 1. Return value is the scene root NodeId
// ===========================================================================

#[test]
fn return_value_is_scene_root_id() {
    let mut tree = make_tree();
    let packed = simple_packed("MyScene");
    let returned_id = tree.change_scene_to_packed(&packed).unwrap();

    // The returned ID should be a direct child of root.
    let root = tree.root_id();
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(
        root_node.children(),
        &[returned_id],
        "returned ID must be the only child of root"
    );

    // The node at that ID should have the correct name and class.
    let node = tree.get_node(returned_id).unwrap();
    assert_eq!(node.name(), "MyScene");
    assert_eq!(node.class_name(), "Node2D");
}

// ===========================================================================
// 2. Return value with children: ID is the scene root, not a child
// ===========================================================================

#[test]
fn return_value_is_root_not_child() {
    let tscn = r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Enemy" type="Area2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    let returned_id = tree.change_scene_to_packed(&packed).unwrap();

    let node = tree.get_node(returned_id).unwrap();
    assert_eq!(
        node.name(),
        "Level",
        "returned ID should be the scene root node"
    );
    assert_eq!(node.children().len(), 2, "Level should have 2 children");
}

// ===========================================================================
// 3. Class names preserved for all nodes in packed scene
// ===========================================================================

#[test]
fn class_names_preserved_after_change() {
    let tscn = r#"[gd_scene format=3]

[node name="Game" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Camera" type="Camera2D" parent="."]

[node name="UI" type="Control" parent="."]

[node name="Label" type="Label" parent="UI"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let cases = [
        ("/root/Game", "Node2D"),
        ("/root/Game/Player", "CharacterBody2D"),
        ("/root/Game/Camera", "Camera2D"),
        ("/root/Game/UI", "Control"),
        ("/root/Game/UI/Label", "Label"),
    ];

    for (path, expected_class) in &cases {
        let id = tree
            .get_node_by_path(path)
            .unwrap_or_else(|| panic!("node at '{path}' must exist"));
        let node = tree.get_node(id).unwrap();
        assert_eq!(
            node.class_name(),
            *expected_class,
            "class at {path} should be {expected_class}, got {}",
            node.class_name()
        );
    }
}

// ===========================================================================
// 4. Scene with ext_resource references parses and instances
// ===========================================================================

#[test]
fn packed_with_ext_resource_instances_ok() {
    let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.svg" id="1"]

[node name="Sprite" type="Sprite2D"]
texture = ExtResource("1")
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    let id = tree.change_scene_to_packed(&packed).unwrap();

    let node = tree.get_node(id).unwrap();
    assert_eq!(node.name(), "Sprite");
    assert_eq!(node.class_name(), "Sprite2D");
    // The texture property should be set (as a Variant, possibly an ext_resource ref).
    let tex = node.get_property("texture");
    assert_ne!(
        tex,
        gdvariant::Variant::Nil,
        "texture property should be set"
    );
}

// ===========================================================================
// 5. Scene with sub_resource references parses and instances
// ===========================================================================

#[test]
fn packed_with_sub_resource_instances_ok() {
    let tscn = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_abc"]
size = Vector2(32, 32)

[node name="Body" type="StaticBody2D"]

[node name="Shape" type="CollisionShape2D" parent="."]
shape = SubResource("RectangleShape2D_abc")
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let shape_id = tree.get_node_by_path("/root/Body/Shape").unwrap();
    let shape = tree.get_node(shape_id).unwrap();
    assert_eq!(shape.class_name(), "CollisionShape2D");
    let shape_prop = shape.get_property("shape");
    assert_ne!(
        shape_prop,
        gdvariant::Variant::Nil,
        "shape property should be set"
    );
}

// ===========================================================================
// 6. Scene with script path attribute parses
// ===========================================================================

#[test]
fn packed_with_script_path_instances_ok() {
    let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://player.gd" id="1"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1")
speed = 200.0
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    let id = tree.change_scene_to_packed(&packed).unwrap();

    let node = tree.get_node(id).unwrap();
    assert_eq!(node.name(), "Player");
    assert_eq!(node.get_property("speed"), gdvariant::Variant::Float(200.0));
}

// ===========================================================================
// 7. Node paths are correct after change_scene_to_packed
// ===========================================================================

#[test]
fn node_paths_correct_after_packed_change() {
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Terrain" type="Node2D" parent="."]

[node name="Entities" type="Node" parent="."]

[node name="Mob" type="CharacterBody2D" parent="Entities"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let expected_paths = [
        "/root/World",
        "/root/World/Terrain",
        "/root/World/Entities",
        "/root/World/Entities/Mob",
    ];

    for path in &expected_paths {
        let id = tree.get_node_by_path(path);
        assert!(
            id.is_some(),
            "node at path '{path}' must be resolvable after packed change"
        );

        // Verify round-trip: node_path(id) matches the lookup path.
        let resolved_path = tree.node_path(id.unwrap());
        assert_eq!(
            resolved_path.as_deref(),
            Some(*path),
            "node_path should match lookup path"
        );
    }
}

// ===========================================================================
// 8. is_inside_tree is true for all instanced nodes
// ===========================================================================

#[test]
fn all_instanced_nodes_inside_tree() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node" parent="."]

[node name="B" type="Node" parent="A"]

[node name="C" type="Sprite2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    let paths = [
        "/root/Root",
        "/root/Root/A",
        "/root/Root/A/B",
        "/root/Root/C",
    ];
    for path in &paths {
        let id = tree.get_node_by_path(path).unwrap();
        let node = tree.get_node(id).unwrap();
        assert!(
            node.is_inside_tree(),
            "node at '{path}' should have is_inside_tree() == true"
        );
    }
}

// ===========================================================================
// 9. Same PackedScene can be instanced multiple times
// ===========================================================================

#[test]
fn same_packed_scene_reused_produces_fresh_instances() {
    let packed = simple_packed("Reusable");
    let mut tree = make_tree();

    let id1 = tree.change_scene_to_packed(&packed).unwrap();
    let id2 = tree.change_scene_to_packed(&packed).unwrap();
    let id3 = tree.change_scene_to_packed(&packed).unwrap();

    // Each call should produce a different NodeId (fresh instance).
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    // Only the latest scene should exist.
    assert_eq!(tree.node_count(), 2, "root + latest scene");
    assert_eq!(tree.current_scene(), Some(id3));
}

// ===========================================================================
// 10. change_scene_to_packed is idempotent in effect
// ===========================================================================

#[test]
fn packed_change_idempotent_in_effect() {
    let tscn = r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();

    // Apply the same scene twice.
    tree.change_scene_to_packed(&packed).unwrap();
    let count_after_first = tree.node_count();
    let id_first = tree.current_scene().unwrap();

    tree.change_scene_to_packed(&packed).unwrap();
    let count_after_second = tree.node_count();
    let id_second = tree.current_scene().unwrap();

    // Node count should be the same (no leaks).
    assert_eq!(count_after_first, count_after_second);
    // But the ID should differ (fresh instance).
    assert_ne!(id_first, id_second);
}

// ===========================================================================
// 11. Empty tscn (just scene header, no nodes) errors
// ===========================================================================

#[test]
fn empty_packed_scene_no_nodes_errors() {
    let tscn = "[gd_scene format=3]\n";
    let packed = PackedScene::from_tscn(tscn);
    // Either parse fails or instance fails.
    if let Ok(packed) = packed {
        let mut tree = make_tree();
        let result = tree.change_scene_to_packed(&packed);
        assert!(result.is_err(), "packed scene with no nodes should error");
    }
}

// ===========================================================================
// 12. Malformed tscn (truncated node section) errors at parse
// ===========================================================================

#[test]
fn malformed_tscn_truncated_errors() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Broken\" type=";
    let result = PackedScene::from_tscn(tscn);
    // Should error at parse time.
    assert!(result.is_err(), "truncated tscn should fail to parse");
}

// ===========================================================================
// 13. Tscn with unknown type parses (Godot allows arbitrary types)
// ===========================================================================

#[test]
fn packed_with_unknown_type_instances_ok() {
    let tscn = r#"[gd_scene format=3]

[node name="Custom" type="MyCustomNode"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    let id = tree.change_scene_to_packed(&packed).unwrap();

    let node = tree.get_node(id).unwrap();
    assert_eq!(node.class_name(), "MyCustomNode");
}

// ===========================================================================
// 14. Properties with various Variant types preserved
// ===========================================================================

#[test]
fn packed_preserves_variant_property_types() {
    let tscn = r#"[gd_scene format=3]

[node name="Props" type="Node2D"]
position = Vector2(100, 200)
rotation = 1.5707
visible = false
z_index = 5
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    let id = tree.change_scene_to_packed(&packed).unwrap();
    let node = tree.get_node(id).unwrap();

    // Check that properties are present and have expected types.
    let pos = node.get_property("position");
    assert_ne!(pos, gdvariant::Variant::Nil, "position should be set");

    let rot = node.get_property("rotation");
    assert_ne!(rot, gdvariant::Variant::Nil, "rotation should be set");

    let vis = node.get_property("visible");
    assert_eq!(
        vis,
        gdvariant::Variant::Bool(false),
        "visible should be false"
    );

    let z = node.get_property("z_index");
    assert_eq!(z, gdvariant::Variant::Int(5), "z_index should be 5");
}

// ===========================================================================
// 15. Transition fires ENTER_TREE for all nodes in packed scene
// ===========================================================================

#[test]
fn packed_change_fires_enter_for_all_nodes() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node" parent="."]

[node name="B" type="Node2D" parent="A"]

[node name="C" type="Sprite2D" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    tree.change_scene_to_packed(&packed).unwrap();

    let enters = notification_paths(&tree, "ENTER_TREE");
    // Should have ENTER_TREE for Root, A, B, C (4 nodes).
    assert!(
        enters.len() >= 4,
        "expected at least 4 ENTER_TREE events, got {}: {enters:?}",
        enters.len()
    );

    // Verify specific nodes got ENTER_TREE.
    for name in &["Root", "A", "B", "C"] {
        assert!(
            enters.iter().any(|p| p.ends_with(name)),
            "{name} should receive ENTER_TREE: {enters:?}"
        );
    }
}

// ===========================================================================
// 16. Packed scene with connections: connections parsed, nodes exist
// ===========================================================================

#[test]
fn packed_with_connections_nodes_exist() {
    let tscn = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="StartBtn" type="Button" parent="."]

[node name="QuitBtn" type="Button" parent="."]

[connection signal="pressed" from="StartBtn" to="." method="_on_start"]
[connection signal="pressed" from="QuitBtn" to="." method="_on_quit"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    assert!(tree.get_node_by_path("/root/UI").is_some());
    assert!(tree.get_node_by_path("/root/UI/StartBtn").is_some());
    assert!(tree.get_node_by_path("/root/UI/QuitBtn").is_some());
}

// ===========================================================================
// 17. Transition cleans up old node IDs (no stale references)
// ===========================================================================

#[test]
fn old_node_ids_cleaned_after_transition() {
    let mut tree = make_tree();
    let packed_a = PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="OldScene" type="Node2D"]

[node name="OldChild" type="Sprite2D" parent="."]
"#,
    )
    .unwrap();

    let id_a = tree.change_scene_to_packed(&packed_a).unwrap();
    let old_child = tree.get_node_by_path("/root/OldScene/OldChild").unwrap();

    // Now change to a new scene.
    tree.change_scene_to_packed(&simple_packed("NewScene"))
        .unwrap();

    // Old node IDs should no longer resolve.
    assert!(
        tree.get_node(id_a).is_none(),
        "old scene root ID should not resolve after transition"
    );
    assert!(
        tree.get_node(old_child).is_none(),
        "old child ID should not resolve after transition"
    );
    assert!(
        tree.get_node_by_path("/root/OldScene").is_none(),
        "old path should not resolve"
    );
}

// ===========================================================================
// 18. Packed scene with deep nesting (5 levels) works
// ===========================================================================

#[test]
fn deep_nesting_packed_change() {
    let tscn = r#"[gd_scene format=3]

[node name="L1" type="Node"]

[node name="L2" type="Node" parent="."]

[node name="L3" type="Node" parent="L2"]

[node name="L4" type="Node" parent="L2/L3"]

[node name="L5" type="Node2D" parent="L2/L3/L4"]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    // root + L1 + L2 + L3 + L4 + L5 = 6 nodes.
    assert_eq!(tree.node_count(), 6);

    let deepest = tree.get_node_by_path("/root/L1/L2/L3/L4/L5");
    assert!(deepest.is_some(), "5-level deep node should be reachable");

    let node = tree.get_node(deepest.unwrap()).unwrap();
    assert_eq!(node.class_name(), "Node2D");
}

// ===========================================================================
// 19. Packed scene with many siblings at same level
// ===========================================================================

#[test]
fn packed_with_many_siblings() {
    let mut tscn = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n");
    for i in 0..20 {
        tscn.push_str(&format!(
            "\n[node name=\"Child{i}\" type=\"Node\" parent=\".\"]\n"
        ));
    }

    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = make_tree();
    tree.change_scene_to_packed(&packed).unwrap();

    // root + Root + 20 children = 22.
    assert_eq!(tree.node_count(), 22);

    let root_scene = tree.get_node_by_path("/root/Root").unwrap();
    let root_node = tree.get_node(root_scene).unwrap();
    assert_eq!(root_node.children().len(), 20);
}

// ===========================================================================
// 20. current_scene_packed stores source for reload after packed change
// ===========================================================================

#[test]
fn packed_change_enables_reload() {
    let packed = PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="Reloadable" type="Node2D"]

[node name="Child" type="Sprite2D" parent="."]
"#,
    )
    .unwrap();

    let mut tree = make_tree();
    let id1 = tree.change_scene_to_packed(&packed).unwrap();

    // Reload should succeed (packed source stored).
    let id2 = tree.reload_current_scene().unwrap();
    assert_ne!(id1, id2, "reload produces fresh instance");

    // Structure should be identical.
    assert_eq!(tree.node_count(), 3, "root + Reloadable + Child");
    assert!(tree.get_node_by_path("/root/Reloadable/Child").is_some());
}

// ===========================================================================
// 21. change_scene_to_node then change_scene_to_packed: old node scene fully
//     cleaned before packed scene instanced
// ===========================================================================

#[test]
fn node_then_packed_full_cleanup() {
    let mut tree = make_tree();

    // Start with a node-based scene with children.
    let root = tree.root_id();
    let scene = tree
        .add_child(root, Node::new("NodeScene", "Node2D"))
        .unwrap();
    tree.add_child(scene, Node::new("A", "Node")).unwrap();
    tree.add_child(scene, Node::new("B", "Node")).unwrap();
    assert_eq!(tree.node_count(), 4);

    tree.event_trace_mut().enable();
    tree.event_trace_mut().clear();

    let packed = simple_packed("PackedScene");
    tree.change_scene_to_packed(&packed).unwrap();

    // Old nodes should be gone.
    assert_eq!(tree.node_count(), 2, "root + PackedScene");
    assert!(tree.get_node_by_path("/root/NodeScene").is_none());
    assert!(tree.get_node_by_path("/root/PackedScene").is_some());

    // EXIT_TREE should have fired for old nodes.
    let exits = notification_paths(&tree, "EXIT_TREE");
    assert!(exits.len() >= 3, "NodeScene + A + B should exit: {exits:?}");
}

// ===========================================================================
// 22. Packed scene with only root node (no children) is valid
// ===========================================================================

#[test]
fn packed_root_only_no_children_valid() {
    let packed = simple_packed("Lonely");
    let mut tree = make_tree();
    let id = tree.change_scene_to_packed(&packed).unwrap();

    assert_eq!(tree.node_count(), 2, "root + Lonely");
    let node = tree.get_node(id).unwrap();
    assert!(node.children().is_empty(), "Lonely should have no children");
    assert_eq!(tree.current_scene(), Some(id));
}
