//! pat-y9z2u: Windows platform layer with Win32 windowing backend.
//!
//! Integration tests covering:
//! 1. WindowsPlatformLayer creation (headless and from WindowConfig)
//! 2. DPI awareness mode — defaults, set/get, per-monitor detection
//! 3. System DPI and scale factor computation
//! 4. Display info queries — DWM compositing, theme, transparency, animations
//! 5. Taskbar integration — progress state/value, flash, overlay icon
//! 6. Application state — foreground, console attached
//! 7. Backend delegation — window size, poll events, quit, frame counting
//! 8. Platform target validation — Windows targets in DESKTOP_TARGETS
//! 9. High-DPI workflow — DPI changes update scale factor
//! 10. Full lifecycle — create, configure, run frames, quit

use gdplatform::os::Platform;
use gdplatform::platform_targets::{self, Architecture};
use gdplatform::window::{WindowConfig, WindowEvent};
use gdplatform::windows::{
    DpiAwarenessMode, TaskbarProgressState, WindowsDisplayInfo, WindowsPlatformLayer, WindowsTheme,
};

// ===========================================================================
// 1. Creation
// ===========================================================================

#[test]
fn headless_creation_defaults() {
    let layer = WindowsPlatformLayer::headless("Test App");
    assert_eq!(layer.app_name(), "Test App");
    assert!(layer.is_foreground());
    assert!(!layer.console_attached());
    assert!(!layer.should_quit());
}

#[test]
fn creation_from_window_config() {
    let config = WindowConfig::new().with_size(1920, 1080).with_title("Game");
    let layer = WindowsPlatformLayer::new("My Game", &config);
    assert_eq!(layer.app_name(), "My Game");
    assert_eq!(layer.window_size(), (1920, 1080));
}

#[test]
fn headless_default_window_size() {
    let layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.window_size(), (1280, 720));
}

// ===========================================================================
// 2. DPI awareness
// ===========================================================================

#[test]
fn dpi_awareness_default_is_per_monitor_v2() {
    let layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::PerMonitorV2);
    assert!(layer.is_per_monitor_dpi());
}

#[test]
fn set_dpi_awareness_system() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_dpi_awareness(DpiAwarenessMode::System);
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::System);
    assert!(!layer.is_per_monitor_dpi());
}

#[test]
fn set_dpi_awareness_unaware() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_dpi_awareness(DpiAwarenessMode::Unaware);
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::Unaware);
    assert!(!layer.is_per_monitor_dpi());
}

#[test]
fn dpi_awareness_per_monitor_v1() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_dpi_awareness(DpiAwarenessMode::PerMonitor);
    assert!(layer.is_per_monitor_dpi());
}

#[test]
fn dpi_awareness_mode_names() {
    assert_eq!(DpiAwarenessMode::Unaware.name(), "Unaware");
    assert_eq!(DpiAwarenessMode::System.name(), "System");
    assert_eq!(DpiAwarenessMode::PerMonitor.name(), "Per-Monitor");
    assert_eq!(DpiAwarenessMode::PerMonitorV2.name(), "Per-Monitor V2");
}

#[test]
fn recommended_dpi_awareness() {
    assert_eq!(
        DpiAwarenessMode::recommended(),
        DpiAwarenessMode::PerMonitorV2
    );
}

// ===========================================================================
// 3. System DPI and scale factor
// ===========================================================================

#[test]
fn default_dpi_is_96() {
    let layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.system_dpi(), 96);
    assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn set_dpi_120_gives_scale_1_25() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_system_dpi(120);
    assert_eq!(layer.system_dpi(), 120);
    assert!((layer.scale_factor() - 1.25).abs() < f32::EPSILON);
}

#[test]
fn set_dpi_144_gives_scale_1_5() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_system_dpi(144);
    assert!((layer.scale_factor() - 1.5).abs() < f32::EPSILON);
}

#[test]
fn set_dpi_192_gives_scale_2_0() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_system_dpi(192);
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
}

// ===========================================================================
// 4. Display info queries
// ===========================================================================

#[test]
fn display_info_defaults() {
    let info = WindowsDisplayInfo::default();
    assert_eq!(info.dpi_awareness, DpiAwarenessMode::PerMonitorV2);
    assert_eq!(info.system_dpi, 96);
    assert!(info.dwm_compositing);
    assert_eq!(info.theme, WindowsTheme::Light);
    assert!(!info.accent_on_title_bars);
    assert!(info.transparency_enabled);
    assert!(info.animations_enabled);
}

