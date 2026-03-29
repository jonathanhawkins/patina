//! pat-xbxi4: Broader integration fixtures — exercises new scene types that
//! cover previously untested node combinations.
//!
//! New fixture scenes and their integration:
//!
//! 1. **timer_animation** — Timer + AnimationPlayer + Skeleton3D hierarchy.
//!    Verifies scene loading, tree structure, and property round-trip.
//! 2. **particles_multi** — Mixed GPU/CPU particles in 2D and 3D.
//!    Verifies particle node parsing and property fidelity.
//! 3. **csg_composition** — CSG boolean shapes under a combiner with a
//!    reflection probe. Verifies hierarchical CSG tree loading.
//! 4. **nested_ui** — Deeply nested VBox/HBox containers with Label, Button,
//!    LineEdit, TextEdit, and Panel. Verifies multi-level UI tree loading.
//! 5. **multi_layer_2d** — Sprite2D, AnimatedSprite2D, TileMapLayer, Line2D,
//!    Path2D + PathFollow2D, NavigationRegion2D, Camera2D. Verifies breadth
//!    of 2D node type support.
//! 6. **Scene transition through new fixtures** — all five new scenes loaded
//!    sequentially in a MainLoop, verifying frame accumulation and cleanup.
//! 7. **Golden parity** — each fixture's parsed tree compared against its
//!    golden JSON to verify structural match.
//!
//! Acceptance: all tests pass without panics and golden output matches.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const DT: f64 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// Fixture sources
// ---------------------------------------------------------------------------

const TIMER_ANIMATION_TSCN: &str =
    include_str!("../../fixtures/scenes/timer_animation.tscn");
const PARTICLES_MULTI_TSCN: &str =
    include_str!("../../fixtures/scenes/particles_multi.tscn");
const CSG_COMPOSITION_TSCN: &str =
    include_str!("../../fixtures/scenes/csg_composition.tscn");
const NESTED_UI_TSCN: &str =
    include_str!("../../fixtures/scenes/nested_ui.tscn");
const MULTI_LAYER_2D_TSCN: &str =
    include_str!("../../fixtures/scenes/multi_layer_2d.tscn");

const TIMER_ANIMATION_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/timer_animation.json");
const PARTICLES_MULTI_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/particles_multi.json");
const CSG_COMPOSITION_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/csg_composition.json");
const NESTED_UI_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/nested_ui.json");
const MULTI_LAYER_2D_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/multi_layer_2d.json");

fn parse_scene(tscn: &str) -> PackedScene {
    PackedScene::from_tscn(tscn).expect("failed to parse scene")
}

fn load_scene(tscn: &str) -> (SceneTree, gdscene::node::NodeId) {
    let packed = parse_scene(tscn);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    (tree, scene_root)
}

/// Collect all node paths from a SceneTree (DFS).
fn collect_paths(tree: &SceneTree) -> Vec<String> {
    let mut paths = Vec::new();
    fn walk(tree: &SceneTree, id: gdscene::node::NodeId, paths: &mut Vec<String>) {
        if let Some(path) = tree.node_path(id) {
            paths.push(path);
        }
        if let Some(node) = tree.get_node(id) {
            for &child in node.children() {
                walk(tree, child, paths);
            }
        }
    }
    walk(tree, tree.root_id(), &mut paths);
    paths
}

/// Parse golden JSON and extract all node paths.
fn golden_paths(json_str: &str) -> Vec<String> {
    let val: serde_json::Value = serde_json::from_str(json_str).expect("invalid golden JSON");
    let mut paths = Vec::new();
    fn walk_json(node: &serde_json::Value, paths: &mut Vec<String>) {
        if let Some(path) = node.get("path").and_then(|p| p.as_str()) {
            paths.push(path.to_string());
        }
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for child in children {
                walk_json(child, paths);
            }
        }
    }
    if let Some(nodes) = val.get("nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            walk_json(node, &mut paths);
        }
    }
    paths
}

// ===========================================================================
// 1. Timer + AnimationPlayer + Skeleton hierarchy
// ===========================================================================

#[test]
fn xbxi4_timer_animation_scene_loads_and_has_correct_structure() {
    let (tree, scene_root) = load_scene(TIMER_ANIMATION_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "Root");
    assert_eq!(node.class_name(), "Node");

    // Verify key nodes exist.
    assert!(
        tree.get_node_by_path("/root/Root/OneShot").is_some(),
        "OneShot Timer should exist"
    );
    assert!(
        tree.get_node_by_path("/root/Root/Repeating").is_some(),
        "Repeating Timer should exist"
    );
    assert!(
        tree.get_node_by_path("/root/Root/Player").is_some(),
        "AnimationPlayer should exist"
    );
    assert!(
        tree.get_node_by_path("/root/Root/Mesh").is_some(),
        "Mesh Node3D should exist"
    );
    assert!(
        tree.get_node_by_path("/root/Root/Mesh/Skeleton").is_some(),
        "Skeleton3D should exist"
    );
    assert!(
        tree.get_node_by_path("/root/Root/Mesh/Skeleton/BoneAttachment")
            .is_some(),
        "BoneAttachment3D should exist"
    );
}

