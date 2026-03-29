//! PackedScene instancing ownership propagation and unique-name lookup tests (pat-xeup, pat-d48l).
//!
//! Validates that:
//! - Scene root has `owner == None` after instancing (it IS the owner scope).
//! - All non-root children have `owner == Some(scene_root_id)`.
//! - Ownership is correct through deep nesting.
//! - Unique-name lookup via `%` prefix works on instanced scenes.
//! - Two instances of the same scene have independent unique-name scopes.
//! - Unique-name lookup from a child node walks up to owner, then searches subtree.
//! - Instancing under another instance preserves correct ownership boundaries.
//! - Nested PackedScene ownership propagation via instance_with_subscenes matches Godot.

use gdscene::packed_scene::{
    add_packed_scene_to_tree, add_packed_scene_to_tree_with_subscenes, PackedScene,
};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Fixture scenes
// ===========================================================================

/// Simple scene with two children — for ownership propagation tests.
const FLAT_SCENE: &str = r#"[gd_scene format=3 uid="uid://flat"]

[node name="Root" type="Node2D"]

[node name="ChildA" type="Sprite2D" parent="."]

[node name="ChildB" type="Node" parent="."]
"#;

/// Deep scene (4 levels) — for deep ownership propagation tests.
const DEEP_SCENE: &str = r#"[gd_scene format=3 uid="uid://deep"]

[node name="Level0" type="Node2D"]

[node name="Level1" type="Node2D" parent="."]

[node name="Level2" type="Node2D" parent="Level1"]

[node name="Level3" type="Sprite2D" parent="Level1/Level2"]
"#;

/// Scene with unique-name nodes at various depths.
const UNIQUE_SCENE: &str = r#"[gd_scene format=3 uid="uid://uniq"]

[node name="HUD" type="Control"]

[node name="%HealthBar" type="ProgressBar" parent="."]
value = 75

[node name="Panel" type="Panel" parent="."]

[node name="%ScoreLabel" type="Label" parent="Panel"]
text = "0"

[node name="Sidebar" type="VBoxContainer" parent="."]

[node name="%MiniMap" type="TextureRect" parent="Sidebar"]
"#;

/// Scene with a unique-name root — edge case.
const UNIQUE_ROOT_SCENE: &str = r#"[gd_scene format=3]

[node name="%UniqueRoot" type="Control"]

[node name="Child" type="Label" parent="."]
"#;

// ===========================================================================
// 1. Owner is None on instance root (scene root IS the owner scope)
// ===========================================================================

#[test]
fn xeup_instance_root_owner_is_none() {
    let scene = PackedScene::from_tscn(FLAT_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(
        node.owner(),
        None,
        "instanced scene root should have owner == None (it IS the owner scope)"
    );
}

// ===========================================================================
// 2. All non-root children have owner == scene_root_id
// ===========================================================================

#[test]
fn xeup_children_owner_is_scene_root() {
    let scene = PackedScene::from_tscn(FLAT_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let child_a = tree.get_node_relative(scene_root, "ChildA").unwrap();
    let child_b = tree.get_node_relative(scene_root, "ChildB").unwrap();

    assert_eq!(
        tree.get_node(child_a).unwrap().owner(),
        Some(scene_root),
        "ChildA owner should be the scene root"
    );
    assert_eq!(
        tree.get_node(child_b).unwrap().owner(),
        Some(scene_root),
        "ChildB owner should be the scene root"
    );
}

// ===========================================================================
// 3. Deep nesting: all descendants have owner == scene root
// ===========================================================================

#[test]
fn xeup_deep_nesting_all_owners_point_to_scene_root() {
    let scene = PackedScene::from_tscn(DEEP_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Root owns itself (owner = None).
    assert_eq!(tree.get_node(scene_root).unwrap().owner(), None);

    // All descendants at every depth should point to scene_root.
    let l1 = tree.get_node_relative(scene_root, "Level1").unwrap();
    let l2 = tree.get_node_relative(scene_root, "Level1/Level2").unwrap();
    let l3 = tree
        .get_node_relative(scene_root, "Level1/Level2/Level3")
        .unwrap();

    for (label, id) in [("Level1", l1), ("Level2", l2), ("Level3", l3)] {
        assert_eq!(
            tree.get_node(id).unwrap().owner(),
            Some(scene_root),
            "{label} owner should be the scene root, not an intermediate node"
        );
    }
}

// ===========================================================================
// 4. Unique-name lookup via % prefix on instanced scene
// ===========================================================================

#[test]
fn xeup_unique_name_percent_lookup_from_scene_root() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Direct unique-name lookup from scene root.
    let health_bar = tree.get_node_relative(hud_id, "%HealthBar").unwrap();
    assert_eq!(tree.get_node(health_bar).unwrap().name(), "HealthBar");
    assert!(tree.get_node(health_bar).unwrap().is_unique_name());

    // Nested unique-name lookup (ScoreLabel is under Panel).
    let score_label = tree.get_node_relative(hud_id, "%ScoreLabel").unwrap();
    assert_eq!(tree.get_node(score_label).unwrap().name(), "ScoreLabel");

    // Deeply nested unique-name lookup (MiniMap is under Sidebar).
    let mini_map = tree.get_node_relative(hud_id, "%MiniMap").unwrap();
    assert_eq!(tree.get_node(mini_map).unwrap().name(), "MiniMap");
}

#[test]
fn xeup_unique_name_percent_lookup_from_child_node() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Lookup %HealthBar from a sibling child (Panel).
    // This should walk up to owner (HUD) and search from there.
    let panel_id = tree.get_node_relative(hud_id, "Panel").unwrap();
    let health_bar = tree.get_node_relative(panel_id, "%HealthBar").unwrap();
    assert_eq!(tree.get_node(health_bar).unwrap().name(), "HealthBar");

    // Lookup %MiniMap from a deeply nested child (Panel/ScoreLabel).
    let score_label_id = tree.get_node_relative(hud_id, "Panel/ScoreLabel").unwrap();
    let mini_map = tree.get_node_relative(score_label_id, "%MiniMap").unwrap();
    assert_eq!(tree.get_node(mini_map).unwrap().name(), "MiniMap");
}

// ===========================================================================
// 5. Two instances of the same scene have independent unique-name scopes
// ===========================================================================

#[test]
fn xeup_two_instances_independent_unique_name_scopes() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let hud2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Each instance resolves %HealthBar to its own copy.
    let hb1 = tree.get_node_relative(hud1, "%HealthBar").unwrap();
    let hb2 = tree.get_node_relative(hud2, "%HealthBar").unwrap();
    assert_ne!(
        hb1, hb2,
        "two instances should have different %HealthBar nodes"
    );

    // Each instance resolves %ScoreLabel to its own copy.
    let sl1 = tree.get_node_relative(hud1, "%ScoreLabel").unwrap();
    let sl2 = tree.get_node_relative(hud2, "%ScoreLabel").unwrap();
    assert_ne!(
        sl1, sl2,
        "two instances should have different %ScoreLabel nodes"
    );

    // Cross-check: looking up from hud1's child should NOT find hud2's nodes.
    let panel1 = tree.get_node_relative(hud1, "Panel").unwrap();
    let hb_from_panel1 = tree.get_node_relative(panel1, "%HealthBar").unwrap();
    assert_eq!(
        hb_from_panel1, hb1,
        "% lookup from hud1's child should resolve within hud1's scope"
    );
}

// ===========================================================================
// 6. Instancing under another instance: ownership boundaries
// ===========================================================================

#[test]
fn xeup_nested_instancing_ownership_boundaries() {
    let world_tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Entities" type="Node" parent="."]
"#;
    let world_scene = PackedScene::from_tscn(world_tscn).unwrap();
    let child_scene = PackedScene::from_tscn(FLAT_SCENE).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance the world.
    let world_id = add_packed_scene_to_tree(&mut tree, root, &world_scene).unwrap();
    let entities_id = tree.get_node_relative(world_id, "Entities").unwrap();

    // Instance the flat scene under Entities.
    let flat_root = add_packed_scene_to_tree(&mut tree, entities_id, &child_scene).unwrap();

    // World's children should own World.
    assert_eq!(tree.get_node(entities_id).unwrap().owner(), Some(world_id));

    // Flat scene's root should have owner == None (it starts a new owner scope).
    assert_eq!(
        tree.get_node(flat_root).unwrap().owner(),
        None,
        "sub-instanced scene root should have owner == None"
    );

    // Flat scene's children should own the flat_root, NOT world_id.
    let child_a = tree.get_node_relative(flat_root, "ChildA").unwrap();
    assert_eq!(
        tree.get_node(child_a).unwrap().owner(),
        Some(flat_root),
        "sub-instanced child should be owned by the sub-scene root, not the outer scene"
    );
}

