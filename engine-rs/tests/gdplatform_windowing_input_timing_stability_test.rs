//! pat-1i3: Stabilize gdplatform windowing, input, and timing layer.
//!
//! Integration tests that exercise the cross-cutting contracts between
//! window management, input routing, and frame timing:
//!
//! 1. Timer integration with HeadlessPlatform frame stepping
//! 2. InputState coherence across frame boundaries
//! 3. WindowEvent → InputEvent → InputState → Action full pipeline
//! 4. Focus loss/gain interaction with input state
//! 5. DisplayServer multi-window input isolation
//! 6. Timer edge cases (zero wait, restart, negative delta)
//! 7. HeadlessPlatform max_frames + close event interaction
//! 8. Platform lifecycle: create → resize → input → timer → close

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::display::{DisplayServer, VsyncMode};
use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
};
use gdplatform::time::Timer;
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowEvent, WindowManager};
use gdcore::math::Vector2;

// ===========================================================================
// 1. Timer + platform frame stepping
// ===========================================================================

#[test]
fn timer_fires_after_enough_platform_frames() {
    let mut backend = HeadlessPlatform::new(640, 480);
    let mut timer = Timer::new(1.0);
    timer.start();

    let dt = 1.0 / 60.0;
    let mut fired_at_frame = None;

    for frame in 0..120 {
        backend.end_frame();
        if timer.step(dt) {
            fired_at_frame = Some(frame);
            break;
        }
    }

    // Timer(1.0) at 60fps should fire around frame 60.
    let frame = fired_at_frame.expect("timer must fire within 120 frames");
    assert!(
        (58..=62).contains(&frame),
        "expected fire around frame 60, got {frame}"
    );
    assert_eq!(backend.frames_run(), frame as u64 + 1);
}

#[test]
fn repeating_timer_fires_multiple_times_across_frames() {
    let mut backend = HeadlessPlatform::new(640, 480);
    let mut timer = Timer::new(0.5);
    timer.start();

    let dt = 1.0 / 60.0;
    let mut fire_count = 0;

    for _ in 0..120 {
        backend.end_frame();
        if timer.step(dt) {
            fire_count += 1;
        }
    }

    // 120 frames at 1/60 = 2.0 seconds, timer period 0.5s → ~4 fires.
    assert!(
        (3..=5).contains(&fire_count),
        "expected ~4 fires in 2s, got {fire_count}"
    );
}

#[test]
fn one_shot_timer_fires_once_then_stops() {
    let mut backend = HeadlessPlatform::new(640, 480);
    let mut timer = Timer::new(0.5).with_one_shot(true);
    timer.start();

    let dt = 1.0 / 60.0;
    let mut fire_count = 0;

    for _ in 0..120 {
        backend.end_frame();
        if timer.step(dt) {
            fire_count += 1;
        }
    }

    assert_eq!(fire_count, 1, "one-shot timer must fire exactly once");
    assert!(timer.is_stopped());
}

// ===========================================================================
// 2. InputState coherence across frame boundaries
// ===========================================================================

#[test]
fn just_pressed_clears_after_flush() {
    let mut input = InputState::new();
    input.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(input.is_key_just_pressed(Key::Space));
    assert!(input.is_key_pressed(Key::Space));

    input.flush_frame();

    assert!(!input.is_key_just_pressed(Key::Space), "just_pressed must clear after flush");
    assert!(input.is_key_pressed(Key::Space), "pressed must persist until release");
}