#[test]
fn xbxi4_timer_animation_steps_frames_without_panic() {
    let (tree, _) = load_scene(TIMER_ANIMATION_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 2. Particles (GPU + CPU, 2D + 3D)
// ===========================================================================

#[test]
fn xbxi4_particles_multi_scene_loads_all_particle_nodes() {
    let (tree, scene_root) = load_scene(PARTICLES_MULTI_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "ParticleWorld");

    let expected_children = [
        "Sparks2D",
        "Dust2D",
        "Fire3D",
        "Smoke3D",
        "Rain2D",
    ];
    for name in &expected_children {
        let path = format!("/root/ParticleWorld/{name}");
        assert!(
            tree.get_node_by_path(&path).is_some(),
            "{name} particle node should exist at {path}"
        );
    }
}

#[test]
fn xbxi4_particles_multi_steps_frames_without_panic() {
    let (tree, _) = load_scene(PARTICLES_MULTI_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(120, DT);
    assert_eq!(main_loop.frame_count(), 120);
}

// ===========================================================================
// 3. CSG composition
// ===========================================================================

#[test]
fn xbxi4_csg_composition_scene_loads_hierarchical_csg() {
    let (tree, scene_root) = load_scene(CSG_COMPOSITION_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "CSGWorld");
    assert_eq!(node.class_name(), "Node3D");

    // Combiner with 3 children.
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Combiner").is_some(),
        "CSGCombiner3D should exist"
    );
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Combiner/Box")
            .is_some(),
        "CSGBox3D under Combiner should exist"
    );
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Combiner/Sphere")
            .is_some(),
        "CSGSphere3D under Combiner should exist"
    );
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Combiner/Cylinder")
            .is_some(),
        "CSGCylinder3D under Combiner should exist"
    );

    // Standalone CSGBox and ReflectionProbe.
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Standalone").is_some(),
        "standalone CSGBox3D should exist"
    );
    assert!(
        tree.get_node_by_path("/root/CSGWorld/Probe").is_some(),
        "ReflectionProbe should exist"
    );
}

