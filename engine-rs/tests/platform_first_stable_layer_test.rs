//! pat-teqa0: gdplatform first stable layer integration test.
//!
//! Source of truth: `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`
//! Classification: Measured (headless stable layer composition)
//!
//! Validates Phase 7 exit criteria:
//! - Runtime can be built and run in a repeatable way
//! - Platform-specific code remains isolated behind traits
//! - Full lifecycle: init → configure → frame loop → input → shutdown
//!
//! This test proves that all gdplatform subsystems compose correctly
//! into a working runtime without requiring any OS window or GPU.

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::clipboard::{Clipboard, HeadlessClipboard};
use gdplatform::cursor::{CursorManager, CursorShape};
use gdplatform::display::DisplayServer;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key, MouseButton};
use gdplatform::os::{current_platform, get_ticks_msec, get_ticks_usec, is_debug_build, OsInfo};
use gdplatform::platform_targets::{
    ci_tested_targets, current_target, supports_capability, validate_current_target,
    Architecture, PlatformCapability,
};
use gdplatform::thread::{GodotMutex, GodotSemaphore, GodotThread, WorkerThreadPool};
use gdplatform::time::Timer;
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowEvent, WindowManager};
use gdcore::math::Vector2;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

// ===========================================================================
// 1. Full runtime lifecycle: init → frame loop → shutdown
// ===========================================================================