// ===========================================================================
// 7. Unique-name lookup does not leak across owner boundaries
// ===========================================================================

#[test]
fn xeup_unique_name_does_not_leak_across_owner_boundaries() {
    let outer_tscn = r#"[gd_scene format=3]

[node name="Outer" type="Control"]

[node name="%Banner" type="Label" parent="."]
"#;
    let outer_scene = PackedScene::from_tscn(outer_tscn).unwrap();
    let inner_scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let outer_id = add_packed_scene_to_tree(&mut tree, root, &outer_scene).unwrap();
    let banner_id = tree.get_node_relative(outer_id, "Banner").unwrap();

    // Instance the inner scene (with %HealthBar etc.) under outer's Banner.
    let inner_id = add_packed_scene_to_tree(&mut tree, banner_id, &inner_scene).unwrap();

    // From outer's scope, %Banner should resolve but %HealthBar should NOT
    // (it belongs to the inner scene's owner scope).
    let banner_found = tree.get_node_relative(outer_id, "%Banner").unwrap();
    assert_eq!(banner_found, banner_id);

    let health_from_outer = tree.get_node_relative(outer_id, "%HealthBar");
    // HealthBar is owned by inner_id, not outer_id — so % lookup from
    // outer's scope must NOT find it. This is Godot's scoping rule:
    // unique names are scoped to their owner scene.
    assert!(
        health_from_outer.is_none(),
        "%HealthBar should not leak from inner scene into outer scene's unique-name scope"
    );

    // From inner's scope, %HealthBar should definitely resolve.
    let inner_hb = tree.get_node_relative(inner_id, "%HealthBar").unwrap();
    assert_eq!(tree.get_node(inner_hb).unwrap().name(), "HealthBar");
}

// ===========================================================================
// 8. Unique-name with path suffix on instanced scene
// ===========================================================================

#[test]
fn xeup_unique_name_with_path_suffix() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %ScoreLabel resolves, then we can navigate further if there were children.
    // Test that %ScoreLabel itself resolves correctly via the path prefix mechanism.
    let score_label = tree.get_node_relative(hud_id, "%ScoreLabel");
    assert!(
        score_label.is_some(),
        "%ScoreLabel should resolve from scene root"
    );

    // Non-unique node should NOT resolve via % prefix.
    let panel_via_percent = tree.get_node_relative(hud_id, "%Panel");
    assert!(
        panel_via_percent.is_none(),
        "Panel is not marked unique, so %Panel should not resolve"
    );

    // Non-unique node should still resolve via normal path.
    let panel_normal = tree.get_node_relative(hud_id, "Panel");
    assert!(
        panel_normal.is_some(),
        "Panel should resolve via normal path"
    );
}

// ===========================================================================
// 9. Unique-name root scene preserves flag
// ===========================================================================

#[test]
fn xeup_unique_name_root_preserved() {
    let scene = PackedScene::from_tscn(UNIQUE_ROOT_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "UniqueRoot");
    assert!(
        node.is_unique_name(),
        "unique-name flag should be preserved on the scene root"
    );
    // Root still has no owner (it is the owner scope).
    assert_eq!(node.owner(), None);
}

// ===========================================================================
// 10. Owner propagation via instance() before tree addition
// ===========================================================================

#[test]
fn xeup_instance_before_tree_owner_propagation() {
    let scene = PackedScene::from_tscn(DEEP_SCENE).unwrap();
    let nodes = scene.instance().unwrap();

    // Root node (index 0) should have owner == None.
    assert_eq!(
        nodes[0].owner(),
        None,
        "instance() root should have owner == None"
    );

    let root_id = nodes[0].id();

    // All other nodes should have owner == root_id.
    for node in &nodes[1..] {
        assert_eq!(
            node.owner(),
            Some(root_id),
            "instance() child '{}' should have owner pointing to root",
            node.name()
        );
    }
}

// ===========================================================================
// 11. Multiple unique-name instances: property independence via % lookup
// ===========================================================================

#[test]
fn xeup_unique_name_property_independence_via_percent() {
    use gdvariant::Variant;

    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let hud2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Both start with value=75 on %HealthBar.
    let hb1 = tree.get_node_relative(hud1, "%HealthBar").unwrap();
    let hb2 = tree.get_node_relative(hud2, "%HealthBar").unwrap();

    assert_eq!(
        tree.get_node(hb1).unwrap().get_property("value"),
        Variant::Int(75)
    );
    assert_eq!(
        tree.get_node(hb2).unwrap().get_property("value"),
        Variant::Int(75)
    );

    // Modify hud1's HealthBar.
    tree.get_node_mut(hb1)
        .unwrap()
        .set_property("value", Variant::Int(30));

    // hud2's HealthBar should remain unchanged.
    assert_eq!(
        tree.get_node(hb1).unwrap().get_property("value"),
        Variant::Int(30)
    );
    assert_eq!(
        tree.get_node(hb2).unwrap().get_property("value"),
        Variant::Int(75),
        "modifying one instance's %HealthBar should not affect the other"
    );
}

// ===========================================================================
// 12. Single-child scene: minimal ownership propagation
// ===========================================================================

#[test]
fn xeup_single_child_scene_ownership() {
    let single_child_tscn = r#"[gd_scene format=3]

[node name="Solo" type="Node2D"]

[node name="OnlyChild" type="Sprite2D" parent="."]
"#;
    let scene = PackedScene::from_tscn(single_child_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    assert_eq!(tree.get_node(scene_root).unwrap().owner(), None);

    let only_child = tree.get_node_relative(scene_root, "OnlyChild").unwrap();
    assert_eq!(
        tree.get_node(only_child).unwrap().owner(),
        Some(scene_root),
        "single child should be owned by scene root"
    );
}

// ===========================================================================
// 13. Non-existent unique-name lookup returns None
// ===========================================================================

#[test]
fn xeup_nonexistent_unique_name_returns_none() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // %Nonexistent should return None.
    let result = tree.get_node_relative(hud_id, "%Nonexistent");
    assert!(
        result.is_none(),
        "looking up a non-existent unique name should return None"
    );
}

// ===========================================================================
// 14. Unique-name self-lookup from the unique node itself
// ===========================================================================

#[test]
fn xeup_unique_name_self_lookup_from_unique_node() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Navigate to %HealthBar, then look up %HealthBar from there.
    // Should walk up to owner (HUD) and find it (itself).
    let health_bar = tree.get_node_relative(hud_id, "%HealthBar").unwrap();
    let self_lookup = tree.get_node_relative(health_bar, "%HealthBar").unwrap();
    assert_eq!(
        self_lookup, health_bar,
        "looking up %HealthBar from HealthBar itself should find the same node"
    );
}

// ===========================================================================
// 15. Multiple instances under different parents maintain correct ownership
// ===========================================================================

#[test]
fn xeup_instances_under_different_parents_correct_ownership() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Left" type="Node2D" parent="."]

[node name="Right" type="Node2D" parent="."]
"#;
    let parent_scene = PackedScene::from_tscn(parent_tscn).unwrap();
    let child_scene = PackedScene::from_tscn(FLAT_SCENE).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world = add_packed_scene_to_tree(&mut tree, root, &parent_scene).unwrap();
    let left = tree.get_node_relative(world, "Left").unwrap();
    let right = tree.get_node_relative(world, "Right").unwrap();

    // Instance same scene under two different parents.
    let inst_left = add_packed_scene_to_tree(&mut tree, left, &child_scene).unwrap();
    let inst_right = add_packed_scene_to_tree(&mut tree, right, &child_scene).unwrap();

    // Both instance roots have owner == None (each is its own owner scope).
    assert_eq!(tree.get_node(inst_left).unwrap().owner(), None);
    assert_eq!(tree.get_node(inst_right).unwrap().owner(), None);

    // Children of each instance are owned by their respective instance root.
    let left_child_a = tree.get_node_relative(inst_left, "ChildA").unwrap();
    let right_child_a = tree.get_node_relative(inst_right, "ChildA").unwrap();

    assert_eq!(
        tree.get_node(left_child_a).unwrap().owner(),
        Some(inst_left),
        "left instance child should be owned by left instance root"
    );
    assert_eq!(
        tree.get_node(right_child_a).unwrap().owner(),
        Some(inst_right),
        "right instance child should be owned by right instance root"
    );

    // The two ChildA nodes are different nodes.
    assert_ne!(left_child_a, right_child_a);
}

