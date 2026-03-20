//! pat-op8: %UniqueName parity across instancing and reparenting.
//!
//! Verifies that %UniqueName resolution works correctly after:
//! 1. PackedScene instancing (sub-scene unique names accessible)
//! 2. Reparenting (unique node still found after move)
//! 3. Duplication (independent copies each have their own unique nodes)
//! 4. Multiple instances (unique names scoped to owner, not global)

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// 1. %UniqueName works after PackedScene instancing
// ===========================================================================

#[test]
fn op8_unique_name_resolved_after_instancing() {
    let tscn = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="%HealthBar" type="ProgressBar" parent="."]

[node name="Panel" type="Panel" parent="."]

[node name="%ScoreLabel" type="Label" parent="Panel"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %HealthBar should resolve from any node in the scene
    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    assert_eq!(tree.get_node(hb).unwrap().name(), "HealthBar");
    assert!(tree.get_node(hb).unwrap().is_unique_name());

    // %ScoreLabel (nested under Panel) should also resolve
    let sl = tree.get_node_relative(scene_root, "%ScoreLabel").unwrap();
    assert_eq!(tree.get_node(sl).unwrap().name(), "ScoreLabel");

    // Resolve from a sibling node within the scene
    let panel = tree.get_node_relative(scene_root, "Panel").unwrap();
    assert_eq!(tree.get_node_relative(panel, "%HealthBar").unwrap(), hb);
}

#[test]
fn op8_unique_name_with_path_suffix_after_instancing() {
    let tscn = r#"[gd_scene format=3]

[node name="Scene" type="Node2D"]

[node name="%Container" type="VBoxContainer" parent="."]

[node name="Label" type="Label" parent="Container"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %Container/Label should resolve the unique node then walk to Label
    let label = tree
        .get_node_relative(scene_root, "%Container/Label")
        .unwrap();
    assert_eq!(tree.get_node(label).unwrap().name(), "Label");
}

// ===========================================================================
// 2. %UniqueName survives reparenting
// ===========================================================================

#[test]
fn op8_unique_name_found_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();
    let parent_a = {
        let mut n = Node::new("A", "Node2D");
        n.set_owner(Some(scene_root));
        tree.add_child(scene_root, n).unwrap()
    };
    let parent_b = {
        let mut n = Node::new("B", "Node2D");
        n.set_owner(Some(scene_root));
        tree.add_child(scene_root, n).unwrap()
    };

    let mut unique = Node::new("Tracker", "Sprite2D");
    unique.set_unique_name(true);
    unique.set_owner(Some(scene_root));
    let uid = tree.add_child(parent_a, unique).unwrap();

    // Before reparent: resolvable from scene root
    assert_eq!(tree.get_node_relative(scene_root, "%Tracker").unwrap(), uid);

    // Reparent from A to B
    tree.reparent(uid, parent_b).unwrap();

    // After reparent: still resolvable (unique_name flag survives)
    assert_eq!(tree.get_node_relative(scene_root, "%Tracker").unwrap(), uid);
    assert!(tree.get_node(uid).unwrap().is_unique_name());
    // Normal path changed
    assert_eq!(tree.get_node_by_path("/root/Scene/B/Tracker").unwrap(), uid);
}

#[test]
fn op8_unique_name_reparent_across_branches() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();
    let deep = {
        let mut n = Node::new("Deep", "Node2D");
        n.set_owner(Some(scene));
        tree.add_child(scene, n).unwrap()
    };
    let deeper = {
        let mut n = Node::new("Deeper", "Node2D");
        n.set_owner(Some(scene));
        tree.add_child(deep, n).unwrap()
    };

    let mut unique = Node::new("Target", "Label");
    unique.set_unique_name(true);
    unique.set_owner(Some(scene));
    let tid = tree.add_child(deeper, unique).unwrap();

    assert_eq!(tree.get_node_relative(scene, "%Target").unwrap(), tid);

    // Reparent directly under scene root
    tree.reparent(tid, scene).unwrap();

    assert_eq!(tree.get_node_relative(scene, "%Target").unwrap(), tid);
    assert_eq!(tree.get_node_relative(deep, "%Target").unwrap(), tid);
}

// ===========================================================================
// 3. %UniqueName after duplication — independent copies
// ===========================================================================

#[test]
fn op8_unique_name_independent_after_duplicate() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();
    let mut unique = Node::new("Widget", "Control");
    unique.set_unique_name(true);
    unique.set_owner(Some(scene));
    let original_id = tree.add_child(scene, unique).unwrap();

    // Duplicate the subtree containing the unique node
    let clones = tree.duplicate_subtree(scene).unwrap();
    assert!(clones.len() >= 2); // scene + widget

    // The clone has the unique_name flag preserved
    let cloned_widget = clones.iter().find(|n| n.name() == "Widget").unwrap();
    assert!(cloned_widget.is_unique_name());
    assert_ne!(cloned_widget.id(), original_id);

    // The cloned root has a fresh ID, independent from the original
    let cloned_root = &clones[0];
    assert_ne!(cloned_root.id(), scene);
}

// ===========================================================================
// 4. Multiple instances: unique names scoped to owner
// ===========================================================================

#[test]
fn op8_unique_name_scoped_to_owner_instance() {
    let tscn = r#"[gd_scene format=3]

[node name="Panel" type="Panel"]

[node name="%Title" type="Label" parent="."]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance the scene twice
    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Each instance's %Title resolves to its own node
    let title1 = tree.get_node_relative(inst1, "%Title").unwrap();
    let title2 = tree.get_node_relative(inst2, "%Title").unwrap();

    assert_ne!(title1, title2);
    assert_eq!(tree.get_node(title1).unwrap().name(), "Title");
    assert_eq!(tree.get_node(title2).unwrap().name(), "Title");

    // Verify owner scoping: title1's owner is inst1
    assert_eq!(tree.get_node(title1).unwrap().owner(), Some(inst1));
    assert_eq!(tree.get_node(title2).unwrap().owner(), Some(inst2));
}

#[test]
fn op8_nonunique_node_not_resolved_by_percent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene = tree.add_child(root, Node::new("Scene", "Node2D")).unwrap();
    let mut regular = Node::new("Regular", "Node2D");
    regular.set_owner(Some(scene));
    tree.add_child(scene, regular).unwrap();

    // %Regular should NOT resolve because unique_name is false
    assert!(tree.get_node_relative(scene, "%Regular").is_none());
}
