//! macOS platform layer with native menu bar integration.
//!
//! Provides [`MacOsPlatformLayer`] which combines:
//! - A [`PlatformBackend`] for event polling and frame driving
//! - A [`NativeMenuBar`] wired to macOS system menus
//! - macOS-specific display queries (Retina scale, dark mode, dock)
//! - Menu action events routed through the platform event system
//!
//! On non-macOS platforms the layer works in headless mode for testing.

use std::collections::VecDeque;

use crate::backend::{HeadlessPlatform, PlatformBackend};
use crate::native_menu::{MenuBarPlatform, MenuItemId, NativeMenuBar};
use crate::os::Platform;
use crate::window::{WindowConfig, WindowEvent};

// ---------------------------------------------------------------------------
// MenuAction
// ---------------------------------------------------------------------------

/// An action triggered by a native menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuAction {
    /// The menu item that was activated.
    pub item_id: MenuItemId,
    /// The label of the menu item (for logging / debugging).
    pub label: String,
}

// ---------------------------------------------------------------------------
// SystemTheme
// ---------------------------------------------------------------------------

/// The system-wide appearance theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    /// Light appearance (default on most systems).
    Light,
    /// Dark appearance (macOS dark mode, etc.).
    Dark,
}

// ---------------------------------------------------------------------------
// DockBadge
// ---------------------------------------------------------------------------

/// Badge content shown on the dock icon (macOS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockBadge {
    /// No badge (clear any existing badge).
    None,
    /// A numeric badge (e.g. unread count).
    Count(u32),
    /// A text badge (e.g. "!").
    Text(String),
}

// ---------------------------------------------------------------------------
// MacOsDisplayInfo
// ---------------------------------------------------------------------------

/// macOS-specific display information.
#[derive(Debug, Clone, PartialEq)]
pub struct MacOsDisplayInfo {
    /// Whether the display supports Retina (HiDPI).
    pub retina: bool,
    /// The backing scale factor (1.0 for standard, 2.0 for Retina).
    pub scale_factor: f32,
    /// The current system appearance theme.
    pub theme: SystemTheme,
    /// Whether "Reduce Motion" is enabled in Accessibility settings.
    pub reduce_motion: bool,
    /// Whether "Reduce Transparency" is enabled.
    pub reduce_transparency: bool,
}

impl Default for MacOsDisplayInfo {
    fn default() -> Self {
        Self {
            retina: cfg!(target_os = "macos"),
            scale_factor: if cfg!(target_os = "macos") { 2.0 } else { 1.0 },
            theme: SystemTheme::Light,
            reduce_motion: false,
            reduce_transparency: false,
        }
    }
}

// ---------------------------------------------------------------------------
// MacOsPlatformLayer
// ---------------------------------------------------------------------------

/// macOS-specific platform layer that integrates native menu bar, display
/// queries, and platform backend into a single cohesive interface.
///
/// Mirrors the macOS-specific parts of Godot's `DisplayServer` and `OS`
/// singletons. On non-macOS platforms, this operates in a headless/simulated
/// mode suitable for testing.
#[derive(Debug)]
pub struct MacOsPlatformLayer {
    /// The native menu bar.
    pub menu_bar: NativeMenuBar,
    /// macOS display information.
    pub display_info: MacOsDisplayInfo,
    /// Pending menu actions (queued when a menu item is activated).
    menu_actions: VecDeque<MenuAction>,
    /// The underlying platform backend for window/event management.
    backend: HeadlessPlatform,
    /// The application name (used for the macOS app menu).
    app_name: String,
    /// Dock badge state.
    dock_badge: DockBadge,
    /// Whether the app menu has been created.
    app_menu_created: bool,
    /// Whether the application is the frontmost (active) application.
    is_frontmost: bool,
}

impl MacOsPlatformLayer {
    /// Creates a new macOS platform layer with the given app name and window config.
    pub fn new(app_name: impl Into<String>, config: &WindowConfig) -> Self {
        let app_name = app_name.into();
        let platform = if cfg!(target_os = "macos") {
            MenuBarPlatform::MacOS
        } else {
            MenuBarPlatform::Headless
        };

        Self {
            menu_bar: NativeMenuBar::with_platform(platform),
            display_info: MacOsDisplayInfo::default(),
            menu_actions: VecDeque::new(),
            backend: HeadlessPlatform::from_config(config),
            app_name,
            dock_badge: DockBadge::None,
            app_menu_created: false,
            is_frontmost: true,
        }
    }

    /// Creates a headless macOS platform layer for testing.
    pub fn headless(app_name: impl Into<String>) -> Self {
        let config = WindowConfig::default();
        let mut layer = Self::new(app_name, &config);
        layer.menu_bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        layer.display_info = MacOsDisplayInfo {
            retina: false,
            scale_factor: 1.0,
            theme: SystemTheme::Light,
            reduce_motion: false,
            reduce_transparency: false,
        };
        layer
    }

