//! pat-v1w: Window lifecycle and resize flow coverage.
//!
//! Tests WindowConfig builder, HeadlessWindow event polling,
//! window resize events, focus events, close flow, and DisplayServer
//! integration.

use gdcore::math::Vector2;
use gdplatform::display::{DisplayServer, VsyncMode};
use gdplatform::input::{InputEvent, InputState, Key, MouseButton};
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowEvent, WindowManager};

// ===========================================================================
// 1. WindowConfig builder patterns
// ===========================================================================

#[test]
fn v1w_config_default_values() {
    let cfg = WindowConfig::new();
    assert_eq!(cfg.width, 1280);
    assert_eq!(cfg.height, 720);
    assert_eq!(cfg.title, "Patina Engine");
    assert!(!cfg.fullscreen);
    assert!(cfg.vsync);
    assert!(cfg.resizable);
}

#[test]
fn v1w_config_builder_chaining() {
    let cfg = WindowConfig::new()
        .with_size(800, 600)
        .with_title("Test Game")
        .with_fullscreen(true)
        .with_vsync(false)
        .with_resizable(false);

    assert_eq!(cfg.width, 800);
    assert_eq!(cfg.height, 600);
    assert_eq!(cfg.title, "Test Game");
    assert!(cfg.fullscreen);
    assert!(!cfg.vsync);
    assert!(!cfg.resizable);
}

#[test]
fn v1w_config_individual_width_height() {
    let cfg = WindowConfig::new().with_width(1920).with_height(1080);
    assert_eq!(cfg.width, 1920);
    assert_eq!(cfg.height, 1080);
}

#[test]
fn v1w_config_clone_and_equality() {
    let a = WindowConfig::new().with_title("Hello").with_size(640, 480);
    let b = a.clone();
    assert_eq!(a, b);
}

// ===========================================================================
// 2. HeadlessWindow creation and queries
// ===========================================================================

#[test]
fn v1w_headless_create_window() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_title("Win A"));
    assert!(wm.is_open(id));
    assert_eq!(wm.get_title(id), Some("Win A"));
    assert_eq!(wm.get_size(id), Some((1280, 720)));
}

#[test]
fn v1w_headless_multiple_windows_unique_ids() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new());
    let id2 = wm.create_window(&WindowConfig::new());
    let id3 = wm.create_window(&WindowConfig::new());
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
    assert!(wm.is_open(id1));
    assert!(wm.is_open(id2));
    assert!(wm.is_open(id3));
}

#[test]
fn v1w_headless_close_window() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new());
    assert!(wm.is_open(id));
    wm.close(id);
    assert!(!wm.is_open(id));
    // Queries on closed window return None
    assert_eq!(wm.get_title(id), None);
    assert_eq!(wm.get_size(id), None);
}

// ===========================================================================
// 3. Window resize events
// ===========================================================================

#[test]
fn v1w_resize_event_polled() {
    let mut wm = HeadlessWindow::new();
    let _id = wm.create_window(&WindowConfig::new());

    wm.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });

    let events = wm.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        WindowEvent::Resized {
            width: 1920,
            height: 1080
        }
    );
}

#[test]
fn v1w_resize_event_not_input_event() {
    // Resize is a window event, NOT an input event.
    let evt = WindowEvent::Resized {
        width: 800,
        height: 600,
    };
    assert!(
        evt.to_input_event().is_none(),
        "Resize events should not convert to InputEvent"
    );
}

#[test]
fn v1w_multiple_resize_events_in_sequence() {
    let mut wm = HeadlessWindow::new();
    let _id = wm.create_window(&WindowConfig::new());

    wm.push_event(WindowEvent::Resized {
        width: 800,
        height: 600,
    });
    wm.push_event(WindowEvent::Resized {
        width: 1024,
        height: 768,
    });
    wm.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });

    let events = wm.poll_events();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[2],
        WindowEvent::Resized {
            width: 1920,
            height: 1080
        }
    );
}

#[test]
fn v1w_set_size_updates_stored_dimensions() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(640, 480));
    assert_eq!(wm.get_size(id), Some((640, 480)));

    wm.set_size(id, 1920, 1080);
    assert_eq!(wm.get_size(id), Some((1920, 1080)));
}

// ===========================================================================
// 4. Focus and close events
// ===========================================================================

#[test]
fn v1w_focus_events_polled() {
    let mut wm = HeadlessWindow::new();
    let _id = wm.create_window(&WindowConfig::new());

    wm.push_event(WindowEvent::FocusGained);
    wm.push_event(WindowEvent::FocusLost);

    let events = wm.poll_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], WindowEvent::FocusGained);
    assert_eq!(events[1], WindowEvent::FocusLost);
}