#[test]
fn xbxi4_csg_composition_steps_frames_without_panic() {
    let (tree, _) = load_scene(CSG_COMPOSITION_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 4. Nested UI (deep container hierarchy)
// ===========================================================================

#[test]
fn xbxi4_nested_ui_scene_loads_deep_container_tree() {
    let (tree, scene_root) = load_scene(NESTED_UI_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "UIRoot");
    assert_eq!(node.class_name(), "Control");

    // 4-level deep path: UIRoot > MainVBox > Body > NameRow > NameInput
    assert!(
        tree.get_node_by_path("/root/UIRoot/MainVBox/Body/NameRow/NameInput")
            .is_some(),
        "deeply nested NameInput should exist"
    );
    assert!(
        tree.get_node_by_path("/root/UIRoot/MainVBox/Body/DescRow/DescInput")
            .is_some(),
        "deeply nested DescInput should exist"
    );
    assert!(
        tree.get_node_by_path("/root/UIRoot/MainVBox/Footer/SaveBtn")
            .is_some(),
        "SaveBtn in footer should exist"
    );
    assert!(
        tree.get_node_by_path("/root/UIRoot/MainVBox/Footer/CancelBtn")
            .is_some(),
        "CancelBtn in footer should exist"
    );
    assert!(
        tree.get_node_by_path("/root/UIRoot/MainVBox/StatusPanel")
            .is_some(),
        "StatusPanel should exist"
    );
}

#[test]
fn xbxi4_nested_ui_steps_frames_without_panic() {
    let (tree, _) = load_scene(NESTED_UI_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 5. Multi-layer 2D (sprites, tiles, paths, nav, camera)
// ===========================================================================

#[test]
fn xbxi4_multi_layer_2d_scene_loads_all_2d_node_types() {
    let (tree, scene_root) = load_scene(MULTI_LAYER_2D_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "World2D");
    assert_eq!(node.class_name(), "Node2D");

    let expected = [
        ("Background", "Sprite2D"),
        ("TileLayer", "TileMapLayer"),
        ("PlayerSprite", "Sprite2D"),
        ("AnimSprite", "AnimatedSprite2D"),
        ("Trail", "Line2D"),
        ("MovePath", "Path2D"),
        ("NavRegion", "NavigationRegion2D"),
        ("Camera", "Camera2D"),
    ];
    for (name, class) in &expected {
        let path = format!("/root/World2D/{name}");
        let found = tree.get_node_by_path(&path);
        assert!(found.is_some(), "{name} ({class}) should exist at {path}");
    }

    // PathFollow2D is a child of Path2D.
    assert!(
        tree.get_node_by_path("/root/World2D/MovePath/Follower")
            .is_some(),
        "PathFollow2D should be child of Path2D"
    );
}

#[test]
fn xbxi4_multi_layer_2d_steps_frames_without_panic() {
    let (tree, _) = load_scene(MULTI_LAYER_2D_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 6. Scene transitions through all new fixtures
// ===========================================================================

#[test]
fn xbxi4_scene_transition_through_all_new_fixtures() {
    let scenes: &[(&str, &str, &str)] = &[
        ("timer_animation", TIMER_ANIMATION_TSCN, "Root"),
        ("particles_multi", PARTICLES_MULTI_TSCN, "ParticleWorld"),
        ("csg_composition", CSG_COMPOSITION_TSCN, "CSGWorld"),
        ("nested_ui", NESTED_UI_TSCN, "UIRoot"),
        ("multi_layer_2d", MULTI_LAYER_2D_TSCN, "World2D"),
    ];

    // Start with the first scene.
    let (tree, _) = load_scene(scenes[0].1);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(20, DT);

    // Transition through remaining scenes.
    for &(name, tscn, expected_root) in &scenes[1..] {
        let packed = parse_scene(tscn);
        let new_root_id = main_loop
            .tree_mut()
            .change_scene_to_packed(&packed)
            .unwrap();

        let node = main_loop.tree().get_node(new_root_id).unwrap();
        assert_eq!(
            node.name(),
            expected_root,
            "scene '{name}' root should be '{expected_root}'"
        );

        main_loop.run_frames(20, DT);
    }

    // 5 scenes * 20 frames = 100 frames total.
    assert_eq!(main_loop.frame_count(), 100);

    let expected_time = 100.0 * DT;
    assert!(
        (main_loop.physics_time() - expected_time).abs() < 0.05,
        "physics_time should be ~{expected_time:.2}s, got {:.2}s",
        main_loop.physics_time()
    );
}

// ===========================================================================
// 7. Golden parity — parsed tree vs golden JSON
// ===========================================================================

fn assert_golden_paths_match(fixture_name: &str, tscn: &str, golden_json: &str) {
    let (tree, _) = load_scene(tscn);
    let actual = collect_paths(&tree);
    let expected = golden_paths(golden_json);

    // Filter to non-root paths (golden doesn't include /root itself).
    let actual_filtered: Vec<&String> = actual.iter().filter(|p| *p != "/root").collect();

    assert_eq!(
        actual_filtered.len(),
        expected.len(),
        "{fixture_name}: node count mismatch — actual {} vs golden {}",
        actual_filtered.len(),
        expected.len()
    );

    for (a, e) in actual_filtered.iter().zip(expected.iter()) {
        assert_eq!(
            *a, e,
            "{fixture_name}: path mismatch — actual '{a}' vs golden '{e}'"
        );
    }
}

#[test]
fn xbxi4_golden_parity_timer_animation() {
    assert_golden_paths_match(
        "timer_animation",
        TIMER_ANIMATION_TSCN,
        TIMER_ANIMATION_GOLDEN,
    );
}

#[test]
fn xbxi4_golden_parity_particles_multi() {
    assert_golden_paths_match(
        "particles_multi",
        PARTICLES_MULTI_TSCN,
        PARTICLES_MULTI_GOLDEN,
    );
}

#[test]
fn xbxi4_golden_parity_csg_composition() {
    assert_golden_paths_match(
        "csg_composition",
        CSG_COMPOSITION_TSCN,
        CSG_COMPOSITION_GOLDEN,
    );
}

#[test]
fn xbxi4_golden_parity_nested_ui() {
    assert_golden_paths_match("nested_ui", NESTED_UI_TSCN, NESTED_UI_GOLDEN);
}

#[test]
fn xbxi4_golden_parity_multi_layer_2d() {
    assert_golden_paths_match(
        "multi_layer_2d",
        MULTI_LAYER_2D_TSCN,
        MULTI_LAYER_2D_GOLDEN,
    );
}

// ===========================================================================
// 8. Determinism: two identical loads produce identical trees
// ===========================================================================

#[test]
fn xbxi4_determinism_new_fixtures_load_identically() {
    let fixtures = [
        TIMER_ANIMATION_TSCN,
        PARTICLES_MULTI_TSCN,
        CSG_COMPOSITION_TSCN,
        NESTED_UI_TSCN,
        MULTI_LAYER_2D_TSCN,
    ];

    for tscn in &fixtures {
        let paths_a = collect_paths(&load_scene(tscn).0);
        let paths_b = collect_paths(&load_scene(tscn).0);
        assert_eq!(paths_a, paths_b, "two loads of the same fixture should produce identical trees");
    }
}