    /// Returns the application name.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    // -- Menu bar integration -------------------------------------------------

    /// Creates the standard macOS application menu (About, Hide, Quit, etc.).
    ///
    /// This should be called once during initialization. Calling it multiple
    /// times replaces the existing app menu.
    pub fn create_app_menu(&mut self) {
        if !self.app_menu_created {
            self.menu_bar.create_macos_app_menu(&self.app_name);
            self.app_menu_created = true;
        }
    }

    /// Returns whether the app menu has been created.
    pub fn has_app_menu(&self) -> bool {
        self.app_menu_created
    }

    /// Simulates a menu item activation (for testing or programmatic triggering).
    pub fn activate_menu_item(&mut self, item_id: MenuItemId) {
        // Look up the label for the action.
        let label = self
            .menu_bar
            .find_item(item_id)
            .map(|(_, item)| item.label.clone())
            .unwrap_or_default();

        // Toggle checkboxes automatically.
        if let Some(item) = self.menu_bar.find_item_mut(item_id) {
            item.toggle();
        }

        self.menu_actions.push_back(MenuAction { item_id, label });
    }

    /// Drains all pending menu actions.
    pub fn poll_menu_actions(&mut self) -> Vec<MenuAction> {
        self.menu_actions.drain(..).collect()
    }

    /// Returns the next pending menu action without removing it.
    pub fn peek_menu_action(&self) -> Option<&MenuAction> {
        self.menu_actions.front()
    }

    /// Returns the number of pending menu actions.
    pub fn pending_menu_action_count(&self) -> usize {
        self.menu_actions.len()
    }

    // -- Display queries ------------------------------------------------------

    /// Returns whether the display supports Retina (HiDPI).
    pub fn is_retina(&self) -> bool {
        self.display_info.retina
    }

