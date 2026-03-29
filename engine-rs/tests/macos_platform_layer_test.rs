//! pat-wid1v: macOS platform layer with native menu bar integration.
//!
//! Validates:
//! 1. MacOsPlatformLayer creation (headless and configured)
//! 2. App menu creation with standard macOS items
//! 3. Custom menu bar construction with shortcuts
//! 4. Menu action dispatching and ordering
//! 5. Checkbox toggle via activate_menu_item
//! 6. Display info queries (Retina, dark mode, accessibility)
//! 7. Dock badge management
//! 8. Platform backend delegation (events, quit, frame counting)
//! 9. Full workflow: menu bar + actions + display + frame loop

use gdplatform::macos::{DockBadge, MacOsPlatformLayer, SystemTheme};
use gdplatform::native_menu::{MenuBarPlatform, MenuItem, MenuShortcut};
use gdplatform::window::{WindowConfig, WindowEvent};

// ── Creation ──────────────────────────────────────────────────────────

#[test]
fn headless_layer_has_correct_defaults() {
    let layer = MacOsPlatformLayer::headless("Test App");
    assert_eq!(layer.app_name(), "Test App");
    assert!(!layer.has_app_menu());
    assert_eq!(layer.menu_bar.platform, MenuBarPlatform::Headless);
    assert!(!layer.is_native_macos());
    assert!(!layer.is_global_menu_bar());
    assert!(layer.is_frontmost());
    assert_eq!(layer.dock_badge(), &DockBadge::None);
}

#[test]
fn layer_from_window_config() {
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_title("My Game");
    let layer = MacOsPlatformLayer::new("My Game", &config);
    assert_eq!(layer.window_size(), (800, 600));
    assert_eq!(layer.app_name(), "My Game");
}

// ── App menu ──────────────────────────────────────────────────────────

#[test]
fn create_app_menu_adds_standard_items() {
    let mut layer = MacOsPlatformLayer::headless("Patina Engine");
    layer.create_app_menu();
    assert!(layer.has_app_menu());
    assert_eq!(layer.menu_bar.menu_count(), 1);

    let menu = &layer.menu_bar.menus()[0];
    assert_eq!(menu.label, "Patina Engine");
    // Standard items: About, sep, Hide, Hide Others, Show All, sep, Quit
    assert_eq!(menu.item_count(), 7);
    assert_eq!(menu.items[0].label, "About Patina Engine");
    assert!(menu.items[1].is_separator());
    assert_eq!(menu.items[6].label, "Quit Patina Engine");

    // Quit should have Cmd+Q.
    let quit_shortcut = menu.items[6].shortcut.as_ref().unwrap();
    assert_eq!(quit_shortcut.key, "Q");
    assert!(quit_shortcut.command);
}

#[test]
fn create_app_menu_idempotent() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.create_app_menu();
    layer.create_app_menu();
    assert_eq!(layer.menu_bar.menu_count(), 1);
}

// ── Custom menus ──────────────────────────────────────────────────────

#[test]
fn add_custom_menus_with_shortcuts() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.create_app_menu();

    let file_id = layer.menu_bar.create_menu("File");
    let new_id = layer.menu_bar.alloc_item_id();
    let save_id = layer.menu_bar.alloc_item_id();
    {
        let file = layer.menu_bar.get_menu_mut(file_id).unwrap();
        file.add_item(MenuItem::action(new_id, "New").with_shortcut(MenuShortcut::cmd("N")));
        file.add_item(MenuItem::action(save_id, "Save").with_shortcut(MenuShortcut::cmd("S")));
    }

    assert_eq!(layer.menu_bar.menu_count(), 2); // App + File
    let (_, item) = layer.menu_bar.find_item(save_id).unwrap();
    assert_eq!(item.label, "Save");
    assert_eq!(
        item.shortcut.as_ref().unwrap().display_string(true),
        "Cmd+S"
    );
}

// ── Menu actions ──────────────────────────────────────────────────────

#[test]
fn activate_menu_item_produces_action() {
    let mut layer = MacOsPlatformLayer::headless("App");
    let menu_id = layer.menu_bar.create_menu("File");
    let item_id = layer.menu_bar.alloc_item_id();
    layer
        .menu_bar
        .get_menu_mut(menu_id)
        .unwrap()
        .add_item(MenuItem::action(item_id, "Open"));

    layer.activate_menu_item(item_id);

    let actions = layer.poll_menu_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].item_id, item_id);
    assert_eq!(actions[0].label, "Open");
}

