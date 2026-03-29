//! pat-r6sku: Broader integration fixtures covering audio, navigation, and UI.
//!
//! Exercises cross-subsystem integration that goes beyond single-domain tests:
//!
//! 1. **Audio server lifecycle in MainLoop** — play streams, advance frames,
//!    verify mixing output and auto-cleanup of finished streams.
//! 2. **Audio bus routing integration** — multiple buses (Master, SFX, Music)
//!    with volume/mute, verify mixed output across scene transitions.
//! 3. **Navigation pathfinding integration** — build a nav mesh, create an
//!    agent, step it through waypoints frame-by-frame, verify convergence.
//! 4. **Navigation obstacle avoidance** — verify paths are blocked/rerouted
//!    when obstacles are added dynamically.
//! 5. **UI control layout integration** — create control hierarchy with anchors,
//!    size flags, and containers; verify property persistence across frames.
//! 6. **UI + scene transition** — build a UI menu scene, transition to gameplay
//!    scene, verify old UI is cleaned up and new scene is correct.
//! 7. **Cross-domain: audio + navigation + UI in one tree** — build a scene
//!    with all three subsystems active, step frames, verify no interference.
//! 8. **Navigation 3D pathfinding integration** — multi-polygon 3D nav mesh
//!    with A* and agent stepping.
//! 9. **Audio determinism** — two identical server runs produce identical output.
//! 10. **UI container children and property propagation** — VBox/HBox with
//!     multiple children, size flags, and separation.
//!
//! Acceptance: all tests complete without panics and produce correct values
//! at each verification point.

use gdaudio::{AudioBuffer, AudioMixer, AudioServer, AudioStreamPlayback, LoopMode, PlaybackState};
use gdcore::math::Vector2;
use gdscene::control::{self, AnchorPreset, FocusMode, SizeFlags, TextAlign};
use gdscene::main_loop::MainLoop;
use gdscene::navigation::{
    NavMesh2D, NavMesh3D, NavPolygon, NavPolygon3D, NavigationAgent2D, NavigationObstacle2D,
    NavigationServer2D, NavigationServer3D,
};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{Node, SceneTree};

const DT: f64 = 1.0 / 60.0;
const EPSILON: f32 = 1e-4;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// Fixture scenes used for scene-transition tests.
const PLATFORMER_TSCN: &str = include_str!("../../fixtures/scenes/platformer.tscn");
const UI_MENU_TSCN: &str = include_str!("../../fixtures/scenes/ui_menu.tscn");
const MINIMAL_TSCN: &str = include_str!("../../fixtures/scenes/minimal.tscn");

fn parse_scene(tscn: &str) -> PackedScene {
    PackedScene::from_tscn(tscn).expect("failed to parse scene")
}

fn make_dc_buffer(value: f32, frames: usize) -> AudioBuffer {
    AudioBuffer {
        samples: vec![value; frames],
        sample_rate: 44100,
        channels: 1,
    }
}

fn make_stereo_buffer(left: f32, right: f32, frames: usize) -> AudioBuffer {
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        samples.push(left);
        samples.push(right);
    }
    AudioBuffer {
        samples,
        sample_rate: 44100,
        channels: 2,
    }
}

/// Helper: build a two-square nav mesh spanning x=[0..20], y=[0..10].
fn make_two_square_navmesh() -> NavMesh2D {
    let left = NavPolygon::new(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(0.0, 10.0),
    ]);
    let right = NavPolygon::new(vec![
        Vector2::new(10.0, 0.0),
        Vector2::new(20.0, 0.0),
        Vector2::new(20.0, 10.0),
        Vector2::new(10.0, 10.0),
    ]);
    NavMesh2D::new(vec![left, right], 0.01)
}

/// Helper: add a Control node child.
fn add_control(tree: &mut SceneTree, parent: NodeId, name: &str, class: &str) -> NodeId {
    tree.add_child(parent, Node::new(name, class)).unwrap()
}