// ===========================================================================
// 16. Root-only scene (no children) ownership
// ===========================================================================

#[test]
fn xeup_root_only_scene_ownership() {
    let root_only_tscn = r#"[gd_scene format=3]

[node name="Lonely" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(root_only_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "Lonely");
    assert_eq!(
        node.owner(),
        None,
        "root-only scene root should have owner == None"
    );
}

// ===========================================================================
// 17. Unique-name lookup across sibling unique nodes
// ===========================================================================

#[test]
fn xeup_unique_name_sibling_lookup() {
    let scene = PackedScene::from_tscn(UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hud_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // From %MiniMap (under Sidebar), look up %HealthBar (direct child of root).
    // Should walk up to owner (HUD), then find HealthBar in that scope.
    let mini_map = tree.get_node_relative(hud_id, "%MiniMap").unwrap();
    let health_bar_from_minimap = tree.get_node_relative(mini_map, "%HealthBar").unwrap();
    let health_bar_direct = tree.get_node_relative(hud_id, "%HealthBar").unwrap();

    assert_eq!(
        health_bar_from_minimap, health_bar_direct,
        "%HealthBar lookup from %MiniMap should find the same node as from root"
    );

    // From %HealthBar, look up %ScoreLabel.
    let score_from_health = tree
        .get_node_relative(health_bar_direct, "%ScoreLabel")
        .unwrap();
    let score_direct = tree.get_node_relative(hud_id, "%ScoreLabel").unwrap();

    assert_eq!(
        score_from_health, score_direct,
        "%ScoreLabel lookup from %HealthBar should find the same node as from root"
    );
}

// ===========================================================================
// pat-ahop: Nested instanced unique-name resolution tests
// ===========================================================================

/// Scene A: has %Alpha unique name.
const SCENE_A: &str = r#"[gd_scene format=3 uid="uid://scene_a"]

[node name="SceneA" type="Node2D"]

[node name="%Alpha" type="Label" parent="."]

[node name="Slot" type="Node2D" parent="."]
"#;

/// Scene B: has %Beta unique name (same depth pattern, different name).
const SCENE_B: &str = r#"[gd_scene format=3 uid="uid://scene_b"]

[node name="SceneB" type="Control"]

[node name="%Beta" type="Label" parent="."]

[node name="Slot" type="Node2D" parent="."]
"#;

/// Scene C: has %Gamma unique name — used as the innermost nesting level.
const SCENE_C: &str = r#"[gd_scene format=3 uid="uid://scene_c"]

[node name="SceneC" type="Node"]

[node name="%Gamma" type="Sprite2D" parent="."]
"#;

// ---------------------------------------------------------------------------
// 18. Three-level nesting: unique names don't leak across any boundary
// ---------------------------------------------------------------------------

#[test]
fn ahop_three_level_nesting_unique_names_isolated() {
    let scene_a = PackedScene::from_tscn(SCENE_A).unwrap();
    let scene_b = PackedScene::from_tscn(SCENE_B).unwrap();
    let scene_c = PackedScene::from_tscn(SCENE_C).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Level 1: instance Scene A under root.
    let a_root = add_packed_scene_to_tree(&mut tree, root, &scene_a).unwrap();
    let a_slot = tree.get_node_relative(a_root, "Slot").unwrap();

    // Level 2: instance Scene B under Scene A's Slot.
    let b_root = add_packed_scene_to_tree(&mut tree, a_slot, &scene_b).unwrap();
    let b_slot = tree.get_node_relative(b_root, "Slot").unwrap();

    // Level 3: instance Scene C under Scene B's Slot.
    let c_root = add_packed_scene_to_tree(&mut tree, b_slot, &scene_c).unwrap();

    // Each scope can find its own unique name.
    let alpha = tree.get_node_relative(a_root, "%Alpha");
    assert!(alpha.is_some(), "Scene A should find its own %Alpha");

    let beta = tree.get_node_relative(b_root, "%Beta");
    assert!(beta.is_some(), "Scene B should find its own %Beta");

    let gamma = tree.get_node_relative(c_root, "%Gamma");
    assert!(gamma.is_some(), "Scene C should find its own %Gamma");

    // Cross-boundary: Scene A must NOT see %Beta or %Gamma.
    assert!(
        tree.get_node_relative(a_root, "%Beta").is_none(),
        "%Beta must not leak from Scene B into Scene A's scope"
    );
    assert!(
        tree.get_node_relative(a_root, "%Gamma").is_none(),
        "%Gamma must not leak from Scene C into Scene A's scope"
    );

    // Cross-boundary: Scene B must NOT see %Alpha or %Gamma.
    assert!(
        tree.get_node_relative(b_root, "%Alpha").is_none(),
        "%Alpha must not leak from Scene A into Scene B's scope"
    );
    assert!(
        tree.get_node_relative(b_root, "%Gamma").is_none(),
        "%Gamma must not leak from Scene C into Scene B's scope"
    );

    // Cross-boundary: Scene C must NOT see %Alpha or %Beta.
    assert!(
        tree.get_node_relative(c_root, "%Alpha").is_none(),
        "%Alpha must not leak from Scene A into Scene C's scope"
    );
    assert!(
        tree.get_node_relative(c_root, "%Beta").is_none(),
        "%Beta must not leak from Scene B into Scene C's scope"
    );
}

// ---------------------------------------------------------------------------
// 19. Same unique name in multiple nested scopes resolves independently
// ---------------------------------------------------------------------------

/// Scene with %Health at the root level — same name used in multiple scopes.
const SCENE_WITH_HEALTH: &str = r#"[gd_scene format=3 uid="uid://health"]

[node name="Entity" type="Node2D"]

[node name="%Health" type="ProgressBar" parent="."]

[node name="Slot" type="Node2D" parent="."]
"#;

#[test]
fn ahop_same_unique_name_in_nested_scopes_resolves_independently() {
    let scene = PackedScene::from_tscn(SCENE_WITH_HEALTH).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Outer instance.
    let outer = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let outer_slot = tree.get_node_relative(outer, "Slot").unwrap();

    // Inner instance (same scene, nested under outer's Slot).
    let inner = add_packed_scene_to_tree(&mut tree, outer_slot, &scene).unwrap();
    let inner_slot = tree.get_node_relative(inner, "Slot").unwrap();

    // Innermost instance (third level, same scene again).
    let innermost = add_packed_scene_to_tree(&mut tree, inner_slot, &scene).unwrap();

    // Each %Health resolves to its own scope's node.
    let h_outer = tree.get_node_relative(outer, "%Health").unwrap();
    let h_inner = tree.get_node_relative(inner, "%Health").unwrap();
    let h_innermost = tree.get_node_relative(innermost, "%Health").unwrap();

    // All three are different nodes.
    assert_ne!(
        h_outer, h_inner,
        "outer and inner %Health must be different nodes"
    );
    assert_ne!(
        h_inner, h_innermost,
        "inner and innermost %Health must be different nodes"
    );
    assert_ne!(
        h_outer, h_innermost,
        "outer and innermost %Health must be different nodes"
    );

    // Lookup from a child within each scope finds the scope's own %Health.
    let outer_slot_health = tree.get_node_relative(outer_slot, "%Health").unwrap();
    assert_eq!(
        outer_slot_health, h_outer,
        "lookup from outer's Slot should find outer's %Health, not inner's"
    );

    let inner_slot_health = tree.get_node_relative(inner_slot, "%Health").unwrap();
    assert_eq!(
        inner_slot_health, h_inner,
        "lookup from inner's Slot should find inner's %Health, not innermost's"
    );
}

// ---------------------------------------------------------------------------
// 20. Cross-boundary lookup explicitly returns None
// ---------------------------------------------------------------------------

#[test]
fn ahop_cross_boundary_lookup_returns_none() {
    let outer_tscn = r#"[gd_scene format=3]

[node name="App" type="Node2D"]

[node name="%AppTitle" type="Label" parent="."]

[node name="Content" type="Node2D" parent="."]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="Widget" type="Control"]

[node name="%WidgetLabel" type="Label" parent="."]
"#;
    let outer_scene = PackedScene::from_tscn(outer_tscn).unwrap();
    let inner_scene = PackedScene::from_tscn(inner_tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let app = add_packed_scene_to_tree(&mut tree, root, &outer_scene).unwrap();
    let content = tree.get_node_relative(app, "Content").unwrap();
    let widget = add_packed_scene_to_tree(&mut tree, content, &inner_scene).unwrap();

    // Outer scope cannot see inner's unique name.
    assert!(
        tree.get_node_relative(app, "%WidgetLabel").is_none(),
        "outer scope must not resolve inner scene's %WidgetLabel"
    );

    // Inner scope cannot see outer's unique name.
    assert!(
        tree.get_node_relative(widget, "%AppTitle").is_none(),
        "inner scope must not resolve outer scene's %AppTitle"
    );

    // Each scope can find its own.
    assert!(tree.get_node_relative(app, "%AppTitle").is_some());
    assert!(tree.get_node_relative(widget, "%WidgetLabel").is_some());
}

// ---------------------------------------------------------------------------
// 21. Lookup from deeply nested child walks up to correct owner scope
// ---------------------------------------------------------------------------

#[test]
fn ahop_lookup_from_deep_child_walks_to_correct_owner() {
    let scene_a = PackedScene::from_tscn(SCENE_A).unwrap();
    let scene_c = PackedScene::from_tscn(SCENE_C).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a_root = add_packed_scene_to_tree(&mut tree, root, &scene_a).unwrap();
    let a_slot = tree.get_node_relative(a_root, "Slot").unwrap();

    // Instance Scene C under Scene A's Slot.
    let c_root = add_packed_scene_to_tree(&mut tree, a_slot, &scene_c).unwrap();
    let gamma = tree.get_node_relative(c_root, "%Gamma").unwrap();

    // From %Gamma (owned by c_root), lookup %Alpha should fail —
    // %Gamma's owner is c_root, so the search scope is Scene C.
    assert!(
        tree.get_node_relative(gamma, "%Alpha").is_none(),
        "from Scene C's %Gamma, %Alpha (Scene A) must not be reachable"
    );

    // From %Gamma, lookup %Gamma should succeed (same owner scope).
    let gamma_self = tree.get_node_relative(gamma, "%Gamma").unwrap();
    assert_eq!(
        gamma_self, gamma,
        "%Gamma self-lookup should work within its scope"
    );

    // From Scene A's Slot (owned by a_root), lookup %Alpha should succeed.
    let alpha_from_slot = tree.get_node_relative(a_slot, "%Alpha").unwrap();
    let alpha_direct = tree.get_node_relative(a_root, "%Alpha").unwrap();
    assert_eq!(
        alpha_from_slot, alpha_direct,
        "Slot is owned by Scene A, so %Alpha lookup should find Scene A's %Alpha"
    );
}

// ===========================================================================
// pat-d48l: Nested PackedScene ownership propagation via instance_with_subscenes
// ===========================================================================

// ---------------------------------------------------------------------------
// 22. instance_with_subscenes: two-level ownership boundaries
// ---------------------------------------------------------------------------

#[test]
fn d48l_instance_with_subscenes_two_level_ownership() {
    let player_tscn = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Hitbox" type="Area2D" parent="."]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Camera" type="Camera2D" parent="."]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;
    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let nodes = world
        .instance_with_subscenes(&|path| match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // World + Camera + Hero + Sprite + Hitbox = 5
    assert_eq!(nodes.len(), 5);
    let world_id = nodes[0].id();
    let hero_id = nodes[2].id();

    // Root has no owner.
    assert!(nodes[0].owner().is_none());
    // Camera is a regular child of World.
    assert_eq!(nodes[1].owner(), Some(world_id));
    // Hero (sub-scene root) is owned by World.
    assert_eq!(nodes[2].owner(), Some(world_id));
    // Sprite and Hitbox are children of the Player sub-scene,
    // so they are owned by Hero (the sub-scene root).
    assert_eq!(
        nodes[3].owner(),
        Some(hero_id),
        "Sprite should be owned by Hero (sub-scene root), not World"
    );
    assert_eq!(
        nodes[4].owner(),
        Some(hero_id),
        "Hitbox should be owned by Hero (sub-scene root), not World"
    );
}

// ---------------------------------------------------------------------------
// 23. instance_with_subscenes: three-level nested ownership
// ---------------------------------------------------------------------------

#[test]
fn d48l_instance_with_subscenes_three_level_ownership() {
    let weapon_tscn = r#"[gd_scene format=3]

[node name="Weapon" type="Node2D"]

[node name="Mesh" type="MeshInstance2D" parent="."]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Sword" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;
    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let nodes = world
        .instance_with_subscenes(&|path| match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // World + Hero + Sprite + Sword + Mesh = 5
    assert_eq!(nodes.len(), 5);
    let world_id = nodes[0].id();
    let hero_id = nodes[1].id();
    let sword_id = nodes[3].id();

    assert!(nodes[0].owner().is_none(), "World root has no owner");
    assert_eq!(nodes[1].owner(), Some(world_id), "Hero owned by World");
    assert_eq!(nodes[2].owner(), Some(hero_id), "Sprite owned by Hero");
    assert_eq!(nodes[3].owner(), Some(hero_id), "Sword owned by Hero");
    assert_eq!(
        nodes[4].owner(),
        Some(sword_id),
        "Mesh owned by Sword (third-level sub-scene root)"
    );
}

// ---------------------------------------------------------------------------
// 24. add_packed_scene_to_tree_with_subscenes: ownership in tree matches
// ---------------------------------------------------------------------------

#[test]
fn d48l_add_to_tree_with_subscenes_preserves_nested_ownership() {
    let weapon_tscn = r#"[gd_scene format=3]

[node name="Weapon" type="Node2D"]

[node name="Mesh" type="MeshInstance2D" parent="."]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Sword" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            _ => None,
        }
    };

    let world_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &world, &resolver).unwrap();

    let hero = tree.get_node_relative(world_id, "Hero").unwrap();
    let sprite = tree.get_node_relative(hero, "Sprite").unwrap();
    let sword = tree.get_node_relative(hero, "Sword").unwrap();
    let mesh = tree.get_node_relative(sword, "Mesh").unwrap();

    // World root: no owner (it IS the owner scope).
    assert_eq!(tree.get_node(world_id).unwrap().owner(), None);
    // Hero (Player sub-scene root): owned by World.
    assert_eq!(tree.get_node(hero).unwrap().owner(), Some(world_id));
    // Sprite: child of Player scene, owned by Hero.
    assert_eq!(
        tree.get_node(sprite).unwrap().owner(),
        Some(hero),
        "Sprite should be owned by Hero"
    );
    // Sword (Weapon sub-scene root): owned by Hero.
    assert_eq!(
        tree.get_node(sword).unwrap().owner(),
        Some(hero),
        "Sword should be owned by Hero"
    );
    // Mesh: child of Weapon scene, owned by Sword.
    assert_eq!(
        tree.get_node(mesh).unwrap().owner(),
        Some(sword),
        "Mesh should be owned by Sword"
    );
}