#[test]
fn v1w_close_requested_event() {
    let mut wm = HeadlessWindow::new();
    let _id = wm.create_window(&WindowConfig::new());

    wm.push_event(WindowEvent::CloseRequested);
    let events = wm.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], WindowEvent::CloseRequested);
}

#[test]
fn v1w_focus_and_close_not_input_events() {
    assert!(WindowEvent::FocusGained.to_input_event().is_none());
    assert!(WindowEvent::FocusLost.to_input_event().is_none());
    assert!(WindowEvent::CloseRequested.to_input_event().is_none());
}

// ===========================================================================
// 5. Event polling drains queue
// ===========================================================================

#[test]
fn v1w_poll_events_drains_queue() {
    let mut wm = HeadlessWindow::new();
    let _id = wm.create_window(&WindowConfig::new());

    wm.push_event(WindowEvent::FocusGained);
    wm.push_event(WindowEvent::CloseRequested);
    assert_eq!(wm.poll_events().len(), 2);
    assert_eq!(wm.poll_events().len(), 0, "second poll should be empty");
}

// ===========================================================================
// 6. Window event to InputEvent conversion
// ===========================================================================

#[test]
fn v1w_key_event_converts_to_input() {
    let evt = WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: true,
        alt: false,
    };
    let input = evt.to_input_event().unwrap();
    match input {
        InputEvent::Key {
            key, pressed, ctrl, ..
        } => {
            assert_eq!(key, Key::Space);
            assert!(pressed);
            assert!(ctrl);
        }
        other => panic!("expected Key event, got {:?}", other),
    }
}

#[test]
fn v1w_mouse_event_converts_to_input() {
    let evt = WindowEvent::MouseInput {
        button: MouseButton::Right,
        pressed: true,
        position: Vector2::new(100.0, 200.0),
    };
    let input = evt.to_input_event().unwrap();
    match input {
        InputEvent::MouseButton {
            button,
            pressed,
            position,
        } => {
            assert_eq!(button, MouseButton::Right);
            assert!(pressed);
            assert_eq!(position, Vector2::new(100.0, 200.0));
        }
        other => panic!("expected MouseButton event, got {:?}", other),
    }
}

#[test]
fn v1w_mouse_motion_converts_to_input() {
    let evt = WindowEvent::MouseMotion {
        position: Vector2::new(50.0, 75.0),
        relative: Vector2::new(2.0, 3.0),
    };
    let input = evt.to_input_event().unwrap();
    match input {
        InputEvent::MouseMotion { position, relative } => {
            assert_eq!(position, Vector2::new(50.0, 75.0));
            assert_eq!(relative, Vector2::new(2.0, 3.0));
        }
        other => panic!("expected MouseMotion event, got {:?}", other),
    }
}

// ===========================================================================
// 7. DisplayServer integration
// ===========================================================================

#[test]
fn v1w_display_server_create_and_poll() {
    let mut ds = DisplayServer::new();
    let cfg = WindowConfig::new()
        .with_title("DS Test")
        .with_size(800, 600);
    let wid = ds.create_window(&cfg);

    assert_eq!(ds.window_count(), 1);

    let win = ds.get_window_mut(wid).unwrap();
    win.push_event(WindowEvent::Resized {
        width: 1024,
        height: 768,
    });

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        WindowEvent::Resized {
            width: 1024,
            height: 768
        }
    );
}

