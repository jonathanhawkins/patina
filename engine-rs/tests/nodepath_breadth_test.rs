//! pat-99r: Broadened packed-scene and NodePath parity coverage.
//!
//! Tests ../Sibling resolution, %UniqueNode paths, deeply nested paths (5+ levels),
//! relative path resolution after reparenting, ../../Sibling, ./Child/GrandChild,
//! paths with :property subnames, empty paths, and paths to non-existent nodes.

use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a deep chain: root -> L1 -> L2 -> L3 -> L4 -> L5 -> Leaf
fn build_deep_tree() -> (SceneTree, Vec<gdscene::NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = vec![root];

    let names = ["L1", "L2", "L3", "L4", "L5", "Leaf"];
    let mut parent = root;
    for name in names {
        let node = Node::new(name, "Node2D");
        let id = tree.add_child(parent, node).unwrap();
        ids.push(id);
        parent = id;
    }
    (tree, ids)
}

// ===========================================================================
// 1. ../Sibling resolution
// ===========================================================================

#[test]
fn dotdot_sibling_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let a = Node::new("A", "Node2D");
    let a_id = tree.add_child(parent_id, a).unwrap();

    let b = Node::new("B", "Node2D");
    let b_id = tree.add_child(parent_id, b).unwrap();

    let c = Node::new("C", "Node2D");
    let c_id = tree.add_child(parent_id, c).unwrap();

    // A -> ../B
    assert_eq!(tree.get_node_relative(a_id, "../B").unwrap(), b_id);
    // A -> ../C
    assert_eq!(tree.get_node_relative(a_id, "../C").unwrap(), c_id);
    // B -> ../A
    assert_eq!(tree.get_node_relative(b_id, "../A").unwrap(), a_id);
    // C -> ../A
    assert_eq!(tree.get_node_relative(c_id, "../A").unwrap(), a_id);
}

#[test]
fn dotdot_sibling_nested_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("P", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();

    let left = Node::new("Left", "Node2D");
    let lid = tree.add_child(pid, left).unwrap();

    let left_child = Node::new("LC", "Sprite2D");
    let _lcid = tree.add_child(lid, left_child).unwrap();

    let right = Node::new("Right", "Node2D");
    let rid = tree.add_child(pid, right).unwrap();

    let right_child = Node::new("RC", "Sprite2D");
    let rcid = tree.add_child(rid, right_child).unwrap();

    // From Left/LC, navigate to Right/RC
    let lc = tree.get_node_relative(lid, "LC").unwrap();
    assert_eq!(tree.get_node_relative(lc, "../../Right/RC").unwrap(), rcid);
}

// ===========================================================================
// 2. %UniqueNode — flag preservation and normal-path access
// ===========================================================================

#[test]
fn unique_name_flag_survives_reparenting() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent_a = Node::new("PA", "Node2D");
    let pa_id = tree.add_child(root, parent_a).unwrap();

    let parent_b = Node::new("PB", "Node2D");
    let pb_id = tree.add_child(root, parent_b).unwrap();

    let mut unique = Node::new("UniqueNode", "Node2D");
    unique.set_unique_name(true);
    let uid = tree.add_child(pa_id, unique).unwrap();

    assert!(tree.get_node(uid).unwrap().is_unique_name());

    // Reparent to different parent
    tree.reparent(uid, pb_id).unwrap();

    // Unique flag should persist after reparent
    assert!(tree.get_node(uid).unwrap().is_unique_name());
    assert_eq!(tree.get_node_relative(pb_id, "UniqueNode").unwrap(), uid);
}

#[test]
fn multiple_unique_nodes_at_different_levels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut top = Node::new("TopUnique", "Node2D");
    top.set_unique_name(true);
    let top_id = tree.add_child(root, top).unwrap();

    let mid = Node::new("Mid", "Node2D");
    let mid_id = tree.add_child(top_id, mid).unwrap();

    let mut deep = Node::new("DeepUnique", "Sprite2D");
    deep.set_unique_name(true);
    let deep_id = tree.add_child(mid_id, deep).unwrap();

    assert!(tree.get_node(top_id).unwrap().is_unique_name());
    assert!(!tree.get_node(mid_id).unwrap().is_unique_name());
    assert!(tree.get_node(deep_id).unwrap().is_unique_name());

    // Still accessible via normal paths
    assert_eq!(
        tree.get_node_by_path("/root/TopUnique/Mid/DeepUnique")
            .unwrap(),
        deep_id
    );
}

// ===========================================================================
// 3. Deeply nested paths (5+ levels)
// ===========================================================================

#[test]
fn resolve_path_five_levels_deep() {
    let (tree, ids) = build_deep_tree();
    let leaf = *ids.last().unwrap();

    assert_eq!(
        tree.get_node_by_path("/root/L1/L2/L3/L4/L5/Leaf").unwrap(),
        leaf
    );
}

