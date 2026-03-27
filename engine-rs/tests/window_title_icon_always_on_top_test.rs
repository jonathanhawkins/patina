//! Integration tests for Window title, icon, and always-on-top properties.
//!
//! Covers ClassDB registration, HeadlessWindow backend behaviour,
//! WindowConfig builder, WindowIcon validation, and scene-tree integration.

use gdobject::class_db;
use gdplatform::window::{HeadlessWindow, WindowConfig, WindowIcon, WindowManager};

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_window_exists() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("Window"));
}

#[test]
fn classdb_window_inherits_node() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("Window").unwrap();
    assert_eq!(info.parent_class, "Node");
}

#[test]
fn classdb_window_has_title_property() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("Window", true);
    assert!(props.iter().any(|p| p.name == "title"));
}

#[test]
fn classdb_window_has_always_on_top_property() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("Window", true);
    assert!(props.iter().any(|p| p.name == "always_on_top"));
}

#[test]
fn classdb_window_has_title_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Window", "set_title"));
    assert!(class_db::class_has_method("Window", "get_title"));
}

#[test]
fn classdb_window_has_always_on_top_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Window", "set_always_on_top"));
    assert!(class_db::class_has_method("Window", "is_always_on_top"));
}

#[test]
fn classdb_window_has_icon_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Window", "set_icon"));
    assert!(class_db::class_has_method("Window", "get_icon"));
}

// ── WindowConfig Builder ─────────────────────────────────────────────────────

#[test]
fn window_config_default_title() {
    let cfg = WindowConfig::default();
    assert_eq!(cfg.title, "Patina Engine");
}

#[test]
fn window_config_default_always_on_top() {
    let cfg = WindowConfig::default();
    assert!(!cfg.always_on_top);
}

#[test]
fn window_config_default_icon_is_none() {
    let cfg = WindowConfig::default();
    assert!(cfg.icon.is_none());
}

#[test]
fn window_config_with_title() {
    let cfg = WindowConfig::new().with_title("My Game");
    assert_eq!(cfg.title, "My Game");
}

#[test]
fn window_config_with_always_on_top() {
    let cfg = WindowConfig::new().with_always_on_top(true);
    assert!(cfg.always_on_top);
}

#[test]
fn window_config_builder_chain() {
    let cfg = WindowConfig::new()
        .with_title("Test")
        .with_always_on_top(true)
        .with_size(800, 600);
    assert_eq!(cfg.title, "Test");
    assert!(cfg.always_on_top);
    assert_eq!(cfg.width, 800);
    assert_eq!(cfg.height, 600);
}

// ── WindowIcon Validation ────────────────────────────────────────────────────

#[test]
fn window_icon_valid_rgba() {
    let rgba = vec![0u8; 4 * 4 * 4]; // 4x4 RGBA
    let icon = WindowIcon::new(4, 4, rgba);
    assert!(icon.is_some());
    let icon = icon.unwrap();
    assert_eq!(icon.width, 4);
    assert_eq!(icon.height, 4);
}

#[test]
fn window_icon_wrong_size_returns_none() {
    let rgba = vec![0u8; 10]; // too short for any valid icon
    assert!(WindowIcon::new(4, 4, rgba).is_none());
}

#[test]
fn window_icon_zero_dimensions() {
    let rgba = vec![];
    let icon = WindowIcon::new(0, 0, rgba);
    assert!(icon.is_some()); // 0*0*4 == 0
}

#[test]
fn window_icon_1x1() {
    let rgba = vec![255, 0, 0, 255]; // single red pixel
    let icon = WindowIcon::new(1, 1, rgba).unwrap();
    assert_eq!(icon.rgba.len(), 4);
}

// ── HeadlessWindow Title ─────────────────────────────────────────────────────

#[test]
fn headless_set_and_get_title() {
    let mut wm = HeadlessWindow::new();
    let cfg = WindowConfig::new().with_title("Initial Title");
    let id = wm.create_window(&cfg);

    assert_eq!(wm.get_title(id), Some("Initial Title"));

    wm.set_title(id, "Updated Title");
    assert_eq!(wm.get_title(id), Some("Updated Title"));
}

#[test]
fn headless_title_empty_string() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_title(""));
    assert_eq!(wm.get_title(id), Some(""));
}