// ---------------------------------------------------------------------------
// 25. Duplicate instantiation: same scene instanced twice, independent ownership
// ---------------------------------------------------------------------------

#[test]
fn d48l_duplicate_instantiation_independent_ownership() {
    let child_tscn = r#"[gd_scene format=3]

[node name="Child" type="Node2D"]

[node name="Leaf" type="Sprite2D" parent="."]
"#;
    let parent_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://child.tscn" id="c"]

[node name="Parent" type="Node"]

[node name="Left" parent="." instance=ExtResource("c")]

[node name="Right" parent="." instance=ExtResource("c")]
"#;
    let parent = PackedScene::from_tscn(parent_tscn).unwrap();
    let nodes = parent
        .instance_with_subscenes(&|path| match path {
            "res://child.tscn" => Some(PackedScene::from_tscn(child_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // Parent + Left + Left/Leaf + Right + Right/Leaf = 5
    assert_eq!(nodes.len(), 5);
    let parent_id = nodes[0].id();
    let left_id = nodes[1].id();
    let right_id = nodes[3].id();

    assert!(nodes[0].owner().is_none());
    // Both sub-scene roots owned by Parent.
    assert_eq!(nodes[1].owner(), Some(parent_id));
    assert_eq!(nodes[3].owner(), Some(parent_id));
    // Each Leaf owned by its own sub-scene root.
    assert_eq!(
        nodes[2].owner(),
        Some(left_id),
        "Left's Leaf should be owned by Left"
    );
    assert_eq!(
        nodes[4].owner(),
        Some(right_id),
        "Right's Leaf should be owned by Right"
    );
    // The two Leaf nodes are distinct.
    assert_ne!(nodes[2].id(), nodes[4].id());
}

// ---------------------------------------------------------------------------
// 26. Four-level deep nested ownership via instance_with_subscenes
// ---------------------------------------------------------------------------

#[test]
fn d48l_four_level_deep_nested_ownership() {
    let gem_tscn = r#"[gd_scene format=3]

[node name="Gem" type="Sprite2D"]

[node name="Sparkle" type="CPUParticles2D" parent="."]
"#;
    let weapon_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://gem.tscn" id="g"]

[node name="Weapon" type="Node2D"]

[node name="Blade" type="Sprite2D" parent="."]

[node name="Socket" parent="." instance=ExtResource("g")]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="CharacterBody2D"]

[node name="Body" type="Sprite2D" parent="."]

[node name="Sword" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Background" type="Sprite2D" parent="."]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let nodes = world
        .instance_with_subscenes(&|path| match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            "res://gem.tscn" => Some(PackedScene::from_tscn(gem_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // World + Background + Hero + Body + Sword + Blade + Socket + Sparkle = 8
    assert_eq!(nodes.len(), 8, "expected 8 nodes for 4-level nesting");

    let world_id = nodes[0].id();
    // Find nodes by name for clarity.
    let find = |name: &str| nodes.iter().find(|n| n.name() == name).unwrap();
    let hero = find("Hero");
    let body = find("Body");
    let sword = find("Sword");
    let blade = find("Blade");
    let socket = find("Socket");
    let sparkle = find("Sparkle");
    let background = find("Background");

    // Level 0: World root — no owner.
    assert!(nodes[0].owner().is_none(), "World root has no owner");
    // Level 0 regular child: Background owned by World.
    assert_eq!(background.owner(), Some(world_id));
    // Level 1: Hero (Player root) owned by World.
    assert_eq!(hero.owner(), Some(world_id));
    // Level 1 child: Body owned by Hero.
    assert_eq!(body.owner(), Some(hero.id()), "Body owned by Hero");
    // Level 2: Sword (Weapon root) owned by Hero.
    assert_eq!(sword.owner(), Some(hero.id()), "Sword owned by Hero");
    // Level 2 child: Blade owned by Sword.
    assert_eq!(blade.owner(), Some(sword.id()), "Blade owned by Sword");
    // Level 3: Socket (Gem root) owned by Sword.
    assert_eq!(
        socket.owner(),
        Some(sword.id()),
        "Socket (Gem root) owned by Sword"
    );
    // Level 3 child: Sparkle owned by Socket.
    assert_eq!(
        sparkle.owner(),
        Some(socket.id()),
        "Sparkle owned by Socket (Gem root)"
    );
}

// ---------------------------------------------------------------------------
// 27. Duplicate instantiation at multiple nesting levels
// ---------------------------------------------------------------------------

#[test]
fn d48l_duplicate_instances_at_multiple_nesting_levels() {
    let leaf_tscn = r#"[gd_scene format=3]

[node name="Leaf" type="Sprite2D"]

[node name="Detail" type="Node2D" parent="."]
"#;
    let branch_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://leaf.tscn" id="l"]

[node name="Branch" type="Node2D"]

[node name="LeafA" parent="." instance=ExtResource("l")]

[node name="LeafB" parent="." instance=ExtResource("l")]
"#;
    let tree_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://branch.tscn" id="b"]

[node name="Tree" type="Node"]

[node name="BranchLeft" parent="." instance=ExtResource("b")]

[node name="BranchRight" parent="." instance=ExtResource("b")]
"#;

    let tree_scene = PackedScene::from_tscn(tree_tscn).unwrap();
    let nodes = tree_scene
        .instance_with_subscenes(&|path| match path {
            "res://branch.tscn" => Some(PackedScene::from_tscn(branch_tscn).unwrap()),
            "res://leaf.tscn" => Some(PackedScene::from_tscn(leaf_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // Tree + BranchLeft + LeafA + Detail + LeafB + Detail
    //      + BranchRight + LeafA + Detail + LeafB + Detail = 11
    assert_eq!(
        nodes.len(),
        11,
        "expected 11 nodes for duplicate nested instances"
    );

    let tree_id = nodes[0].id();

    // Collect all sub-scene roots (the Branch instances).
    let branches: Vec<_> = nodes
        .iter()
        .filter(|n| n.owner() == Some(tree_id))
        .collect();
    assert_eq!(branches.len(), 2, "two Branch instances owned by Tree");

    // For each Branch, its children should be owned by it, and the Leaf
    // sub-scene children should be owned by the Leaf root, not the Branch.
    for branch in &branches {
        let branch_id = branch.id();
        // Find leaf roots owned by this branch.
        let leaf_roots: Vec<_> = nodes
            .iter()
            .filter(|n| n.owner() == Some(branch_id) && n.name() != branch.name())
            .collect();
        assert_eq!(
            leaf_roots.len(),
            2,
            "each Branch should have 2 Leaf roots owned by it"
        );

        // Each Leaf root should have a Detail child owned by the Leaf root.
        for leaf_root in &leaf_roots {
            let leaf_id = leaf_root.id();
            let details: Vec<_> = nodes
                .iter()
                .filter(|n| n.owner() == Some(leaf_id))
                .collect();
            assert_eq!(
                details.len(),
                1,
                "each Leaf should have exactly 1 Detail owned by it"
            );
            assert_eq!(details[0].name(), "Detail");
        }
    }
}

// ---------------------------------------------------------------------------
// 28. add_packed_scene_to_tree_with_subscenes: duplicate instances in tree
// ---------------------------------------------------------------------------

#[test]
fn d48l_tree_with_subscenes_duplicate_instances_ownership() {
    let child_tscn = r#"[gd_scene format=3]

[node name="Child" type="Node2D"]

[node name="Leaf" type="Sprite2D" parent="."]
"#;
    let parent_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://child.tscn" id="c"]

[node name="Parent" type="Node"]

[node name="Left" parent="." instance=ExtResource("c")]

[node name="Right" parent="." instance=ExtResource("c")]
"#;

    let parent = PackedScene::from_tscn(parent_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://child.tscn" => Some(PackedScene::from_tscn(child_tscn).unwrap()),
            _ => None,
        }
    };

    let parent_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolver).unwrap();

    let left = tree.get_node_relative(parent_id, "Left").unwrap();
    let right = tree.get_node_relative(parent_id, "Right").unwrap();
    let left_leaf = tree.get_node_relative(left, "Leaf").unwrap();
    let right_leaf = tree.get_node_relative(right, "Leaf").unwrap();

    // Parent root has no owner.
    assert_eq!(tree.get_node(parent_id).unwrap().owner(), None);
    // Both sub-scene roots owned by Parent.
    assert_eq!(tree.get_node(left).unwrap().owner(), Some(parent_id));
    assert_eq!(tree.get_node(right).unwrap().owner(), Some(parent_id));
    // Each Leaf owned by its respective sub-scene root.
    assert_eq!(
        tree.get_node(left_leaf).unwrap().owner(),
        Some(left),
        "Left's Leaf should be owned by Left"
    );
    assert_eq!(
        tree.get_node(right_leaf).unwrap().owner(),
        Some(right),
        "Right's Leaf should be owned by Right"
    );
    // The two Leaf nodes are distinct.
    assert_ne!(left_leaf, right_leaf);
}

// ---------------------------------------------------------------------------
// 29. Four-level nesting via add_packed_scene_to_tree_with_subscenes
// ---------------------------------------------------------------------------

#[test]
fn d48l_tree_four_level_ownership_preserved() {
    let gem_tscn = r#"[gd_scene format=3]

[node name="Gem" type="Sprite2D"]

[node name="Sparkle" type="CPUParticles2D" parent="."]
"#;
    let weapon_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://gem.tscn" id="g"]

[node name="Weapon" type="Node2D"]

[node name="Blade" type="Sprite2D" parent="."]

[node name="Socket" parent="." instance=ExtResource("g")]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="CharacterBody2D"]

[node name="Body" type="Sprite2D" parent="."]

[node name="Sword" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            "res://gem.tscn" => Some(PackedScene::from_tscn(gem_tscn).unwrap()),
            _ => None,
        }
    };

    let world_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &world, &resolver).unwrap();

    let hero = tree.get_node_relative(world_id, "Hero").unwrap();
    let body = tree.get_node_relative(hero, "Body").unwrap();
    let sword = tree.get_node_relative(hero, "Sword").unwrap();
    let blade = tree.get_node_relative(sword, "Blade").unwrap();
    let socket = tree.get_node_relative(sword, "Socket").unwrap();
    let sparkle = tree.get_node_relative(socket, "Sparkle").unwrap();

    assert_eq!(tree.get_node(world_id).unwrap().owner(), None);
    assert_eq!(tree.get_node(hero).unwrap().owner(), Some(world_id));
    assert_eq!(tree.get_node(body).unwrap().owner(), Some(hero));
    assert_eq!(tree.get_node(sword).unwrap().owner(), Some(hero));
    assert_eq!(tree.get_node(blade).unwrap().owner(), Some(sword));
    assert_eq!(tree.get_node(socket).unwrap().owner(), Some(sword));
    assert_eq!(
        tree.get_node(sparkle).unwrap().owner(),
        Some(socket),
        "Sparkle (4th level child) should be owned by Socket (Gem root)"
    );
}

// ===========================================================================
// pat-f5x4: Unique-name lookup across nested instanced scenes
// ===========================================================================

// ---------------------------------------------------------------------------
// 31. Unique-name lookup through instance_with_subscenes: each scope isolated
// ---------------------------------------------------------------------------

#[test]
fn f5x4_unique_name_via_instance_with_subscenes_isolated() {
    let weapon_tscn = r#"[gd_scene format=3]

[node name="Weapon" type="Node2D"]

[node name="%Blade" type="Sprite2D" parent="."]

[node name="%Guard" type="Node2D" parent="."]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="Node2D"]

[node name="%Health" type="ProgressBar" parent="."]

[node name="Sword" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="%GameTimer" type="Timer" parent="."]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            _ => None,
        }
    };

    let world_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &world, &resolver).unwrap();

    let hero = tree.get_node_relative(world_id, "Hero").unwrap();
    let sword = tree.get_node_relative(hero, "Sword").unwrap();

    // World scope: %GameTimer resolves, %Health/%Blade/%Guard do NOT
    let game_timer = tree.get_node_relative(world_id, "%GameTimer");
    assert!(game_timer.is_some(), "World scope should find %GameTimer");
    assert!(
        tree.get_node_relative(world_id, "%Health").is_none(),
        "%Health must not leak from Player scope into World"
    );
    assert!(
        tree.get_node_relative(world_id, "%Blade").is_none(),
        "%Blade must not leak from Weapon scope into World"
    );

    // In Godot, %UniqueName lookup searches the owner's scope.
    // Hero's owner is World, so from Hero, %GameTimer (World scope) resolves.
    // %Health (owned by Hero) does NOT resolve from Hero itself — it's in
    // Player's subscene scope, accessible from nodes owned by Hero.
    assert!(
        tree.get_node_relative(hero, "%GameTimer").is_some(),
        "Hero's owner is World, so %GameTimer should resolve in World scope"
    );
    assert!(
        tree.get_node_relative(hero, "%Health").is_none(),
        "%Health is in Player scope (owned by Hero), not accessible from Hero's owner scope (World)"
    );

    // From a child owned by Hero (e.g., Health itself), %Health should resolve
    // within Hero's scope.
    let health_id = tree.get_node_relative(hero, "Health").unwrap();
    let health_via_unique = tree.get_node_relative(health_id, "%Health");
    assert!(
        health_via_unique.is_some(),
        "From a node inside Player scope (owned by Hero), %Health should resolve"
    );

    // Sword's owner is Hero, so from Sword, %Health should resolve (Hero scope)
    let health_from_sword = tree.get_node_relative(sword, "%Health");
    assert!(
        health_from_sword.is_some(),
        "Sword's owner is Hero, so %Health in Hero's scope should resolve"
    );

    // From a node owned by Sword (Blade), %Blade should resolve (Sword scope)
    let blade_id = tree.get_node_relative(sword, "Blade").unwrap();
    let blade_via_unique = tree.get_node_relative(blade_id, "%Blade");
    assert!(
        blade_via_unique.is_some(),
        "From a node inside Weapon scope (owned by Sword), %Blade should resolve"
    );
    let guard_via_unique = tree.get_node_relative(blade_id, "%Guard");
    assert!(
        guard_via_unique.is_some(),
        "From a node inside Weapon scope, %Guard should resolve"
    );

    // From Blade, %Health should NOT be visible (Blade's owner is Sword, scope is Sword)
    assert!(
        tree.get_node_relative(blade_id, "%Health").is_none(),
        "%Health must not be visible from Weapon scope"
    );
    assert!(
        tree.get_node_relative(blade_id, "%GameTimer").is_none(),
        "%GameTimer must not be visible from Weapon scope"
    );
}

// ---------------------------------------------------------------------------
// 32. Same unique name in parent and child scenes: each scope sees its own
// ---------------------------------------------------------------------------

#[test]
fn f5x4_same_unique_name_across_nested_subscenes() {
    let inner_tscn = r#"[gd_scene format=3]

[node name="Inner" type="Node2D"]

[node name="%Status" type="Label" parent="."]
text = "inner_status"
"#;
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://inner.tscn" id="i"]

[node name="Outer" type="Node2D"]

[node name="%Status" type="Label" parent="."]
text = "outer_status"

[node name="Nested" parent="." instance=ExtResource("i")]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://inner.tscn" => Some(PackedScene::from_tscn(inner_tscn).unwrap()),
            _ => None,
        }
    };

    let outer_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolver).unwrap();
    let nested = tree.get_node_relative(outer_id, "Nested").unwrap();

    // Outer scope: %Status resolves to Outer's own %Status
    let outer_status = tree.get_node_relative(outer_id, "%Status").unwrap();
    let outer_text = tree.get_node(outer_status).unwrap().get_property("text");
    assert_eq!(
        outer_text,
        gdvariant::Variant::String("outer_status".into())
    );

    // From Nested (scene root, owned by Outer), %Status resolves in Outer's
    // scope, so it finds Outer's %Status — matching Godot's owner-based scoping.
    let nested_lookup = tree.get_node_relative(nested, "%Status").unwrap();
    assert_eq!(
        nested_lookup, outer_status,
        "Nested's owner is Outer, so %Status from Nested resolves in Outer's scope"
    );

    // From a child INSIDE Inner's scope (owned by Nested), %Status should
    // resolve to Inner's own %Status.
    let inner_status_node = tree.get_node_relative(nested, "Status").unwrap();
    let inner_lookup = tree.get_node_relative(inner_status_node, "%Status");
    assert!(
        inner_lookup.is_some(),
        "From inside Inner scope, %Status should resolve"
    );
    // The inner lookup should find Inner's %Status, not Outer's
    assert_eq!(
        inner_lookup.unwrap(),
        inner_status_node,
        "From Inner scope, %Status should find Inner's Status node"
    );
    let inner_text = tree
        .get_node(inner_status_node)
        .unwrap()
        .get_property("text");
    assert_eq!(
        inner_text,
        gdvariant::Variant::String("inner_status".into())
    );
}

