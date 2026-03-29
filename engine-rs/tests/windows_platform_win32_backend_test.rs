//! Integration tests for Windows platform layer with Win32 windowing backend.
//!
//! Covers ClassDB registration, DPI awareness, taskbar integration,
//! theme detection, overlay icons, and the headless windowing backend.

use gdobject::class_db;
use gdplatform::window::WindowConfig;
use gdplatform::windows::{
    DpiAwarenessMode, TaskbarProgressState, WindowsDisplayInfo, WindowsPlatformLayer, WindowsTheme,
};

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_windows_platform_layer_exists() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("WindowsPlatformLayer"));
}

#[test]
fn classdb_windows_platform_layer_inherits_object() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("WindowsPlatformLayer").unwrap();
    assert_eq!(info.parent_class, "Object");
}

#[test]
fn classdb_has_dpi_properties() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("WindowsPlatformLayer");
    assert!(props.iter().any(|p| p.name == "dpi_awareness"));
    assert!(props.iter().any(|p| p.name == "system_dpi"));
    assert!(props.iter().any(|p| p.name == "scale_factor"));
}

#[test]
fn classdb_has_display_properties() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("WindowsPlatformLayer");
    assert!(props.iter().any(|p| p.name == "dwm_compositing"));
    assert!(props.iter().any(|p| p.name == "theme"));
}

#[test]
fn classdb_has_taskbar_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "set_taskbar_progress"
    ));
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "flash_taskbar"
    ));
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "set_overlay_icon"
    ));
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "clear_overlay_icon"
    ));
}

#[test]
fn classdb_has_dpi_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "set_dpi_awareness"
    ));
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "get_dpi_awareness"
    ));
}

#[test]
fn classdb_has_theme_and_foreground_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "set_system_theme"
    ));
    assert!(class_db::class_has_method(
        "WindowsPlatformLayer",
        "is_foreground"
    ));
}

// ── DpiAwarenessMode ─────────────────────────────────────────────────────────

#[test]
fn dpi_awareness_names() {
    assert_eq!(DpiAwarenessMode::Unaware.name(), "Unaware");
    assert_eq!(DpiAwarenessMode::System.name(), "System");
    assert_eq!(DpiAwarenessMode::PerMonitor.name(), "Per-Monitor");
    assert_eq!(DpiAwarenessMode::PerMonitorV2.name(), "Per-Monitor V2");
}

#[test]
fn dpi_awareness_per_monitor_check() {
    assert!(!DpiAwarenessMode::Unaware.is_per_monitor());
    assert!(!DpiAwarenessMode::System.is_per_monitor());
    assert!(DpiAwarenessMode::PerMonitor.is_per_monitor());
    assert!(DpiAwarenessMode::PerMonitorV2.is_per_monitor());
}

#[test]
fn dpi_awareness_recommended() {
    assert_eq!(
        DpiAwarenessMode::recommended(),
        DpiAwarenessMode::PerMonitorV2
    );
}

// ── TaskbarProgressState ─────────────────────────────────────────────────────

#[test]
fn taskbar_progress_state_names() {
    assert_eq!(TaskbarProgressState::None.name(), "None");
    assert_eq!(TaskbarProgressState::Indeterminate.name(), "Indeterminate");
    assert_eq!(TaskbarProgressState::Normal.name(), "Normal");
    assert_eq!(TaskbarProgressState::Error.name(), "Error");
    assert_eq!(TaskbarProgressState::Paused.name(), "Paused");
}

// ── WindowsDisplayInfo Defaults ──────────────────────────────────────────────

#[test]
fn display_info_defaults() {
    let info = WindowsDisplayInfo::default();
    assert_eq!(info.dpi_awareness, DpiAwarenessMode::PerMonitorV2);
    assert!((info.scale_factor - 1.0).abs() < f32::EPSILON);
    assert_eq!(info.system_dpi, 96);
    assert!(info.dwm_compositing);
    assert_eq!(info.theme, WindowsTheme::Light);
    assert!(!info.accent_on_title_bars);
    assert!(info.transparency_enabled);
    assert!(info.animations_enabled);
}

// ── WindowsPlatformLayer ─────────────────────────────────────────────────────

#[test]
fn platform_layer_headless_creation() {
    let layer = WindowsPlatformLayer::headless("TestApp");
    assert_eq!(layer.app_name(), "TestApp");
}

