//! pat-ahop: %UniqueName lookup across nested instanced scenes.
//!
//! Verifies that unique-name resolution respects owner-scope boundaries when
//! scenes are recursively instanced via `instance_with_subscenes`. Scenarios:
//!
//! 1. Two-level nesting (A instances B): unique names in B don't leak to A's scope.
//! 2. Three-level nesting (A → B → C): each scope is independent.
//! 3. Lookup FROM a sub-scene root searches the parent scene's scope (Godot behavior).
//! 4. Lookup FROM a sub-scene child searches the sub-scene's scope.
//! 5. Multiple instances of the same nested scene have independent unique-name scopes.
//! 6. Unique names on the instanced sub-scene root ARE visible in the parent scope.
//! 7. Path suffix after %UniqueName works across nesting boundaries.

use gdscene::packed_scene::{add_packed_scene_to_tree_with_subscenes, PackedScene};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Fixture scenes
// ===========================================================================

/// Inner scene: has %HealthBar and %ScoreLabel as unique-name nodes.
const INNER_SCENE: &str = r#"[gd_scene format=3 uid="uid://inner_hud"]

[node name="HUD" type="Control"]

[node name="%HealthBar" type="ProgressBar" parent="."]

[node name="Panel" type="Panel" parent="."]

[node name="%ScoreLabel" type="Label" parent="Panel"]
"#;

/// Outer scene: has its own %Banner and instances the inner scene.
const OUTER_SCENE: &str = r#"[gd_scene format=3 uid="uid://outer_ui"]

[ext_resource type="PackedScene" uid="uid://inner_hud" path="res://inner_hud.tscn" id="1_abc"]

[node name="UI" type="Control"]

[node name="%Banner" type="Label" parent="."]

[node name="InnerHUD" parent="." instance=ExtResource("1_abc")]
"#;

/// Leaf scene: a tiny scene with %Gem unique node.
const LEAF_SCENE: &str = r#"[gd_scene format=3 uid="uid://leaf_gem"]

[node name="Pickup" type="Area2D"]

[node name="%Gem" type="Sprite2D" parent="."]
"#;

/// Middle scene: instances the leaf scene and has its own %Marker.
const MIDDLE_SCENE: &str = r#"[gd_scene format=3 uid="uid://middle_level"]

[ext_resource type="PackedScene" uid="uid://leaf_gem" path="res://leaf_gem.tscn" id="1_leaf"]

[node name="Level" type="Node2D"]

[node name="%Marker" type="Node2D" parent="."]

[node name="PickupSpot" parent="." instance=ExtResource("1_leaf")]
"#;

/// Top scene: instances the middle scene and has its own %Title.
const TOP_SCENE: &str = r#"[gd_scene format=3 uid="uid://top_world"]

[ext_resource type="PackedScene" uid="uid://middle_level" path="res://middle_level.tscn" id="1_mid"]

[node name="World" type="Node2D"]

[node name="%Title" type="Label" parent="."]

[node name="LevelArea" parent="." instance=ExtResource("1_mid")]
"#;

/// Outer scene that marks the sub-scene root with % (unique in parent scope).
const OUTER_WITH_UNIQUE_SUBSCENE: &str = r#"[gd_scene format=3 uid="uid://outer_unique_sub"]

[ext_resource type="PackedScene" uid="uid://inner_hud" path="res://inner_hud.tscn" id="1_abc"]

[node name="Root" type="Control"]

[node name="%MyHUD" parent="." instance=ExtResource("1_abc")]
"#;

/// Scene that instances the inner HUD twice.
const OUTER_DOUBLE_INSTANCE: &str = r#"[gd_scene format=3 uid="uid://outer_double"]

[ext_resource type="PackedScene" uid="uid://inner_hud" path="res://inner_hud.tscn" id="1_abc"]

[node name="Root" type="Control"]

[node name="HUD_Left" parent="." instance=ExtResource("1_abc")]

[node name="HUD_Right" parent="." instance=ExtResource("1_abc")]
"#;

/// Helper: resolves scene paths to parsed PackedScenes for two-level nesting.
fn resolve_two_level(path: &str) -> Option<PackedScene> {
    match path {
        "res://inner_hud.tscn" => Some(PackedScene::from_tscn(INNER_SCENE).unwrap()),
        _ => None,
    }
}