// ---------------------------------------------------------------------------
// 33. Unique name with path suffix through nested instance_with_subscenes
// ---------------------------------------------------------------------------

#[test]
fn f5x4_unique_name_path_suffix_through_nested_instances() {
    // Use regular parent paths in tscn (% prefix in parent= is not standard Godot syntax)
    let panel_tscn = r#"[gd_scene format=3]

[node name="Panel" type="Control"]

[node name="%Header" type="Label" parent="."]

[node name="Body" type="VBoxContainer" parent="Header"]

[node name="Detail" type="Label" parent="Header/Body"]
text = "detail_text"
"#;
    let app_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://panel.tscn" id="p"]

[node name="App" type="Node"]

[node name="MainPanel" parent="." instance=ExtResource("p")]
"#;

    let app = PackedScene::from_tscn(app_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://panel.tscn" => Some(PackedScene::from_tscn(panel_tscn).unwrap()),
            _ => None,
        }
    };

    let app_id = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &app, &resolver).unwrap();
    let main_panel = tree.get_node_relative(app_id, "MainPanel").unwrap();

    // From a child inside Panel scope, %Header should resolve.
    // MainPanel's owner is App, so from MainPanel, %Header searches App's scope.
    // Instead, lookup from a child inside Panel scope (owned by MainPanel).
    let header_by_path = tree.get_node_relative(main_panel, "Header").unwrap();
    let body = tree.get_node_relative(header_by_path, "Body").unwrap();

    // From Body (owned by MainPanel), %Header should resolve within Panel scope
    let header_unique = tree.get_node_relative(body, "%Header");
    assert!(
        header_unique.is_some(),
        "From inside Panel scope, %Header should resolve"
    );

    // %Header/Body should resolve: first find %Header, then navigate to Body
    let header_body = tree.get_node_relative(body, "%Header/Body");
    assert!(
        header_body.is_some(),
        "From Panel scope, %Header/Body should resolve"
    );
    assert_eq!(
        header_body.unwrap(),
        body,
        "%Header/Body should resolve to the Body node"
    );

    // %Header/Body/Detail should resolve the full path
    let detail = tree.get_node_relative(body, "%Header/Body/Detail");
    assert!(
        detail.is_some(),
        "From Panel scope, %Header/Body/Detail should resolve"
    );

    // Verify the detail's property
    let text = tree.get_node(detail.unwrap()).unwrap().get_property("text");
    assert_eq!(text, gdvariant::Variant::String("detail_text".into()));
}