#[test]
fn platform_layer_with_config() {
    let config = WindowConfig::new().with_title("Game").with_size(1920, 1080);
    let layer = WindowsPlatformLayer::new("MyGame", &config);
    assert_eq!(layer.app_name(), "MyGame");
    assert_eq!(layer.window_size(), (1920, 1080));
}

#[test]
fn platform_layer_default_dpi_awareness() {
    let layer = WindowsPlatformLayer::headless("Test");
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::PerMonitorV2);
}

#[test]
fn platform_layer_set_dpi_awareness() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_dpi_awareness(DpiAwarenessMode::System);
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::System);
    assert!(!layer.is_per_monitor_dpi());
}

#[test]
fn platform_layer_system_dpi() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    assert_eq!(layer.system_dpi(), 96);
    layer.set_system_dpi(144);
    assert_eq!(layer.system_dpi(), 144);
    // Scale factor should update: 144/96 = 1.5
    assert!((layer.scale_factor() - 1.5).abs() < f32::EPSILON);
}

#[test]
fn platform_layer_dwm_compositing() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    assert!(layer.dwm_compositing());
    layer.set_dwm_compositing(false);
    assert!(!layer.dwm_compositing());
}

#[test]
fn platform_layer_system_theme() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    assert_eq!(layer.system_theme(), WindowsTheme::Light);
    layer.set_system_theme(WindowsTheme::Dark);
    assert_eq!(layer.system_theme(), WindowsTheme::Dark);
}

#[test]
fn platform_layer_transparency() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    assert!(layer.transparency_enabled());
    layer.set_transparency_enabled(false);
    assert!(!layer.transparency_enabled());
}

// ── Taskbar Integration ──────────────────────────────────────────────────────

#[test]
fn taskbar_progress_default() {
    let layer = WindowsPlatformLayer::headless("Test");
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
    assert_eq!(layer.taskbar_progress_value(), 0);
}

#[test]
fn taskbar_set_progress() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 50);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Normal);
    assert_eq!(layer.taskbar_progress_value(), 50);
}

#[test]
fn taskbar_progress_clamps_to_100() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_taskbar_progress(TaskbarProgressState::Normal, 200);
    assert!(layer.taskbar_progress_value() <= 100);
}

#[test]
fn taskbar_error_state() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_taskbar_progress(TaskbarProgressState::Error, 75);
    assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Error);
}

#[test]
fn taskbar_flash() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    assert!(!layer.is_flash_requested());
    layer.flash_taskbar();
    assert!(layer.is_flash_requested());
    layer.clear_flash();
    assert!(!layer.is_flash_requested());
}

// ── Overlay Icon ─────────────────────────────────────────────────────────────

#[test]
fn overlay_icon_default_none() {
    let layer = WindowsPlatformLayer::headless("Test");
    assert!(layer.overlay_icon().is_none());
}

#[test]
fn overlay_icon_set_and_clear() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_overlay_icon("New messages");
    assert_eq!(layer.overlay_icon(), Some("New messages"));
    layer.clear_overlay_icon();
    assert!(layer.overlay_icon().is_none());
}

// ── Foreground State ─────────────────────────────────────────────────────────

#[test]
fn foreground_default_true_in_headless() {
    let layer = WindowsPlatformLayer::headless("Test");
    assert!(layer.is_foreground());
}

#[test]
fn foreground_set() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_foreground(true);
    assert!(layer.is_foreground());
}

// ── Backend Integration ──────────────────────────────────────────────────────

#[test]
fn poll_events_empty_initially() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    let events = layer.poll_window_events();
    assert!(events.is_empty());
}

#[test]
fn should_not_quit_initially() {
    let layer = WindowsPlatformLayer::headless("Test");
    assert!(!layer.should_quit());
}

// ── High DPI Simulation ─────────────────────────────────────────────────────

#[test]
fn high_dpi_display_simulation() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_dpi_awareness(DpiAwarenessMode::PerMonitorV2);
    layer.set_system_dpi(192); // 200% scaling
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
    assert!(layer.is_per_monitor_dpi());
}

#[test]
fn legacy_dpi_unaware_simulation() {
    let mut layer = WindowsPlatformLayer::headless("Test");
    layer.set_dpi_awareness(DpiAwarenessMode::Unaware);
    assert!(!layer.is_per_monitor_dpi());
    assert_eq!(layer.dpi_awareness().name(), "Unaware");
}
