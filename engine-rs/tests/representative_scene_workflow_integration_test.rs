//! pat-pm64: Broader integration workflows for representative scene and project runs.
//!
//! Exercises representative scene archetypes (platformer, UI menu, physics
//! playground, multi-scene composition, signal-heavy scenes) through the full
//! engine pipeline: parse → tree build → property init → MainLoop step →
//! input injection → frame evolution. Proves the engine handles diverse
//! real-world scene structures beyond the minimal demo_2d fixture.
//!
//! Acceptance: representative scenes load, step, and produce correct
//! tree structure and frame evolution without panicking.

use gdcore::math::Vector2;
use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdscene::main_loop::MainLoop;
use gdscene::node2d::{get_position, set_position};
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{PackedScene, SceneTree};

const DT: f64 = 1.0 / 60.0;

// ===========================================================================
// Fixture loaders
// ===========================================================================

fn load_scene(tscn: &str) -> (SceneTree, gdscene::node::NodeId) {
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    (tree, scene_root)
}

// ===========================================================================
// 1. Platformer scene: parse + tree structure + step
// ===========================================================================

const PLATFORMER_TSCN: &str = include_str!("../../fixtures/scenes/platformer.tscn");

#[test]
fn pm64_platformer_loads_correct_structure() {
    let (tree, scene_root) = load_scene(PLATFORMER_TSCN);

    // Verify all expected nodes exist.
    assert_eq!(tree.get_node(scene_root).unwrap().name(), "World");
    assert!(tree.get_node_by_path("/root/World/Player").is_some());
    assert!(tree.get_node_by_path("/root/World/Platform1").is_some());
    assert!(tree.get_node_by_path("/root/World/Platform2").is_some());
    assert!(tree.get_node_by_path("/root/World/Platform3").is_some());
    assert!(tree.get_node_by_path("/root/World/Camera").is_some());
    assert!(tree.get_node_by_path("/root/World/Collectible").is_some());

    // Verify child count (6 children of World).
    let world_children = tree.get_node(scene_root).unwrap().children();
    assert_eq!(world_children.len(), 6, "World should have 6 children");
}

#[test]
fn pm64_platformer_initial_positions() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);

    let player = tree.get_node_by_path("/root/World/Player").unwrap();
    let pos = get_position(&tree, player);
    assert_eq!(pos, Vector2::new(100.0, 300.0), "Player starts at (100, 300)");

    let collectible = tree.get_node_by_path("/root/World/Collectible").unwrap();
    let cpos = get_position(&tree, collectible);
    assert_eq!(cpos, Vector2::new(450.0, 250.0));
}

#[test]
fn pm64_platformer_runs_60_frames() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);

    assert_eq!(main_loop.frame_count(), 60);
    assert!((main_loop.physics_time() - 1.0).abs() < 0.02);
}

#[test]
fn pm64_platformer_headless_backend_60_frames() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(60);

    main_loop.run(&mut backend, DT);

    assert_eq!(main_loop.frame_count(), 60);
    assert_eq!(backend.frames_run(), 60);
    assert!(backend.should_quit());
}

// ===========================================================================
// 2. UI menu scene: signal connections + tree structure
// ===========================================================================

const UI_MENU_TSCN: &str = include_str!("../../fixtures/scenes/ui_menu.tscn");

#[test]
fn pm64_ui_menu_loads_correct_structure() {
    let (tree, scene_root) = load_scene(UI_MENU_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "MenuRoot");
    assert!(tree.get_node_by_path("/root/MenuRoot/Title").is_some());
    assert!(tree.get_node_by_path("/root/MenuRoot/PlayButton").is_some());
    assert!(tree.get_node_by_path("/root/MenuRoot/SettingsButton").is_some());
    assert!(tree.get_node_by_path("/root/MenuRoot/QuitButton").is_some());

    let children = tree.get_node(scene_root).unwrap().children();
    assert_eq!(children.len(), 4, "MenuRoot should have 4 children");
}

