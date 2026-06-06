//! pat-xbas: Startup packaging flow and supported-target CI matrix gate tests.
//!
//! Validates that the engine's startup and packaging flow works correctly
//! across all supported desktop targets. These tests serve as CI gates to
//! ensure the engine initializes, runs, and shuts down cleanly on every
//! platform in the supported-target matrix (Linux, macOS, Windows).
//!
//! Coverage areas:
//!   1. HeadlessPlatform initialization — default and from WindowConfig
//!   2. MainLoop startup and teardown — clean lifecycle without panics
//!   3. Platform target detection — compile-time target assertions
//!   4. WindowConfig builder — all configuration paths produce valid state
//!   5. Release-mode startup invariants — no debug-only dependencies
//!   6. Multi-resolution startup — various window sizes initialize correctly
//!   7. Graceful shutdown — close events trigger clean exit
//!   8. Frame pacing — max_frames limit works across backends
//!   9. Event pipeline startup — input events route correctly from first frame

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowManager};
use gdscene::main_loop::MainLoop;
use gdscene::scene_tree::SceneTree;

const DT: f64 = 1.0 / 60.0;

// ===========================================================================
// 1. HeadlessPlatform initialization
// ===========================================================================

#[test]
fn headless_platform_initializes_with_default_dimensions() {
    let platform = HeadlessPlatform::new(640, 480);
    assert_eq!(platform.window_size(), (640, 480));
    assert!(!platform.should_quit());
    assert_eq!(platform.frames_run(), 0);
}

#[test]
fn headless_platform_initializes_from_window_config() {
    let config = WindowConfig::default();
    let platform = HeadlessPlatform::from_config(&config);
    assert_eq!(platform.window_size(), (config.width, config.height));
    assert!(!platform.should_quit());
}

#[test]
fn headless_platform_custom_config_dimensions() {
    let config = WindowConfig {
        width: 1920,
        height: 1080,
        ..WindowConfig::default()
    };
    let platform = HeadlessPlatform::from_config(&config);
    assert_eq!(platform.window_size(), (1920, 1080));
}

// ===========================================================================
// 2. MainLoop startup and teardown
// ===========================================================================

#[test]
fn mainloop_startup_empty_tree_no_panic() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(1);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 1);
}

#[test]
fn mainloop_startup_and_immediate_quit() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(0);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 0);
    assert!(backend.should_quit());
}

#[test]
fn mainloop_multi_frame_startup_teardown() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(800, 600).with_max_frames(120);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 120);
    assert_eq!(backend.frames_run(), 120);
    assert!(backend.should_quit());
}

// ===========================================================================
// 3. Platform target detection (compile-time gates)
// ===========================================================================

#[test]
fn platform_target_is_supported_desktop() {
    // This test verifies that the current compilation target is one of
    // the supported desktop platforms. If this test compiles and runs,
    // the target is valid.
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let supported_os = ["linux", "macos", "windows"];
    let supported_arch = ["x86_64", "aarch64"];

    assert!(
        supported_os.contains(&os),
        "unsupported OS for Patina engine: {os}"
    );
    assert!(
        supported_arch.contains(&arch),
        "unsupported architecture for Patina engine: {arch}"
    );
}

#[test]
fn platform_target_family_is_unix_or_windows() {
    // Ensures we're on a supported target family
    let is_unix = cfg!(target_family = "unix");
    let is_windows = cfg!(target_family = "windows");
    assert!(
        is_unix || is_windows,
        "target family must be unix or windows"
    );
}

// ===========================================================================
// 4. WindowConfig builder validation
// ===========================================================================

#[test]
fn window_config_default_has_valid_dimensions() {
    let config = WindowConfig::default();
    assert!(config.width > 0, "default width must be positive");
    assert!(config.height > 0, "default height must be positive");
}

#[test]
fn window_config_title_preserved() {
    let config = WindowConfig {
        title: "Patina Engine".to_string(),
        ..WindowConfig::default()
    };
    assert_eq!(config.title, "Patina Engine");
}

