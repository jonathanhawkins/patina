//! Integration tests for Linux X11 and Wayland platform backends.
//!
//! Covers ClassDB registration, display protocol detection, desktop environment
//! detection, compositing, IME, cursor theming, and headless backend.

use gdobject::class_db;
use gdplatform::linux::{
    LinuxDesktopEnvironment, LinuxDisplayInfo, LinuxDisplayProtocol, LinuxPlatformLayer,
};
use gdplatform::window::WindowConfig;

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_linux_platform_layer_exists() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("LinuxPlatformLayer"));
}

#[test]
fn classdb_linux_platform_layer_inherits_object() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("LinuxPlatformLayer").unwrap();
    assert_eq!(info.parent_class, "Object");
}

#[test]
fn classdb_has_display_properties() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("LinuxPlatformLayer", true);
    assert!(props.iter().any(|p| p.name == "display_protocol"));
    assert!(props.iter().any(|p| p.name == "scale_factor"));
    assert!(props.iter().any(|p| p.name == "compositing_enabled"));
    assert!(props.iter().any(|p| p.name == "ime_enabled"));
    assert!(props.iter().any(|p| p.name == "cursor_size"));
}

#[test]
fn classdb_has_protocol_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("LinuxPlatformLayer", "is_x11"));
    assert!(class_db::class_has_method("LinuxPlatformLayer", "is_wayland"));
    assert!(class_db::class_has_method("LinuxPlatformLayer", "is_headless"));
}

#[test]
fn classdb_has_capability_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("LinuxPlatformLayer", "supports_window_positioning"));
    assert!(class_db::class_has_method("LinuxPlatformLayer", "supports_global_shortcuts"));
    assert!(class_db::class_has_method("LinuxPlatformLayer", "has_system_tray"));
}

#[test]
fn classdb_has_cursor_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("LinuxPlatformLayer", "set_cursor_theme"));
    assert!(class_db::class_has_method("LinuxPlatformLayer", "get_cursor_theme"));
}

// ── LinuxDisplayProtocol ─────────────────────────────────────────────────────

#[test]
fn display_protocol_names() {
    assert_eq!(LinuxDisplayProtocol::X11.name(), "X11");
    assert_eq!(LinuxDisplayProtocol::Wayland.name(), "Wayland");
    assert_eq!(LinuxDisplayProtocol::XWayland.name(), "XWayland");
    assert_eq!(LinuxDisplayProtocol::Headless.name(), "Headless");
}

#[test]
fn x11_supports_window_positioning() {
    assert!(LinuxDisplayProtocol::X11.supports_window_positioning());
    assert!(LinuxDisplayProtocol::XWayland.supports_window_positioning());
    assert!(!LinuxDisplayProtocol::Wayland.supports_window_positioning());
    assert!(!LinuxDisplayProtocol::Headless.supports_window_positioning());
}

#[test]
fn x11_supports_global_shortcuts() {
    assert!(LinuxDisplayProtocol::X11.supports_global_shortcuts());
    assert!(LinuxDisplayProtocol::XWayland.supports_global_shortcuts());
    assert!(!LinuxDisplayProtocol::Wayland.supports_global_shortcuts());
    assert!(!LinuxDisplayProtocol::Headless.supports_global_shortcuts());
}

#[test]
fn wayland_check() {
    assert!(!LinuxDisplayProtocol::X11.is_wayland());
    assert!(LinuxDisplayProtocol::Wayland.is_wayland());
    assert!(LinuxDisplayProtocol::XWayland.is_wayland());
    assert!(!LinuxDisplayProtocol::Headless.is_wayland());
}

// ── LinuxDesktopEnvironment ──────────────────────────────────────────────────

