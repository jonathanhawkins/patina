//! pat-ntojn: Window title, icon, and always-on-top properties.
//!
//! Integration tests covering:
//! 1. WindowConfig defaults — title, always_on_top, icon
//! 2. WindowConfig builder — with_title, with_always_on_top, with_icon
//! 3. WindowIcon construction and validation
//! 4. HeadlessWindow — set/get title, always_on_top, icon
//! 5. Title changes on live windows
//! 6. Always-on-top toggle
//! 7. Icon set and clear
//! 8. Operations on closed windows are no-ops
//! 9. Multiple windows with independent properties
//! 10. DisplayServer integration — title/icon/always_on_top flow through

use gdplatform::window::{HeadlessWindow, WindowConfig, WindowIcon, WindowId, WindowManager, WindowMode};
use gdplatform::display::DisplayServer;

// ===========================================================================
// 1. WindowConfig defaults
// ===========================================================================

#[test]
fn config_default_title() {
    let cfg = WindowConfig::default();
    assert_eq!(cfg.title, "Patina Engine");
}

#[test]
fn config_default_always_on_top_is_false() {
    let cfg = WindowConfig::default();
    assert!(!cfg.always_on_top);
}

#[test]
fn config_default_icon_is_none() {
    let cfg = WindowConfig::default();
    assert!(cfg.icon.is_none());
}

// ===========================================================================
// 2. WindowConfig builder
// ===========================================================================

#[test]
fn config_with_title() {
    let cfg = WindowConfig::new().with_title("My Game");
    assert_eq!(cfg.title, "My Game");
}

#[test]
fn config_with_always_on_top() {
    let cfg = WindowConfig::new().with_always_on_top(true);
    assert!(cfg.always_on_top);
}

#[test]
fn config_with_icon() {
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    let cfg = WindowConfig::new().with_icon(icon.clone());
    assert_eq!(cfg.icon.as_ref().unwrap().width, 2);
    assert_eq!(cfg.icon.as_ref().unwrap().height, 2);
    assert_eq!(cfg.icon.unwrap().rgba.len(), 16);
}

#[test]
fn config_builder_chain() {
    let icon = WindowIcon::new(1, 1, vec![255, 0, 0, 255]).unwrap();
    let cfg = WindowConfig::new()
        .with_title("Chained")
        .with_always_on_top(true)
        .with_icon(icon)
        .with_size(800, 600);
    assert_eq!(cfg.title, "Chained");
    assert!(cfg.always_on_top);
    assert!(cfg.icon.is_some());
    assert_eq!(cfg.width, 800);
}

// ===========================================================================
// 3. WindowIcon construction and validation
// ===========================================================================

#[test]
fn icon_valid_dimensions() {
    let icon = WindowIcon::new(4, 4, vec![0u8; 64]);
    assert!(icon.is_some());
    let i = icon.unwrap();
    assert_eq!(i.width, 4);
    assert_eq!(i.height, 4);
}

#[test]
fn icon_invalid_data_length_returns_none() {
    // 2x2 needs 16 bytes, provide 10
    assert!(WindowIcon::new(2, 2, vec![0u8; 10]).is_none());
}

#[test]
fn icon_zero_dimensions() {
    // 0x0 icon needs 0 bytes
    let icon = WindowIcon::new(0, 0, vec![]);
    assert!(icon.is_some());
}

#[test]
fn icon_1x1_rgba() {
    let icon = WindowIcon::new(1, 1, vec![255, 128, 64, 255]).unwrap();
    assert_eq!(icon.rgba, vec![255, 128, 64, 255]);
}

#[test]
fn icon_too_many_bytes_returns_none() {
    // 1x1 needs 4, provide 8
    assert!(WindowIcon::new(1, 1, vec![0u8; 8]).is_none());
}

// ===========================================================================
// 4. HeadlessWindow — create with title, always_on_top, icon
// ===========================================================================

#[test]
fn headless_create_applies_title() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_title("Test Title"));
    assert_eq!(wm.get_title(id), Some("Test Title"));
}

#[test]
fn headless_create_applies_always_on_top() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_always_on_top(true));
    assert_eq!(wm.get_always_on_top(id), Some(true));
}

#[test]
fn headless_create_default_always_on_top_false() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_always_on_top(id), Some(false));
}

#[test]
fn headless_create_applies_icon() {
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_icon(icon));
    assert!(wm.get_icon(id).is_some());
    assert_eq!(wm.get_icon(id).unwrap().width, 2);
}

#[test]
fn headless_create_default_icon_none() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert!(wm.get_icon(id).is_none());
}

// ===========================================================================
// 5. Title changes on live windows
// ===========================================================================

#[test]
fn set_title_changes_title() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_title(id), Some("Patina Engine"));
    wm.set_title(id, "New Title");
    assert_eq!(wm.get_title(id), Some("New Title"));
}

#[test]
fn set_title_to_empty_string() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.set_title(id, "");
    assert_eq!(wm.get_title(id), Some(""));
}

#[test]
fn set_title_unicode() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.set_title(id, "Hello World");
    assert_eq!(wm.get_title(id), Some("Hello World"));
}

// ===========================================================================
// 6. Always-on-top toggle
// ===========================================================================

#[test]
fn toggle_always_on_top() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_always_on_top(id), Some(false));

    wm.set_always_on_top(id, true);
    assert_eq!(wm.get_always_on_top(id), Some(true));

    wm.set_always_on_top(id, false);
    assert_eq!(wm.get_always_on_top(id), Some(false));
}