#[test]
fn headless_window_manager_creates_and_closes() {
    let config = WindowConfig::default();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    assert!(wm.is_open(id));
    wm.close(id);
    assert!(!wm.is_open(id));
}

#[test]
fn headless_window_manager_set_title() {
    let config = WindowConfig::default();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&config);
    wm.set_title(id, "Test Window");
    assert_eq!(wm.get_title(id), Some("Test Window"));
}

// ===========================================================================
// 5. Release-mode startup invariants
// ===========================================================================

#[test]
fn startup_no_debug_assertions_dependency() {
    // Verify the engine can initialize without depending on debug_assertions.
    // This test runs in both debug and release CI builds.
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(5);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 5);
}

// ===========================================================================
// 6. Multi-resolution startup
// ===========================================================================

#[test]
fn startup_minimum_resolution() {
    let mut backend = HeadlessPlatform::new(320, 240);
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frame(&mut backend, DT);
    assert_eq!(backend.window_size(), (320, 240));
    assert_eq!(main_loop.frame_count(), 1);
}

#[test]
fn startup_4k_resolution() {
    let mut backend = HeadlessPlatform::new(3840, 2160);
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frame(&mut backend, DT);
    assert_eq!(backend.window_size(), (3840, 2160));
    assert_eq!(main_loop.frame_count(), 1);
}

#[test]
fn startup_various_resolutions_all_succeed() {
    let resolutions = [
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 720),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
    ];

    for (w, h) in resolutions {
        let mut backend = HeadlessPlatform::new(w, h);
        let tree = SceneTree::new();
        let mut main_loop = MainLoop::new(tree);
        main_loop.run_frame(&mut backend, DT);
        assert_eq!(
            backend.window_size(),
            (w, h),
            "resolution {w}x{h} should initialize correctly"
        );
    }
}

// ===========================================================================
// 7. Graceful shutdown
// ===========================================================================

#[test]
fn close_event_stops_run_loop() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    // Run a few frames
    for _ in 0..5 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 5);

    // Inject close and run one more frame to process it
    backend.push_event(gdplatform::window::WindowEvent::CloseRequested);
    main_loop.run_frame(&mut backend, DT);
    assert!(backend.should_quit());

    // run() should not advance further
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 6);
}

#[test]
fn request_quit_stops_run_loop() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    main_loop.run_frame(&mut backend, DT);
    backend.request_quit();
    assert!(backend.should_quit());

    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 1, "should not advance after quit");
}

// ===========================================================================
// 8. Frame pacing / max_frames limit
// ===========================================================================

#[test]
fn max_frames_limit_exact() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(10);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 10);
    assert_eq!(backend.frames_run(), 10);
}

#[test]
fn max_frames_one_produces_single_frame() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(1);
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 1);
}

// ===========================================================================
// 9. Event pipeline startup — input events from first frame
// ===========================================================================

#[test]
fn input_events_route_from_first_frame() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    // Push a key event before the first frame
    backend.push_event(gdplatform::window::WindowEvent::KeyInput {
        key: gdplatform::input::Key::Escape,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    main_loop.run_frame(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 1);
    // No panic = input pipeline initialized and routed correctly
}

#[test]
fn resize_event_updates_backend_on_first_frame() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(640, 480);

    backend.push_event(gdplatform::window::WindowEvent::Resized {
        width: 1024,
        height: 768,
    });

    main_loop.run_frame(&mut backend, DT);
    assert_eq!(backend.window_size(), (1024, 768));
}

// ===========================================================================
// 10. CI matrix target validation
// ===========================================================================

#[test]
fn ci_target_triple_is_known() {
    // Validates that the compiled target is one of the CI matrix targets.
    // This test will pass on all supported platforms and fail on unsupported ones.
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Build a target identifier matching CI matrix expectations
    let target_id = format!("{os}-{arch}");
    let known_targets = [
        "linux-x86_64",
        "linux-aarch64",
        "macos-x86_64",
        "macos-aarch64",
        "windows-x86_64",
        "windows-aarch64",
    ];

    assert!(
        known_targets.contains(&target_id.as_str()),
        "CI target {target_id} is not in the supported matrix: {known_targets:?}"
    );
}