/// Helper: resolves scene paths for three-level nesting.
fn resolve_three_level(path: &str) -> Option<PackedScene> {
    match path {
        "res://middle_level.tscn" => Some(PackedScene::from_tscn(MIDDLE_SCENE).unwrap()),
        "res://leaf_gem.tscn" => Some(PackedScene::from_tscn(LEAF_SCENE).unwrap()),
        _ => None,
    }
}

// ===========================================================================
// 1. Two-level nesting: unique names in inner scene don't leak to outer scope
// ===========================================================================

#[test]
fn ahop_nested_unique_names_do_not_leak_to_outer_scope() {
    let outer = PackedScene::from_tscn(OUTER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ui_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // %Banner is in the outer scope — should resolve.
    let banner = tree
        .get_node_relative(ui_root, "%Banner")
        .expect("%Banner should resolve in outer scope");
    assert_eq!(tree.get_node(banner).unwrap().name(), "Banner");

    // %HealthBar belongs to the inner scene's scope — should NOT be found
    // from the outer scope.
    assert!(
        tree.get_node_relative(ui_root, "%HealthBar").is_none(),
        "%HealthBar should not leak from inner scene to outer scope"
    );

    // %ScoreLabel also belongs to the inner scope.
    assert!(
        tree.get_node_relative(ui_root, "%ScoreLabel").is_none(),
        "%ScoreLabel should not leak from inner scene to outer scope"
    );
}

// ===========================================================================
// 2. Two-level nesting: unique names accessible from within inner scope
// ===========================================================================

#[test]
fn ahop_nested_unique_names_accessible_from_inner_scope() {
    let outer = PackedScene::from_tscn(OUTER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ui_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // Navigate to the inner HUD root.
    let inner_hud = tree
        .get_node_relative(ui_root, "InnerHUD")
        .expect("InnerHUD should exist as instanced sub-scene root");

    // From a CHILD of the inner scene (not the root), unique names should resolve
    // within the inner scope.
    let inner_panel = tree
        .get_node_relative(inner_hud, "Panel")
        .expect("Panel should exist in inner scene");

    let hb_from_child = tree
        .get_node_relative(inner_panel, "%HealthBar")
        .expect("%HealthBar should resolve from inner scene child");
    assert_eq!(tree.get_node(hb_from_child).unwrap().name(), "HealthBar");

    let sl_from_child = tree
        .get_node_relative(inner_panel, "%ScoreLabel")
        .expect("%ScoreLabel should resolve from inner scene child");
    assert_eq!(tree.get_node(sl_from_child).unwrap().name(), "ScoreLabel");
}

// ===========================================================================
// 3. Lookup FROM sub-scene root searches parent scope (Godot behavior)
// ===========================================================================

#[test]
fn ahop_lookup_from_subscene_root_searches_parent_scope() {
    let outer = PackedScene::from_tscn(OUTER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ui_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    let inner_hud = tree.get_node_relative(ui_root, "InnerHUD").unwrap();

    // In Godot, calling get_node("%Banner") from a sub-scene root searches
    // the parent scene's scope (since the sub-scene root's owner is the
    // parent scene root). %Banner is in the outer scope.
    let banner_from_inner_root = tree.get_node_relative(inner_hud, "%Banner");
    assert!(
        banner_from_inner_root.is_some(),
        "%Banner should be found from sub-scene root (searches parent scope)"
    );

    // But %HealthBar is NOT in the parent scope — it's in the inner scope.
    // From the inner root (whose owner is the outer root), the search is in
    // the outer scope, so %HealthBar should NOT be found.
    assert!(
        tree.get_node_relative(inner_hud, "%HealthBar").is_none(),
        "%HealthBar should not be found from sub-scene root (wrong scope)"
    );
}

// ===========================================================================
// 4. Three-level nesting: each scope is independent
// ===========================================================================

#[test]
fn ahop_three_level_nesting_scopes_independent() {
    let top = PackedScene::from_tscn(TOP_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &top, &resolve_three_level)
            .unwrap();

    // Top scope has %Title.
    let title = tree
        .get_node_relative(world, "%Title")
        .expect("%Title should resolve in top scope");
    assert_eq!(tree.get_node(title).unwrap().name(), "Title");

    // %Marker belongs to the middle scope — NOT visible from top.
    assert!(
        tree.get_node_relative(world, "%Marker").is_none(),
        "%Marker (middle scope) should not be visible from top scope"
    );

    // %Gem belongs to the leaf scope — NOT visible from top.
    assert!(
        tree.get_node_relative(world, "%Gem").is_none(),
        "%Gem (leaf scope) should not be visible from top scope"
    );

    // Navigate to middle scene and verify its scope.
    let level = tree
        .get_node_relative(world, "LevelArea")
        .expect("LevelArea (middle scene root) should exist");
    let marker_node = tree
        .get_node_relative(level, "Marker")
        .expect("Marker should exist in middle scene");

    // From a child of the middle scene, %Marker should resolve.
    let marker_from_child = tree
        .get_node_relative(marker_node, "%Marker")
        .expect("%Marker should resolve from within middle scope");
    assert_eq!(tree.get_node(marker_from_child).unwrap().name(), "Marker");

    // %Gem belongs to the leaf scope — NOT visible from middle scope child.
    assert!(
        tree.get_node_relative(marker_node, "%Gem").is_none(),
        "%Gem (leaf scope) should not be visible from middle scope"
    );

    // Navigate to the leaf scene and verify its scope.
    let pickup = tree
        .get_node_relative(level, "PickupSpot")
        .expect("PickupSpot (leaf scene root) should exist");
    let gem_child = tree
        .get_node_relative(pickup, "Gem")
        .expect("Gem should exist in leaf scene");

    // From a child of the leaf scene, %Gem should resolve.
    let gem_from_child = tree
        .get_node_relative(gem_child, "%Gem")
        .expect("%Gem should resolve from within leaf scope (queried from Gem itself)");
    assert_eq!(tree.get_node(gem_from_child).unwrap().name(), "Gem");

    // %Marker and %Title should NOT be visible from within the leaf scope.
    assert!(
        tree.get_node_relative(gem_child, "%Marker").is_none(),
        "%Marker should not be visible from leaf scope"
    );
    assert!(
        tree.get_node_relative(gem_child, "%Title").is_none(),
        "%Title should not be visible from leaf scope"
    );
}

// ===========================================================================
// 5. Multiple instances of same nested scene have independent scopes
// ===========================================================================

#[test]
fn ahop_multiple_nested_instances_independent_scopes() {
    let outer = PackedScene::from_tscn(OUTER_DOUBLE_INSTANCE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let outer_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // Get both instanced sub-scene roots.
    let hud_left = tree.get_node_relative(outer_root, "HUD_Left").unwrap();
    let hud_right = tree.get_node_relative(outer_root, "HUD_Right").unwrap();
    assert_ne!(hud_left, hud_right);

    // Get a child inside each for inner-scope lookups.
    let left_panel = tree.get_node_relative(hud_left, "Panel").unwrap();
    let right_panel = tree.get_node_relative(hud_right, "Panel").unwrap();

    // Each instance's child resolves %HealthBar to its own copy.
    let hb_left = tree
        .get_node_relative(left_panel, "%HealthBar")
        .expect("%HealthBar should resolve in left instance");
    let hb_right = tree
        .get_node_relative(right_panel, "%HealthBar")
        .expect("%HealthBar should resolve in right instance");

    assert_ne!(
        hb_left, hb_right,
        "two nested instances should have different %HealthBar nodes"
    );
    assert_eq!(tree.get_node(hb_left).unwrap().name(), "HealthBar");
    assert_eq!(tree.get_node(hb_right).unwrap().name(), "HealthBar");

    // Each has independent ownership.
    assert_eq!(tree.get_node(hb_left).unwrap().owner(), Some(hud_left));
    assert_eq!(tree.get_node(hb_right).unwrap().owner(), Some(hud_right));

    // Neither leaks to the outer scope.
    assert!(tree.get_node_relative(outer_root, "%HealthBar").is_none());
}

// ===========================================================================
// 6. Unique-name on sub-scene root IS visible in parent scope
// ===========================================================================

#[test]
fn ahop_unique_subscene_root_visible_in_parent_scope() {
    let outer = PackedScene::from_tscn(OUTER_WITH_UNIQUE_SUBSCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let outer_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // %MyHUD is the sub-scene root marked as unique in the outer scene's template.
    // It should be visible in the outer scope.
    let my_hud = tree
        .get_node_relative(outer_root, "%MyHUD")
        .expect("%MyHUD (unique sub-scene root) should be visible in parent scope");
    assert_eq!(tree.get_node(my_hud).unwrap().name(), "MyHUD");
    assert!(tree.get_node(my_hud).unwrap().is_unique_name());

    // But the inner scene's unique names should NOT leak to the outer scope.
    assert!(
        tree.get_node_relative(outer_root, "%HealthBar").is_none(),
        "inner %HealthBar should not leak to outer scope even though sub-root is unique"
    );
}

// ===========================================================================
// 7. Path suffix after %UniqueName works across nesting
// ===========================================================================

#[test]
fn ahop_path_suffix_after_unique_name_across_nesting() {
    let outer = PackedScene::from_tscn(OUTER_WITH_UNIQUE_SUBSCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let outer_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // %MyHUD/Panel should resolve: first find %MyHUD (the sub-scene root),
    // then walk to its child "Panel".
    let panel_via_unique = tree
        .get_node_relative(outer_root, "%MyHUD/Panel")
        .expect("%MyHUD/Panel should resolve via unique name + path suffix");
    assert_eq!(tree.get_node(panel_via_unique).unwrap().name(), "Panel");

    // %MyHUD/Panel/ScoreLabel should also work (deeper path suffix).
    let score_via_unique = tree
        .get_node_relative(outer_root, "%MyHUD/Panel/ScoreLabel")
        .expect("%MyHUD/Panel/ScoreLabel should resolve via unique name + deep path suffix");
    assert_eq!(
        tree.get_node(score_via_unique).unwrap().name(),
        "ScoreLabel"
    );
}

// ===========================================================================
// 8. Owner chain correctness through nested instancing
// ===========================================================================

#[test]
fn ahop_owner_chain_correct_through_nested_instancing() {
    let outer = PackedScene::from_tscn(OUTER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ui_root =
        add_packed_scene_to_tree_with_subscenes(&mut tree, root, &outer, &resolve_two_level)
            .unwrap();

    // Outer root has no owner (it IS the owner scope).
    assert_eq!(
        tree.get_node(ui_root).unwrap().owner(),
        None,
        "outer scene root should have owner == None"
    );

    // %Banner is owned by the outer root.
    let banner = tree.get_node_relative(ui_root, "%Banner").unwrap();
    assert_eq!(
        tree.get_node(banner).unwrap().owner(),
        Some(ui_root),
        "Banner should be owned by outer root"
    );

    // InnerHUD root is owned by the outer root (it's embedded in the outer scene).
    let inner_hud = tree.get_node_relative(ui_root, "InnerHUD").unwrap();
    assert_eq!(
        tree.get_node(inner_hud).unwrap().owner(),
        Some(ui_root),
        "instanced sub-scene root should be owned by outer root"
    );

    // InnerHUD's children are owned by InnerHUD (not the outer root).
    let inner_panel = tree.get_node_relative(inner_hud, "Panel").unwrap();
    assert_eq!(
        tree.get_node(inner_panel).unwrap().owner(),
        Some(inner_hud),
        "inner scene's child should be owned by inner scene root"
    );

    let inner_hb = tree.get_node_relative(inner_hud, "HealthBar").unwrap();
    assert_eq!(
        tree.get_node(inner_hb).unwrap().owner(),
        Some(inner_hud),
        "inner scene's unique-name child should be owned by inner scene root"
    );

    // Deeply nested: ScoreLabel is under Panel, still owned by InnerHUD.
    let inner_sl = tree
        .get_node_relative(inner_hud, "Panel/ScoreLabel")
        .unwrap();
    assert_eq!(
        tree.get_node(inner_sl).unwrap().owner(),
        Some(inner_hud),
        "deeply nested inner child should still be owned by inner scene root"
    );
}