#[test]
fn relative_navigation_five_levels_up() {
    let (tree, ids) = build_deep_tree();
    let leaf = ids[6]; // Leaf
    let l1 = ids[1]; // L1

    // From Leaf, go 5 levels up to L1
    assert_eq!(tree.get_node_relative(leaf, "../../../../..").unwrap(), l1);
}

#[test]
fn relative_from_deep_to_sibling_subtree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Branch A: A -> A1 -> A2 -> A3
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Node2D")).unwrap();
    let a2 = tree.add_child(a1, Node::new("A2", "Node2D")).unwrap();
    let _a3 = tree.add_child(a2, Node::new("A3", "Node2D")).unwrap();

    // Branch B: B -> B1 -> B2
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let b1 = tree.add_child(b, Node::new("B1", "Node2D")).unwrap();
    let b2 = tree.add_child(b1, Node::new("B2", "Node2D")).unwrap();

    // From A/A1/A2/A3, navigate to B/B1/B2
    assert_eq!(
        tree.get_node_relative(_a3, "../../../../B/B1/B2").unwrap(),
        b2
    );
}

// ===========================================================================
// 4. Relative paths after reparenting
// ===========================================================================

#[test]
fn relative_path_updates_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let old_parent = tree.add_child(root, Node::new("Old", "Node2D")).unwrap();
    let new_parent = tree.add_child(root, Node::new("New", "Node2D")).unwrap();
    let child = tree
        .add_child(old_parent, Node::new("Child", "Node2D"))
        .unwrap();
    let grandchild = tree.add_child(child, Node::new("GC", "Sprite2D")).unwrap();

    // Before reparent
    assert_eq!(
        tree.get_node_by_path("/root/Old/Child/GC").unwrap(),
        grandchild
    );

    // Reparent Child (with its subtree) under New
    tree.reparent(child, new_parent).unwrap();

    // Old path should no longer work
    assert!(tree.get_node_by_path("/root/Old/Child/GC").is_none());

    // New path should work
    assert_eq!(
        tree.get_node_by_path("/root/New/Child/GC").unwrap(),
        grandchild
    );

    // Relative path from grandchild to new parent
    assert_eq!(
        tree.get_node_relative(grandchild, "../..").unwrap(),
        new_parent
    );

    // Relative path from new parent to grandchild
    assert_eq!(
        tree.get_node_relative(new_parent, "Child/GC").unwrap(),
        grandchild
    );
}

#[test]
fn sibling_relative_path_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let container = tree
        .add_child(root, Node::new("Container", "Node2D"))
        .unwrap();
    let a = tree.add_child(container, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(container, Node::new("B", "Node2D")).unwrap();
    let target = tree.add_child(root, Node::new("Target", "Node2D")).unwrap();

    // Initially A and B are siblings
    assert_eq!(tree.get_node_relative(a, "../B").unwrap(), b);

    // Reparent A under Target
    tree.reparent(a, target).unwrap();

    // A and B are no longer siblings
    assert!(tree.get_node_relative(a, "../B").is_none());

    // But A can still reach B via absolute-style relative navigation
    assert_eq!(tree.get_node_relative(a, "../../Container/B").unwrap(), b);
}

// ===========================================================================
// 5. NodePath in packed scenes with deep nesting
// ===========================================================================

#[test]
fn packed_scene_deep_nesting_paths() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Node2D" parent="A"]

[node name="C" type="Node2D" parent="A/B"]

[node name="D" type="Sprite2D" parent="A/B/C"]

[node name="E" type="Node" parent="A/B/C/D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 6);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Verify full chain
    let e = tree.get_node_by_path("/root/Root/A/B/C/D/E").unwrap();
    assert_eq!(tree.get_node(e).unwrap().class_name(), "Node");

    // Relative navigation from E back to Root
    assert_eq!(
        tree.get_node_relative(e, "../../../../..").unwrap(),
        scene_root
    );
}

// ===========================================================================
// 6. node_path() correctness after reparent
// ===========================================================================

#[test]
fn node_path_correct_after_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
    let child = tree.add_child(a, Node::new("Child", "Sprite2D")).unwrap();

    assert_eq!(tree.node_path(child).unwrap(), "/root/A/Child");

    tree.reparent(child, b).unwrap();

    assert_eq!(tree.node_path(child).unwrap(), "/root/B/Child");
}

// ===========================================================================
// 7. ../../Sibling resolution (pat-99r broadening)
// ===========================================================================

#[test]
fn dotdot_dotdot_sibling_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let grandparent = tree.add_child(root, Node::new("GP", "Node2D")).unwrap();
    let parent = tree
        .add_child(grandparent, Node::new("Parent", "Node2D"))
        .unwrap();
    let child = tree
        .add_child(parent, Node::new("Child", "Node2D"))
        .unwrap();
    let sibling_of_parent = tree
        .add_child(grandparent, Node::new("Uncle", "Node2D"))
        .unwrap();

    // From Child, ../../Uncle reaches the sibling of Parent
    assert_eq!(
        tree.get_node_relative(child, "../../Uncle").unwrap(),
        sibling_of_parent
    );
}