    /// Returns the backing scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.display_info.scale_factor
    }

    /// Returns the current system theme.
    pub fn system_theme(&self) -> SystemTheme {
        self.display_info.theme
    }

    /// Sets the system theme (for testing or responding to system notifications).
    pub fn set_system_theme(&mut self, theme: SystemTheme) {
        self.display_info.theme = theme;
    }

    /// Returns whether the user has enabled "Reduce Motion" in Accessibility.
    pub fn reduce_motion(&self) -> bool {
        self.display_info.reduce_motion
    }

    /// Sets the reduce motion flag.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        self.display_info.reduce_motion = reduce;
    }

    // -- Dock integration -----------------------------------------------------

    /// Sets the dock badge content.
    pub fn set_dock_badge(&mut self, badge: DockBadge) {
        self.dock_badge = badge;
    }

    /// Returns the current dock badge.
    pub fn dock_badge(&self) -> &DockBadge {
        &self.dock_badge
    }

    // -- Application state ----------------------------------------------------

    /// Returns whether this application is the frontmost (active) app.
    pub fn is_frontmost(&self) -> bool {
        self.is_frontmost
    }

    /// Sets the frontmost state (called when app activation changes).
    pub fn set_frontmost(&mut self, frontmost: bool) {
        self.is_frontmost = frontmost;
    }

    /// Returns the current platform.
    pub fn platform(&self) -> Platform {
        crate::os::current_platform()
    }

    /// Returns whether we're running on actual macOS.
    pub fn is_native_macos(&self) -> bool {
        self.menu_bar.platform == MenuBarPlatform::MacOS
    }

    /// Returns whether the menu bar is global (macOS) vs per-window (other).
    pub fn is_global_menu_bar(&self) -> bool {
        self.menu_bar.is_global_menu_bar()
    }

    // -- Backend delegation ---------------------------------------------------

    /// Returns a reference to the underlying platform backend.
    pub fn backend(&self) -> &HeadlessPlatform {
        &self.backend
    }

    /// Returns a mutable reference to the underlying platform backend.
    pub fn backend_mut(&mut self) -> &mut HeadlessPlatform {
        &mut self.backend
    }

    /// Convenience: polls window events from the backend.
    pub fn poll_window_events(&mut self) -> Vec<WindowEvent> {
        self.backend.poll_events()
    }

    /// Convenience: checks if the app should quit.
    pub fn should_quit(&self) -> bool {
        self.backend.should_quit()
    }

    /// Convenience: ends the current frame.
    pub fn end_frame(&mut self) {
        self.backend.end_frame();
    }

    /// Convenience: returns the window size.
    pub fn window_size(&self) -> (u32, u32) {
        self.backend.window_size()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_menu::{MenuId, MenuItem, MenuItemKind, MenuShortcut};

    #[test]
    fn headless_layer_creation() {
        let layer = MacOsPlatformLayer::headless("Test App");
        assert_eq!(layer.app_name(), "Test App");
        assert!(!layer.has_app_menu());
        assert_eq!(layer.menu_bar.platform, MenuBarPlatform::Headless);
    }

    #[test]
    fn create_app_menu_populates_menu_bar() {
        let mut layer = MacOsPlatformLayer::headless("My Game");
        assert_eq!(layer.menu_bar.menu_count(), 0);
        layer.create_app_menu();
        assert!(layer.has_app_menu());
        assert_eq!(layer.menu_bar.menu_count(), 1);

        // App menu should have standard items.
        let menu = &layer.menu_bar.menus()[0];
        assert_eq!(menu.label, "My Game");
        assert_eq!(menu.item_count(), 7); // About, sep, Hide, Hide Others, Show All, sep, Quit
    }

    #[test]
    fn create_app_menu_is_idempotent() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.create_app_menu();
        layer.create_app_menu(); // Should not add a second menu.
        assert_eq!(layer.menu_bar.menu_count(), 1);
    }

    #[test]
    fn activate_menu_item_queues_action() {
        let mut layer = MacOsPlatformLayer::headless("App");
        let menu_id = layer.menu_bar.create_menu("File");
        let item_id = layer.menu_bar.alloc_item_id();
        layer
            .menu_bar
            .get_menu_mut(menu_id)
            .unwrap()
            .add_item(MenuItem::action(item_id, "Save"));

        layer.activate_menu_item(item_id);
        assert_eq!(layer.pending_menu_action_count(), 1);

        let actions = layer.poll_menu_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].item_id, item_id);
        assert_eq!(actions[0].label, "Save");
        assert_eq!(layer.pending_menu_action_count(), 0);
    }

    #[test]
    fn activate_checkbox_toggles_state() {
        let mut layer = MacOsPlatformLayer::headless("App");
        let menu_id = layer.menu_bar.create_menu("View");
        let item_id = layer.menu_bar.alloc_item_id();
        layer
            .menu_bar
            .get_menu_mut(menu_id)
            .unwrap()
            .add_item(MenuItem::checkbox(item_id, "Show Grid", false));

        // First activation should toggle to checked.
        layer.activate_menu_item(item_id);
        let (_, item) = layer.menu_bar.find_item(item_id).unwrap();
        assert!(item.is_checked());

        // Second activation toggles back.
        layer.activate_menu_item(item_id);
        let (_, item) = layer.menu_bar.find_item(item_id).unwrap();
        assert!(!item.is_checked());
    }

    #[test]
    fn peek_menu_action_does_not_consume() {
        let mut layer = MacOsPlatformLayer::headless("App");
        let menu_id = layer.menu_bar.create_menu("Edit");
        let item_id = layer.menu_bar.alloc_item_id();
        layer
            .menu_bar
            .get_menu_mut(menu_id)
            .unwrap()
            .add_item(MenuItem::action(item_id, "Undo"));

        layer.activate_menu_item(item_id);
        assert!(layer.peek_menu_action().is_some());
        assert_eq!(layer.pending_menu_action_count(), 1);
    }

    #[test]
    fn display_info_defaults() {
        let layer = MacOsPlatformLayer::headless("App");
        assert!(!layer.is_retina());
        assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);
        assert_eq!(layer.system_theme(), SystemTheme::Light);
        assert!(!layer.reduce_motion());
    }

    #[test]
    fn set_system_theme() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.set_system_theme(SystemTheme::Dark);
        assert_eq!(layer.system_theme(), SystemTheme::Dark);
    }

    #[test]
    fn set_reduce_motion() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.set_reduce_motion(true);
        assert!(layer.reduce_motion());
    }

    #[test]
    fn dock_badge_default_is_none() {
        let layer = MacOsPlatformLayer::headless("App");
        assert_eq!(layer.dock_badge(), &DockBadge::None);
    }

    #[test]
    fn set_dock_badge_count() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.set_dock_badge(DockBadge::Count(5));
        assert_eq!(layer.dock_badge(), &DockBadge::Count(5));
    }

    #[test]
    fn set_dock_badge_text() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.set_dock_badge(DockBadge::Text("!".to_string()));
        assert_eq!(layer.dock_badge(), &DockBadge::Text("!".to_string()));
    }

    #[test]
    fn frontmost_state() {
        let mut layer = MacOsPlatformLayer::headless("App");
        assert!(layer.is_frontmost());
        layer.set_frontmost(false);
        assert!(!layer.is_frontmost());
        layer.set_frontmost(true);
        assert!(layer.is_frontmost());
    }

    #[test]
    fn headless_is_not_native_macos() {
        let layer = MacOsPlatformLayer::headless("App");
        assert!(!layer.is_native_macos());
        assert!(!layer.is_global_menu_bar());
    }

    #[test]
    fn backend_delegation_window_size() {
        let config = WindowConfig::new().with_size(1920, 1080);
        let layer = MacOsPlatformLayer::new("App", &config);
        assert_eq!(layer.window_size(), (1920, 1080));
    }

    #[test]
    fn backend_delegation_should_quit() {
        let mut layer = MacOsPlatformLayer::headless("App");
        assert!(!layer.should_quit());
        layer.backend_mut().request_quit();
        assert!(layer.should_quit());
    }

    #[test]
    fn backend_delegation_poll_events() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.backend_mut().push_event(WindowEvent::FocusGained);
        let events = layer.poll_window_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], WindowEvent::FocusGained);
    }

    #[test]
    fn backend_delegation_end_frame() {
        let mut layer = MacOsPlatformLayer::headless("App");
        assert_eq!(layer.backend().frames_run(), 0);
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    #[test]
    fn full_macos_workflow() {
        let mut layer = MacOsPlatformLayer::headless("Patina Engine");

        // 1. Create app menu.
        layer.create_app_menu();
        assert!(layer.has_app_menu());

        // 2. Add File menu with items.
        let file_id = layer.menu_bar.create_menu("File");
        let new_id = layer.menu_bar.alloc_item_id();
        let save_id = layer.menu_bar.alloc_item_id();
        let quit_id = layer.menu_bar.alloc_item_id();
        {
            let file = layer.menu_bar.get_menu_mut(file_id).unwrap();
            file.add_item(
                MenuItem::action(new_id, "New Project").with_shortcut(MenuShortcut::cmd("N")),
            );
            file.add_item(MenuItem::action(save_id, "Save").with_shortcut(MenuShortcut::cmd("S")));
            file.add_item(MenuItem::action(quit_id, "Quit").with_shortcut(MenuShortcut::cmd("Q")));
        }

        // 3. Add View menu with toggle.
        let view_id = layer.menu_bar.create_menu("View");
        let grid_id = layer.menu_bar.alloc_item_id();
        layer
            .menu_bar
            .get_menu_mut(view_id)
            .unwrap()
            .add_item(MenuItem::checkbox(grid_id, "Show Grid", true));

        assert_eq!(layer.menu_bar.menu_count(), 3); // App, File, View

        // 4. Simulate user clicking Save.
        layer.activate_menu_item(save_id);
        let actions = layer.poll_menu_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].label, "Save");

        // 5. Toggle grid.
        layer.activate_menu_item(grid_id);
        let (_, item) = layer.menu_bar.find_item(grid_id).unwrap();
        assert!(!item.is_checked()); // Was true, toggled to false.

        // 6. Check display info.
        layer.set_system_theme(SystemTheme::Dark);
        assert_eq!(layer.system_theme(), SystemTheme::Dark);

        // 7. Set dock badge.
        layer.set_dock_badge(DockBadge::Count(3));
        assert_eq!(layer.dock_badge(), &DockBadge::Count(3));

        // 8. Simulate frame loop.
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    #[test]
    fn multiple_menu_actions_drain_in_order() {
        let mut layer = MacOsPlatformLayer::headless("App");
        let menu_id = layer.menu_bar.create_menu("File");
        let id1 = layer.menu_bar.alloc_item_id();
        let id2 = layer.menu_bar.alloc_item_id();
        let id3 = layer.menu_bar.alloc_item_id();
        {
            let menu = layer.menu_bar.get_menu_mut(menu_id).unwrap();
            menu.add_item(MenuItem::action(id1, "A"));
            menu.add_item(MenuItem::action(id2, "B"));
            menu.add_item(MenuItem::action(id3, "C"));
        }

        layer.activate_menu_item(id1);
        layer.activate_menu_item(id2);
        layer.activate_menu_item(id3);

        let actions = layer.poll_menu_actions();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].label, "A");
        assert_eq!(actions[1].label, "B");
        assert_eq!(actions[2].label, "C");
    }

    #[test]
    fn activate_nonexistent_item_queues_empty_label() {
        let mut layer = MacOsPlatformLayer::headless("App");
        let fake_id = MenuItemId(999);
        layer.activate_menu_item(fake_id);

        let actions = layer.poll_menu_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].item_id, fake_id);
        assert!(actions[0].label.is_empty());
    }

    #[test]
    fn display_info_retina_override() {
        let mut layer = MacOsPlatformLayer::headless("App");
        layer.display_info.retina = true;
        layer.display_info.scale_factor = 2.0;
        assert!(layer.is_retina());
        assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reduce_transparency_flag() {
        let mut layer = MacOsPlatformLayer::headless("App");
        assert!(!layer.display_info.reduce_transparency);
        layer.display_info.reduce_transparency = true;
        assert!(layer.display_info.reduce_transparency);
    }
}