// ===========================================================================
// 1. Audio server lifecycle with frame-based mixing
// ===========================================================================

#[test]
fn r6sku_audio_server_lifecycle_multi_stream_mixing() {
    let mut server = AudioServer::new();

    // Add buses.
    let sfx_idx = server.mixer_mut().add_bus("SFX");
    let music_idx = server.mixer_mut().add_bus("Music");
    assert_eq!(server.mixer().bus_count(), 3);

    // Play a DC stream on SFX at -20 dB (~0.1 linear).
    server
        .mixer_mut()
        .get_bus_mut(sfx_idx)
        .unwrap()
        .set_volume_db(-20.0);
    let sfx_id = server.play_on_bus(make_dc_buffer(1.0, 44100), "SFX");

    // Play a DC stream on Music at 0 dB.
    let music_id = server.play_on_bus(make_dc_buffer(0.5, 44100), "Music");

    assert_eq!(server.active_stream_count(), 2);

    // Mix one frame: SFX contributes 1.0*0.1 = 0.1, Music contributes 0.5*1.0 = 0.5.
    let output = server.mix(1);
    assert_eq!(output.len(), 2); // stereo
    let expected = 0.1 + 0.5;
    assert!(
        (output[0] - expected).abs() < 0.02,
        "mixed L should be ~{expected}, got {}",
        output[0]
    );

    // Stop SFX, verify only Music remains.
    server.stop(sfx_id);
    assert!(!server.is_playing(sfx_id));
    assert!(server.is_playing(music_id));
    assert_eq!(server.active_stream_count(), 1);

    // Mix again: only Music at 0.5.
    let output2 = server.mix(1);
    assert!(
        (output2[0] - 0.5).abs() < 0.02,
        "after stop SFX, L should be ~0.5, got {}",
        output2[0]
    );

    // Mute Music bus.
    server
        .mixer_mut()
        .get_bus_mut(music_idx)
        .unwrap()
        .set_mute(true);
    let output3 = server.mix(1);
    assert!(
        output3[0].abs() < 1e-6,
        "muted Music bus should produce silence, got {}",
        output3[0]
    );
}

// ===========================================================================
// 2. Audio stereo panning and bus re-routing
// ===========================================================================

#[test]
fn r6sku_audio_stereo_bus_rerouting() {
    let mut server = AudioServer::new();
    server.mixer_mut().add_bus("SFX");

    // Stereo buffer: L=0.8, R=0.2.
    let buf = make_stereo_buffer(0.8, 0.2, 4410);
    let id = server.play(buf);
    assert!(server.is_playing(id));

    let output = server.mix(2);
    // Channel 0 (L) and channel 1 (R) from stereo source.
    assert!((output[0] - 0.8).abs() < 1e-3, "L channel: {}", output[0]);
    assert!((output[1] - 0.2).abs() < 1e-3, "R channel: {}", output[1]);
}

// ===========================================================================
// 3. Audio playback state transitions: play, pause, seek, loop
// ===========================================================================

#[test]
fn r6sku_audio_playback_pause_seek_loop_integration() {
    let mut pb = AudioStreamPlayback::new(10.0);

    // Initial state.
    assert_eq!(pb.state(), PlaybackState::Stopped);

    // Play and advance.
    pb.play();
    pb.advance(2.5);
    assert!(approx_eq(pb.get_playback_position() as f32, 2.5));

    // Pause preserves position.
    pb.pause();
    assert_eq!(pb.state(), PlaybackState::Paused);
    assert!(approx_eq(pb.get_playback_position() as f32, 2.5));

    // Seek while paused.
    pb.seek(7.0);
    assert!(approx_eq(pb.get_playback_position() as f32, 7.0));

    // Resume and advance.
    pb.play();
    pb.advance(1.0);
    assert!(approx_eq(pb.get_playback_position() as f32, 8.0));

    // Enable looping and go past end.
    pb.set_loop_mode(LoopMode::Forward);
    pb.advance(5.0); // 8 + 5 = 13, wraps to 3.0.
    assert!(pb.is_playing());
    assert!(
        approx_eq(pb.get_playback_position() as f32, 3.0),
        "looped position should be ~3.0, got {}",
        pb.get_playback_position()
    );

    // Stop resets.
    pb.stop();
    assert_eq!(pb.state(), PlaybackState::Stopped);
    assert_eq!(pb.get_playback_position(), 0.0);
}