#[test]
fn dwm_compositing_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(layer.dwm_compositing());
    layer.set_dwm_compositing(false);
    assert!(!layer.dwm_compositing());
    layer.set_dwm_compositing(true);
    assert!(layer.dwm_compositing());
}

#[test]
fn theme_light_and_dark() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.system_theme(), WindowsTheme::Light);
    layer.set_system_theme(WindowsTheme::Dark);
    assert_eq!(layer.system_theme(), WindowsTheme::Dark);
}

#[test]
fn accent_on_title_bars_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(!layer.accent_on_title_bars());
    layer.set_accent_on_title_bars(true);
    assert!(layer.accent_on_title_bars());
}

#[test]
fn transparency_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(layer.transparency_enabled());
    layer.set_transparency_enabled(false);
    assert!(!layer.transparency_enabled());
}

#[test]
fn animations_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(layer.animations_enabled());
    layer.set_animations_enabled(false);
    assert!(!layer.animations_enabled());
}

// ===========================================================================
// 5. Taskbar integration
// ===========================================================================

#[test]
fn taskbar_progress_default_is_none() {
    let layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
    assert_eq!(layer.taskbar_progress_value(), 0);
}

#[test]
fn taskbar_progress_normal() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 75);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Normal);
    assert_eq!(layer.taskbar_progress_value(), 75);
}

#[test]
fn taskbar_progress_clamps_at_100() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 250);
    assert_eq!(layer.taskbar_progress_value(), 100);
}

#[test]
fn taskbar_progress_error_state() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Error, 50);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Error);
}

#[test]
fn taskbar_progress_paused_state() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Paused, 30);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Paused);
}

#[test]
fn taskbar_progress_indeterminate() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Indeterminate, 0);
    assert_eq!(
        layer.taskbar_progress_state(),
        TaskbarProgressState::Indeterminate
    );
}

#[test]
fn taskbar_progress_transitions() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 60);
    layer.set_taskbar_progress(TaskbarProgressState::Error, 60);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Error);
    layer.set_taskbar_progress(TaskbarProgressState::Paused, 60);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Paused);
    layer.set_taskbar_progress(TaskbarProgressState::None, 0);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
}

#[test]
fn taskbar_flash_request() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(!layer.is_flash_requested());
    layer.flash_taskbar();
    assert!(layer.is_flash_requested());
    layer.clear_flash();
    assert!(!layer.is_flash_requested());
}

#[test]
fn overlay_icon_set_and_clear() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(layer.overlay_icon().is_none());
    layer.set_overlay_icon("Playing");
    assert_eq!(layer.overlay_icon(), Some("Playing"));
    layer.set_overlay_icon("Downloading");
    assert_eq!(layer.overlay_icon(), Some("Downloading"));
    layer.clear_overlay_icon();
    assert!(layer.overlay_icon().is_none());
}

#[test]
fn taskbar_state_names() {
    assert_eq!(TaskbarProgressState::None.name(), "None");
    assert_eq!(TaskbarProgressState::Indeterminate.name(), "Indeterminate");
    assert_eq!(TaskbarProgressState::Normal.name(), "Normal");
    assert_eq!(TaskbarProgressState::Error.name(), "Error");
    assert_eq!(TaskbarProgressState::Paused.name(), "Paused");
}

// ===========================================================================
// 6. Application state
// ===========================================================================

#[test]
fn foreground_state_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(layer.is_foreground());
    layer.set_foreground(false);
    assert!(!layer.is_foreground());
    layer.set_foreground(true);
    assert!(layer.is_foreground());
}

#[test]
fn console_attached_toggle() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(!layer.console_attached());
    layer.set_console_attached(true);
    assert!(layer.console_attached());
    layer.set_console_attached(false);
    assert!(!layer.console_attached());
}

// ===========================================================================
// 7. Backend delegation
// ===========================================================================

#[test]
fn backend_window_size_from_config() {
    let config = WindowConfig::new().with_size(800, 600);
    let layer = WindowsPlatformLayer::new("App", &config);
    assert_eq!(layer.window_size(), (800, 600));
}

#[test]
fn backend_poll_events() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.backend_mut().push_event(WindowEvent::FocusGained);
    layer.backend_mut().push_event(WindowEvent::CloseRequested);
    let events = layer.poll_window_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], WindowEvent::FocusGained);
    assert_eq!(events[1], WindowEvent::CloseRequested);
}