#[test]
fn full_headless_runtime_lifecycle() {
    // Phase 1: Platform initialization
    let config = WindowConfig::new()
        .with_size(1280, 720)
        .with_title("Patina Headless")
        .with_vsync(true);
    let mut platform = HeadlessPlatform::from_config(&config).with_max_frames(60);
    let mut input = InputState::new();
    let mut timer = Timer::new(0.5);
    timer.start();

    // Phase 2: Validate initial state
    assert!(!platform.should_quit());
    assert_eq!(platform.window_size(), (1280, 720));
    assert_eq!(platform.frames_run(), 0);

    // Phase 3: Run a frame loop
    let dt = 1.0 / 60.0;
    let mut timer_fired = false;
    let mut frames_completed = 0u64;

    while !platform.should_quit() {
        // Poll events
        let events = platform.poll_events();
        for event in &events {
            if let Some(input_event) = event.to_input_event() {
                input.process_event(input_event);
            }
        }

        // Step timer
        if timer.step(dt) {
            timer_fired = true;
        }

        // End frame
        platform.end_frame();
        frames_completed += 1;

        // Inject some events at specific frames
        if frames_completed == 10 {
            platform.push_event(WindowEvent::KeyInput {
                key: Key::Escape,
                pressed: true,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
        if frames_completed == 11 {
            platform.push_event(WindowEvent::KeyInput {
                key: Key::Escape,
                pressed: false,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
    }

    // Phase 4: Validate post-loop state
    assert_eq!(frames_completed, 60, "should run exactly 60 frames");
    assert!(timer_fired, "timer(0.5s) should fire within 60 frames at 60fps");
    assert!(platform.should_quit());
}

// ===========================================================================
// 2. Platform isolation: traits allow swapping backends
// ===========================================================================

#[test]
fn platform_backend_trait_is_object_safe() {
    // Prove PlatformBackend can be used as a trait object
    let mut backend: Box<dyn PlatformBackend> = Box::new(HeadlessPlatform::new(800, 600));
    assert_eq!(backend.window_size(), (800, 600));
    assert!(!backend.should_quit());
    let events = backend.poll_events();
    assert!(events.is_empty());
    backend.end_frame();
}

#[test]
fn window_manager_trait_is_object_safe() {
    let mut wm: Box<dyn WindowManager> = Box::new(HeadlessWindow::new());
    let id = wm.create_window(&WindowConfig::default());
    assert!(wm.is_open(id));
    wm.set_title(id, "Test");
    wm.set_size(id, 1024, 768);
    wm.close(id);
    assert!(!wm.is_open(id));
}

#[test]
fn clipboard_trait_is_object_safe() {
    let mut cb: Box<dyn Clipboard> = Box::new(HeadlessClipboard::new());
    cb.set_text("platform test");
    assert!(cb.has_content());
    assert_eq!(cb.get().as_text(), Some("platform test"));
}

// ===========================================================================
// 3. Subsystem composition: display + input + window
// ===========================================================================

#[test]
fn display_server_routes_input_through_full_pipeline() {
    let mut display = DisplayServer::new();
    let win_id = display.create_window(&WindowConfig::default());

    // Set up input with an action
    let mut input_map = InputMap::new();
    input_map.add_action("jump", 0.0);
    input_map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

    let mut input = InputState::new();
    input.set_input_map(input_map);

    // Inject key press
    let backend = display.get_window_mut(win_id).unwrap();
    backend.push_event(WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    // Poll events through display server
    display.poll_events(&mut input);

    // Verify input state
    assert!(input.is_key_pressed(Key::Space));
    assert!(input.is_action_pressed("jump"));
}

#[test]
fn display_server_multi_window_isolation() {
    let mut display = DisplayServer::new();
    let win1 = display.create_window(&WindowConfig::new().with_title("Win 1"));
    let win2 = display.create_window(&WindowConfig::new().with_title("Win 2"));

    // Send different events to different windows
    display
        .get_window_mut(win1)
        .unwrap()
        .push_event(WindowEvent::KeyInput {
            key: Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });
    display
        .get_window_mut(win2)
        .unwrap()
        .push_event(WindowEvent::MouseInput {
            button: MouseButton::Right,
            pressed: true,
            position: Vector2::new(50.0, 50.0),
        });

    let mut input = InputState::new();
    let events = display.poll_events(&mut input);

    // Both events arrive
    assert_eq!(events.len(), 2);
    // Verify source window IDs
    assert!(events.iter().any(|(id, _)| *id == win1));
    assert!(events.iter().any(|(id, _)| *id == win2));
    // Input state has both
    assert!(input.is_key_pressed(Key::A));
    assert!(input.is_mouse_button_pressed(MouseButton::Right));
}

// ===========================================================================
// 4. Platform target validation
// ===========================================================================

#[test]
fn current_target_is_valid_and_ci_tested() {
    let target = current_target().expect("must match a supported target");
    assert!(!target.name.is_empty());
    assert!(!target.rust_triple.is_empty());
    assert_eq!(target.platform, current_platform());
    assert_eq!(target.arch, Architecture::current());

    // Our dev/CI machines should be CI-tested targets
    let ci = ci_tested_targets();
    assert!(
        ci.iter().any(|t| t.rust_triple == target.rust_triple),
        "current target {} should be CI-tested",
        target.rust_triple
    );
}

#[test]
fn validate_current_target_succeeds() {
    assert!(validate_current_target().is_ok());
}

#[test]
fn desktop_capabilities_on_current_target() {
    assert!(supports_capability(PlatformCapability::FileSystem));
    assert!(supports_capability(PlatformCapability::Networking));
    assert!(supports_capability(PlatformCapability::Threading));
    // Desktop targets support windowing
    assert!(supports_capability(PlatformCapability::Windowing));
}

// ===========================================================================
// 5. OS and timing sanity
// ===========================================================================

#[test]
fn os_info_is_consistent() {
    let info = OsInfo::detect();
    let platform = current_platform();
    match platform {
        gdplatform::os::Platform::MacOS => assert_eq!(info.name, "macOS"),
        gdplatform::os::Platform::Linux => assert_eq!(info.name, "Linux"),
        gdplatform::os::Platform::Windows => assert_eq!(info.name, "Windows"),
        _ => {} // Unknown/Web handled elsewhere
    }
}

#[test]
fn ticks_are_monotonic() {
    let t1 = get_ticks_usec();
    let t2 = get_ticks_usec();
    let t3 = get_ticks_msec();
    assert!(t2 >= t1, "usec ticks must be monotonic");
    // msec should be roughly consistent with usec
    assert!(t3 <= t2 / 1000 + 1, "msec/usec should be consistent");
}

// ===========================================================================
// 6. Threading primitives compose correctly
// ===========================================================================

#[test]
fn worker_pool_and_semaphore_integration() {
    let pool = WorkerThreadPool::new();
    let sem = GodotSemaphore::new(0);
    let counter = Arc::new(AtomicU32::new(0));

    // Submit 4 tasks that each wait on the semaphore, then increment
    let mut ids = Vec::new();
    for _ in 0..4 {
        let s = sem.clone();
        let c = counter.clone();
        ids.push(pool.add_task(move || {
            s.wait();
            c.fetch_add(1, Ordering::SeqCst);
        }));
    }

    // Release all 4
    for _ in 0..4 {
        sem.post();
    }

    // Wait for all
    for id in &ids {
        pool.wait_for_task_completion(*id);
    }

    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[test]
fn godot_thread_and_mutex_integration() {
    let mutex = GodotMutex::new();
    let shared = Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));

    let mut threads: Vec<GodotThread> = Vec::new();
    for i in 0..3 {
        let m = mutex.clone();
        let s = shared.clone();
        let mut t = GodotThread::new();
        t.start(move || {
            let _guard = m.lock();
            s.lock().unwrap().push(i);
        });
        threads.push(t);
    }

    for t in &mut threads {
        t.wait_to_finish();
    }

    let values = shared.lock().unwrap();
    assert_eq!(values.len(), 3);
    // All three values should be present (order may vary)
    let mut sorted = values.clone();
    sorted.sort();
    assert_eq!(sorted, vec![0, 1, 2]);
}

// ===========================================================================
// 7. Cursor + clipboard + display compose without conflict
// ===========================================================================

#[test]
fn cursor_clipboard_display_composition() {
    // Simulate a frame where cursor, clipboard, and display all operate
    let mut cursor = CursorManager::new();
    let mut clipboard = HeadlessClipboard::new();
    let mut display = DisplayServer::new();
    let win = display.create_window(&WindowConfig::default());

    // User clicks → cursor updates, clipboard copies
    cursor.set_position(100.0, 200.0);
    cursor.set_cursor_shape(CursorShape::Ibeam);
    clipboard.set_text("selected text");

    // Mouse event through display
    display
        .get_window_mut(win)
        .unwrap()
        .push_event(WindowEvent::MouseInput {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(100.0, 200.0),
        });

    let mut input = InputState::new();
    display.poll_events(&mut input);

    // All subsystems have consistent state
    assert_eq!(cursor.current_shape(), CursorShape::Ibeam);
    assert_eq!(cursor.position(), (100.0, 200.0));
    assert_eq!(clipboard.get().as_text(), Some("selected text"));
    assert!(input.is_mouse_button_pressed(MouseButton::Left));
}

// ===========================================================================
// 8. WindowConfig builder validates all fields
// ===========================================================================

#[test]
fn window_config_builder_all_fields() {
    let config = WindowConfig::new()
        .with_size(1920, 1080)
        .with_title("Test Game")
        .with_fullscreen(true)
        .with_vsync(false)
        .with_resizable(false);

    assert_eq!(config.width, 1920);
    assert_eq!(config.height, 1080);
    assert_eq!(config.title, "Test Game");
    assert!(config.fullscreen);
    assert!(!config.vsync);
    assert!(!config.resizable);

    // HeadlessPlatform respects the config
    let platform = HeadlessPlatform::from_config(&config);
    assert_eq!(platform.window_size(), (1920, 1080));
}

// ===========================================================================
// 9. Event pipeline: WindowEvent → InputEvent roundtrip
// ===========================================================================

#[test]
fn window_event_to_input_event_roundtrip_all_types() {
    // Key events
    let key_event = WindowEvent::KeyInput {
        key: Key::Enter,
        pressed: true,
        shift: true,
        ctrl: false,
        alt: true,
    };
    let input = key_event.to_input_event().unwrap();
    assert!(matches!(
        input,
        InputEvent::Key {
            key: Key::Enter,
            pressed: true,
            shift: true,
            ctrl: false,
            alt: true,
        }
    ));

    // Mouse button events
    let mouse_event = WindowEvent::MouseInput {
        button: MouseButton::Middle,
        pressed: true,
        position: Vector2::new(42.0, 84.0),
    };
    let input = mouse_event.to_input_event().unwrap();
    assert!(matches!(
        input,
        InputEvent::MouseButton {
            button: MouseButton::Middle,
            pressed: true,
            ..
        }
    ));

    // Mouse motion events
    let motion_event = WindowEvent::MouseMotion {
        position: Vector2::new(10.0, 20.0),
        relative: Vector2::new(1.0, -1.0),
    };
    assert!(motion_event.to_input_event().is_some());

    // Non-input events return None
    assert!(WindowEvent::CloseRequested.to_input_event().is_none());
    assert!(WindowEvent::FocusGained.to_input_event().is_none());
    assert!(WindowEvent::FocusLost.to_input_event().is_none());
    assert!((WindowEvent::Resized {
        width: 100,
        height: 100
    })
    .to_input_event()
    .is_none());
}

// ===========================================================================
// 10. Repeatable build: all public API types construct deterministically
// ===========================================================================

#[test]
fn deterministic_construction_all_public_types() {
    // Prove that all key types can be constructed and are deterministic
    // (same inputs → same outputs), which is a Phase 7 stability requirement.

    let config1 = WindowConfig::default();
    let config2 = WindowConfig::default();
    assert_eq!(config1, config2, "WindowConfig::default must be deterministic");

    let info1 = OsInfo::detect();
    let info2 = OsInfo::detect();
    assert_eq!(info1, info2, "OsInfo::detect must be deterministic");

    let platform1 = current_platform();
    let platform2 = current_platform();
    assert_eq!(platform1, platform2, "current_platform must be deterministic");

    let target1 = current_target();
    let target2 = current_target();
    assert_eq!(target1, target2, "current_target must be deterministic");

    let arch1 = Architecture::current();
    let arch2 = Architecture::current();
    assert_eq!(arch1, arch2, "Architecture::current must be deterministic");

    let debug1 = is_debug_build();
    let debug2 = is_debug_build();
    assert_eq!(debug1, debug2, "is_debug_build must be deterministic");
}

// ===========================================================================
// 11. Documentation validation: stable layer spec exists and is substantive
// ===========================================================================

#[test]
fn stable_layer_documentation_exists_and_covers_subsystems() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/PLATFORM_STABLE_LAYER.md"),
    )
    .expect("docs/PLATFORM_STABLE_LAYER.md must exist");

    // Must document all core subsystem responsibilities
    for subsystem in &[
        "Windowing",
        "Input",
        "Timing",
        "OS Integration",
        "Clipboard",
        "Cursor",
        "Thread",
    ] {
        assert!(
            doc.contains(subsystem),
            "stable layer doc must cover {} subsystem",
            subsystem
        );
    }

    // Must reference key types
    for key_type in &[
        "PlatformBackend",
        "WindowManager",
        "DisplayServer",
        "InputState",
        "InputMap",
        "Timer",
        "WorkerThreadPool",
    ] {
        assert!(
            doc.contains(key_type),
            "stable layer doc must reference {}",
            key_type
        );
    }

    // Must document stability guarantees
    assert!(
        doc.contains("Stability Guarantees"),
        "doc must have stability guarantees section"
    );
    assert!(
        doc.contains("Test Coverage"),
        "doc must have test coverage section"
    );
}

// ===========================================================================
// 12. Audit alignment: stable layer doc cites Phase 7 audit and
//     distinguishes headless from native coverage
// ===========================================================================

#[test]
fn stable_layer_doc_cites_phase7_audit_as_source_of_truth() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/PLATFORM_STABLE_LAYER.md"),
    )
    .expect("docs/PLATFORM_STABLE_LAYER.md must exist");

    assert!(
        doc.contains("PHASE7_PLATFORM_PARITY_AUDIT.md"),
        "stable layer doc must cite the Phase 7 audit as source of truth"
    );
    assert!(
        doc.contains("Source of truth"),
        "stable layer doc must have a Source of truth callout"
    );
}

#[test]
fn stable_layer_doc_documents_coverage_scope() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/PLATFORM_STABLE_LAYER.md"),
    )
    .expect("docs/PLATFORM_STABLE_LAYER.md must exist");

    assert!(
        doc.contains("Coverage Scope"),
        "doc must have a Coverage Scope section"
    );
    assert!(
        doc.contains("headless"),
        "doc must mention headless scope"
    );
}

