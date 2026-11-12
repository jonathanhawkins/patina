//! pat-1bax: Window creation abstraction backed by winit.

use gdplatform::window::{HeadlessWindow, WindowConfig};

#[test]
fn window_config_builder() {
    let config = WindowConfig::new()
        .with_size(1920, 1080)
        .with_title("Test Game");
    assert_eq!(config.width, 1920);
    assert_eq!(config.height, 1080);
    assert_eq!(config.title, "Test Game");
}

#[test]
fn window_config_defaults() {
    let config = WindowConfig::new();
    assert!(config.width > 0, "default width should be positive");
    assert!(config.height > 0, "default height should be positive");
}

#[test]
fn window_config_fullscreen() {
    let config = WindowConfig::new().with_fullscreen(true);
    assert!(config.fullscreen);
}

#[test]
fn window_config_resizable() {
    let config = WindowConfig::new().with_resizable(false);
    assert!(!config.resizable);
}

#[test]
fn headless_window_creates() {
    let _window = HeadlessWindow::new();
}

#[test]
fn headless_window_debug_printable() {
    let window = HeadlessWindow::new();
    let s = format!("{:?}", window);
    assert!(!s.is_empty());
}
