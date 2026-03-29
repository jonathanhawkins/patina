//! pat-s6w: Build broader integration workflows for representative scene and project runs.
//!
//! Exercises multi-scene project workflows that go beyond single-scene loading:
//!
//! 1. **Scene transition workflows** — load scene A, step, change to scene B, step,
//!    verify old scene is cleaned up and new scene is correct.
//! 2. **Full project lifecycle** — load all fixture scenes sequentially through
//!    `change_scene_to_packed`, simulating a game that progresses through levels.
//! 3. **Cross-subsystem integration** — scenes with physics, input, signals, and
//!    properties exercised together in a single MainLoop.
//! 4. **Pause/unpause across scene transitions** — verify paused state persists
//!    or is correctly reset during scene changes.
//! 5. **Determinism across scene transitions** — two identical runs of a multi-scene
//!    workflow produce identical results.
//! 6. **Extended frame runs** — 300+ frame runs across multiple scenes to verify
//!    long-running stability and time accumulator accuracy.
//! 7. **Resource reuse** — the same PackedScene loaded and changed-to multiple times
//!    produces consistent state each time.
//!
//! Acceptance: all workflows complete without panics and produce correct tree
//! structure, frame counts, and property values at each stage.

use gdcore::math::Vector2;
use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdscene::main_loop::MainLoop;
use gdscene::node2d::{get_position, set_position};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const DT: f64 = 1.0 / 60.0;

// ===========================================================================
// Fixture sources
// ===========================================================================

const MINIMAL_TSCN: &str = include_str!("../../fixtures/scenes/minimal.tscn");
const HIERARCHY_TSCN: &str = include_str!("../../fixtures/scenes/hierarchy.tscn");
const PLATFORMER_TSCN: &str = include_str!("../../fixtures/scenes/platformer.tscn");
const PHYSICS_TSCN: &str = include_str!("../../fixtures/scenes/physics_playground.tscn");
const UI_MENU_TSCN: &str = include_str!("../../fixtures/scenes/ui_menu.tscn");
const SIGNALS_COMPLEX_TSCN: &str = include_str!("../../fixtures/scenes/signals_complex.tscn");
const SPACE_SHOOTER_TSCN: &str = include_str!("../../fixtures/scenes/space_shooter.tscn");
const PROPERTIES_TSCN: &str = include_str!("../../fixtures/scenes/with_properties.tscn");
const CHARACTER_BODY_TSCN: &str = include_str!("../../fixtures/scenes/character_body_test.tscn");
const UNIQUE_NAME_TSCN: &str = include_str!("../../fixtures/scenes/unique_name_resolution.tscn");
const SIGNAL_INSTANTIATION_TSCN: &str =
    include_str!("../../fixtures/scenes/signal_instantiation.tscn");
const PHYSICS_EXTENDED_TSCN: &str =
    include_str!("../../fixtures/scenes/physics_playground_extended.tscn");
const TEST_SCRIPTS_TSCN: &str = include_str!("../../fixtures/scenes/test_scripts.tscn");
const MINIMAL_3D_TSCN: &str = include_str!("../../fixtures/scenes/minimal_3d.tscn");
const HIERARCHY_3D_TSCN: &str = include_str!("../../fixtures/scenes/hierarchy_3d.tscn");
const INDOOR_3D_TSCN: &str = include_str!("../../fixtures/scenes/indoor_3d.tscn");
const MULTI_LIGHT_3D_TSCN: &str = include_str!("../../fixtures/scenes/multi_light_3d.tscn");
const PHYSICS_3D_PLAYGROUND_TSCN: &str =
    include_str!("../../fixtures/scenes/physics_3d_playground.tscn");

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

// ===========================================================================
// 1. Scene transition via change_scene_to_packed in a MainLoop
// ===========================================================================

#[test]
fn s6w_scene_transition_replaces_tree_and_continues_stepping() {
    let packed_a = parse_scene(PLATFORMER_TSCN);
    let packed_b = parse_scene(UI_MENU_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Step scene A for 30 frames.
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Player")
            .is_some(),
        "scene A should have Player"
    );

    // Transition to scene B.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_b)
        .unwrap();

    // Old scene nodes should be gone, new scene nodes present.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Player")
            .is_none(),
        "scene A nodes should be removed after change_scene"
    );
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/MenuRoot/PlayButton")
            .is_some(),
        "scene B should have PlayButton"
    );

    // Step scene B for 30 more frames.
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 60);
    assert!(
        (main_loop.physics_time() - 1.0).abs() < 0.02,
        "physics time should accumulate across scene transitions"
    );
}