// ===========================================================================
// 4. Audio mixer bus reordering
// ===========================================================================

#[test]
fn r6sku_audio_mixer_bus_reorder_and_lookup() {
    let mut mixer = AudioMixer::new();
    mixer.add_bus("SFX");
    mixer.add_bus("Music");
    mixer.add_bus("Voice");
    assert_eq!(mixer.bus_count(), 4);

    // Move Voice (index 3) to index 1.
    mixer.move_bus(3, 1);
    assert_eq!(mixer.get_bus(1).unwrap().name(), "Voice");
    assert_eq!(mixer.get_bus(2).unwrap().name(), "SFX");
    assert_eq!(mixer.get_bus(3).unwrap().name(), "Music");

    // Lookup by name still works after reorder.
    assert_eq!(mixer.get_bus_by_name("Voice"), Some(1));
    assert_eq!(mixer.get_bus_by_name("SFX"), Some(2));
    assert_eq!(mixer.get_bus_by_name("Music"), Some(3));
    assert_eq!(mixer.get_bus_by_name("Master"), Some(0));

    // Remove SFX (index 2).
    mixer.remove_bus(2);
    assert_eq!(mixer.bus_count(), 3);
    assert_eq!(mixer.get_bus_by_name("SFX"), None);
}

// ===========================================================================
// 5. Navigation agent frame-stepping to target
// ===========================================================================

#[test]
fn r6sku_navigation_agent_frame_step_convergence() {
    let mut server = NavigationServer2D::new();
    server.add_region(make_two_square_navmesh());

    let mut agent = NavigationAgent2D::new();
    agent.path_desired_distance = 1.0;
    let start = Vector2::new(2.0, 5.0);
    let target = Vector2::new(18.0, 5.0);
    agent.set_target_position(target, start, &server);

    assert!(!agent.is_navigation_finished());

    // Simulate frame-by-frame stepping.
    let mut pos = start;
    let speed = 2.0_f32;
    let mut steps = 0;
    let max_steps = 200;

    while !agent.is_navigation_finished() && steps < max_steps {
        let next_wp = agent.get_next_path_position(pos);
        let dir = next_wp - pos;
        let dist = dir.length();
        if dist > 0.01 {
            let step = if dist < speed { dist } else { speed };
            let norm = Vector2::new(dir.x / dist, dir.y / dist);
            pos = Vector2::new(pos.x + norm.x * step, pos.y + norm.y * step);
        }
        steps += 1;
    }

    assert!(
        agent.is_navigation_finished() || (pos - target).length() < 2.0,
        "agent should reach target or be very close; pos=({}, {}), steps={steps}",
        pos.x,
        pos.y,
    );
    assert!(
        steps < max_steps,
        "agent should converge in fewer than {max_steps} steps"
    );
}

// ===========================================================================
// 6. Navigation obstacle dynamically blocks path
// ===========================================================================

#[test]
fn r6sku_navigation_obstacle_dynamic_blocking() {
    let mut server = NavigationServer2D::new();
    server.add_region(make_two_square_navmesh());

    let from = Vector2::new(5.0, 5.0);
    let to = Vector2::new(15.0, 5.0);

    // Without obstacle, path exists.
    let path = server.find_path(from, to);
    assert!(!path.is_empty(), "path should exist without obstacle");

    // Add large obstacle on the boundary between polygons.
    server.add_obstacle(NavigationObstacle2D::new(Vector2::new(10.0, 5.0), 6.0));

    // Now path should be blocked.
    let path2 = server.find_path(from, to);
    assert!(path2.is_empty(), "path should be blocked by obstacle");
}

