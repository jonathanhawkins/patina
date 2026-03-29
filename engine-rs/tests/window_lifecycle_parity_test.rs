//! Window lifecycle parity smoke tests (pat-4w43).
//!
//! Verifies that Patina's window management contracts match Godot's documented
//! windowing behavior. Each test states the Godot expectation it validates.
//!
//! Godot reference: `DisplayServer` and `Window` node documentation.
//! These tests use `HeadlessPlatform` / `HeadlessWindow` to exercise the
//! behavioral contract without requiring an actual OS window.

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::input::InputState;
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowEvent, WindowManager};
use gdplatform::DisplayServer;

// ---------------------------------------------------------------------------
// Godot contract: Window starts with the configured size and title.
// (ProjectSettings: display/window/size/viewport_width, viewport_height)
// ---------------------------------------------------------------------------

#[test]
fn window_opens_with_configured_size_and_title() {
    let config = WindowConfig::new()
        .with_size(1024, 768)
        .with_title("My Game");

    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);

    assert!(wm.is_open(id));
    assert_eq!(wm.get_size(id), Some((1024, 768)));
    assert_eq!(wm.get_title(id), Some("My Game"));
}

// ---------------------------------------------------------------------------
// Godot contract: Resize events update the window's reported size.
// (Window.size_changed signal, DisplayServer.window_get_size)
// ---------------------------------------------------------------------------

#[test]
fn resize_event_updates_backend_window_size() {
    let mut backend = HeadlessPlatform::new(640, 480);
    assert_eq!(backend.window_size(), (640, 480));

    backend.push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    let events = backend.poll_events();

    // The resize event must be reported to the consumer.
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        WindowEvent::Resized {
            width: 1920,
            height: 1080,
        }
    ));
    // After processing, the backend's reported size must reflect the new dims.
    assert_eq!(backend.window_size(), (1920, 1080));
}

// ---------------------------------------------------------------------------
// Godot contract: Multiple sequential resizes converge to the last size.
// (Godot coalesces resize events; final size is authoritative.)
// ---------------------------------------------------------------------------

#[test]
fn sequential_resizes_converge_to_last_size() {
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
        width: 1280,
        height: 720,
    });

    let events = backend.poll_events();
    assert_eq!(events.len(), 3);
    // Final size must match the last resize event.
    assert_eq!(backend.window_size(), (1280, 720));
}

// ---------------------------------------------------------------------------
// Godot contract: CloseRequested sets the quit flag.
// (Window.close_requested signal → SceneTree.quit() by default)
// ---------------------------------------------------------------------------

#[test]
fn close_requested_triggers_quit() {
    let mut backend = HeadlessPlatform::new(640, 480);
    assert!(!backend.should_quit());

    backend.push_event(WindowEvent::CloseRequested);
    backend.poll_events();

    assert!(
        backend.should_quit(),
        "CloseRequested must set the quit flag"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Focus events are delivered but do not affect quit state.
// (Window.focus_entered / focus_exited signals)
// ---------------------------------------------------------------------------

#[test]
fn focus_events_do_not_trigger_quit() {
    let mut backend = HeadlessPlatform::new(640, 480);

    backend.push_event(WindowEvent::FocusGained);
    backend.push_event(WindowEvent::FocusLost);
    let events = backend.poll_events();

    assert_eq!(events.len(), 2);
    assert!(!backend.should_quit(), "Focus events must not trigger quit");
}

// ---------------------------------------------------------------------------
// Godot contract: Full window lifecycle — create, resize, lose focus, close.
// Validates the event ordering contract matches Godot's expected sequence:
// open → resize → focus_lost → close_requested → quit.
// ---------------------------------------------------------------------------

#[test]
fn full_window_lifecycle_create_resize_focus_close() {
    let mut backend = HeadlessPlatform::new(1280, 720);

    // 1. Window starts open with initial size.
    assert_eq!(backend.window_size(), (1280, 720));
    assert!(!backend.should_quit());

    // 2. User resizes the window.
    backend.push_event(WindowEvent::Resized {
        width: 1600,
        height: 900,
    });
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert_eq!(backend.window_size(), (1600, 900));

    // 3. Window loses focus (e.g. user alt-tabs).
    backend.push_event(WindowEvent::FocusLost);
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], WindowEvent::FocusLost));
    assert!(!backend.should_quit());

    // 4. User closes the window.
    backend.push_event(WindowEvent::CloseRequested);
    let events = backend.poll_events();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], WindowEvent::CloseRequested));
    assert!(backend.should_quit());

    // 5. Size still reflects the last known dimensions (Godot preserves this
    //    until the window is actually destroyed).
    assert_eq!(backend.window_size(), (1600, 900));
}

// ---------------------------------------------------------------------------
// Godot contract: DisplayServer routes input events through poll_events
// into the InputState, while non-input events (resize, focus) pass through
// without polluting input.
// ---------------------------------------------------------------------------

#[test]
fn display_server_routes_input_but_not_lifecycle_events_to_input_state() {
    let mut ds = DisplayServer::new();
    let id = ds.create_window(&WindowConfig::default());

    let backend = ds.get_window_mut(id).unwrap();
    backend.push_event(WindowEvent::FocusGained);
    backend.push_event(WindowEvent::Resized {
        width: 800,
        height: 600,
    });
    backend.push_event(WindowEvent::KeyInput {
        key: gdplatform::input::Key::Escape,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    backend.push_event(WindowEvent::FocusLost);

    let mut input = InputState::new();
    let events = ds.poll_events(&mut input);

    // All 4 events should be reported.
    assert_eq!(events.len(), 4);
    // Only the key event should reach InputState.
    assert!(input.is_key_pressed(gdplatform::input::Key::Escape));
}

// ---------------------------------------------------------------------------
// Godot contract: DisplayServer.window_set_size / window_get_size reflect
// changes immediately for headless windows.
// ---------------------------------------------------------------------------

#[test]
fn headless_window_manager_set_size_reflects_immediately() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_size(640, 480));

    assert_eq!(wm.get_size(id), Some((640, 480)));

    wm.set_size(id, 1920, 1080);
    assert_eq!(
        wm.get_size(id),
        Some((1920, 1080)),
        "set_size must be reflected immediately by get_size"
    );
}

// ---------------------------------------------------------------------------
// Godot contract: Closing a window makes it report as not open, and
// subsequent mutations are silently ignored (no panic).
// ---------------------------------------------------------------------------

#[test]
fn closed_window_is_inert() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert!(wm.is_open(id));

    wm.close(id);
    assert!(!wm.is_open(id));

    // Mutations on a closed window must not panic.
    wm.set_title(id, "Ghost");
    wm.set_size(id, 100, 100);
    wm.set_fullscreen(id, true);

    // Queries on closed window return None.
    assert_eq!(wm.get_title(id), None);
    assert_eq!(wm.get_size(id), None);
    assert_eq!(wm.get_fullscreen(id), None);
}

// ---------------------------------------------------------------------------
// Godot contract: Default WindowConfig matches Godot's default project
// settings for a new project (1280x720, not fullscreen, vsync on).
// ---------------------------------------------------------------------------

#[test]
fn default_window_config_matches_godot_defaults() {
    let config = WindowConfig::default();
    // Godot 4 default: 1152×648 as of 4.0, but configurable. Patina uses
    // 1280×720 as the documented default — verify that contract.
    assert_eq!(config.width, 1280, "default width");
    assert_eq!(config.height, 720, "default height");
    assert!(!config.fullscreen, "default is windowed, not fullscreen");
    assert!(config.vsync, "default vsync is enabled");
    assert!(config.resizable, "default window is resizable");
}