// ===========================================================================
// 8. ./Child/GrandChild resolution
// ===========================================================================

#[test]
fn dot_slash_child_grandchild() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
    let child = tree
        .add_child(parent, Node::new("Child", "Node2D"))
        .unwrap();
    let grandchild = tree
        .add_child(child, Node::new("GrandChild", "Sprite2D"))
        .unwrap();

    // ./Child/GrandChild from Parent
    assert_eq!(
        tree.get_node_relative(parent, "./Child/GrandChild")
            .unwrap(),
        grandchild
    );
    // ./GrandChild from Child
    assert_eq!(
        tree.get_node_relative(child, "./GrandChild").unwrap(),
        grandchild
    );
}

// ===========================================================================
// 9. NodePath with :property subnames (parsing only — resolution strips subnames)
// ===========================================================================

#[test]
fn nodepath_property_subname_parsing() {
    use gdcore::NodePath;

    let p = NodePath::new("Player:position");
    assert_eq!(p.get_name_count(), 1);
    assert_eq!(p.get_name(0), Some("Player"));
    assert_eq!(p.get_subname_count(), 1);
    assert_eq!(p.get_subname(0), Some("position"));

    let p2 = NodePath::new("../Sibling:scale:x");
    assert_eq!(p2.get_name_count(), 2);
    assert_eq!(p2.get_name(0), Some(".."));
    assert_eq!(p2.get_name(1), Some("Sibling"));
    assert_eq!(p2.get_subname_count(), 2);
    assert_eq!(p2.get_subname(0), Some("scale"));
    assert_eq!(p2.get_subname(1), Some("x"));
    assert_eq!(p2.get_concatenated_subnames(), "scale:x");
}

#[test]
fn nodepath_absolute_with_subnames() {
    use gdcore::NodePath;

    let p = NodePath::new("/root/Player/Sprite:modulate:a");
    assert!(p.is_absolute());
    assert_eq!(p.get_name_count(), 3);
    assert_eq!(p.get_name(0), Some("root"));
    assert_eq!(p.get_name(1), Some("Player"));
    assert_eq!(p.get_name(2), Some("Sprite"));
    assert_eq!(p.get_subname_count(), 2);
    assert_eq!(p.get_subname(0), Some("modulate"));
    assert_eq!(p.get_subname(1), Some("a"));
}

// ===========================================================================
// 10. Empty path and edge cases
// ===========================================================================

#[test]
fn empty_path_returns_self() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("A", "Node2D")).unwrap();

    // Empty relative path returns self
    assert_eq!(tree.get_node_relative(node, "").unwrap(), node);
    // Empty via get_node_or_null
    assert_eq!(tree.get_node_or_null(node, "").unwrap(), node);
}

#[test]
fn dot_only_returns_self() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    assert_eq!(tree.get_node_relative(node, ".").unwrap(), node);
}

// ===========================================================================
// 11. Paths to non-existent nodes
// ===========================================================================

#[test]
fn nonexistent_child_returns_none() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("P", "Node2D")).unwrap();
    tree.add_child(parent, Node::new("A", "Node2D")).unwrap();

    assert!(tree.get_node_relative(parent, "B").is_none());
    assert!(tree.get_node_relative(parent, "A/NonExistent").is_none());
    assert!(tree.get_node_relative(parent, "A/B/C/D").is_none());
}

#[test]
fn nonexistent_absolute_path_returns_none() {
    let tree = SceneTree::new();

    assert!(tree.get_node_by_path("/root/DoesNotExist").is_none());
    assert!(tree.get_node_by_path("/root/A/B/C").is_none());
    assert!(tree.get_node_by_path("/wrong").is_none());
}

#[test]
fn dotdot_past_root_returns_none() {
    let tree = SceneTree::new();
    let root = tree.root_id();

    assert!(tree.get_node_relative(root, "..").is_none());
    assert!(tree.get_node_relative(root, "../..").is_none());
}

// ===========================================================================
// 12. %UniqueName within packed scene breadth (pat-99r)
// ===========================================================================

#[test]
fn unique_name_in_packed_scene_breadth() {
    let tscn = r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="Enemies" type="Node2D" parent="."]

[node name="%BossEnemy" type="CharacterBody2D" parent="Enemies"]

[node name="UI" type="Control" parent="."]

[node name="%HUD" type="CanvasLayer" parent="UI"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let level = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %BossEnemy resolves from level root
    let boss = tree.get_node_relative(level, "%BossEnemy").unwrap();
    assert_eq!(tree.get_node(boss).unwrap().name(), "BossEnemy");

    // %HUD resolves from level root
    let hud = tree.get_node_relative(level, "%HUD").unwrap();
    assert_eq!(tree.get_node(hud).unwrap().name(), "HUD");

    // Cross-resolve: from boss's parent, find HUD
    let enemies = tree.get_node_relative(level, "Enemies").unwrap();
    assert_eq!(tree.get_node_relative(enemies, "%HUD").unwrap(), hud);
}