#[test]
fn stable_layer_doc_windowing_input_timing_have_audit_status() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/PLATFORM_STABLE_LAYER.md"),
    )
    .expect("docs/PLATFORM_STABLE_LAYER.md must exist");

    // Each subsystem table must include an Audit Status column
    assert!(
        doc.contains("Audit Status"),
        "subsystem tables must include Audit Status column"
    );

    // Windowing section must note native scope limitation
    assert!(
        doc.contains("Native OS windowing"),
        "windowing table must list native OS windowing row"
    );
    assert!(
        doc.contains("Implemented, partly measured"),
        "doc must classify native windowing as partly measured"
    );

    // Input section must note headless scope
    assert!(
        doc.contains("Physical device input"),
        "input section must note physical device scope"
    );
}

#[test]
fn phase7_audit_doc_exists_and_references_stable_layer() {
    let audit = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../prd/PHASE7_PLATFORM_PARITY_AUDIT.md"),
    )
    .expect("prd/PHASE7_PLATFORM_PARITY_AUDIT.md must exist");

    assert!(
        audit.contains("PLATFORM_STABLE_LAYER.md"),
        "Phase 7 audit must reference the stable layer doc"
    );
    assert!(
        audit.contains("Measured"),
        "Phase 7 audit must use Measured classification"
    );
    assert!(
        audit.contains("Implemented, not yet measured"),
        "Phase 7 audit must use Implemented-not-yet-measured classification"
    );
    assert!(
        audit.contains("Deferred"),
        "Phase 7 audit must use Deferred classification"
    );
}