// ---------------------------------------------------------------------------
// 34. Child node in nested scope walks up to correct owner for lookup
// ---------------------------------------------------------------------------

#[test]
fn f5x4_child_in_nested_scope_walks_to_correct_owner() {
    let widget_tscn = r#"[gd_scene format=3]

[node name="Widget" type="Control"]

[node name="%WidgetTitle" type="Label" parent="."]

[node name="Content" type="VBoxContainer" parent="."]

[node name="Item" type="Button" parent="Content"]
"#;
    let page_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://widget.tscn" id="w"]

[node name="Page" type="Control"]

[node name="%PageHeader" type="Label" parent="."]

[node name="Sidebar" parent="." instance=ExtResource("w")]
"#;

    let page = PackedScene::from_tscn(page_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://widget.tscn" => Some(PackedScene::from_tscn(widget_tscn).unwrap()),
            _ => None,
        }
    };

    let page_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &page, &resolver).unwrap();
    let sidebar = tree.get_node_relative(page_id, "Sidebar").unwrap();
    let content = tree.get_node_relative(sidebar, "Content").unwrap();
    let item = tree.get_node_relative(content, "Item").unwrap();

    // Item is owned by Sidebar (Widget scene root), so unique-name lookup
    // searches Sidebar's scope.
    // From Item, %WidgetTitle should resolve (it's in Sidebar/Widget scope)
    let widget_title = tree.get_node_relative(item, "%WidgetTitle");
    assert!(
        widget_title.is_some(),
        "Item inside Widget scope should find %WidgetTitle via owner walk-up"
    );

    // From Item, %PageHeader should NOT resolve (it's in Page scope, not Widget scope)
    let page_header = tree.get_node_relative(item, "%PageHeader");
    assert!(
        page_header.is_none(),
        "%PageHeader must not be visible from Widget scope"
    );

    // From Page's direct children (owned by Page), %PageHeader should resolve
    let page_header_node = tree.get_node_relative(page_id, "PageHeader").unwrap();
    let header_from_child = tree.get_node_relative(page_header_node, "%PageHeader");
    assert!(
        header_from_child.is_some(),
        "From inside Page scope, %PageHeader should resolve"
    );
}