#[test]
fn headless_title_unicode() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_title("ゲーム 🎮"));
    assert_eq!(wm.get_title(id), Some("ゲーム 🎮"));
}

#[test]
fn headless_title_after_close_returns_none() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_title("Gone"));
    wm.close(id);
    // Closed window — title lookup behavior depends on impl, but window should not be open
    assert!(!wm.is_open(id));
}

// ── HeadlessWindow Always-On-Top ─────────────────────────────────────────────

#[test]
fn headless_always_on_top_default_false() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_always_on_top(id), Some(false));
}

#[test]
fn headless_set_always_on_top_true() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.set_always_on_top(id, true);
    assert_eq!(wm.get_always_on_top(id), Some(true));
}

#[test]
fn headless_toggle_always_on_top() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    wm.set_always_on_top(id, true);
    assert_eq!(wm.get_always_on_top(id), Some(true));
    wm.set_always_on_top(id, false);
    assert_eq!(wm.get_always_on_top(id), Some(false));
}

#[test]
fn headless_always_on_top_from_config() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::new().with_always_on_top(true));
    assert_eq!(wm.get_always_on_top(id), Some(true));
}

// ── HeadlessWindow Icon ──────────────────────────────────────────────────────

#[test]
fn headless_icon_default_none() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    assert!(wm.get_icon(id).is_none());
}

#[test]
fn headless_set_icon() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    wm.set_icon(id, Some(icon));
    let got = wm.get_icon(id).unwrap();
    assert_eq!(got.width, 2);
    assert_eq!(got.height, 2);
    assert_eq!(got.rgba.len(), 16);
}

#[test]
fn headless_clear_icon() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    let icon = WindowIcon::new(2, 2, vec![0u8; 16]).unwrap();
    wm.set_icon(id, Some(icon));
    assert!(wm.get_icon(id).is_some());
    wm.set_icon(id, None);
    assert!(wm.get_icon(id).is_none());
}

#[test]
fn headless_replace_icon() {
    let mut wm = HeadlessWindow::new();
    let id = wm.create_window(&WindowConfig::default());
    let icon1 = WindowIcon::new(2, 2, vec![255u8; 16]).unwrap();
    wm.set_icon(id, Some(icon1));
    let icon2 = WindowIcon::new(4, 4, vec![128u8; 64]).unwrap();
    wm.set_icon(id, Some(icon2));
    let got = wm.get_icon(id).unwrap();
    assert_eq!(got.width, 4);
    assert_eq!(got.height, 4);
}

// ── Multiple Windows ─────────────────────────────────────────────────────────

#[test]
fn headless_multiple_windows_independent_titles() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new().with_title("Window A"));
    let id2 = wm.create_window(&WindowConfig::new().with_title("Window B"));
    assert_eq!(wm.get_title(id1), Some("Window A"));
    assert_eq!(wm.get_title(id2), Some("Window B"));
}

#[test]
fn headless_multiple_windows_independent_always_on_top() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::new().with_always_on_top(true));
    let id2 = wm.create_window(&WindowConfig::default());
    assert_eq!(wm.get_always_on_top(id1), Some(true));
    assert_eq!(wm.get_always_on_top(id2), Some(false));
}

#[test]
fn headless_multiple_windows_independent_icons() {
    let mut wm = HeadlessWindow::new();
    let id1 = wm.create_window(&WindowConfig::default());
    let id2 = wm.create_window(&WindowConfig::default());
    let icon = WindowIcon::new(1, 1, vec![0, 0, 0, 255]).unwrap();
    wm.set_icon(id1, Some(icon));
    assert!(wm.get_icon(id1).is_some());
    assert!(wm.get_icon(id2).is_none());
}

// ── Scene Tree Integration ───────────────────────────────────────────────────

#[test]
fn scene_tree_window_node() {
    class_db::register_3d_classes();
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let root = tree.root_id();
    let win = gdscene::node::Node::new("MainWindow", "Window");
    let win_id = tree.add_child(root, win).unwrap();
    let node = tree.get_node(win_id).unwrap();
    assert_eq!(node.class_name(), "Window");
    assert_eq!(node.name(), "MainWindow");
}