#[test]
fn release_then_flush_clears_just_released() {
    let mut input = InputState::new();
    // Press
    input.process_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    input.flush_frame();

    // Release
    input.process_event(InputEvent::Key {
        key: Key::W,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(input.is_key_just_released(Key::W));
    assert!(!input.is_key_pressed(Key::W));

    input.flush_frame();
    assert!(!input.is_key_just_released(Key::W), "just_released must clear after flush");
}

#[test]
fn multiple_keys_tracked_independently() {
    let mut input = InputState::new();
    input.process_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    input.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(input.is_key_pressed(Key::W));
    assert!(input.is_key_pressed(Key::Space));
    assert!(!input.is_key_pressed(Key::A));

    // Release only W
    input.flush_frame();
    input.process_event(InputEvent::Key {
        key: Key::W,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(!input.is_key_pressed(Key::W));
    assert!(input.is_key_pressed(Key::Space), "Space must still be pressed");
}

// ===========================================================================
// 3. WindowEvent → InputEvent → InputState → Action full pipeline
// ===========================================================================

#[test]
fn full_pipeline_window_event_to_action() {
    let mut input = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    input.set_input_map(map);

    let window_event = WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };

    let input_event = window_event.to_input_event().expect("key event must convert");
    input.process_event(input_event);

    assert!(input.is_key_pressed(Key::Space));
    assert!(input.is_action_pressed("jump"));
    assert!(input.is_action_just_pressed("jump"));
    assert!((input.get_action_strength("jump") - 1.0).abs() < f32::EPSILON);
}

#[test]
fn full_pipeline_mouse_event_to_action() {
    let mut input = InputState::new();
    let mut map = InputMap::new();
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));
    input.set_input_map(map);

    let window_event = WindowEvent::MouseInput {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(100.0, 200.0),
    };
    let input_event = window_event.to_input_event().unwrap();
    input.process_event(input_event);

    assert!(input.is_mouse_button_pressed(MouseButton::Left));
    assert!(input.is_action_pressed("shoot"));
    assert_eq!(input.get_mouse_position(), Vector2::new(100.0, 200.0));
}

#[test]
fn mouse_motion_updates_position_without_action() {
    let mut input = InputState::new();

    let window_event = WindowEvent::MouseMotion {
        position: Vector2::new(300.0, 400.0),
        relative: Vector2::new(10.0, -5.0),
    };
    let input_event = window_event.to_input_event().unwrap();
    input.process_event(input_event);

    assert_eq!(input.get_mouse_position(), Vector2::new(300.0, 400.0));
}

// ===========================================================================
// 4. Focus loss/gain interaction with input state
// ===========================================================================

#[test]
fn focus_events_do_not_produce_input_events() {
    assert!(WindowEvent::FocusGained.to_input_event().is_none());
    assert!(WindowEvent::FocusLost.to_input_event().is_none());
}

#[test]
fn display_server_focus_events_pass_through_without_polluting_input() {
    let mut ds = DisplayServer::new();
    let id = ds.create_window(&WindowConfig::default());
    let backend = ds.get_window_mut(id).unwrap();

    backend.push_event(WindowEvent::FocusGained);
    backend.push_event(WindowEvent::KeyInput {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    backend.push_event(WindowEvent::FocusLost);

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);

    assert_eq!(events.len(), 3, "all 3 events must be reported");
    assert!(input.is_key_pressed(Key::A), "key press must be registered");
    assert!(!input.is_key_pressed(Key::Escape));
}

// ===========================================================================
// 5. DisplayServer multi-window input isolation
// ===========================================================================

#[test]
fn multi_window_input_events_merge_into_single_input_state() {
    let mut ds = DisplayServer::new();
    let id1 = ds.create_window(&WindowConfig::new().with_title("Win1"));
    let id2 = ds.create_window(&WindowConfig::new().with_title("Win2"));

    ds.get_window_mut(id1).unwrap().push_event(WindowEvent::KeyInput {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ds.get_window_mut(id2).unwrap().push_event(WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);

    assert_eq!(events.len(), 2);
    assert!(input.is_key_pressed(Key::W));
    assert!(input.is_key_pressed(Key::Space));
}

#[test]
fn closing_one_window_does_not_affect_other_windows_input() {
    let mut ds = DisplayServer::new();
    let id1 = ds.create_window(&WindowConfig::new().with_title("Win1"));
    let id2 = ds.create_window(&WindowConfig::new().with_title("Win2"));

    ds.close_window(id1);
    assert!(!ds.is_open(id1));
    assert!(ds.is_open(id2));

    ds.get_window_mut(id2).unwrap().push_event(WindowEvent::KeyInput {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);
    assert_eq!(events.len(), 1);
    assert!(input.is_key_pressed(Key::A));
}

// ===========================================================================
// 6. Timer edge cases
// ===========================================================================

#[test]
fn timer_zero_wait_time_fires_immediately() {
    let mut timer = Timer::new(0.0);
    timer.start();
    assert!(timer.step(0.001));
}

#[test]
fn timer_restart_resets_countdown() {
    let mut timer = Timer::new(1.0).with_one_shot(true);
    timer.start();
    timer.step(0.5);
    assert!(!timer.timeout());

    timer.start();
    assert!(!timer.step(0.5));
    assert!(timer.step(0.6));
}

#[test]
fn timer_negative_delta_does_not_fire() {
    let mut timer = Timer::new(1.0);
    timer.start();
    assert!(!timer.step(-0.5));
    assert!(!timer.timeout());
    assert!(!timer.step(1.0)); // net: -0.5 + 1.0 = 0.5 < 1.0
    assert!(timer.step(0.6)); // net: 1.1 ≥ 1.0 → fires
}

#[test]
fn timer_autostart_fires_without_explicit_start() {
    let mut timer = Timer::new(0.5).with_autostart(true);
    assert!(!timer.is_stopped());
    assert!(timer.step(0.6));
}

#[test]
fn timer_stop_then_step_does_nothing() {
    let mut timer = Timer::new(1.0);
    timer.start();
    timer.step(0.5);
    timer.stop();
    assert!(!timer.step(1.0), "stopped timer must not fire");
    assert!(timer.is_stopped());
}

// ===========================================================================
// 7. HeadlessPlatform max_frames + close event interaction
// ===========================================================================

#[test]
fn max_frames_triggers_quit_before_close_event() {
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(3);

    backend.end_frame();
    backend.end_frame();
    assert!(!backend.should_quit());
    backend.end_frame();
    assert!(backend.should_quit(), "should quit after max_frames reached");
}

#[test]
fn close_event_and_max_frames_both_trigger_quit() {
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(100);

    backend.push_event(WindowEvent::CloseRequested);
    backend.poll_events();
    assert!(backend.should_quit());
    assert_eq!(backend.frames_run(), 0);
}

#[test]
fn request_quit_is_immediate() {
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(100);
    assert!(!backend.should_quit());
    backend.request_quit();
    assert!(backend.should_quit());
}

// ===========================================================================
// 8. Full platform lifecycle: create → resize → input → timer → close
// ===========================================================================

#[test]
fn full_platform_lifecycle() {
    let mut backend = HeadlessPlatform::new(1280, 720);
    let mut input = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    input.set_input_map(map);

    let mut timer = Timer::new(0.5);
    timer.start();

    let dt = 1.0 / 60.0;

    // Phase 1: initial state
    assert_eq!(backend.window_size(), (1280, 720));
    assert!(!backend.should_quit());

    // Phase 2: resize event
    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(backend.window_size(), (1920, 1080));

    // Phase 3: input events
    backend.push_event(WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    for event in backend.poll_events() {
        if let Some(ie) = event.to_input_event() {
            input.process_event(ie);
        }
    }
    assert!(input.is_action_pressed("jump"));
    backend.end_frame();
    input.flush_frame();

    // Phase 4: timer fires after enough frames
    let mut timer_fired = false;
    for _ in 0..60 {
        backend.end_frame();
        if timer.step(dt) {
            timer_fired = true;
            break;
        }
    }
    assert!(timer_fired, "timer must fire within the frame loop");

    // Phase 5: close
    backend.push_event(WindowEvent::CloseRequested);
    backend.poll_events();
    assert!(backend.should_quit());
}

// ===========================================================================
// 9. VsyncMode configuration
// ===========================================================================

#[test]
fn vsync_mode_defaults_to_enabled() {
    let ds = DisplayServer::new();
    assert_eq!(ds.vsync(), VsyncMode::Enabled);
}

#[test]
fn vsync_mode_round_trips() {
    let mut ds = DisplayServer::new();
    for mode in [VsyncMode::Disabled, VsyncMode::Adaptive, VsyncMode::Enabled] {
        ds.set_vsync(mode);
        assert_eq!(ds.vsync(), mode);
    }
}

// ===========================================================================
// 10. InputSnapshot pipeline integration
// ===========================================================================

#[test]
fn input_snapshot_captures_current_state() {
    let mut input = InputState::new();
    input.process_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    input.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(50.0, 75.0),
    });

    let snap = input.snapshot();
    assert!(snap.is_key_pressed(Key::W));
    assert!(snap.is_key_just_pressed(Key::W));
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
    assert_eq!(snap.get_mouse_position(), Vector2::new(50.0, 75.0));

    input.flush_frame();
    let snap2 = input.snapshot();
    assert!(snap2.is_key_pressed(Key::W), "pressed persists");
    assert!(!snap2.is_key_just_pressed(Key::W), "just_pressed cleared");
}

// ===========================================================================
// 11. Gamepad input through InputState
// ===========================================================================

#[test]
fn gamepad_button_press_and_release_cycle() {
    let mut input = InputState::new();

    input.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(input.is_gamepad_button_pressed(0, GamepadButton::FaceA));

    input.flush_frame();

    input.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: false,
        gamepad_id: 0,
    });
    assert!(!input.is_gamepad_button_pressed(0, GamepadButton::FaceA));
}

#[test]
fn gamepad_axis_tracks_value() {
    let mut input = InputState::new();

    input.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.75,
        gamepad_id: 0,
    });
    assert!((input.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.75).abs() < f32::EPSILON);
    assert!((input.get_gamepad_axis_value(0, GamepadAxis::RightStickY)).abs() < f32::EPSILON);
}