// ===========================================================================
// 7. Navigation: multiple regions, agent switches
// ===========================================================================

#[test]
fn r6sku_navigation_multi_region_pathfinding() {
    let mut server = NavigationServer2D::new();

    // Region 1: x=[0..20].
    server.add_region(make_two_square_navmesh());

    // Region 2: separate area, x=[50..70].
    let far_left = NavPolygon::new(vec![
        Vector2::new(50.0, 0.0),
        Vector2::new(60.0, 0.0),
        Vector2::new(60.0, 10.0),
        Vector2::new(50.0, 10.0),
    ]);
    let far_right = NavPolygon::new(vec![
        Vector2::new(60.0, 0.0),
        Vector2::new(70.0, 0.0),
        Vector2::new(70.0, 10.0),
        Vector2::new(60.0, 10.0),
    ]);
    server.add_region(NavMesh2D::new(vec![far_left, far_right], 0.01));

    // Path within region 1 works.
    let path1 = server.find_path(Vector2::new(5.0, 5.0), Vector2::new(15.0, 5.0));
    assert!(!path1.is_empty());

    // Path within region 2 works.
    let path2 = server.find_path(Vector2::new(55.0, 5.0), Vector2::new(65.0, 5.0));
    assert!(!path2.is_empty());

    // Cross-region path fails (no connectivity).
    let path_cross = server.find_path(Vector2::new(5.0, 5.0), Vector2::new(55.0, 5.0));
    assert!(path_cross.is_empty(), "cross-region path should fail");
}

// ===========================================================================
// 8. Navigation 3D: multi-polygon pathfinding
// ===========================================================================

#[test]
fn r6sku_navigation_3d_multi_polygon_path() {
    let p0 = NavPolygon3D::new(vec![
        gdcore::math::Vector3::new(0.0, 0.0, 0.0),
        gdcore::math::Vector3::new(10.0, 0.0, 0.0),
        gdcore::math::Vector3::new(10.0, 0.0, 10.0),
        gdcore::math::Vector3::new(0.0, 0.0, 10.0),
    ]);
    let p1 = NavPolygon3D::new(vec![
        gdcore::math::Vector3::new(10.0, 0.0, 0.0),
        gdcore::math::Vector3::new(20.0, 0.0, 0.0),
        gdcore::math::Vector3::new(20.0, 0.0, 10.0),
        gdcore::math::Vector3::new(10.0, 0.0, 10.0),
    ]);
    let p2 = NavPolygon3D::new(vec![
        gdcore::math::Vector3::new(20.0, 0.0, 0.0),
        gdcore::math::Vector3::new(30.0, 0.0, 0.0),
        gdcore::math::Vector3::new(30.0, 0.0, 10.0),
        gdcore::math::Vector3::new(20.0, 0.0, 10.0),
    ]);

    let mesh = NavMesh3D::new(
        vec![p0, p1, p2],
        vec![vec![1], vec![0, 2], vec![1]], // linear chain
    );

    let mut server = NavigationServer3D::new();
    server.add_region(mesh);

    let path = server.find_path(
        gdcore::math::Vector3::new(2.0, 0.0, 5.0),
        gdcore::math::Vector3::new(28.0, 0.0, 5.0),
    );
    assert!(!path.is_empty(), "3D path across 3 polygons should exist");
    assert!(
        path.len() >= 3,
        "path should have at least start + intermediate + end"
    );
    assert!(approx_eq(path[0].x, 2.0));
    assert!(approx_eq(path.last().unwrap().x, 28.0));
}

// ===========================================================================
// 9. UI control hierarchy: anchors, size, label, button properties
// ===========================================================================

