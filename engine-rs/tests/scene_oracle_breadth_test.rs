//! pat-09t: Broadened scene-system oracle coverage.
//!
//! Tests nested scene instancing, scenes with 20+ nodes, mixed node types,
//! groups, and signal connections in instanced scenes.

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdscene::LifecycleManager;

// ===========================================================================
// 1. Nested scene instancing — three levels deep
// ===========================================================================

#[test]
fn nested_instancing_three_levels_deep() {
    let leaf_tscn = r#"[gd_scene format=3]

[node name="Leaf" type="Sprite2D"]
"#;
    let mid_tscn = r#"[gd_scene format=3]

[node name="Mid" type="Node2D"]

[node name="Child" type="Node2D" parent="."]
"#;
    let top_tscn = r#"[gd_scene format=3]

[node name="Top" type="Node2D"]

[node name="Branch" type="Node2D" parent="."]
"#;

    let leaf_scene = PackedScene::from_tscn(leaf_tscn).unwrap();
    let mid_scene = PackedScene::from_tscn(mid_tscn).unwrap();
    let top_scene = PackedScene::from_tscn(top_tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let top_id = add_packed_scene_to_tree(&mut tree, root, &top_scene).unwrap();
    let branch = tree.get_node_relative(top_id, "Branch").unwrap();
    let mid_id = add_packed_scene_to_tree(&mut tree, branch, &mid_scene).unwrap();
    let mid_child = tree.get_node_relative(mid_id, "Child").unwrap();
    let _leaf_id = add_packed_scene_to_tree(&mut tree, mid_child, &leaf_scene).unwrap();

    // Verify full path resolves through all three levels
    assert!(tree
        .get_node_by_path("/root/Top/Branch/Mid/Child/Leaf")
        .is_some());
}

// ===========================================================================
// 2. Scene with 20+ nodes — breadth and depth
// ===========================================================================

#[test]
fn scene_with_25_nodes_builds_correctly() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let level = Node::new("Level", "Node2D");
    let level_id = tree.add_child(root, level).unwrap();

    // Add 20 direct children
    let mut child_ids = Vec::new();
    for i in 0..20 {
        let child = Node::new(&format!("Item{i}"), "Node2D");
        let cid = tree.add_child(level_id, child).unwrap();
        child_ids.push(cid);
    }

    // Add 5 nested grandchildren under first 5 children
    for i in 0..5 {
        let grandchild = Node::new(&format!("Sub{i}"), "Sprite2D");
        tree.add_child(child_ids[i], grandchild).unwrap();
    }

    // Total: root + Level + 20 children + 5 grandchildren = 27 nodes
    // Verify specific paths
    assert!(tree.get_node_by_path("/root/Level/Item0/Sub0").is_some());
    assert!(tree.get_node_by_path("/root/Level/Item4/Sub4").is_some());
    assert!(tree.get_node_by_path("/root/Level/Item19").is_some());
    assert!(tree.get_node_by_path("/root/Level/Item20").is_none()); // out of range

    // Verify child count
    let level_node = tree.get_node(level_id).unwrap();
    assert_eq!(level_node.children().len(), 20);
}

#[test]
fn large_tscn_with_20_nodes_parses() {
    let mut tscn = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n");
    for i in 0..20 {
        tscn.push_str(&format!(
            "\n[node name=\"N{i}\" type=\"Node2D\" parent=\".\"]\n"
        ));
    }

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    assert_eq!(scene.node_count(), 21); // root + 20 children

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    assert!(tree.get_node_by_path("/root/Root/N0").is_some());
    assert!(tree.get_node_by_path("/root/Root/N19").is_some());
}

// ===========================================================================
// 3. Mixed node types in a single scene
// ===========================================================================

#[test]
fn mixed_node_types_in_scene() {
    let tscn = r#"[gd_scene format=3]

[node name="Game" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="Player"]

[node name="Camera" type="Camera2D" parent="Player"]

[node name="World" type="Node" parent="."]

[node name="Ground" type="StaticBody2D" parent="World"]

[node name="UI" type="Control" parent="."]

[node name="Label" type="Label" parent="UI"]

[node name="Button" type="Button" parent="UI"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 9);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Verify class names
    let player = tree.get_node_by_path("/root/Game/Player").unwrap();
    assert_eq!(
        tree.get_node(player).unwrap().class_name(),
        "CharacterBody2D"
    );

    let sprite = tree.get_node_by_path("/root/Game/Player/Sprite").unwrap();
    assert_eq!(tree.get_node(sprite).unwrap().class_name(), "Sprite2D");

    let camera = tree.get_node_by_path("/root/Game/Player/Camera").unwrap();
    assert_eq!(tree.get_node(camera).unwrap().class_name(), "Camera2D");

    let ground = tree.get_node_by_path("/root/Game/World/Ground").unwrap();
    assert_eq!(tree.get_node(ground).unwrap().class_name(), "StaticBody2D");

    let ui = tree.get_node_by_path("/root/Game/UI").unwrap();
    assert_eq!(tree.get_node(ui).unwrap().class_name(), "Control");

    let label = tree.get_node_by_path("/root/Game/UI/Label").unwrap();
    assert_eq!(tree.get_node(label).unwrap().class_name(), "Label");
}