#[test]
fn v1w_display_server_routes_input_events() {
    let mut ds = DisplayServer::new();
    let wid = ds.create_window(&WindowConfig::new());

    let win = ds.get_window_mut(wid).unwrap();
    win.push_event(WindowEvent::KeyInput {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    let mut input = InputState::new();
    let _events = ds.poll_events(&mut input);

    // The DisplayServer should have routed the key event to InputState.
    assert!(
        input.is_key_pressed(Key::A),
        "DisplayServer should route key events to InputState"
    );
}

#[test]
fn v1w_display_server_vsync_modes() {
    let mut ds = DisplayServer::new();
    assert_eq!(ds.vsync(), VsyncMode::Enabled); // default

    ds.set_vsync(VsyncMode::Disabled);
    assert_eq!(ds.vsync(), VsyncMode::Disabled);

    ds.set_vsync(VsyncMode::Adaptive);
    assert_eq!(ds.vsync(), VsyncMode::Adaptive);
}

#[test]
fn v1w_display_server_close_window() {
    let mut ds = DisplayServer::new();
    let wid = ds.create_window(&WindowConfig::new());
    assert_eq!(ds.window_count(), 1);
    assert!(ds.is_open(wid));

    ds.close_window(wid);
    assert!(!ds.is_open(wid));
    assert_eq!(ds.window_count(), 0);
}

// ===========================================================================
// 8. Full lifecycle: create → resize → focus → input → close
// ===========================================================================

#[test]
fn v1w_full_window_lifecycle() {
    let mut ds = DisplayServer::new();
    let wid = ds.create_window(
        &WindowConfig::new()
            .with_title("Lifecycle Test")
            .with_size(1280, 720),
    );
    assert!(ds.is_open(wid));

    // Simulate lifecycle events
    let win = ds.get_window_mut(wid).unwrap();
    win.push_event(WindowEvent::FocusGained);
    win.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    win.push_event(WindowEvent::KeyInput {
        key: Key::Escape,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    win.push_event(WindowEvent::CloseRequested);

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);

    assert_eq!(events.len(), 4);
    assert_eq!(events[0].1, WindowEvent::FocusGained);
    assert!(matches!(events[1].1, WindowEvent::Resized { .. }));
    assert!(matches!(events[2].1, WindowEvent::KeyInput { .. }));
    assert_eq!(events[3].1, WindowEvent::CloseRequested);

    // Key should be routed to input state
    assert!(input.is_key_pressed(Key::Escape));

    // Close the window
    ds.close_window(wid);
    assert!(!ds.is_open(wid));
}

// ===========================================================================
// 9. Resize updates viewport size via HeadlessPlatform
// ===========================================================================

#[test]
fn v1w_resize_event_updates_backend_viewport_size() {
    use gdplatform::backend::{HeadlessPlatform, PlatformBackend};

    let mut backend = HeadlessPlatform::new(640, 480);
    assert_eq!(backend.window_size(), (640, 480));

    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });

    // Before polling, size is still the original.
    assert_eq!(backend.window_size(), (640, 480));

    // Polling the event updates the stored viewport size.
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(backend.window_size(), (1920, 1080));
}

#[test]
fn v1w_multiple_resizes_applies_last() {
    use gdplatform::backend::{HeadlessPlatform, PlatformBackend};

    let mut backend = HeadlessPlatform::new(640, 480);

    backend.push_event(WindowEvent::Resized {
        width: 800,
        height: 600,
    });
    backend.push_event(WindowEvent::Resized {
        width: 1024,
        height: 768,
    });
    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });

    let _events = backend.poll_events();
    assert_eq!(
        backend.window_size(),
        (1920, 1080),
        "viewport should reflect the last resize"
    );
}

// ===========================================================================
// 10. Close triggers quit via HeadlessPlatform
// ===========================================================================

#[test]
fn v1w_close_requested_triggers_quit() {
    use gdplatform::backend::{HeadlessPlatform, PlatformBackend};

    let mut backend = HeadlessPlatform::new(640, 480);
    assert!(!backend.should_quit());

    backend.push_event(WindowEvent::CloseRequested);

    // Before polling, quit is not set.
    assert!(!backend.should_quit());

    // Polling the close event sets the quit flag.
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], WindowEvent::CloseRequested);
    assert!(backend.should_quit(), "close request should trigger quit");
}

#[test]
fn v1w_close_event_stops_main_loop_run() {
    use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
    let mut ml = MainLoop::new(tree);

    let mut backend = HeadlessPlatform::new(640, 480);

    // Run one frame, then inject a close event.
    ml.run_frame(&mut backend, 1.0 / 60.0);
    assert_eq!(ml.frame_count(), 1);
    assert!(!backend.should_quit());

    // Push close event — next poll_events in run_frame will set quit.
    backend.push_event(WindowEvent::CloseRequested);
    ml.run_frame(&mut backend, 1.0 / 60.0);
    assert_eq!(ml.frame_count(), 2);
    assert!(
        backend.should_quit(),
        "should_quit must be true after CloseRequested"
    );

    // A subsequent run() should exit immediately since quit is set.
    ml.run(&mut backend, 1.0 / 60.0);
    assert_eq!(ml.frame_count(), 2, "no more frames after quit");
}

// ===========================================================================
// 11. Resize through MainLoop run_frame updates backend size
// ===========================================================================

#[test]
fn v1w_resize_through_run_frame_updates_viewport() {
    use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
    let mut ml = MainLoop::new(tree);

    let mut backend = HeadlessPlatform::new(640, 480);

    // Push a resize event, run one frame.
    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    ml.run_frame(&mut backend, 1.0 / 60.0);

    // After the frame, viewport size should reflect the resize.
    assert_eq!(
        backend.window_size(),
        (1920, 1080),
        "run_frame should cause resize event to update viewport"
    );
}

// ===========================================================================
// 12. Operations on closed window are safe no-ops
// ===========================================================================

#[test]
fn v1w_operations_on_closed_window_safe() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new());
    wm.close(id);

    // These should not panic
    wm.set_title(id, "Should not crash");
    wm.set_size(id, 100, 100);
    wm.set_fullscreen(id, true);

    assert!(!wm.is_open(id));
    assert_eq!(wm.get_title(id), None);
    assert_eq!(wm.get_size(id), None);
    assert_eq!(wm.get_fullscreen(id), None);
}