// ===========================================================================
// 2. Full project lifecycle: sequential scene changes through all fixtures
// ===========================================================================

#[test]
fn s6w_full_project_lifecycle_sequential_scene_changes() {
    let scenes: &[(&str, &str, &str)] = &[
        ("minimal", MINIMAL_TSCN, "Root"),
        ("hierarchy", HIERARCHY_TSCN, "Root"),
        ("platformer", PLATFORMER_TSCN, "World"),
        ("physics_playground", PHYSICS_TSCN, "World"),
        ("ui_menu", UI_MENU_TSCN, "MenuRoot"),
        ("signals_complex", SIGNALS_COMPLEX_TSCN, "Root"),
        ("space_shooter", SPACE_SHOOTER_TSCN, "SpaceShooter"),
        ("with_properties", PROPERTIES_TSCN, "Root"),
        ("character_body", CHARACTER_BODY_TSCN, "World"),
        ("unique_name", UNIQUE_NAME_TSCN, "Root"),
    ];

    // Start with first scene.
    let (tree, _) = load_scene(scenes[0].1);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(10, DT);

    // Transition through remaining scenes.
    for &(name, tscn, expected_root_name) in &scenes[1..] {
        let packed = parse_scene(tscn);
        let new_root_id = main_loop
            .tree_mut()
            .change_scene_to_packed(&packed)
            .unwrap();

        let node = main_loop.tree().get_node(new_root_id).unwrap();
        assert_eq!(
            node.name(),
            expected_root_name,
            "scene '{name}' root should be '{expected_root_name}'"
        );

        // Step 10 frames per scene.
        main_loop.run_frames(10, DT);
    }

    // Total: 10 scenes * 10 frames = 100 frames.
    assert_eq!(main_loop.frame_count(), 100);

    // Physics time should accumulate correctly across all transitions.
    let expected_time = 100.0 * DT;
    assert!(
        (main_loop.physics_time() - expected_time).abs() < 0.05,
        "physics_time should be ~{:.2}s, got {:.2}s",
        expected_time,
        main_loop.physics_time()
    );
}

// ===========================================================================
// 3. Cross-subsystem workflow: physics + input + properties in one run
// ===========================================================================

#[test]
fn s6w_cross_subsystem_physics_input_properties() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Configure input.
    let mut map = InputMap::new();
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    main_loop.set_input_map(map);

    // Register physics bodies.
    main_loop.register_physics_bodies();

    // Run 10 frames without input.
    main_loop.run_frames(10, DT);
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    let _pos_after_10 = get_position(main_loop.tree(), player);

    // Inject movement input and run more frames.
    main_loop.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    main_loop.run_frames(10, DT);

    // Position should still be retrievable without panic.
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    let _pos_after_20 = get_position(main_loop.tree(), player);

    // Verify properties scene loads and maintains properties.
    let packed_props = parse_scene(PROPERTIES_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_props)
        .unwrap();

    let prop_player = main_loop
        .tree()
        .get_node_by_path("/root/Root/Player")
        .unwrap();
    let speed = main_loop
        .tree()
        .get_node(prop_player)
        .unwrap()
        .get_property("speed");
    assert_ne!(
        speed,
        gdvariant::Variant::Nil,
        "speed property should persist after scene change"
    );

    // Step the properties scene.
    main_loop.run_frames(10, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 4. Pause/unpause across scene transitions
// ===========================================================================

#[test]
fn s6w_pause_persists_across_scene_transition() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Step, then pause.
    main_loop.run_frames(10, DT);
    main_loop.set_paused(true);
    assert!(main_loop.paused());

    // Transition scene while paused.
    let packed_ui = parse_scene(UI_MENU_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_ui)
        .unwrap();

    // Pause state should persist.
    assert!(
        main_loop.paused(),
        "pause state should persist across scene transition"
    );

    // Step while paused (frames still tick, but physics/process may be skipped).
    main_loop.run_frames(10, DT);
    assert_eq!(main_loop.frame_count(), 20);

    // Unpause and verify resumed execution.
    main_loop.set_paused(false);
    assert!(!main_loop.paused());
    main_loop.run_frames(10, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 5. Determinism: identical multi-scene workflows produce identical results
// ===========================================================================

fn run_multi_scene_workflow() -> (u64, f64, Vector2) {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Set up input.
    let mut map = InputMap::new();
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    main_loop.set_input_map(map);

    // Phase 1: run platformer 20 frames.
    main_loop.run_frames(20, DT);

    // Phase 2: inject input and run 10 more.
    main_loop.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    main_loop.run_frames(10, DT);

    // Phase 3: transition to physics and run 20.
    let packed_phys = parse_scene(PHYSICS_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_phys)
        .unwrap();
    main_loop.run_frames(20, DT);

    // Capture final state.
    let ball = main_loop
        .tree()
        .get_node_by_path("/root/World/Ball")
        .unwrap();
    let ball_pos = get_position(main_loop.tree(), ball);

    (main_loop.frame_count(), main_loop.physics_time(), ball_pos)
}

#[test]
fn s6w_multi_scene_workflow_is_deterministic() {
    let (fc_a, pt_a, pos_a) = run_multi_scene_workflow();
    let (fc_b, pt_b, pos_b) = run_multi_scene_workflow();

    assert_eq!(fc_a, fc_b, "frame counts must be identical");
    assert_eq!(pt_a, pt_b, "physics times must be identical");
    assert_eq!(pos_a, pos_b, "ball positions must be identical");
}

// ===========================================================================
// 6. Extended frame run across multiple scenes (300+ frames)
// ===========================================================================

#[test]
fn s6w_extended_300_frame_multi_scene_stability() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Phase 1: platformer for 100 frames.
    main_loop.run_frames(100, DT);
    assert_eq!(main_loop.frame_count(), 100);

    // Phase 2: transition to physics, run 100 frames.
    let packed_phys = parse_scene(PHYSICS_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_phys)
        .unwrap();
    main_loop.run_frames(100, DT);
    assert_eq!(main_loop.frame_count(), 200);

    // Phase 3: transition to signals_complex, run 100 frames.
    let packed_sig = parse_scene(SIGNALS_COMPLEX_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_sig)
        .unwrap();
    main_loop.run_frames(100, DT);
    assert_eq!(main_loop.frame_count(), 300);

    // Verify accumulated time (5 seconds at 60fps).
    let expected = 300.0 * DT;
    assert!(
        (main_loop.physics_time() - expected).abs() < 0.05,
        "physics time should be ~{:.2}s after 300 frames, got {:.2}s",
        expected,
        main_loop.physics_time()
    );

    // Verify final scene is intact.
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/Root/Player")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/Root/Enemy")
        .is_some());
}