// ===========================================================================
// 4. Groups
// ===========================================================================

#[test]
fn nodes_added_to_groups() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut enemy1 = Node::new("Enemy1", "Node2D");
    enemy1.add_to_group("enemies");
    enemy1.add_to_group("damageable");
    let e1 = tree.add_child(root, enemy1).unwrap();

    let mut enemy2 = Node::new("Enemy2", "Node2D");
    enemy2.add_to_group("enemies");
    let e2 = tree.add_child(root, enemy2).unwrap();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.add_to_group("damageable");
    let p = tree.add_child(root, player).unwrap();

    // Check group membership
    assert!(tree.get_node(e1).unwrap().is_in_group("enemies"));
    assert!(tree.get_node(e1).unwrap().is_in_group("damageable"));
    assert!(tree.get_node(e2).unwrap().is_in_group("enemies"));
    assert!(!tree.get_node(e2).unwrap().is_in_group("damageable"));
    assert!(!tree.get_node(p).unwrap().is_in_group("enemies"));
    assert!(tree.get_node(p).unwrap().is_in_group("damageable"));
}

#[test]
fn tree_level_group_management() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Enemy", "Node2D");
    let nid = tree.add_child(root, node).unwrap();

    tree.add_to_group(nid, "monsters").unwrap();
    assert!(tree.get_node(nid).unwrap().is_in_group("monsters"));
}

// ===========================================================================
// 5. Signal connections defined in .tscn scenes
// ===========================================================================

#[test]
fn tscn_with_signal_connections_parsed() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Button" type="Button" parent="."]

[node name="Handler" type="Node" parent="."]

[connection signal="pressed" from="Button" to="Handler" method="_on_button_pressed"]
[connection signal="mouse_entered" from="Button" to="." method="_on_mouse_enter"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.connection_count(), 2);
    assert_eq!(scene.node_count(), 3);
}

// ===========================================================================
// 6. Lifecycle tracing on a 20+ node tree
// ===========================================================================

#[test]
fn enter_tree_traces_20_plus_nodes_top_down() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    for i in 0..20 {
        let child = Node::new(&format!("C{i}"), "Node2D");
        tree.add_child(parent_id, child).unwrap();
    }

    tree.event_trace_mut().enable();
    LifecycleManager::enter_tree(&mut tree, root);

    let enter_events: Vec<String> = tree
        .event_trace()
        .events()
        .iter()
        .filter(|e| e.detail == "ENTER_TREE" && e.event_type == TraceEventType::Notification)
        .map(|e| e.node_path.clone())
        .collect();

    // Root enters first (top-down), then Parent, then children
    assert_eq!(enter_events[0], "/root");
    assert_eq!(enter_events[1], "/root/Parent");
    assert!(enter_events.len() >= 22); // root + Parent + 20 children

    // Children should appear after parent
    for event in &enter_events[2..] {
        assert!(event.starts_with("/root/Parent/C"));
    }
}

// ===========================================================================
// 7. Multiple instances of same scene have independent subtrees
// ===========================================================================

#[test]
fn five_instances_of_same_scene_independent() {
    let tscn = r#"[gd_scene format=3]

[node name="Mob" type="Node2D"]

[node name="Body" type="Sprite2D" parent="."]

[node name="AI" type="Node" parent="."]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut instances = Vec::new();
    for _ in 0..5 {
        let id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        instances.push(id);
    }

    // All instances are distinct
    for i in 0..5 {
        for j in (i + 1)..5 {
            assert_ne!(instances[i], instances[j]);
        }
    }

    // Each instance has its own Body and AI
    for inst in &instances {
        assert!(tree.get_node_relative(*inst, "Body").is_some());
        assert!(tree.get_node_relative(*inst, "AI").is_some());
    }
}
