//! Integration tests for Display server resolution and DPI queries.
//!
//! Covers ClassDB registration, ScreenInfo defaults, multi-screen management,
//! DPI/scale/refresh_rate/size queries, and out-of-bounds screen fallbacks.

use gdobject::class_db;
use gdplatform::{DisplayServer, ScreenInfo};

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_display_server_exists() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("DisplayServer"));
}

#[test]
fn classdb_display_server_has_screen_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("DisplayServer", "screen_get_size"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_dpi"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_refresh_rate"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_scale"));
}

// ── ScreenInfo Defaults ──────────────────────────────────────────────────────

#[test]
fn screen_info_default_resolution() {
    let info = ScreenInfo::default();
    assert_eq!(info.width, 1920);
    assert_eq!(info.height, 1080);
}

#[test]
fn screen_info_default_dpi() {
    let info = ScreenInfo::default();
    assert!((info.dpi - 96.0).abs() < f32::EPSILON);
}

#[test]
fn screen_info_default_scale() {
    let info = ScreenInfo::default();
    assert!((info.scale - 1.0).abs() < f32::EPSILON);
}

#[test]
fn screen_info_default_refresh_rate() {
    let info = ScreenInfo::default();
    assert!((info.refresh_rate - 60.0).abs() < f32::EPSILON);
}

#[test]
fn screen_info_default_position() {
    let info = ScreenInfo::default();
    assert_eq!(info.position, (0, 0));
}

// ── DisplayServer Screen Queries ─────────────────────────────────────────────

#[test]
fn display_server_default_screen_count() {
    let ds = DisplayServer::new();
    assert_eq!(ds.get_screen_count(), 1);
}

#[test]
fn display_server_default_screen_size() {
    let ds = DisplayServer::new();
    let (w, h) = ds.screen_get_size(0);
    assert_eq!(w, 1920);
    assert_eq!(h, 1080);
}

#[test]
fn display_server_default_dpi() {
    let ds = DisplayServer::new();
    assert!((ds.screen_get_dpi(0) - 96.0).abs() < f32::EPSILON);
}

#[test]
fn display_server_default_scale() {
    let ds = DisplayServer::new();
    assert!((ds.screen_get_scale(0) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn display_server_default_refresh_rate() {
    let ds = DisplayServer::new();
    assert!((ds.screen_get_refresh_rate(0) - 60.0).abs() < f32::EPSILON);
}

// ── Custom Screen Info ───────────────────────────────────────────────────────

#[test]
fn display_server_custom_screen_info() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        width: 3840,
        height: 2160,
        dpi: 218.0,
        scale: 2.0,
        refresh_rate: 144.0,
        position: (0, 0),
        name: "Main Display".into(),
    });
    assert_eq!(ds.screen_get_size(0), (3840, 2160));
    assert!((ds.screen_get_dpi(0) - 218.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_scale(0) - 2.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_refresh_rate(0) - 144.0).abs() < f32::EPSILON);
}

#[test]
fn display_server_screen_info_name() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        name: "My Monitor".into(),
        ..ScreenInfo::default()
    });
    let info = ds.get_screen_info(0).unwrap();
    assert_eq!(info.name, "My Monitor");
}

// ── Multi-Screen ─────────────────────────────────────────────────────────────

#[test]
fn display_server_add_second_screen() {
    let mut ds = DisplayServer::new();
    ds.add_screen(ScreenInfo {
        width: 2560,
        height: 1440,
        dpi: 109.0,
        scale: 1.0,
        refresh_rate: 165.0,
        position: (1920, 0),
        name: "Secondary".into(),
    });
    assert_eq!(ds.get_screen_count(), 2);
    assert_eq!(ds.screen_get_size(1), (2560, 1440));
    assert!((ds.screen_get_refresh_rate(1) - 165.0).abs() < f32::EPSILON);
}

#[test]
fn display_server_multi_screen_independent_dpi() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        dpi: 218.0,
        ..ScreenInfo::default()
    });
    ds.add_screen(ScreenInfo {
        dpi: 96.0,
        ..ScreenInfo::default()
    });
    assert!((ds.screen_get_dpi(0) - 218.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_dpi(1) - 96.0).abs() < f32::EPSILON);
}

#[test]
fn display_server_multi_screen_positions() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        position: (0, 0),
        ..ScreenInfo::default()
    });
    ds.add_screen(ScreenInfo {
        position: (1920, 0),
        ..ScreenInfo::default()
    });
    let info0 = ds.get_screen_info(0).unwrap();
    let info1 = ds.get_screen_info(1).unwrap();
    assert_eq!(info0.position, (0, 0));
    assert_eq!(info1.position, (1920, 0));
}

// ── Out-of-Bounds Fallback ───────────────────────────────────────────────────

#[test]
fn display_server_invalid_screen_falls_back() {
    let ds = DisplayServer::new();
    // Out-of-bounds should fall back to primary screen
    assert!((ds.screen_get_dpi(99) - 96.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_scale(99) - 1.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_refresh_rate(99) - 60.0).abs() < f32::EPSILON);
    assert_eq!(ds.screen_get_size(99), (1920, 1080));
}

#[test]
fn display_server_get_screen_info_none_for_invalid() {
    let ds = DisplayServer::new();
    assert!(ds.get_screen_info(99).is_none());
}

// ── Logical Size Calculation ─────────────────────────────────────────────────

#[test]
fn logical_size_from_physical_and_scale() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        width: 3840,
        height: 2160,
        scale: 2.0,
        ..ScreenInfo::default()
    });
    let (w, h) = ds.screen_get_size(0);
    let scale = ds.screen_get_scale(0);
    let logical_w = (w as f32 / scale) as u32;
    let logical_h = (h as f32 / scale) as u32;
    assert_eq!(logical_w, 1920);
    assert_eq!(logical_h, 1080);
}

// ── HiDPI Screen ─────────────────────────────────────────────────────────────

#[test]
fn hidpi_retina_screen_config() {
    let mut ds = DisplayServer::new();
    ds.set_screen_info(0, ScreenInfo {
        width: 5120,
        height: 2880,
        dpi: 218.0,
        scale: 2.0,
        refresh_rate: 120.0,
        position: (0, 0),
        name: "Retina 5K".into(),
    });
    assert_eq!(ds.screen_get_size(0), (5120, 2880));
    assert!((ds.screen_get_dpi(0) - 218.0).abs() < f32::EPSILON);
    assert!((ds.screen_get_scale(0) - 2.0).abs() < f32::EPSILON);
}