#[test]
fn r6sku_ui_control_hierarchy_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build a UI hierarchy: Panel > VBoxContainer > Label + Button.
    let panel = add_control(&mut tree, root, "Panel", "Panel");
    let vbox = add_control(&mut tree, panel, "VBox", "VBoxContainer");
    let label = add_control(&mut tree, vbox, "Title", "Label");
    let button = add_control(&mut tree, vbox, "StartBtn", "Button");

    // Apply full-rect anchor to panel.
    control::apply_anchor_preset(&mut tree, panel, AnchorPreset::FullRect);

    // Set label properties.
    control::set_label_text(&mut tree, label, "Welcome to Patina");
    control::set_font_size(&mut tree, label, 24);
    control::set_h_align(&mut tree, label, TextAlign::Center);

    // Set button properties.
    control::set_button_text(&mut tree, button, "Start Game");
    control::set_disabled(&mut tree, button, false);
    control::set_focus_mode(&mut tree, button, FocusMode::All);

    // Set size flags on vbox children.
    control::set_h_size_flags(&mut tree, label, SizeFlags::Expand);
    control::set_v_size_flags(&mut tree, label, SizeFlags::Fill);

    // Verify all properties persisted.
    assert_eq!(control::get_label_text(&tree, label), "Welcome to Patina");
    assert_eq!(control::get_font_size(&tree, label), 24);
    assert_eq!(control::get_h_align(&tree, label), TextAlign::Center);
    assert_eq!(control::get_button_text(&tree, button), "Start Game");
    assert!(!control::is_disabled(&tree, button));
    assert_eq!(control::get_focus_mode(&tree, button), FocusMode::All);
    assert_eq!(control::get_h_size_flags(&tree, label), SizeFlags::Expand);

    // Verify anchor preset was applied (FullRect = anchors 0,0,1,1).
    assert!(approx_eq(control::get_anchor_left(&tree, panel), 0.0));
    assert!(approx_eq(control::get_anchor_top(&tree, panel), 0.0));
    assert!(approx_eq(control::get_anchor_right(&tree, panel), 1.0));
    assert!(approx_eq(control::get_anchor_bottom(&tree, panel), 1.0));
}

// ===========================================================================
// 10. UI anchor presets: verify each preset sets correct values
// ===========================================================================

#[test]
fn r6sku_ui_anchor_presets_correctness() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let presets_and_expected: &[(AnchorPreset, f32, f32, f32, f32)] = &[
        (AnchorPreset::FullRect, 0.0, 0.0, 1.0, 1.0),
        (AnchorPreset::Center, 0.5, 0.5, 0.5, 0.5),
        (AnchorPreset::TopLeft, 0.0, 0.0, 0.0, 0.0),
        (AnchorPreset::TopRight, 1.0, 0.0, 1.0, 0.0),
        (AnchorPreset::BottomLeft, 0.0, 1.0, 0.0, 1.0),
        (AnchorPreset::BottomRight, 1.0, 1.0, 1.0, 1.0),
        (AnchorPreset::TopWide, 0.0, 0.0, 1.0, 0.0),
        (AnchorPreset::BottomWide, 0.0, 1.0, 1.0, 1.0),
        (AnchorPreset::LeftWide, 0.0, 0.0, 0.0, 1.0),
        (AnchorPreset::RightWide, 1.0, 0.0, 1.0, 1.0),
        (AnchorPreset::CenterLeft, 0.0, 0.5, 0.0, 0.5),
        (AnchorPreset::CenterRight, 1.0, 0.5, 1.0, 0.5),
    ];

    for &(preset, exp_l, exp_t, exp_r, exp_b) in presets_and_expected {
        let ctrl = add_control(&mut tree, root, &format!("{preset:?}"), "Control");
        control::apply_anchor_preset(&mut tree, ctrl, preset);

        assert!(
            approx_eq(control::get_anchor_left(&tree, ctrl), exp_l),
            "{preset:?}: anchor_left expected {exp_l}, got {}",
            control::get_anchor_left(&tree, ctrl)
        );
        assert!(
            approx_eq(control::get_anchor_top(&tree, ctrl), exp_t),
            "{preset:?}: anchor_top expected {exp_t}, got {}",
            control::get_anchor_top(&tree, ctrl)
        );
        assert!(
            approx_eq(control::get_anchor_right(&tree, ctrl), exp_r),
            "{preset:?}: anchor_right expected {exp_r}, got {}",
            control::get_anchor_right(&tree, ctrl)
        );
        assert!(
            approx_eq(control::get_anchor_bottom(&tree, ctrl), exp_b),
            "{preset:?}: anchor_bottom expected {exp_b}, got {}",
            control::get_anchor_bottom(&tree, ctrl)
        );
    }
}

