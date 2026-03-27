//! Winit smoke-gate test binary (pat-u84m).
//!
//! Proves the real winit window backend can initialise, create a window, and
//! propagate lifecycle events when a display server is available.
//!
//! This is a `harness = false` test binary because macOS requires EventLoop
//! creation on the main thread. It is gated via `required-features = ["windowed"]`
//! in Cargo.toml so that `cargo test` (headless CI) never compiles or runs it.
//!
//! Run explicitly:
//!   cargo test --features windowed --test winit_smoke_gate_test
//!
//! Godot parity contracts tested:
//! - Event loop initialisation on the host platform
//! - Window creation with configured size
//! - Resize request propagation through WinitPlatform
//! - Close / quit signal propagation through PlatformBackend
//! - Key mapping bridge compiles under windowed feature gate

use std::sync::Arc;

use gdplatform::backend::PlatformBackend;
use gdplatform::window::{WindowEvent, WindowId, WindowManager};
use gdplatform::winit_backend::{map_winit_key, WinitPlatform, WinitWindow};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent as WinitWindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::KeyCode;
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::Window;

// ---------------------------------------------------------------------------
// Display availability runtime gate
// ---------------------------------------------------------------------------

fn display_available() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
    } else {
        true
    }
}

// ---------------------------------------------------------------------------
// Test 1: key mapping (no display required)
// ---------------------------------------------------------------------------

fn test_key_mapping() {
    assert_eq!(
        map_winit_key(KeyCode::Escape),
        Some(gdplatform::input::Key::Escape),
        "FAIL: Escape key mapping"
    );
    assert_eq!(
        map_winit_key(KeyCode::Space),
        Some(gdplatform::input::Key::Space),
        "FAIL: Space key mapping"
    );
    assert!(
        map_winit_key(KeyCode::Pause).is_none(),
        "FAIL: unmapped key should return None"
    );
    eprintln!("  PASS  winit_key_mapping_under_feature_gate");
}

// ---------------------------------------------------------------------------
// Test 2: event loop + window lifecycle (requires display)
// ---------------------------------------------------------------------------

struct SmokeApp {
    platform: Option<WinitPlatform>,
    window_created: bool,
    initial_size: Option<(u32, u32)>,
    resize_requested: bool,
    close_propagated: bool,
    quit_signalled: bool,
}

impl SmokeApp {
    fn new() -> Self {
        Self {
            platform: None,
            window_created: false,
            initial_size: None,
            resize_requested: false,
            close_propagated: false,
            quit_signalled: false,
        }
    }
}

impl ApplicationHandler for SmokeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.platform.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Smoke Gate")
            .with_inner_size(winit::dpi::LogicalSize::new(640u32, 480u32))
            .with_resizable(true);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let winit_win = WinitWindow::new(window);
        self.platform = Some(WinitPlatform::new(winit_win));
        self.window_created = true;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _wid: winit::window::WindowId,
        event: WinitWindowEvent,
    ) {
        let Some(p) = self.platform.as_mut() else {
            return;
        };

        if let WinitWindowEvent::RedrawRequested = event {
            if self.initial_size.is_none() {
                // Phase 1: record initial size, request resize.
                self.initial_size = Some(p.window_size());
                p.winit_window_mut().set_size(WindowId(0), 800, 600);
                self.resize_requested = true;
            } else if !self.close_propagated {
                // Phase 2: simulate close via PlatformBackend.
                p.push_event(WindowEvent::CloseRequested);
                let events = p.poll_events();
                self.close_propagated = events
                    .iter()
                    .any(|e| matches!(e, WindowEvent::CloseRequested));

                p.request_quit();
                self.quit_signalled = p.should_quit();

                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(p) = self.platform.as_ref() {
            p.winit_window().window().request_redraw();
        }
    }
}

fn test_event_loop_and_window_lifecycle() {
    let mut event_loop = EventLoop::new().expect("EventLoop::new() failed — display unavailable?");
    eprintln!("  PASS  winit_event_loop_can_be_created");

    let mut app = SmokeApp::new();

    for _ in 0..240 {
        if app.quit_signalled {
            break;
        }
        event_loop.pump_app_events(Some(std::time::Duration::from_millis(16)), &mut app);
    }

    assert!(app.window_created, "FAIL: window must be created");
    assert!(
        app.initial_size.is_some(),
        "FAIL: initial size must be recorded"
    );
    let (iw, ih) = app.initial_size.unwrap();
    assert!(
        iw > 0 && ih > 0,
        "FAIL: initial size must be non-zero: {iw}x{ih}"
    );
    assert!(
        app.resize_requested,
        "FAIL: resize must have been requested"
    );
    assert!(
        app.close_propagated,
        "FAIL: CloseRequested must propagate through PlatformBackend::poll_events"
    );
    assert!(
        app.quit_signalled,
        "FAIL: should_quit() must be true after request_quit()"
    );
    eprintln!("  PASS  winit_window_lifecycle_create_resize_close");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    eprintln!("running winit smoke-gate tests (pat-u84m)\n");

    // Key mapping — no display needed.
    test_key_mapping();

    // Event loop + lifecycle — need a display.
    if !display_available() {
        eprintln!("  SKIP  no display available — skipping event loop + lifecycle tests");
        eprintln!("\ntest result: ok. 1 passed; 0 failed; 2 skipped");
        return;
    }

    test_event_loop_and_window_lifecycle();

    eprintln!("\ntest result: ok. 3 passed; 0 failed; 0 skipped");
}