#[test]
fn always_on_top_independent_of_mode() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_always_on_top(true));
    wm.set_mode(id, WindowMode::Maximized);
    assert_eq!(wm.get_always_on_top(id), Some(true));
    wm.set_mode(id, WindowMode::Fullscreen);
    assert_eq!(wm.get_always_on_top(id), Some(true));
    wm.restore(id);
    assert_eq!(wm.get_always_on_top(id), Some(true));
}

// ===========================================================================
// 7. Icon set and clear
// ===========================================================================

#[test]
fn set_icon_on_window() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert!(wm.get_icon(id).is_none());

    let icon = WindowIcon::new(4, 4, vec![128u8; 64]).unwrap();
    wm.set_icon(id, Some(icon));
    let got = wm.get_icon(id).unwrap();
    assert_eq!(got.width, 4);
    assert_eq!(got.height, 4);
}

#[test]
fn clear_icon() {
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_icon(icon));
    assert!(wm.get_icon(id).is_some());

    wm.set_icon(id, None);
    assert!(wm.get_icon(id).is_none());
}

#[test]
fn replace_icon() {
    let icon1 = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    let icon2 = WindowIcon::new(4, 4, vec![255u8; 64]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_icon(icon1));
    assert_eq!(wm.get_icon(id).unwrap().width, 2);

    wm.set_icon(id, Some(icon2));
    assert_eq!(wm.get_icon(id).unwrap().width, 4);
}

// ===========================================================================
// 8. Operations on closed windows are no-ops
// ===========================================================================

#[test]
fn set_title_on_closed_is_noop() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.close(id);
    wm.set_title(id, "Should not crash");
    assert!(wm.get_title(id).is_none());
}

#[test]
fn set_always_on_top_on_closed_is_noop() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.close(id);
    wm.set_always_on_top(id, true);
    assert!(wm.get_always_on_top(id).is_none());
}

#[test]
fn set_icon_on_closed_is_noop() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.close(id);
    let icon = WindowIcon::new(1, 1, vec![0u8; 4]).unwrap();
    wm.set_icon(id, Some(icon));
    assert!(wm.get_icon(id).is_none());
}

#[test]
fn close_clears_always_on_top_and_icon() {
    let icon = WindowIcon::new(1, 1, vec![0u8; 4]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(
        &WindowConfig::new()
            .with_always_on_top(true)
            .with_icon(icon),
    );
    assert_eq!(wm.get_always_on_top(id), Some(true));
    assert!(wm.get_icon(id).is_some());

    wm.close(id);
    assert!(wm.get_always_on_top(id).is_none());
    assert!(wm.get_icon(id).is_none());
}

// ===========================================================================
// 9. Multiple windows with independent properties
// ===========================================================================

#[test]
fn multiple_windows_independent_titles() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new().with_title("Window A"));
    let id2 = wm.create_window(&WindowConfig::new().with_title("Window B"));
    assert_eq!(wm.get_title(id1), Some("Window A"));
    assert_eq!(wm.get_title(id2), Some("Window B"));

    wm.set_title(id1, "Changed A");
    assert_eq!(wm.get_title(id1), Some("Changed A"));
    assert_eq!(wm.get_title(id2), Some("Window B"));
}

#[test]
fn multiple_windows_independent_always_on_top() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new().with_always_on_top(true));
    let id2 = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_always_on_top(id1), Some(true));
    assert_eq!(wm.get_always_on_top(id2), Some(false));
}

#[test]
fn multiple_windows_independent_icons() {
    let icon = WindowIcon::new(1, 1, vec![0u8; 4]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new().with_icon(icon));
    let id2 = wm.create_window(&WindowConfig::default());
    assert!(wm.get_icon(id1).is_some());
    assert!(wm.get_icon(id2).is_none());
}

#[test]
fn close_one_window_does_not_affect_other() {
    let icon = WindowIcon::new(1, 1, vec![0u8; 4]).unwrap();
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(
        &WindowConfig::new()
            .with_title("Keep")
            .with_always_on_top(true)
            .with_icon(icon),
    );
    let id2 = wm.create_window(&WindowConfig::new().with_title("Close Me"));
    wm.close(id2);

    assert!(wm.is_open(id1));
    assert_eq!(wm.get_title(id1), Some("Keep"));
    assert_eq!(wm.get_always_on_top(id1), Some(true));
    assert!(wm.get_icon(id1).is_some());
}

// ===========================================================================
// 10. DisplayServer integration
// ===========================================================================

#[test]
fn display_server_window_title_via_backend() {
    let mut ds = DisplayServer::new();
    let id = ds.create_window(&WindowConfig::new().with_title("DS Title"));
    let backend = ds.get_window_mut(id).unwrap();
    // The inner HeadlessWindow uses its own id (WindowId(1))
    let inner_id = WindowId(1);
    assert_eq!(backend.get_title(inner_id), Some("DS Title"));

    backend.set_title(inner_id, "Updated");
    assert_eq!(backend.get_title(inner_id), Some("Updated"));
}

#[test]
fn display_server_always_on_top_via_backend() {
    let mut ds = DisplayServer::new();
    let id = ds.create_window(&WindowConfig::new().with_always_on_top(true));
    let backend = ds.get_window_mut(id).unwrap();
    let inner_id = WindowId(1);
    assert_eq!(backend.get_always_on_top(inner_id), Some(true));

    backend.set_always_on_top(inner_id, false);
    assert_eq!(backend.get_always_on_top(inner_id), Some(false));
}

#[test]
fn display_server_icon_via_backend() {
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    let mut ds = DisplayServer::new();
    let id = ds.create_window(&WindowConfig::new().with_icon(icon));
    let backend = ds.get_window_mut(id).unwrap();
    let inner_id = WindowId(1);
    assert!(backend.get_icon(inner_id).is_some());
    assert_eq!(backend.get_icon(inner_id).unwrap().width, 2);
}