#[test]
fn pm64_ui_menu_runs_without_panic() {
    let (tree, _) = load_scene(UI_MENU_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // UI scenes should run smoothly even with no input.
    main_loop.run_frames(120, DT);
    assert_eq!(main_loop.frame_count(), 120);
}

// ===========================================================================
// 3. Physics playground: multi-type node tree
// ===========================================================================

const PHYSICS_TSCN: &str = include_str!("../../fixtures/scenes/physics_playground.tscn");

#[test]
fn pm64_physics_playground_loads_nested_structure() {
    let (tree, scene_root) = load_scene(PHYSICS_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "World");

    // Ball is a RigidBody2D with a CollisionShape2D child.
    let ball = tree.get_node_by_path("/root/World/Ball").unwrap();
    assert!(tree.get_node_by_path("/root/World/Ball/CollisionShape").is_some());

    // Wall and Floor are StaticBody2D with children.
    assert!(tree.get_node_by_path("/root/World/Wall").is_some());
    assert!(tree.get_node_by_path("/root/World/Wall/CollisionShape").is_some());
    assert!(tree.get_node_by_path("/root/World/Floor").is_some());
    assert!(tree.get_node_by_path("/root/World/Floor/CollisionShape").is_some());

    // Verify Ball initial position.
    let ball_pos = get_position(&tree, ball);
    assert_eq!(ball_pos, Vector2::new(400.0, 100.0));
}

#[test]
fn pm64_physics_playground_runs_60_frames() {
    let (tree, _) = load_scene(PHYSICS_TSCN);
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 4. Complex signals scene: connections parsed + frame stepping
// ===========================================================================

const SIGNALS_COMPLEX_TSCN: &str =
    include_str!("../../fixtures/scenes/signals_complex.tscn");

#[test]
fn pm64_signals_complex_loads_all_nodes() {
    let (tree, scene_root) = load_scene(SIGNALS_COMPLEX_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "Root");
    assert!(tree.get_node_by_path("/root/Root/Player").is_some());
    assert!(tree.get_node_by_path("/root/Root/Enemy").is_some());
    assert!(tree.get_node_by_path("/root/Root/HUD").is_some());
    assert!(tree.get_node_by_path("/root/Root/ItemDrop").is_some());

    // Nested child under Player.
    assert!(
        tree.get_node_by_path("/root/Root/Player/TriggerZone").is_some(),
        "TriggerZone should be a child of Player"
    );
}

#[test]
fn pm64_signals_complex_initial_positions() {
    let (tree, _) = load_scene(SIGNALS_COMPLEX_TSCN);

    let player = tree.get_node_by_path("/root/Root/Player").unwrap();
    assert_eq!(get_position(&tree, player), Vector2::new(200.0, 300.0));

    let enemy = tree.get_node_by_path("/root/Root/Enemy").unwrap();
    assert_eq!(get_position(&tree, enemy), Vector2::new(600.0, 300.0));

    let item = tree.get_node_by_path("/root/Root/ItemDrop").unwrap();
    assert_eq!(get_position(&tree, item), Vector2::new(400.0, 200.0));
}

#[test]
fn pm64_signals_complex_runs_without_panic() {
    let (tree, _) = load_scene(SIGNALS_COMPLEX_TSCN);
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 5. Properties scene: custom property init from tscn
// ===========================================================================

const PROPERTIES_TSCN: &str = include_str!("../../fixtures/scenes/with_properties.tscn");

#[test]
fn pm64_properties_scene_loads_custom_values() {
    let (tree, scene_root) = load_scene(PROPERTIES_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "Root");

    let player = tree.get_node_by_path("/root/Root/Player").unwrap();
    let pos = get_position(&tree, player);
    assert_eq!(pos, Vector2::new(100.0, 200.0));

    // Custom properties should be stored.
    let node = tree.get_node(player).unwrap();
    let speed = node.get_property("speed");
    assert_ne!(speed, gdvariant::Variant::Nil, "speed property should be set");
}

// ===========================================================================
// 6. Hierarchy scene: minimal nested structure
// ===========================================================================

const HIERARCHY_TSCN: &str = include_str!("../../fixtures/scenes/hierarchy.tscn");

#[test]
fn pm64_hierarchy_scene_nested_paths() {
    let (tree, scene_root) = load_scene(HIERARCHY_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "Root");

    let player = tree.get_node_by_path("/root/Root/Player").unwrap();
    assert!(tree.get_node_by_path("/root/Root/Player/Sprite").is_some());

    // Player should have exactly 1 child.
    let children = tree.get_node(player).unwrap().children();
    assert_eq!(children.len(), 1);
}

// ===========================================================================
// 7. Multi-scene composition: two scenes instanced under one tree
// ===========================================================================

#[test]
fn pm64_multi_scene_composition() {
    let platformer = PackedScene::from_tscn(PLATFORMER_TSCN).unwrap();
    let hierarchy = PackedScene::from_tscn(HIERARCHY_TSCN).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let plat_root = add_packed_scene_to_tree(&mut tree, root, &platformer).unwrap();
    let hier_root = add_packed_scene_to_tree(&mut tree, root, &hierarchy).unwrap();

    // Both scene roots should be children of the tree root.
    let root_children = tree.get_node(root).unwrap().children();
    assert!(root_children.contains(&plat_root));
    assert!(root_children.contains(&hier_root));

    // Nodes from both scenes should be reachable.
    assert!(tree.get_node_by_path("/root/World/Player").is_some());
    assert!(tree.get_node_by_path("/root/Root/Player/Sprite").is_some());

    // Run the composed tree.
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 8. Input injection through representative scene workflow
// ===========================================================================

#[test]
fn pm64_platformer_with_input_injection() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Set up an input map.
    let mut map = InputMap::new();
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    main_loop.set_input_map(map);

    // Inject key press.
    main_loop.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    // Step a few frames.
    for _ in 0..10 {
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), 10);

    // Inject jump.
    main_loop.push_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    for _ in 0..10 {
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), 20);
}

// ===========================================================================
// 9. Full project run: all fixtures load without error
// ===========================================================================

const MINIMAL_TSCN: &str = include_str!("../../fixtures/scenes/minimal.tscn");
const SPACE_SHOOTER_TSCN: &str = include_str!("../../fixtures/scenes/space_shooter.tscn");

#[test]
fn pm64_all_fixture_scenes_parse_successfully() {
    let scenes: &[(&str, &str)] = &[
        ("minimal", MINIMAL_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("with_properties", PROPERTIES_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("physics_playground", PHYSICS_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
    ];

    for (name, tscn) in scenes {
        let packed = PackedScene::from_tscn(tscn);
        assert!(
            packed.is_ok(),
            "scene '{name}' failed to parse: {:?}",
            packed.err()
        );
    }
}

#[test]
fn pm64_all_fixture_scenes_instantiate_and_step() {
    let scenes: &[(&str, &str)] = &[
        ("minimal", MINIMAL_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("with_properties", PROPERTIES_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("physics_playground", PHYSICS_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
    ];

    for (name, tscn) in scenes {
        let (tree, scene_root) = load_scene(tscn);
        let node = tree.get_node(scene_root).unwrap();
        assert!(
            !node.name().is_empty(),
            "scene '{name}' root should have a name"
        );

        let mut main_loop = MainLoop::new(tree);
        main_loop.run_frames(10, DT);
        assert_eq!(
            main_loop.frame_count(),
            10,
            "scene '{name}' should run 10 frames"
        );
    }
}

// ===========================================================================
// 10. Step vs run_frame determinism across representative scenes
// ===========================================================================

#[test]
fn pm64_step_vs_run_frame_deterministic_platformer() {
    let frames = 30u64;

    // Path A: step()
    let (tree_a, _) = load_scene(PLATFORMER_TSCN);
    let mut ml_a = MainLoop::new(tree_a);
    for _ in 0..frames {
        ml_a.step(DT);
    }

    // Path B: run_frame()
    let (tree_b, _) = load_scene(PLATFORMER_TSCN);
    let mut ml_b = MainLoop::new(tree_b);
    let mut backend = HeadlessPlatform::new(640, 480);
    for _ in 0..frames {
        ml_b.run_frame(&mut backend, DT);
    }

    assert_eq!(ml_a.frame_count(), ml_b.frame_count());
    assert_eq!(ml_a.physics_time(), ml_b.physics_time());

    // Positions should match.
    let player_a = ml_a.tree().get_node_by_path("/root/World/Player").unwrap();
    let player_b = ml_b.tree().get_node_by_path("/root/World/Player").unwrap();
    assert_eq!(
        get_position(ml_a.tree(), player_a),
        get_position(ml_b.tree(), player_b),
        "player position must be identical across step() and run_frame()"
    );
}

// ===========================================================================
// 11. Traced frame evolution on platformer
// ===========================================================================

#[test]
fn pm64_traced_frame_evolution_platformer() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    let trace = main_loop.run_frames_traced(10, DT);

    assert_eq!(trace.frames.len(), 10, "should have 10 traced frames");

    // Each frame should have a valid delta.
    for (i, frame) in trace.frames.iter().enumerate() {
        assert!(
            (frame.delta - DT).abs() < 1e-10,
            "frame {i} delta should be ~DT"
        );
    }
}

// ===========================================================================
// 12. Space shooter with ext_resource refs loads structure
// ===========================================================================

#[test]
fn pm64_space_shooter_loads_with_ext_resources() {
    let (tree, scene_root) = load_scene(SPACE_SHOOTER_TSCN);

    assert_eq!(tree.get_node(scene_root).unwrap().name(), "SpaceShooter");
    assert!(tree.get_node_by_path("/root/SpaceShooter/Player").is_some());
    assert!(tree.get_node_by_path("/root/SpaceShooter/Background").is_some());
    assert!(tree.get_node_by_path("/root/SpaceShooter/EnemySpawner").is_some());
    assert!(tree.get_node_by_path("/root/SpaceShooter/ScoreLabel").is_some());

    let player = tree.get_node_by_path("/root/SpaceShooter/Player").unwrap();
    let pos = get_position(&tree, player);
    assert_eq!(pos, Vector2::new(320.0, 400.0));
}

#[test]
fn pm64_space_shooter_runs_120_frames() {
    let (tree, _) = load_scene(SPACE_SHOOTER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(120, DT);
    assert_eq!(main_loop.frame_count(), 120);
    assert!((main_loop.physics_time() - 2.0).abs() < 0.02);
}

// ===========================================================================
// 13. Early quit via close event on platformer
// ===========================================================================

#[test]
fn pm64_platformer_early_quit_via_close() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    // Run 10 frames.
    for _ in 0..10 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 10);

    // Inject close.
    backend.push_event(gdplatform::window::WindowEvent::CloseRequested);
    main_loop.run_frame(&mut backend, DT);

    assert!(backend.should_quit());
    assert_eq!(main_loop.frame_count(), 11);
}

// ===========================================================================
// 14. Position modification persists across frames
// ===========================================================================

#[test]
fn pm64_position_modification_persists() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    let player = main_loop.tree().get_node_by_path("/root/World/Player").unwrap();
    let initial = get_position(main_loop.tree(), player);

    // Move the player.
    let new_pos = Vector2::new(500.0, 100.0);
    set_position(main_loop.tree_mut(), player, new_pos);

    // Run frames.
    main_loop.run_frames(30, DT);

    // Position should still be the modified value (no scripts to change it).
    let final_pos = get_position(main_loop.tree(), player);
    assert_eq!(final_pos, new_pos, "position set before stepping should persist");
    assert_ne!(final_pos, initial);
}