// ===========================================================================
// 7. Resource reuse: same PackedScene loaded multiple times
// ===========================================================================

#[test]
fn s6w_same_scene_reloaded_produces_consistent_state() {
    let packed = parse_scene(PLATFORMER_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Run and modify state.
    main_loop.run_frames(30, DT);
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    set_position(main_loop.tree_mut(), player, Vector2::new(999.0, 999.0));

    // Reload the same scene.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed)
        .unwrap();

    // State should be fresh from the packed scene, not the modified state.
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    let pos = get_position(main_loop.tree(), player);
    assert_eq!(
        pos,
        Vector2::new(100.0, 300.0),
        "reloaded scene should have original position, not modified"
    );

    // Should still step normally.
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 8. Headless backend through multi-scene workflow
// ===========================================================================

#[test]
fn s6w_headless_backend_multi_scene_workflow() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    // Run 30 frames via backend.
    for _ in 0..30 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 30);
    assert_eq!(backend.frames_run(), 30);

    // Change scene via tree, continue with backend.
    let packed_sig = parse_scene(SIGNALS_COMPLEX_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_sig)
        .unwrap();

    for _ in 0..30 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 60);
    assert_eq!(backend.frames_run(), 60);

    // Verify final scene nodes.
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/Root/HUD")
        .is_some());
}

// ===========================================================================
// 9. Traced frame evolution across scene transitions
// ===========================================================================

#[test]
fn s6w_traced_frames_across_scene_transition() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Trace 5 frames on scene A.
    let trace_a = main_loop.run_frames_traced(5, DT);
    assert_eq!(trace_a.frames.len(), 5);
    for frame in &trace_a.frames {
        assert!((frame.delta - DT).abs() < 1e-10);
    }

    // Transition.
    let packed_hier = parse_scene(HIERARCHY_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_hier)
        .unwrap();

    // Trace 5 frames on scene B.
    let trace_b = main_loop.run_frames_traced(5, DT);
    assert_eq!(trace_b.frames.len(), 5);

    // Frame counter should be cumulative.
    assert_eq!(main_loop.frame_count(), 10);
}

// ===========================================================================
// 10. Multi-scene composition under one tree (additive, not replacement)
// ===========================================================================