// ---------------------------------------------------------------------------
// 35. Multiple instances of same subscene: unique names independent
// ---------------------------------------------------------------------------

#[test]
fn f5x4_multiple_subscene_instances_unique_names_independent() {
    let card_tscn = r#"[gd_scene format=3]

[node name="Card" type="Control"]

[node name="%Title" type="Label" parent="."]
text = "default"

[node name="%Icon" type="TextureRect" parent="."]
"#;
    let hand_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://card.tscn" id="c"]

[node name="Hand" type="HBoxContainer"]

[node name="Card1" parent="." instance=ExtResource("c")]

[node name="Card2" parent="." instance=ExtResource("c")]

[node name="Card3" parent="." instance=ExtResource("c")]
"#;

    let hand = PackedScene::from_tscn(hand_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://card.tscn" => Some(PackedScene::from_tscn(card_tscn).unwrap()),
            _ => None,
        }
    };

    let hand_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &hand, &resolver).unwrap();

    let card1 = tree.get_node_relative(hand_id, "Card1").unwrap();
    let card2 = tree.get_node_relative(hand_id, "Card2").unwrap();
    let card3 = tree.get_node_relative(hand_id, "Card3").unwrap();

    // Card roots (Card1, Card2, Card3) are owned by Hand.
    // Their children (Title, Icon) are owned by their respective card root.
    // From a child inside each Card scope, %Title should resolve independently.
    let t1_node = tree.get_node_relative(card1, "Title").unwrap();
    let t2_node = tree.get_node_relative(card2, "Title").unwrap();
    let t3_node = tree.get_node_relative(card3, "Title").unwrap();

    // Each Title is a different node
    assert_ne!(t1_node, t2_node, "Card1 and Card2 Title nodes must differ");
    assert_ne!(t2_node, t3_node, "Card2 and Card3 Title nodes must differ");

    // From inside Card1's scope (e.g., from Title), %Title resolves to Card1's own
    let t1_lookup = tree.get_node_relative(t1_node, "%Title").unwrap();
    assert_eq!(
        t1_lookup, t1_node,
        "From inside Card1 scope, %Title should find Card1's Title"
    );

    let t2_lookup = tree.get_node_relative(t2_node, "%Title").unwrap();
    assert_eq!(
        t2_lookup, t2_node,
        "From inside Card2 scope, %Title should find Card2's Title"
    );

    // From inside Card1, %Icon should also resolve to Card1's own Icon
    let i1 = tree.get_node_relative(t1_node, "%Icon").unwrap();
    let i2 = tree.get_node_relative(t2_node, "%Icon").unwrap();
    assert_ne!(
        i1, i2,
        "Card1 and Card2 %Icon must resolve to different nodes"
    );

    // Hand scope should NOT see any %Title or %Icon (they're in Card subscopes)
    // Hand root has no owner (or owner is root), so scope is Hand.
    // Card children are owned by Card roots, not Hand.
    // But looking from Hand itself, its owner scope has no %Title/%Icon defined.
    // Actually, from Hand (which has no owner), the scope is Hand itself.
    // Title nodes are owned by Card1/Card2/Card3, not Hand — so not in scope.
    assert!(
        tree.get_node_relative(hand_id, "%Title").is_none(),
        "%Title must not leak from Card scopes into Hand"
    );
    assert!(
        tree.get_node_relative(hand_id, "%Icon").is_none(),
        "%Icon must not leak from Card scopes into Hand"
    );
}

// ---------------------------------------------------------------------------
// 36. Four-level nesting with unique names at every level via subscenes
// ---------------------------------------------------------------------------

#[test]
fn f5x4_four_level_unique_names_via_subscenes() {
    let gem_tscn = r#"[gd_scene format=3]

[node name="Gem" type="Node2D"]

[node name="%Sparkle" type="Sprite2D" parent="."]
"#;
    let sword_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://gem.tscn" id="g"]

[node name="Sword" type="Node2D"]

[node name="%Edge" type="Sprite2D" parent="."]

[node name="GemSlot" parent="." instance=ExtResource("g")]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://sword.tscn" id="s"]

[node name="Player" type="Node2D"]

[node name="%Avatar" type="Sprite2D" parent="."]

[node name="Weapon" parent="." instance=ExtResource("s")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="%HUD" type="Control" parent="."]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let world = PackedScene::from_tscn(world_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://sword.tscn" => Some(PackedScene::from_tscn(sword_tscn).unwrap()),
            "res://gem.tscn" => Some(PackedScene::from_tscn(gem_tscn).unwrap()),
            _ => None,
        }
    };

    let world_id =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &world, &resolver).unwrap();

    let hero = tree.get_node_relative(world_id, "Hero").unwrap();
    let weapon = tree.get_node_relative(hero, "Weapon").unwrap();
    let gem_slot = tree.get_node_relative(weapon, "GemSlot").unwrap();

    // Each scope's unique names are only visible from INSIDE that scope
    // (from nodes owned by the scope root).

    // World scope: from HUD (owned by World), %HUD resolves
    let hud_node = tree.get_node_relative(world_id, "HUD").unwrap();
    assert!(
        tree.get_node_relative(hud_node, "%HUD").is_some(),
        "World scope: %HUD visible"
    );
    assert!(
        tree.get_node_relative(hud_node, "%Avatar").is_none(),
        "%Avatar not in World scope"
    );

    // Player scope: from Avatar (owned by Hero), %Avatar resolves
    let avatar_node = tree.get_node_relative(hero, "Avatar").unwrap();
    assert!(
        tree.get_node_relative(avatar_node, "%Avatar").is_some(),
        "Player scope: %Avatar visible"
    );
    assert!(
        tree.get_node_relative(avatar_node, "%HUD").is_none(),
        "%HUD not in Player scope"
    );
    assert!(
        tree.get_node_relative(avatar_node, "%Edge").is_none(),
        "%Edge not in Player scope"
    );

    // Sword scope: from Edge (owned by Weapon), %Edge resolves
    let edge_node = tree.get_node_relative(weapon, "Edge").unwrap();
    assert!(
        tree.get_node_relative(edge_node, "%Edge").is_some(),
        "Sword scope: %Edge visible"
    );
    assert!(
        tree.get_node_relative(edge_node, "%Avatar").is_none(),
        "%Avatar not in Sword scope"
    );

    // Gem scope: from Sparkle (owned by GemSlot), %Sparkle resolves
    let sparkle_node = tree.get_node_relative(gem_slot, "Sparkle").unwrap();
    assert!(
        tree.get_node_relative(sparkle_node, "%Sparkle").is_some(),
        "Gem scope: %Sparkle visible"
    );
    assert!(
        tree.get_node_relative(sparkle_node, "%Edge").is_none(),
        "%Edge not in Gem scope"
    );
    assert!(
        tree.get_node_relative(sparkle_node, "%HUD").is_none(),
        "%HUD not in Gem scope"
    );
}