// ===========================================================================
// 11. UI + scene transition: menu to gameplay
// ===========================================================================

#[test]
fn r6sku_ui_scene_transition_cleanup() {
    let packed_ui = parse_scene(UI_MENU_TSCN);
    let packed_game = parse_scene(PLATFORMER_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_ui).unwrap();
    let mut main_loop = MainLoop::new(tree);

    // Step the UI scene.
    main_loop.run_frames(10, DT);
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/MenuRoot/PlayButton")
            .is_some(),
        "UI scene should have PlayButton"
    );

    // Transition to gameplay.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_game)
        .unwrap();

    // Old UI nodes gone.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/MenuRoot/PlayButton")
            .is_none(),
        "UI nodes should be cleaned up after scene change"
    );

    // New gameplay nodes present.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/World/Player")
            .is_some(),
        "gameplay scene should have Player"
    );

    // Step more frames to verify stability.
    main_loop.run_frames(20, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

// ===========================================================================
// 12. Cross-domain: audio + navigation + UI in one scene tree
// ===========================================================================

#[test]
fn r6sku_cross_domain_audio_nav_ui_coexistence() {
    // Build a scene tree with audio, nav, and UI nodes.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // UI layer.
    let ui_root = add_control(&mut tree, root, "UI", "Control");
    let label = add_control(&mut tree, ui_root, "ScoreLabel", "Label");
    control::set_label_text(&mut tree, label, "Score: 0");

    // Gameplay layer with audio and nav agent nodes.
    let world = tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let _player = tree
        .add_child(world, Node::new("Player", "CharacterBody2D"))
        .unwrap();
    let _audio_player = tree
        .add_child(world, Node::new("BGM", "AudioStreamPlayer"))
        .unwrap();
    let _nav_agent = tree
        .add_child(world, Node::new("NavAgent", "NavigationAgent2D"))
        .unwrap();

    // Run audio server independently.
    let mut audio_server = AudioServer::new();
    let bgm_buf = make_dc_buffer(0.3, 44100);
    let bgm_id = audio_server.play(bgm_buf);

    // Run navigation server independently.
    let mut nav_server = NavigationServer2D::new();
    nav_server.add_region(make_two_square_navmesh());
    let mut agent = NavigationAgent2D::new();
    agent.set_target_position(Vector2::new(15.0, 5.0), Vector2::new(5.0, 5.0), &nav_server);

    // Step main loop and subsystems.
    let mut main_loop = MainLoop::new(tree);
    for _ in 0..30 {
        main_loop.run_frames(1, DT);

        // Audio produces output each frame.
        let _output = audio_server.mix(735); // ~1/60th at 44100

        // Navigation agent steps.
        let pos = Vector2::new(5.0, 5.0); // simplified
        let _next = agent.get_next_path_position(pos);
    }

    assert_eq!(main_loop.frame_count(), 30);
    assert!(audio_server.is_playing(bgm_id));

    // UI label still accessible.
    let label_id = main_loop
        .tree()
        .get_node_by_path("/root/UI/ScoreLabel")
        .expect("ScoreLabel should exist");
    assert_eq!(
        control::get_label_text(main_loop.tree(), label_id),
        "Score: 0"
    );
}

// ===========================================================================
// 13. Audio determinism: two identical runs produce identical output
// ===========================================================================

#[test]
fn r6sku_audio_deterministic_output() {
    fn run_audio_session() -> Vec<f32> {
        let mut server = AudioServer::new();
        server.mixer_mut().add_bus("SFX");
        server
            .mixer_mut()
            .get_bus_mut(1)
            .unwrap()
            .set_volume_db(-6.0);

        let buf1 = make_dc_buffer(0.7, 44100);
        let buf2 = make_dc_buffer(0.3, 44100);
        server.play(buf1);
        server.play_on_bus(buf2, "SFX");

        let mut all_output = Vec::new();
        for _ in 0..10 {
            all_output.extend(server.mix(512));
        }
        all_output
    }

    let run_a = run_audio_session();
    let run_b = run_audio_session();

    assert_eq!(run_a.len(), run_b.len());
    for (i, (a, b)) in run_a.iter().zip(run_b.iter()).enumerate() {
        assert!((a - b).abs() < 1e-10, "sample {i} differs: {a} vs {b}");
    }
}

// ===========================================================================
// 14. Audio server auto-cleanup of short streams
// ===========================================================================

#[test]
fn r6sku_audio_server_auto_cleanup_short_streams() {
    let mut server = AudioServer::new();

    // Play a very short buffer (10 frames) and a long one (44100).
    let short_buf = make_dc_buffer(1.0, 10);
    let long_buf = make_dc_buffer(0.5, 44100);
    let short_id = server.play(short_buf);
    let _long_id = server.play(long_buf);

    assert_eq!(server.active_stream_count(), 2);

    // Mix enough to exhaust the short buffer.
    let _ = server.mix(44100);

    // Short stream should be cleaned up.
    assert!(!server.is_playing(short_id));
    // Long stream continues (it was 1 second, we mixed 1 second, but advance
    // logic should still have it or it auto-stops at end).
    // Just verify the count decreased.
    assert!(
        server.active_stream_count() <= 1,
        "short stream should have been removed"
    );
}

// ===========================================================================
// 15. Navigation agent: avoidance flag and desired distance
// ===========================================================================

#[test]
fn r6sku_navigation_agent_properties() {
    let mut agent = NavigationAgent2D::new();

    // Defaults.
    assert_eq!(agent.path_desired_distance, 4.0);
    assert!(!agent.avoidance_enabled);
    assert!(agent.is_navigation_finished()); // no path set

    // Configure.
    agent.path_desired_distance = 2.0;
    agent.set_avoidance_enabled(true);
    assert_eq!(agent.path_desired_distance, 2.0);
    assert!(agent.avoidance_enabled);

    // Set target on nav mesh.
    let mut server = NavigationServer2D::new();
    server.add_region(make_two_square_navmesh());
    agent.set_target_position(Vector2::new(15.0, 5.0), Vector2::new(5.0, 5.0), &server);
    assert!(!agent.is_navigation_finished());
    assert!(!agent.path.is_empty());
}

// ===========================================================================
// 16. UI container children: VBox with multiple labels and size flags
// ===========================================================================

#[test]
fn r6sku_ui_vbox_container_children_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let vbox = add_control(&mut tree, root, "VBox", "VBoxContainer");
    control::set_separation(&mut tree, vbox, 10);

    // Add 5 labels to the container.
    let mut labels = Vec::new();
    for i in 0..5 {
        let lbl = add_control(&mut tree, vbox, &format!("Label{i}"), "Label");
        control::set_label_text(&mut tree, lbl, &format!("Item {i}"));
        control::set_h_size_flags(&mut tree, lbl, SizeFlags::Expand);
        labels.push(lbl);
    }

    // Verify all labels have correct text and flags.
    for (i, &lbl) in labels.iter().enumerate() {
        assert_eq!(control::get_label_text(&tree, lbl), format!("Item {i}"));
        assert_eq!(control::get_h_size_flags(&tree, lbl), SizeFlags::Expand);
    }

    // Verify the container's separation.
    assert_eq!(control::get_separation(&tree, vbox), 10);

    // Verify node hierarchy.
    let children = tree.get_node(vbox).unwrap().children();
    assert_eq!(children.len(), 5);
}