// ===========================================================================
// 12. Touch input through InputState
// ===========================================================================

#[test]
fn touch_press_and_release_cycle() {
    let mut input = InputState::new();

    input.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(100.0, 200.0),
        pressed: true,
    });
    assert!(input.is_touch_pressed(0));
    assert!(input.is_touch_just_pressed(0));
    assert_eq!(input.get_touch_position(0), Vector2::new(100.0, 200.0));
    assert_eq!(input.get_touch_count(), 1);

    input.flush_frame();
    assert!(!input.is_touch_just_pressed(0));

    input.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(100.0, 200.0),
        pressed: false,
    });
    assert!(!input.is_touch_pressed(0));
    assert!(input.is_touch_just_released(0));
    assert_eq!(input.get_touch_count(), 0);
}

// ===========================================================================
// 13. Action axis and vector helpers
// ===========================================================================

#[test]
fn get_axis_returns_difference_of_action_strengths() {
    let mut input = InputState::new();
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.add_action("move_right", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
    input.set_input_map(map);

    input.process_event(InputEvent::Key {
        key: Key::Right,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!((input.get_axis("move_left", "move_right") - 1.0).abs() < f32::EPSILON);

    input.process_event(InputEvent::Key {
        key: Key::Left,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!((input.get_axis("move_left", "move_right")).abs() < f32::EPSILON);
}

#[test]
fn get_vector_normalizes_diagonal() {
    let mut input = InputState::new();
    let mut map = InputMap::new();
    map.add_action("left", 0.0);
    map.add_action("right", 0.0);
    map.add_action("up", 0.0);
    map.add_action("down", 0.0);
    map.action_add_event("right", ActionBinding::KeyBinding(Key::Right));
    map.action_add_event("down", ActionBinding::KeyBinding(Key::Down));
    input.set_input_map(map);

    input.process_event(InputEvent::Key {
        key: Key::Right,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    input.process_event(InputEvent::Key {
        key: Key::Down,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    let v = input.get_vector("left", "right", "up", "down");
    let len = (v.x * v.x + v.y * v.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.01,
        "diagonal vector must be normalized, got length {len}"
    );
}

// ===========================================================================
// 14. HeadlessWindow config round-trips
// ===========================================================================

#[test]
fn headless_window_config_fullscreen_round_trip() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_fullscreen(true));
    assert_eq!(wm.get_fullscreen(id), Some(true));
    wm.set_fullscreen(id, false);
    assert_eq!(wm.get_fullscreen(id), Some(false));
}

#[test]
fn headless_window_unique_ids() {
    let mut wm = HeadlessWindow::new();
    let mut ids = Vec::new();
    for i in 0..10 {
        let id = wm.create_window(&WindowConfig::new().with_title(format!("Win{i}")));
        assert!(!ids.contains(&id), "window IDs must be unique");
        ids.push(id);
    }
}

// ===========================================================================
// 15. InputMap management (erase, has_action)
// ===========================================================================

#[test]
fn input_map_erase_action_removes_completely() {
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    assert!(map.has_action("jump"));

    map.erase_action("jump");
    assert!(!map.has_action("jump"));
    assert!(map.get_bindings("jump").is_none());
}

#[test]
fn input_map_action_erase_events_keeps_action() {
    let mut map = InputMap::new();
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));
    assert_eq!(map.get_bindings("shoot").unwrap().len(), 1);

    map.action_erase_events("shoot");
    assert!(map.has_action("shoot"), "action must still exist");
    assert_eq!(map.get_bindings("shoot").unwrap().len(), 0, "bindings must be empty");
}

#[test]
fn input_map_action_count() {
    let mut map = InputMap::new();
    assert_eq!(map.action_count(), 0);
    map.add_action("a", 0.0);
    map.add_action("b", 0.0);
    assert_eq!(map.action_count(), 2);
    map.erase_action("a");
    assert_eq!(map.action_count(), 1);
}

// ===========================================================================
// 16. Key name round-trip
// ===========================================================================

#[test]
fn key_name_from_name_round_trip() {
    let keys_to_test = [
        Key::A, Key::Z, Key::Space, Key::Enter, Key::Escape,
        Key::F1, Key::F12, Key::Up, Key::Down, Key::Left, Key::Right,
        Key::Num0, Key::Num9, Key::Tab, Key::Shift, Key::Ctrl, Key::Alt,
    ];

    for key in &keys_to_test {
        let name = key.name();
        let parsed = Key::from_name(name);
        assert_eq!(
            parsed,
            Some(*key),
            "Key::{:?}.name() = {:?}, from_name({:?}) = {:?}",
            key, name, name, parsed
        );
    }
}

#[test]
fn key_from_name_unknown_returns_none() {
    assert!(Key::from_name("UnknownKey").is_none());
    assert!(Key::from_name("").is_none());
}