#[test]
fn menu_actions_drain_in_fifo_order() {
    let mut layer = MacOsPlatformLayer::headless("App");
    let menu_id = layer.menu_bar.create_menu("Edit");
    let cut = layer.menu_bar.alloc_item_id();
    let copy = layer.menu_bar.alloc_item_id();
    let paste = layer.menu_bar.alloc_item_id();
    {
        let menu = layer.menu_bar.get_menu_mut(menu_id).unwrap();
        menu.add_item(MenuItem::action(cut, "Cut"));
        menu.add_item(MenuItem::action(copy, "Copy"));
        menu.add_item(MenuItem::action(paste, "Paste"));
    }

    layer.activate_menu_item(cut);
    layer.activate_menu_item(copy);
    layer.activate_menu_item(paste);

    let actions = layer.poll_menu_actions();
    assert_eq!(actions[0].label, "Cut");
    assert_eq!(actions[1].label, "Copy");
    assert_eq!(actions[2].label, "Paste");
}

#[test]
fn poll_menu_actions_clears_queue() {
    let mut layer = MacOsPlatformLayer::headless("App");
    let menu_id = layer.menu_bar.create_menu("File");
    let id = layer.menu_bar.alloc_item_id();
    layer
        .menu_bar
        .get_menu_mut(menu_id)
        .unwrap()
        .add_item(MenuItem::action(id, "Save"));

    layer.activate_menu_item(id);
    assert_eq!(layer.pending_menu_action_count(), 1);

    let _ = layer.poll_menu_actions();
    assert_eq!(layer.pending_menu_action_count(), 0);
}

#[test]
fn checkbox_toggle_via_activation() {
    let mut layer = MacOsPlatformLayer::headless("App");
    let menu_id = layer.menu_bar.create_menu("View");
    let grid_id = layer.menu_bar.alloc_item_id();
    layer
        .menu_bar
        .get_menu_mut(menu_id)
        .unwrap()
        .add_item(MenuItem::checkbox(grid_id, "Show Grid", false));

    // Toggle on.
    layer.activate_menu_item(grid_id);
    assert!(layer.menu_bar.find_item(grid_id).unwrap().1.is_checked());

    // Toggle off.
    layer.activate_menu_item(grid_id);
    assert!(!layer.menu_bar.find_item(grid_id).unwrap().1.is_checked());
}

// ── Display info ──────────────────────────────────────────────────────

#[test]
fn headless_display_info_defaults() {
    let layer = MacOsPlatformLayer::headless("App");
    assert!(!layer.is_retina());
    assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);
    assert_eq!(layer.system_theme(), SystemTheme::Light);
    assert!(!layer.reduce_motion());
}

#[test]
fn dark_mode_toggle() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.set_system_theme(SystemTheme::Dark);
    assert_eq!(layer.system_theme(), SystemTheme::Dark);
    layer.set_system_theme(SystemTheme::Light);
    assert_eq!(layer.system_theme(), SystemTheme::Light);
}

#[test]
fn reduce_motion_toggle() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.set_reduce_motion(true);
    assert!(layer.reduce_motion());
    layer.set_reduce_motion(false);
    assert!(!layer.reduce_motion());
}

#[test]
fn retina_override() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.display_info.retina = true;
    layer.display_info.scale_factor = 2.0;
    assert!(layer.is_retina());
    assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
}

// ── Dock ──────────────────────────────────────────────────────────────

#[test]
fn dock_badge_count() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.set_dock_badge(DockBadge::Count(42));
    assert_eq!(layer.dock_badge(), &DockBadge::Count(42));
}

#[test]
fn dock_badge_text() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.set_dock_badge(DockBadge::Text("!".to_string()));
    assert_eq!(layer.dock_badge(), &DockBadge::Text("!".to_string()));
}

#[test]
fn dock_badge_clear() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.set_dock_badge(DockBadge::Count(5));
    layer.set_dock_badge(DockBadge::None);
    assert_eq!(layer.dock_badge(), &DockBadge::None);
}

// ── Platform backend delegation ───────────────────────────────────────

#[test]
fn backend_event_polling() {
    let mut layer = MacOsPlatformLayer::headless("App");
    layer.backend_mut().push_event(WindowEvent::FocusGained);
    layer.backend_mut().push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });

    let events = layer.poll_window_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], WindowEvent::FocusGained);
}