#[test]
fn s6w_three_scene_additive_composition() {
    let packed_plat = parse_scene(PLATFORMER_TSCN);
    let packed_hier = parse_scene(HIERARCHY_TSCN);
    let packed_unique = parse_scene(UNIQUE_NAME_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let _plat_root = add_packed_scene_to_tree(&mut tree, root, &packed_plat).unwrap();
    let _hier_root = add_packed_scene_to_tree(&mut tree, root, &packed_hier).unwrap();
    let _uniq_root = add_packed_scene_to_tree(&mut tree, root, &packed_unique).unwrap();

    // All three scene subtrees should coexist.
    assert!(tree.get_node_by_path("/root/World/Player").is_some());
    assert!(tree.get_node_by_path("/root/Root/Player/Sprite").is_some());

    // Run the composed tree.
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);

    // All scenes should still have their nodes after stepping.
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/Root/Player/Sprite")
        .is_some());
}

// ===========================================================================
// 11. Scene transition cleans up node count (no memory leak)
// ===========================================================================

#[test]
fn s6w_scene_transitions_no_node_accumulation() {
    let packed_plat = parse_scene(PLATFORMER_TSCN);
    let packed_min = parse_scene(MINIMAL_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_plat).unwrap();

    // Count nodes with platformer loaded.
    let plat_count = tree.all_nodes_in_tree_order().len();

    // Switch to minimal.
    tree.change_scene_to_packed(&packed_min).unwrap();
    let min_count = tree.all_nodes_in_tree_order().len();

    assert!(
        min_count < plat_count,
        "minimal scene ({min_count}) should have fewer nodes than platformer ({plat_count})"
    );

    // Switch back to platformer.
    tree.change_scene_to_packed(&packed_plat).unwrap();
    let plat_count_2 = tree.all_nodes_in_tree_order().len();

    assert_eq!(
        plat_count, plat_count_2,
        "reloading platformer should produce the same node count"
    );
}

// ===========================================================================
// 12. Character body scene with physics stepping
// ===========================================================================

#[test]
fn s6w_character_body_physics_workflow() {
    let (tree, _) = load_scene(CHARACTER_BODY_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.register_physics_bodies();

    // Verify initial structure.
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/Platform")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/Wall")
        .is_some());

    // Step with physics.
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);

    // Player should still be queryable.
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    let _pos = get_position(main_loop.tree(), player);
}

// ===========================================================================
// 13. Input injection through scene transition
// ===========================================================================

#[test]
fn s6w_input_map_survives_scene_transition() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Set up input map.
    let mut map = InputMap::new();
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    main_loop.set_input_map(map);

    // Inject and step.
    main_loop.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    main_loop.run_frames(10, DT);

    // Transition scene.
    let packed_phys = parse_scene(PHYSICS_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_phys)
        .unwrap();

    // Input state should still work (maps are on MainLoop, not tree).
    main_loop.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    main_loop.run_frames(10, DT);
    assert_eq!(main_loop.frame_count(), 20);
}

// ===========================================================================
// 14. Close event terminates after scene transition
// ===========================================================================

#[test]
fn s6w_close_event_after_scene_transition() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    // Run 10 frames.
    for _ in 0..10 {
        main_loop.run_frame(&mut backend, DT);
    }

    // Transition.
    let packed_ui = parse_scene(UI_MENU_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_ui)
        .unwrap();

    // Run a few more frames.
    for _ in 0..5 {
        main_loop.run_frame(&mut backend, DT);
    }

    // Close event on new scene.
    backend.push_event(gdplatform::window::WindowEvent::CloseRequested);
    main_loop.run_frame(&mut backend, DT);

    assert!(backend.should_quit());
    assert_eq!(main_loop.frame_count(), 16);
}

// ===========================================================================
// 15. All 10 fixture scenes parse, load, step, and transition without error
// ===========================================================================