#[test]
fn desktop_environment_names() {
    assert_eq!(LinuxDesktopEnvironment::Gnome.name(), "GNOME");
    assert_eq!(LinuxDesktopEnvironment::Kde.name(), "KDE Plasma");
    assert_eq!(LinuxDesktopEnvironment::Xfce.name(), "Xfce");
    assert_eq!(LinuxDesktopEnvironment::Wlroots.name(), "wlroots");
    assert_eq!(LinuxDesktopEnvironment::TilingWm.name(), "Tiling WM");
    assert_eq!(LinuxDesktopEnvironment::Unknown.name(), "Unknown");
}

#[test]
fn system_tray_support() {
    assert!(LinuxDesktopEnvironment::Gnome.has_system_tray());
    assert!(LinuxDesktopEnvironment::Kde.has_system_tray());
    assert!(LinuxDesktopEnvironment::Xfce.has_system_tray());
    assert!(!LinuxDesktopEnvironment::Wlroots.has_system_tray());
    assert!(!LinuxDesktopEnvironment::TilingWm.has_system_tray());
    assert!(!LinuxDesktopEnvironment::Unknown.has_system_tray());
}

// ── LinuxDisplayInfo Defaults ────────────────────────────────────────────────

#[test]
fn display_info_defaults() {
    let info = LinuxDisplayInfo::default();
    assert_eq!(info.protocol, LinuxDisplayProtocol::Headless);
    assert_eq!(info.desktop, LinuxDesktopEnvironment::Unknown);
    assert!((info.scale_factor - 1.0).abs() < f32::EPSILON);
    assert!(!info.compositing_enabled);
    assert!(!info.ime_enabled);
    assert_eq!(info.cursor_theme, "default");
    assert_eq!(info.cursor_size, 24);
}

// ── LinuxPlatformLayer ───────────────────────────────────────────────────────

#[test]
fn headless_creation() {
    let layer = LinuxPlatformLayer::headless("TestApp");
    assert_eq!(layer.app_name(), "TestApp");
    assert!(layer.is_headless());
}

#[test]
fn headless_not_x11_or_wayland() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.is_x11());
    assert!(!layer.is_wayland());
}

#[test]
fn with_config() {
    let config = WindowConfig::new().with_title("Linux Game").with_size(1920, 1080);
    let layer = LinuxPlatformLayer::new("LinuxGame", &config);
    assert_eq!(layer.app_name(), "LinuxGame");
    assert_eq!(layer.window_size(), (1920, 1080));
}

#[test]
fn scale_factor_default() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn set_scale_factor() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    layer.set_scale_factor(2.0);
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
}

#[test]
fn compositing_default_off_headless() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.compositing_enabled());
}

#[test]
fn set_compositing() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    layer.set_compositing_enabled(true);
    assert!(layer.compositing_enabled());
}

#[test]
fn ime_default_off() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.ime_enabled());
}

#[test]
fn set_ime() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    layer.set_ime_enabled(true);
    assert!(layer.ime_enabled());
}

#[test]
fn cursor_theme_default() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert_eq!(layer.cursor_theme(), "default");
}

#[test]
fn set_cursor_theme() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    layer.set_cursor_theme("Adwaita");
    assert_eq!(layer.cursor_theme(), "Adwaita");
}

#[test]
fn cursor_size_default() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert_eq!(layer.cursor_size(), 24);
}

#[test]
fn set_cursor_size() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    layer.set_cursor_size(48);
    assert_eq!(layer.cursor_size(), 48);
}

// ── Capability Queries ───────────────────────────────────────────────────────

#[test]
fn headless_no_window_positioning() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.supports_window_positioning());
}

#[test]
fn headless_no_global_shortcuts() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.supports_global_shortcuts());
}

// ── Backend Integration ──────────────────────────────────────────────────────

#[test]
fn poll_events_empty() {
    let mut layer = LinuxPlatformLayer::headless("Test");
    assert!(layer.poll_window_events().is_empty());
}

#[test]
fn should_not_quit_initially() {
    let layer = LinuxPlatformLayer::headless("Test");
    assert!(!layer.should_quit());
}