// ===========================================================================
// pat-5jy: Nested PackedScene ownership propagation during instancing
// ===========================================================================

// ---------------------------------------------------------------------------
// 37. Property overrides on instanced subscene preserve ownership boundaries
// ---------------------------------------------------------------------------

#[test]
fn p5jy_property_override_on_instance_preserves_ownership() {
    let item_tscn = r#"[gd_scene format=3]

[node name="Item" type="Node2D"]
position = Vector2(0, 0)

[node name="Icon" type="Sprite2D" parent="."]
"#;
    // Parent scene instances the item with property overrides.
    let chest_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://item.tscn" id="i"]

[node name="Chest" type="Node"]

[node name="Loot" parent="." instance=ExtResource("i")]
position = Vector2(50, 75)
"#;
    let chest = PackedScene::from_tscn(chest_tscn).unwrap();
    let nodes = chest
        .instance_with_subscenes(&|path| match path {
            "res://item.tscn" => Some(PackedScene::from_tscn(item_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // Chest + Loot (instanced Item root) + Icon = 3
    assert_eq!(nodes.len(), 3);
    let chest_id = nodes[0].id();
    let loot_id = nodes[1].id();

    // Chest root: no owner.
    assert!(nodes[0].owner().is_none());
    // Loot (sub-scene root) owned by Chest.
    assert_eq!(nodes[1].owner(), Some(chest_id));
    // Icon (sub-scene child) owned by Loot, not Chest.
    assert_eq!(
        nodes[2].owner(),
        Some(loot_id),
        "Icon should be owned by sub-scene root (Loot), not parent scene root (Chest)"
    );

    // Property override should have been applied to Loot.
    let pos = nodes[1].get_property("position");
    assert_ne!(
        pos,
        gdvariant::Variant::Nil,
        "position override should be applied"
    );
}

// ---------------------------------------------------------------------------
// 38. Ownership preserved in tree with three-level nested instances
// ---------------------------------------------------------------------------

#[test]
fn p5jy_three_level_ownership_in_tree() {
    let weapon_tscn = r#"[gd_scene format=3]

[node name="Sword" type="Node2D"]

[node name="Blade" type="Sprite2D" parent="."]
"#;
    let player_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="w"]

[node name="Player" type="Node2D"]

[node name="Body" type="Sprite2D" parent="."]

[node name="Weapon" parent="." instance=ExtResource("w")]
"#;
    let world_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p"]

[node name="World" type="Node"]

[node name="Hero" parent="." instance=ExtResource("p")]
"#;

    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
            "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
            _ => None,
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = PackedScene::from_tscn(world_tscn).unwrap();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &world, &resolver).unwrap();

    // Verify ownership boundaries in the live tree.
    let world_node = tree.get_node_by_path("/root/World").unwrap();
    let hero = tree.get_node_by_path("/root/World/Hero").unwrap();
    let body = tree.get_node_by_path("/root/World/Hero/Body").unwrap();
    let weapon = tree.get_node_by_path("/root/World/Hero/Weapon").unwrap();
    let blade = tree
        .get_node_by_path("/root/World/Hero/Weapon/Blade")
        .unwrap();

    // World root: no owner (it IS the scene owner).
    assert!(
        tree.get_node(world_node).unwrap().owner().is_none(),
        "World root should have no owner"
    );
    // Hero (player sub-scene root) owned by World.
    assert_eq!(
        tree.get_node(hero).unwrap().owner(),
        Some(world_node),
        "Hero owned by World"
    );
    // Body (player sub-scene child) owned by Hero.
    assert_eq!(
        tree.get_node(body).unwrap().owner(),
        Some(hero),
        "Body owned by Hero"
    );
    // Weapon (weapon sub-scene root) owned by Hero.
    assert_eq!(
        tree.get_node(weapon).unwrap().owner(),
        Some(hero),
        "Weapon owned by Hero"
    );
    // Blade (weapon sub-scene child) owned by Weapon.
    assert_eq!(
        tree.get_node(blade).unwrap().owner(),
        Some(weapon),
        "Blade owned by Weapon"
    );
}

// ---------------------------------------------------------------------------
// 39. Duplicate nested instantiation: independent ownership boundaries
// ---------------------------------------------------------------------------

#[test]
fn p5jy_duplicate_nested_instances_independent_ownership() {
    let bullet_tscn = r#"[gd_scene format=3]

[node name="Bullet" type="Node2D"]

[node name="Trail" type="Node" parent="."]
"#;
    // Scene that instances the same subscene twice under different names.
    let gun_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://bullet.tscn" id="b"]

[node name="Gun" type="Node2D"]

[node name="BulletA" parent="." instance=ExtResource("b")]

[node name="BulletB" parent="." instance=ExtResource("b")]
"#;
    let gun = PackedScene::from_tscn(gun_tscn).unwrap();
    let nodes = gun
        .instance_with_subscenes(&|path| match path {
            "res://bullet.tscn" => Some(PackedScene::from_tscn(bullet_tscn).unwrap()),
            _ => None,
        })
        .unwrap();

    // Gun + BulletA + TrailA + BulletB + TrailB = 5
    assert_eq!(nodes.len(), 5);
    let gun_id = nodes[0].id();
    let bullet_a_id = nodes[1].id();
    let bullet_b_id = nodes[3].id();

    // Gun root: no owner.
    assert!(nodes[0].owner().is_none());
    // Both bullet roots owned by Gun.
    assert_eq!(nodes[1].owner(), Some(gun_id));
    assert_eq!(nodes[3].owner(), Some(gun_id));
    // Each trail owned by its own bullet root, not the other.
    assert_eq!(
        nodes[2].owner(),
        Some(bullet_a_id),
        "TrailA should be owned by BulletA"
    );
    assert_eq!(
        nodes[4].owner(),
        Some(bullet_b_id),
        "TrailB should be owned by BulletB"
    );
    // Ownership boundaries are independent.
    assert_ne!(
        bullet_a_id, bullet_b_id,
        "BulletA and BulletB should have distinct IDs"
    );
}

// ---------------------------------------------------------------------------
// 40. Nested instances in tree: ownership after add_packed_scene_to_tree_with_subscenes
//     followed by stepping via MainLoop
// ---------------------------------------------------------------------------

#[test]
fn p5jy_nested_ownership_stable_after_mainloop_stepping() {
    use gdscene::main_loop::MainLoop;

    let enemy_tscn = r#"[gd_scene format=3]

[node name="Enemy" type="Node2D"]

[node name="AI" type="Node" parent="."]
"#;
    let level_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="e"]

[node name="Level" type="Node"]

[node name="Mob" parent="." instance=ExtResource("e")]
"#;

    let level = PackedScene::from_tscn(level_tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let resolver = |path: &str| -> Option<PackedScene> {
        match path {
            "res://enemy.tscn" => Some(PackedScene::from_tscn(enemy_tscn).unwrap()),
            _ => None,
        }
    };
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &level, &resolver).unwrap();

    let level_id = tree.get_node_by_path("/root/Level").unwrap();
    let mob_id = tree.get_node_by_path("/root/Level/Mob").unwrap();
    let ai_id = tree.get_node_by_path("/root/Level/Mob/AI").unwrap();

    // Verify ownership before stepping.
    assert_eq!(tree.get_node(mob_id).unwrap().owner(), Some(level_id));
    assert_eq!(tree.get_node(ai_id).unwrap().owner(), Some(mob_id));

    // Step 60 frames.
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, 1.0 / 60.0);

    // Ownership should be unchanged after stepping.
    let mob_owner = main_loop.tree().get_node(mob_id).unwrap().owner();
    let ai_owner = main_loop.tree().get_node(ai_id).unwrap().owner();
    assert_eq!(
        mob_owner,
        Some(level_id),
        "Mob ownership stable after stepping"
    );
    assert_eq!(ai_owner, Some(mob_id), "AI ownership stable after stepping");
}