#[test]
fn s6w_all_fixtures_round_trip_through_change_scene() {
    let fixture_data: &[(&str, &str)] = &[
        ("minimal", MINIMAL_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("physics_playground", PHYSICS_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("with_properties", PROPERTIES_TSCN),
        ("character_body", CHARACTER_BODY_TSCN),
        ("unique_name", UNIQUE_NAME_TSCN),
    ];

    // Pre-parse all scenes.
    let packed_scenes: Vec<(&str, PackedScene)> = fixture_data
        .iter()
        .map(|&(name, tscn)| (name, parse_scene(tscn)))
        .collect();

    // Start with first scene.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_scenes[0].1).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Cycle through all scenes twice: A→B→C→...→J→A→B→...→J.
    for cycle in 0..2u32 {
        for (i, (name, packed)) in packed_scenes.iter().enumerate() {
            if cycle == 0 && i == 0 {
                // Already loaded.
                main_loop.run_frames(5, DT);
                continue;
            }
            main_loop
                .tree_mut()
                .change_scene_to_packed(packed)
                .unwrap_or_else(|e| panic!("cycle {cycle}, scene '{name}' failed: {e}"));

            main_loop.run_frames(5, DT);
        }
    }

    // 20 transitions * 5 frames = 100 frames total.
    assert_eq!(main_loop.frame_count(), 100);
}

// ===========================================================================
// 16. Physics TPS configuration across scenes
// ===========================================================================

#[test]
fn s6w_physics_tps_configuration_across_scenes() {
    let (tree, _) = load_scene(PHYSICS_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Default TPS.
    assert_eq!(main_loop.physics_ticks_per_second(), 60);
    main_loop.run_frames(60, DT);

    // Change to higher TPS.
    main_loop.set_physics_ticks_per_second(120);
    assert_eq!(main_loop.physics_ticks_per_second(), 120);

    // Transition scene — TPS setting should persist.
    let packed_sig = parse_scene(SIGNALS_COMPLEX_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_sig)
        .unwrap();

    assert_eq!(
        main_loop.physics_ticks_per_second(),
        120,
        "TPS should persist across scene transitions"
    );

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 120);
}

// ===========================================================================
// 17. Position modification then transition then reload original
// ===========================================================================

#[test]
fn s6w_modify_position_transition_reload_resets() {
    let packed_plat = parse_scene(PLATFORMER_TSCN);
    let packed_ui = parse_scene(UI_MENU_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_plat).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Modify player position.
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    set_position(main_loop.tree_mut(), player, Vector2::new(777.0, 888.0));
    main_loop.run_frames(10, DT);

    // Transition to UI.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_ui)
        .unwrap();
    main_loop.run_frames(10, DT);

    // Reload platformer — should get original positions.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_plat)
        .unwrap();
    let player = main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .unwrap();
    let pos = get_position(main_loop.tree(), player);
    assert_eq!(
        pos,
        Vector2::new(100.0, 300.0),
        "reloaded platformer should have original position"
    );
}

// ===========================================================================
// 18. Unique name resolution works after scene transition
// ===========================================================================

#[test]
fn s6w_unique_name_resolution_after_transition() {
    let (tree, _) = load_scene(PLATFORMER_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(10, DT);

    // Transition to unique name scene.
    let packed_uniq = parse_scene(UNIQUE_NAME_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_uniq)
        .unwrap();

    // Unique names should be resolvable from the scene root scope.
    // get_node_by_unique_name resolves within the owner scope of `from`.
    // The scene root "Root" owns the unique-named children.
    let scene_root = main_loop
        .tree()
        .get_node_by_path("/root/Root")
        .expect("scene root 'Root' should exist after change_scene");
    let health = main_loop
        .tree()
        .get_node_by_unique_name(scene_root, "HealthBar");
    assert!(
        health.is_some(),
        "%HealthBar should be resolvable after scene transition"
    );

    let score = main_loop
        .tree()
        .get_node_by_unique_name(scene_root, "ScoreLabel");
    assert!(
        score.is_some(),
        "%ScoreLabel should be resolvable after scene transition"
    );

    let status = main_loop
        .tree()
        .get_node_by_unique_name(scene_root, "StatusIcon");
    assert!(
        status.is_some(),
        "%StatusIcon should be resolvable after scene transition"
    );

    main_loop.run_frames(10, DT);
    assert_eq!(main_loop.frame_count(), 20);
}

// ===========================================================================
// 19. Step-vs-run_frame determinism across scene transition
// ===========================================================================

#[test]
fn s6w_step_vs_run_frame_deterministic_across_transition() {
    let frames_per_phase = 20u64;

    // Path A: step()
    let (tree_a, _) = load_scene(PLATFORMER_TSCN);
    let mut ml_a = MainLoop::new(tree_a);
    for _ in 0..frames_per_phase {
        ml_a.step(DT);
    }
    ml_a.tree_mut()
        .change_scene_to_packed(&parse_scene(PHYSICS_TSCN))
        .unwrap();
    for _ in 0..frames_per_phase {
        ml_a.step(DT);
    }

    // Path B: run_frame()
    let (tree_b, _) = load_scene(PLATFORMER_TSCN);
    let mut ml_b = MainLoop::new(tree_b);
    let mut backend = HeadlessPlatform::new(640, 480);
    for _ in 0..frames_per_phase {
        ml_b.run_frame(&mut backend, DT);
    }
    ml_b.tree_mut()
        .change_scene_to_packed(&parse_scene(PHYSICS_TSCN))
        .unwrap();
    for _ in 0..frames_per_phase {
        ml_b.run_frame(&mut backend, DT);
    }

    assert_eq!(ml_a.frame_count(), ml_b.frame_count());
    assert_eq!(ml_a.physics_time(), ml_b.physics_time());

    // Ball position should match.
    let ball_a = ml_a.tree().get_node_by_path("/root/World/Ball").unwrap();
    let ball_b = ml_b.tree().get_node_by_path("/root/World/Ball").unwrap();
    assert_eq!(
        get_position(ml_a.tree(), ball_a),
        get_position(ml_b.tree(), ball_b),
        "ball position must be identical across step() and run_frame()"
    );
}

// ===========================================================================
// 20. Rapid scene transitions (stress test)
// ===========================================================================

#[test]
fn s6w_rapid_scene_transitions_100_cycles() {
    let packed_min = parse_scene(MINIMAL_TSCN);
    let packed_hier = parse_scene(HIERARCHY_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_min).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Rapidly alternate between two scenes, 1 frame each.
    for i in 0..100u32 {
        let packed = if i % 2 == 0 {
            &packed_hier
        } else {
            &packed_min
        };
        main_loop.tree_mut().change_scene_to_packed(packed).unwrap();
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), 100);

    // Verify final scene is consistent.
    let final_nodes = main_loop.tree().all_nodes_in_tree_order();
    assert!(
        final_nodes.len() >= 2,
        "final scene should have at least root + scene nodes"
    );
}

// ===========================================================================
// 21. Signal instantiation scene: connections survive scene transitions
// ===========================================================================

#[test]
fn s6w_signal_instantiation_scene_transition_workflow() {
    let packed_sig_inst = parse_scene(SIGNAL_INSTANTIATION_TSCN);
    let packed_plat = parse_scene(PLATFORMER_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_sig_inst).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Verify signal scene structure.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/GameWorld/Player")
            .is_some(),
        "signal_instantiation should have Player"
    );
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/GameWorld/Enemy")
            .is_some(),
        "signal_instantiation should have Enemy"
    );
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/GameWorld/HUD/ScoreLabel")
            .is_some(),
        "signal_instantiation should have HUD/ScoreLabel"
    );

    // Step the signal scene.
    main_loop.run_frames(20, DT);
    assert_eq!(main_loop.frame_count(), 20);

    // Transition to platformer and back to signal scene.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_plat)
        .unwrap();
    main_loop.run_frames(10, DT);

    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_sig_inst)
        .unwrap();
    main_loop.run_frames(10, DT);

    // Signal scene should be fully reconstructed.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/GameWorld/Player/Hitbox")
            .is_some(),
        "Player/Hitbox should exist after reload"
    );
    assert_eq!(main_loop.frame_count(), 40);
}