#[test]
fn backend_quit_request() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert!(!layer.should_quit());
    layer.backend_mut().request_quit();
    assert!(layer.should_quit());
}

#[test]
fn backend_frame_counting() {
    let mut layer = WindowsPlatformLayer::headless("App");
    assert_eq!(layer.backend().frames_run(), 0);
    layer.end_frame();
    assert_eq!(layer.backend().frames_run(), 1);
    layer.end_frame();
    layer.end_frame();
    assert_eq!(layer.backend().frames_run(), 3);
}

// ===========================================================================
// 8. Platform target validation
// ===========================================================================

#[test]
fn windows_targets_exist_in_desktop_targets() {
    let windows = platform_targets::targets_for_platform(Platform::Windows);
    assert!(
        !windows.is_empty(),
        "must define at least one Windows target"
    );
}

#[test]
fn windows_x86_64_target_exists() {
    let windows = platform_targets::targets_for_platform(Platform::Windows);
    let x64 = windows.iter().find(|t| t.arch == Architecture::X86_64);
    assert!(x64.is_some(), "Windows x86_64 target must exist");
    let t = x64.unwrap();
    assert_eq!(t.rust_triple, "x86_64-pc-windows-msvc");
    assert!(t.ci_tested, "Windows x86_64 should be CI-tested");
    assert!(t.gpu_supported);
    assert!(t.windowing_supported);
}

#[test]
fn windows_aarch64_target_exists() {
    let windows = platform_targets::targets_for_platform(Platform::Windows);
    let arm = windows.iter().find(|t| t.arch == Architecture::Aarch64);
    assert!(arm.is_some(), "Windows aarch64 target must exist");
    assert_eq!(arm.unwrap().rust_triple, "aarch64-pc-windows-msvc");
}

// ===========================================================================
// 9. High-DPI workflow
// ===========================================================================

#[test]
fn high_dpi_4k_workflow() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_system_dpi(192); // 200%
    layer.set_dpi_awareness(DpiAwarenessMode::PerMonitorV2);

    assert!(layer.is_per_monitor_dpi());
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);

    // Simulate DPI change to 150%
    layer.set_system_dpi(144);
    assert!((layer.scale_factor() - 1.5).abs() < f32::EPSILON);
    assert_eq!(layer.system_dpi(), 144);
}

#[test]
fn dpi_change_while_unaware_still_tracks() {
    let mut layer = WindowsPlatformLayer::headless("App");
    layer.set_dpi_awareness(DpiAwarenessMode::Unaware);
    layer.set_system_dpi(192);
    // Even in unaware mode, we track the DPI value
    assert_eq!(layer.system_dpi(), 192);
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
}

// ===========================================================================
// 10. Full lifecycle
// ===========================================================================

#[test]
fn full_windows_lifecycle() {
    let config = WindowConfig::new()
        .with_size(1920, 1080)
        .with_title("Patina Engine");
    let mut layer = WindowsPlatformLayer::new("Patina Engine", &config);

    // Configure display
    layer.set_system_dpi(120);
    layer.set_system_theme(WindowsTheme::Dark);
    layer.set_dpi_awareness(DpiAwarenessMode::PerMonitorV2);
    layer.set_dwm_compositing(true);

    assert_eq!(layer.app_name(), "Patina Engine");
    assert_eq!(layer.window_size(), (1920, 1080));
    assert!((layer.scale_factor() - 1.25).abs() < f32::EPSILON);
    assert_eq!(layer.system_theme(), WindowsTheme::Dark);
    assert!(layer.is_per_monitor_dpi());

    // Taskbar progress
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 50);
    assert_eq!(layer.taskbar_progress_value(), 50);

    layer.set_overlay_icon("Loading");
    assert_eq!(layer.overlay_icon(), Some("Loading"));

    layer.flash_taskbar();
    assert!(layer.is_flash_requested());

    // Run frames
    layer.end_frame();
    layer.end_frame();
    assert_eq!(layer.backend().frames_run(), 2);

    // App loses focus
    layer.set_foreground(false);
    assert!(!layer.is_foreground());

    // Cleanup
    layer.set_taskbar_progress(TaskbarProgressState::None, 0);
    layer.clear_overlay_icon();
    layer.clear_flash();
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
    assert!(layer.overlay_icon().is_none());
    assert!(!layer.is_flash_requested());
    assert!(!layer.should_quit());
}