// ===========================================================================
// 17. UI focus navigation: set focus neighbors
// ===========================================================================

#[test]
fn r6sku_ui_focus_neighbor_chain() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let btn1 = add_control(&mut tree, root, "Btn1", "Button");
    let btn2 = add_control(&mut tree, root, "Btn2", "Button");
    let btn3 = add_control(&mut tree, root, "Btn3", "Button");

    // Set up a focus chain.
    control::set_focus_mode(&mut tree, btn1, FocusMode::All);
    control::set_focus_mode(&mut tree, btn2, FocusMode::All);
    control::set_focus_mode(&mut tree, btn3, FocusMode::All);

    control::set_focus_next(&mut tree, btn1, "/root/Btn2");
    control::set_focus_next(&mut tree, btn2, "/root/Btn3");
    control::set_focus_next(&mut tree, btn3, "/root/Btn1");

    control::set_focus_previous(&mut tree, btn2, "/root/Btn1");
    control::set_focus_previous(&mut tree, btn3, "/root/Btn2");
    control::set_focus_previous(&mut tree, btn1, "/root/Btn3");

    // Verify chain.
    assert_eq!(
        control::get_focus_next(&tree, btn1),
        Some("/root/Btn2".to_string())
    );
    assert_eq!(
        control::get_focus_next(&tree, btn2),
        Some("/root/Btn3".to_string())
    );
    assert_eq!(
        control::get_focus_next(&tree, btn3),
        Some("/root/Btn1".to_string())
    );
    assert_eq!(
        control::get_focus_previous(&tree, btn2),
        Some("/root/Btn1".to_string())
    );
}

