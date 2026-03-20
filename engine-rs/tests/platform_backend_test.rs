//! Integration tests for PlatformBackend + MainLoop.
//!
//! Verifies that MainLoop::run_frame() and MainLoop::run() correctly integrate
//! with PlatformBackend implementations, using HeadlessPlatform.

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::input::{ActionBinding, Key};
use gdplatform::window::WindowEvent;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

fn make_main_loop() -> MainLoop {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = Node::new("Player", "Node2D");
    tree.add_child(root, child).unwrap();
    MainLoop::new(tree)
}

#[test]
fn run_frame_steps_once_and_increments_backend_frame() {
    let mut ml = make_main_loop();
    let mut backend = HeadlessPlatform::new(640, 480);

    let output = ml.run_frame(&mut backend, 1.0 / 60.0);

    assert_eq!(output.frame_count, 1);
    assert_eq!(ml.frame_count(), 1);
    assert_eq!(backend.frames_run(), 1);
}

#[test]
fn run_with_max_frames_stops_at_limit() {
    let mut ml = make_main_loop();
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(60);

    ml.run(&mut backend, 1.0 / 60.0);

    assert_eq!(ml.frame_count(), 60);
    assert_eq!(backend.frames_run(), 60);
    assert!(backend.should_quit());
}

#[test]
fn run_frame_routes_key_events_into_main_loop_input() {
    let mut ml = make_main_loop();
    let mut input_map = gdplatform::input::InputMap::new();
    input_map.add_action("jump", 0.0);
    input_map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    ml.set_input_map(input_map);

    let mut backend = HeadlessPlatform::new(640, 480);

    // Push a key press event for frame 0.
    backend.push_event(WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    // After run_frame, the key should have been processed and flushed.
    ml.run_frame(&mut backend, 1.0 / 60.0);
    assert_eq!(ml.frame_count(), 1);

    // just_pressed is cleared after the frame, but pressed persists
    // (key was not released).
    assert!(!ml.input_state().is_key_just_pressed(Key::Space));
    assert!(ml.input_state().is_key_pressed(Key::Space));
}

#[test]
fn run_frame_ignores_non_input_window_events() {
    let mut ml = make_main_loop();
    let mut backend = HeadlessPlatform::new(640, 480);

    backend.push_event(WindowEvent::FocusGained);
    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    backend.push_event(WindowEvent::CloseRequested);

    ml.run_frame(&mut backend, 1.0 / 60.0);
    assert_eq!(ml.frame_count(), 1);
    // No keys pressed — non-input events were silently skipped.
    assert!(!ml.input_state().is_key_pressed(Key::Space));
}

#[test]
fn run_processes_multiple_input_events_per_frame() {
    let mut ml = make_main_loop();
    let mut input_map = gdplatform::input::InputMap::new();
    input_map.add_action("move_right", 0.0);
    input_map.add_action("jump", 0.0);
    input_map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
    input_map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    ml.set_input_map(input_map);

    let mut backend = HeadlessPlatform::new(640, 480);

    // Push two key presses in the same frame.
    backend.push_event(WindowEvent::KeyInput {
        key: Key::Right,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    backend.push_event(WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    ml.run_frame(&mut backend, 1.0 / 60.0);

    // Both keys should be pressed after the frame.
    assert!(ml.input_state().is_key_pressed(Key::Right));
    assert!(ml.input_state().is_key_pressed(Key::Space));
}

#[test]
fn headless_platform_window_size_updates() {
    let mut backend = HeadlessPlatform::new(640, 480);
    assert_eq!(backend.window_size(), (640, 480));

    backend.set_size(1920, 1080);
    assert_eq!(backend.window_size(), (1920, 1080));
}

#[test]
fn run_quit_request_stops_immediately() {
    let mut ml = make_main_loop();
    let mut backend = HeadlessPlatform::new(640, 480);
    backend.request_quit();

    ml.run(&mut backend, 1.0 / 60.0);

    assert_eq!(ml.frame_count(), 0);
    assert!(backend.should_quit());
}

#[test]
fn headless_backend_from_window_config() {
    let config = gdplatform::WindowConfig::new()
        .with_size(800, 600)
        .with_title("Test");
    let backend = HeadlessPlatform::from_config(&config);
    assert_eq!(backend.window_size(), (800, 600));
    assert!(!backend.should_quit());
}

#[test]
fn run_physics_ticks_accumulate_through_backend() {
    let mut ml = make_main_loop();
    ml.set_physics_ticks_per_second(60);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(60);

    ml.run(&mut backend, 1.0 / 60.0);

    // 60 frames at 1/60s each = 1 physics tick per frame = 60 total.
    assert_eq!(ml.frame_count(), 60);
    // Physics time should be approximately 1 second.
    assert!((ml.physics_time() - 1.0).abs() < 0.001);
}