// ===========================================================================
// 22. Extended physics playground with multi-body stepping
// ===========================================================================

#[test]
fn s6w_extended_physics_multi_body_workflow() {
    let (tree, _) = load_scene(PHYSICS_EXTENDED_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.register_physics_bodies();

    // Verify multi-body structure.
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/BallA")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/BallB")
        .is_some());
    assert!(main_loop
        .tree()
        .get_node_by_path("/root/World/Player")
        .is_some());

    // Run 120 frames with physics.
    main_loop.run_frames(120, DT);
    assert_eq!(main_loop.frame_count(), 120);

    // Transition to basic physics and back.
    let packed_basic_phys = parse_scene(PHYSICS_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_basic_phys)
        .unwrap();
    main_loop.run_frames(30, DT);

    let packed_ext = parse_scene(PHYSICS_EXTENDED_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_ext)
        .unwrap();
    main_loop.run_frames(30, DT);

    assert_eq!(main_loop.frame_count(), 180);
}

// ===========================================================================
// 23. 3D scene integration: parse, load, step, transition 2D ↔ 3D
// ===========================================================================

#[test]
fn s6w_3d_scene_parse_load_step_workflow() {
    let packed_3d = parse_scene(MINIMAL_3D_TSCN);
    let packed_2d = parse_scene(PLATFORMER_TSCN);

    // Start with 3D scene.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_3d).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Verify 3D structure.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Camera")
            .is_some(),
        "minimal_3d should have Camera"
    );
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Cube")
            .is_some(),
        "minimal_3d should have Cube"
    );
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Sun")
            .is_some(),
        "minimal_3d should have Sun"
    );

    // Step the 3D scene.
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);

    // Transition 3D → 2D.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_2d)
        .unwrap();
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Player")
            .is_some(),
        "should have 2D Player after 3D→2D transition"
    );
    main_loop.run_frames(30, DT);

    // Transition 2D → 3D.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_3d)
        .unwrap();
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Camera")
            .is_some(),
        "Camera should exist after 2D→3D transition"
    );
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 90);
}