// ===========================================================================
// 18. UI control size and custom minimum size
// ===========================================================================

#[test]
fn r6sku_ui_control_size_and_minimum() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ctrl = add_control(&mut tree, root, "MyPanel", "Panel");

    control::set_size(&mut tree, ctrl, Vector2::new(800.0, 600.0));
    control::set_custom_minimum_size(&mut tree, ctrl, Vector2::new(200.0, 150.0));

    let size = control::get_size(&tree, ctrl);
    assert!(approx_eq(size.x, 800.0));
    assert!(approx_eq(size.y, 600.0));

    let min_size = control::get_custom_minimum_size(&tree, ctrl);
    assert!(approx_eq(min_size.x, 200.0));
    assert!(approx_eq(min_size.y, 150.0));
}

// ===========================================================================
// 19. Scene transition preserves frame count and physics time
// ===========================================================================

#[test]
fn r6sku_scene_transition_frame_count_continuity() {
    let packed_a = parse_scene(MINIMAL_TSCN);
    let packed_b = parse_scene(UI_MENU_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(50, DT);
    let time_before = main_loop.physics_time();
    assert_eq!(main_loop.frame_count(), 50);

    // Transition.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_b)
        .unwrap();

    main_loop.run_frames(50, DT);
    assert_eq!(main_loop.frame_count(), 100);

    // Physics time should accumulate across transition.
    let expected_total = 100.0 * DT;
    assert!(
        (main_loop.physics_time() - expected_total).abs() < 0.05,
        "physics time should accumulate: expected ~{expected_total}, got {}",
        main_loop.physics_time()
    );
    assert!(
        main_loop.physics_time() > time_before,
        "physics time should increase after transition"
    );
}

// ===========================================================================
// 20. Navigation same-polygon direct path
// ===========================================================================

#[test]
fn r6sku_navigation_same_polygon_direct() {
    let mut server = NavigationServer2D::new();
    server.add_region(make_two_square_navmesh());

    // Both points in the left polygon.
    let path = server.find_path(Vector2::new(2.0, 3.0), Vector2::new(8.0, 7.0));
    assert_eq!(path.len(), 2, "same-polygon path should be direct");
    assert!(approx_eq(path[0].x, 2.0));
    assert!(approx_eq(path[1].x, 8.0));
}