#[test]
fn backend_quit_signal() {
    let mut layer = MacOsPlatformLayer::headless("App");
    assert!(!layer.should_quit());
    layer.backend_mut().request_quit();
    assert!(layer.should_quit());
}

#[test]
fn backend_frame_counting() {
    let mut layer = MacOsPlatformLayer::headless("App");
    assert_eq!(layer.backend().frames_run(), 0);
    layer.end_frame();
    layer.end_frame();
    layer.end_frame();
    assert_eq!(layer.backend().frames_run(), 3);
}

#[test]
fn frontmost_state_tracking() {
    let mut layer = MacOsPlatformLayer::headless("App");
    assert!(layer.is_frontmost());
    layer.set_frontmost(false);
    assert!(!layer.is_frontmost());
}

// ── Full workflow ─────────────────────────────────────────────────────

#[test]
fn complete_macos_editor_workflow() {
    let config = WindowConfig::new()
        .with_size(1280, 720)
        .with_title("Patina Editor");
    let mut layer = MacOsPlatformLayer::new("Patina Editor", &config);
    // Force headless for testing.
    layer.menu_bar =
        gdplatform::native_menu::NativeMenuBar::with_platform(MenuBarPlatform::Headless);

    // 1. Create standard app menu.
    layer.create_app_menu();
    assert!(layer.has_app_menu());

    // 2. Add editor menus.
    let file_id = layer.menu_bar.create_menu("File");
    let edit_id = layer.menu_bar.create_menu("Edit");
    let view_id = layer.menu_bar.create_menu("View");

    let new_scene = layer.menu_bar.alloc_item_id();
    let save_scene = layer.menu_bar.alloc_item_id();
    {
        let file = layer.menu_bar.get_menu_mut(file_id).unwrap();
        file.add_item(
            MenuItem::action(new_scene, "New Scene").with_shortcut(MenuShortcut::cmd("N")),
        );
        file.add_item(
            MenuItem::action(save_scene, "Save Scene").with_shortcut(MenuShortcut::cmd("S")),
        );
    }

    let undo = layer.menu_bar.alloc_item_id();
    let redo = layer.menu_bar.alloc_item_id();
    {
        let edit = layer.menu_bar.get_menu_mut(edit_id).unwrap();
        edit.add_item(MenuItem::action(undo, "Undo").with_shortcut(MenuShortcut::cmd("Z")));
        edit.add_item(MenuItem::action(redo, "Redo").with_shortcut(MenuShortcut::cmd_shift("Z")));
    }

    let show_grid = layer.menu_bar.alloc_item_id();
    let show_fps = layer.menu_bar.alloc_item_id();
    {
        let view = layer.menu_bar.get_menu_mut(view_id).unwrap();
        view.add_item(MenuItem::checkbox(show_grid, "Show Grid", true));
        view.add_item(MenuItem::checkbox(show_fps, "Show FPS", false));
    }

    assert_eq!(layer.menu_bar.menu_count(), 4); // App, File, Edit, View

    // 3. Simulate user actions.
    layer.activate_menu_item(save_scene);
    layer.activate_menu_item(show_grid); // Toggle grid off.
    layer.activate_menu_item(show_fps); // Toggle FPS on.

    let actions = layer.poll_menu_actions();
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[0].label, "Save Scene");
    assert_eq!(actions[1].label, "Show Grid");
    assert_eq!(actions[2].label, "Show FPS");

    // Verify checkbox states.
    assert!(!layer.menu_bar.find_item(show_grid).unwrap().1.is_checked());
    assert!(layer.menu_bar.find_item(show_fps).unwrap().1.is_checked());

    // 4. Display and accessibility.
    layer.set_system_theme(SystemTheme::Dark);
    assert_eq!(layer.system_theme(), SystemTheme::Dark);

    // 5. Dock badge.
    layer.set_dock_badge(DockBadge::Count(1));
    assert_eq!(layer.dock_badge(), &DockBadge::Count(1));

    // 6. Frame loop.
    assert_eq!(layer.window_size(), (1280, 720));
    for _ in 0..60 {
        layer.end_frame();
    }
    assert_eq!(layer.backend().frames_run(), 60);

    // 7. Quit.
    layer.backend_mut().request_quit();
    assert!(layer.should_quit());
}