// ===========================================================================
// 24. 3D hierarchy scene with scene transitions
// ===========================================================================

#[test]
fn s6w_3d_hierarchy_scene_transition_workflow() {
    let packed_hier3d = parse_scene(HIERARCHY_3D_TSCN);
    let packed_min3d = parse_scene(MINIMAL_3D_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_hier3d).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Step hierarchy_3d.
    main_loop.run_frames(20, DT);

    // Transition to minimal_3d.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_min3d)
        .unwrap();
    main_loop.run_frames(20, DT);

    // Back to hierarchy_3d.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_hier3d)
        .unwrap();
    main_loop.run_frames(20, DT);

    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 25. Expanded full project lifecycle: all 13 2D + 2 3D fixtures
// ===========================================================================

#[test]
fn s6w_expanded_lifecycle_all_fixtures_2d_and_3d() {
    let all_fixtures: &[(&str, &str)] = &[
        ("minimal", MINIMAL_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("physics_playground", PHYSICS_TSCN),
        ("physics_extended", PHYSICS_EXTENDED_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
        ("signal_instantiation", SIGNAL_INSTANTIATION_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("with_properties", PROPERTIES_TSCN),
        ("character_body", CHARACTER_BODY_TSCN),
        ("unique_name", UNIQUE_NAME_TSCN),
        ("test_scripts", TEST_SCRIPTS_TSCN),
        ("minimal_3d", MINIMAL_3D_TSCN),
        ("hierarchy_3d", HIERARCHY_3D_TSCN),
        ("indoor_3d", INDOOR_3D_TSCN),
        ("multi_light_3d", MULTI_LIGHT_3D_TSCN),
        ("physics_3d_playground", PHYSICS_3D_PLAYGROUND_TSCN),
    ];

    // Pre-parse all scenes.
    let packed: Vec<(&str, PackedScene)> = all_fixtures
        .iter()
        .map(|&(name, tscn)| (name, parse_scene(tscn)))
        .collect();

    // Start with first scene.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed[0].1).unwrap();
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(5, DT);

    // Transition through all remaining scenes.
    for (name, scene) in &packed[1..] {
        main_loop
            .tree_mut()
            .change_scene_to_packed(scene)
            .unwrap_or_else(|e| panic!("scene '{name}' failed: {e}"));
        main_loop.run_frames(5, DT);
    }

    // 18 scenes * 5 frames = 90 frames.
    assert_eq!(main_loop.frame_count(), 90);

    let expected_time = 90.0 * DT;
    assert!(
        (main_loop.physics_time() - expected_time).abs() < 0.05,
        "physics_time should be ~{:.2}s, got {:.2}s",
        expected_time,
        main_loop.physics_time()
    );
}

// ===========================================================================
// 26. Test scripts scene in workflow (GDScript fixture)
// ===========================================================================

#[test]
fn s6w_test_scripts_scene_workflow() {
    let (tree, _) = load_scene(TEST_SCRIPTS_TSCN);
    let mut main_loop = MainLoop::new(tree);

    // Step the scripts scene.
    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 30);

    // Transition to another scene and back.
    let packed_hier = parse_scene(HIERARCHY_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_hier)
        .unwrap();
    main_loop.run_frames(10, DT);

    let packed_scripts = parse_scene(TEST_SCRIPTS_TSCN);
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_scripts)
        .unwrap();
    main_loop.run_frames(10, DT);

    assert_eq!(main_loop.frame_count(), 50);
}

// ===========================================================================
// pat-tno: Broader 3D scene integration workflows
// ===========================================================================

// 27. Indoor 3D scene loads and steps without panic.
#[test]
fn s6w_indoor_3d_scene_workflow() {
    let (tree, _) = load_scene(INDOOR_3D_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// 28. Multi-light 3D scene loads and steps without panic.
#[test]
fn s6w_multi_light_3d_scene_workflow() {
    let (tree, _) = load_scene(MULTI_LIGHT_3D_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// 29. Physics 3D playground scene loads and steps without panic.
#[test]
fn s6w_physics_3d_playground_scene_workflow() {
    let (tree, _) = load_scene(PHYSICS_3D_PLAYGROUND_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// 30. All 3D scenes in sequence through scene transitions.
#[test]
fn s6w_all_3d_scenes_sequential_transitions() {
    let scenes_3d: &[(&str, &str)] = &[
        ("minimal_3d", MINIMAL_3D_TSCN),
        ("hierarchy_3d", HIERARCHY_3D_TSCN),
        ("indoor_3d", INDOOR_3D_TSCN),
        ("multi_light_3d", MULTI_LIGHT_3D_TSCN),
        ("physics_3d_playground", PHYSICS_3D_PLAYGROUND_TSCN),
    ];

    let packed: Vec<(&str, PackedScene)> = scenes_3d
        .iter()
        .map(|&(name, tscn)| (name, parse_scene(tscn)))
        .collect();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed[0].1).unwrap();
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(10, DT);

    for (name, scene) in &packed[1..] {
        main_loop
            .tree_mut()
            .change_scene_to_packed(scene)
            .unwrap_or_else(|e| panic!("3D scene '{name}' transition failed: {e}"));
        main_loop.run_frames(10, DT);
    }

    // 5 scenes × 10 frames each = 50 frames
    assert_eq!(main_loop.frame_count(), 50);
}

// 31. 3D→2D→3D scene transitions (cross-dimension workflow).
#[test]
fn s6w_3d_to_2d_to_3d_scene_transition() {
    let packed_3d = parse_scene(HIERARCHY_3D_TSCN);
    let packed_2d = parse_scene(PLATFORMER_TSCN);
    let packed_3d_again = parse_scene(INDOOR_3D_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_3d).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Start in 3D.
    main_loop.run_frames(20, DT);

    // Transition to 2D.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_2d)
        .unwrap();
    main_loop.run_frames(20, DT);

    // Back to 3D (different scene).
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_3d_again)
        .unwrap();
    main_loop.run_frames(20, DT);

    assert_eq!(main_loop.frame_count(), 60);
}

// 32. Full 18-scene project lifecycle (all 2D + all 3D fixtures).
#[test]
fn s6w_full_18_scene_project_lifecycle() {
    let all_fixtures: &[(&str, &str)] = &[
        ("minimal", MINIMAL_TSCN),
        ("hierarchy", HIERARCHY_TSCN),
        ("platformer", PLATFORMER_TSCN),
        ("physics_playground", PHYSICS_TSCN),
        ("physics_extended", PHYSICS_EXTENDED_TSCN),
        ("ui_menu", UI_MENU_TSCN),
        ("signals_complex", SIGNALS_COMPLEX_TSCN),
        ("signal_instantiation", SIGNAL_INSTANTIATION_TSCN),
        ("space_shooter", SPACE_SHOOTER_TSCN),
        ("with_properties", PROPERTIES_TSCN),
        ("character_body", CHARACTER_BODY_TSCN),
        ("unique_name", UNIQUE_NAME_TSCN),
        ("test_scripts", TEST_SCRIPTS_TSCN),
        ("minimal_3d", MINIMAL_3D_TSCN),
        ("hierarchy_3d", HIERARCHY_3D_TSCN),
        ("indoor_3d", INDOOR_3D_TSCN),
        ("multi_light_3d", MULTI_LIGHT_3D_TSCN),
        ("physics_3d_playground", PHYSICS_3D_PLAYGROUND_TSCN),
    ];

    let packed: Vec<(&str, PackedScene)> = all_fixtures
        .iter()
        .map(|&(name, tscn)| (name, parse_scene(tscn)))
        .collect();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed[0].1).unwrap();
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(3, DT);

    for (name, scene) in &packed[1..] {
        main_loop
            .tree_mut()
            .change_scene_to_packed(scene)
            .unwrap_or_else(|e| panic!("scene '{name}' failed: {e}"));
        main_loop.run_frames(3, DT);
    }

    // 18 scenes × 3 frames each = 54 frames
    assert_eq!(main_loop.frame_count(), 54);
}

// 33. 3D scene extended stability run (200 frames).
#[test]
fn s6w_3d_scene_extended_200_frame_stability() {
    let (tree, _) = load_scene(INDOOR_3D_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(200, DT);
    assert_eq!(main_loop.frame_count(), 200);
}

// 34. 3D scene determinism across transitions.
#[test]
fn s6w_3d_scene_transition_determinism() {
    fn run_3d_workflow() -> u64 {
        let packed_a = parse_scene(MINIMAL_3D_TSCN);
        let packed_b = parse_scene(MULTI_LIGHT_3D_TSCN);

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
        let mut main_loop = MainLoop::new(tree);
        main_loop.run_frames(10, DT);

        main_loop
            .tree_mut()
            .change_scene_to_packed(&packed_b)
            .unwrap();
        main_loop.run_frames(10, DT);

        main_loop.frame_count()
    }

    let run_a = run_3d_workflow();
    let run_b = run_3d_workflow();
    assert_eq!(run_a, run_b, "deterministic frame count across runs");
    assert_eq!(run_a, 20);
}
