//! Native menu bar integration for desktop platforms.
//!
//! Provides a platform-agnostic API for building native application menus
//! (menu bars, submenus, items with shortcuts). The primary target is macOS
//! where the menu bar lives outside the window, but the API works on all
//! desktop platforms.
//!
//! Mirrors Godot's `NativeMenu` singleton and `DisplayServer` menu methods.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// MenuItemId / MenuId
// ---------------------------------------------------------------------------

/// Opaque identifier for a menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuItemId(pub u64);

/// Opaque identifier for a menu (top-level or submenu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuId(pub u64);

// ---------------------------------------------------------------------------
// MenuShortcut
// ---------------------------------------------------------------------------

/// A keyboard shortcut for a menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuShortcut {
    /// The key character or name (e.g. "Q", "N", "F1").
    pub key: String,
    /// Whether Cmd (macOS) / Ctrl (other) is required.
    pub command: bool,
    /// Whether Shift is required.
    pub shift: bool,
    /// Whether Alt/Option is required.
    pub alt: bool,
}

impl MenuShortcut {
    /// Creates a shortcut with the command modifier (Cmd on macOS, Ctrl elsewhere).
    pub fn cmd(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            command: true,
            shift: false,
            alt: false,
        }
    }

    /// Creates a shortcut with Cmd+Shift.
    pub fn cmd_shift(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            command: true,
            shift: true,
            alt: false,
        }
    }

    /// Returns the display string for this shortcut (e.g. "Cmd+Q").
    pub fn display_string(&self, is_macos: bool) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push(if is_macos { "Cmd" } else { "Ctrl" });
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push(if is_macos { "Option" } else { "Alt" });
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

// ---------------------------------------------------------------------------
// MenuItemKind
// ---------------------------------------------------------------------------

/// The type of a menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemKind {
    /// A normal clickable action item.
    Action,
    /// A checkable (toggle) item.
    CheckBox { checked: bool },
    /// A separator line.
    Separator,
    /// A submenu that opens another menu.
    Submenu { menu_id: MenuId },
}

// ---------------------------------------------------------------------------
// MenuItem
// ---------------------------------------------------------------------------

/// A single item in a native menu.
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// Unique identifier for this item.
    pub id: MenuItemId,
    /// Display label (empty for separators).
    pub label: String,
    /// The kind of menu item.
    pub kind: MenuItemKind,
    /// Optional keyboard shortcut.
    pub shortcut: Option<MenuShortcut>,
    /// Whether the item is enabled (grayed out if false).
    pub enabled: bool,
    /// Optional tooltip text.
    pub tooltip: String,
    /// Optional tag for application-specific data.
    pub tag: i64,
}

impl MenuItem {
    /// Creates a new action menu item.
    pub fn action(id: MenuItemId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: MenuItemKind::Action,
            shortcut: None,
            enabled: true,
            tooltip: String::new(),
            tag: 0,
        }
    }

    /// Creates a new checkbox menu item.
    pub fn checkbox(id: MenuItemId, label: impl Into<String>, checked: bool) -> Self {
        Self {
            id,
            label: label.into(),
            kind: MenuItemKind::CheckBox { checked },
            shortcut: None,
            enabled: true,
            tooltip: String::new(),
            tag: 0,
        }
    }

    /// Creates a separator.
    pub fn separator(id: MenuItemId) -> Self {
        Self {
            id,
            label: String::new(),
            kind: MenuItemKind::Separator,
            shortcut: None,
            enabled: true,
            tooltip: String::new(),
            tag: 0,
        }
    }

    /// Creates a submenu item.
    pub fn submenu(id: MenuItemId, label: impl Into<String>, menu_id: MenuId) -> Self {
        Self {
            id,
            label: label.into(),
            kind: MenuItemKind::Submenu { menu_id },
            shortcut: None,
            enabled: true,
            tooltip: String::new(),
            tag: 0,
        }
    }

    /// Sets the keyboard shortcut.
    pub fn with_shortcut(mut self, shortcut: MenuShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets the enabled state.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the tooltip.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }

    /// Sets the tag.
    pub fn with_tag(mut self, tag: i64) -> Self {
        self.tag = tag;
        self
    }

    /// Returns `true` if this is a separator.
    pub fn is_separator(&self) -> bool {
        matches!(self.kind, MenuItemKind::Separator)
    }

    /// Returns `true` if this is a checkbox and it's checked.
    pub fn is_checked(&self) -> bool {
        matches!(self.kind, MenuItemKind::CheckBox { checked: true })
    }

    /// Toggles a checkbox item. No-op for non-checkbox items.
    pub fn toggle(&mut self) {
        if let MenuItemKind::CheckBox { checked } = &mut self.kind {
            *checked = !*checked;
        }
    }
}

// ---------------------------------------------------------------------------
// NativeMenu
// ---------------------------------------------------------------------------

/// A single menu (can be a top-level menu bar entry or a submenu).
#[derive(Debug, Clone)]
pub struct NativeMenu {
    /// Unique identifier for this menu.
    pub id: MenuId,
    /// Display label for this menu (shown in the menu bar).
    pub label: String,
    /// Items in this menu, in display order.
    pub items: Vec<MenuItem>,
}

impl NativeMenu {
    /// Creates a new empty menu.
    pub fn new(id: MenuId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            items: Vec::new(),
        }
    }

    /// Adds an item and returns its id.
    pub fn add_item(&mut self, item: MenuItem) -> MenuItemId {
        let id = item.id;
        self.items.push(item);
        id
    }

    /// Returns the number of items (including separators).
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Returns an item by its id, if it exists in this menu.
    pub fn get_item(&self, id: MenuItemId) -> Option<&MenuItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Returns a mutable reference to an item by its id.
    pub fn get_item_mut(&mut self, id: MenuItemId) -> Option<&mut MenuItem> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    /// Removes an item by its id. Returns `true` if found and removed.
    pub fn remove_item(&mut self, id: MenuItemId) -> bool {
        let len = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() < len
    }

    /// Clears all items from this menu.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

// ---------------------------------------------------------------------------
// NativeMenuBar
// ---------------------------------------------------------------------------

/// The application's native menu bar.
///
/// Manages a collection of top-level menus. On macOS, this maps to the
/// system menu bar; on other platforms, it maps to the window's menu bar.
///
/// Mirrors Godot's `NativeMenu` singleton.
#[derive(Debug, Clone)]
pub struct NativeMenuBar {
    /// Top-level menus in display order.
    menus: Vec<NativeMenu>,
    /// All menus by id (including submenus).
    menu_map: HashMap<MenuId, usize>,
    /// Next auto-generated menu id.
    next_menu_id: u64,
    /// Next auto-generated item id.
    next_item_id: u64,
    /// The platform this menu bar targets.
    pub platform: MenuBarPlatform,
}

/// Which platform style to use for menu bar rendering/behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuBarPlatform {
    /// macOS: menu bar is at the top of the screen, outside the window.
    MacOS,
    /// Windows/Linux: menu bar is inside the window.
    Desktop,
    /// Headless: no actual rendering, for testing.
    Headless,
}

impl Default for MenuBarPlatform {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else {
            Self::Desktop
        }
    }
}

impl NativeMenuBar {
    /// Creates a new empty menu bar with platform auto-detection.
    pub fn new() -> Self {
        Self {
            menus: Vec::new(),
            menu_map: HashMap::new(),
            next_menu_id: 1,
            next_item_id: 1,
            platform: MenuBarPlatform::default(),
        }
    }

    /// Creates a new menu bar for a specific platform.
    pub fn with_platform(platform: MenuBarPlatform) -> Self {
        Self {
            menus: Vec::new(),
            menu_map: HashMap::new(),
            next_menu_id: 1,
            next_item_id: 1,
            platform,
        }
    }

    /// Allocates a new unique menu id.
    pub fn alloc_menu_id(&mut self) -> MenuId {
        let id = MenuId(self.next_menu_id);
        self.next_menu_id += 1;
        id
    }

    /// Allocates a new unique menu item id.
    pub fn alloc_item_id(&mut self) -> MenuItemId {
        let id = MenuItemId(self.next_item_id);
        self.next_item_id += 1;
        id
    }

    /// Adds a top-level menu and returns its id.
    pub fn add_menu(&mut self, menu: NativeMenu) -> MenuId {
        let id = menu.id;
        let idx = self.menus.len();
        self.menu_map.insert(id, idx);
        self.menus.push(menu);
        id
    }

    /// Creates and adds a new top-level menu with the given label.
    pub fn create_menu(&mut self, label: impl Into<String>) -> MenuId {
        let id = self.alloc_menu_id();
        let menu = NativeMenu::new(id, label);
        self.add_menu(menu)
    }

    /// Returns the number of top-level menus.
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// Returns a reference to all top-level menus.
    pub fn menus(&self) -> &[NativeMenu] {
        &self.menus
    }

    /// Returns a top-level menu by its id.
    pub fn get_menu(&self, id: MenuId) -> Option<&NativeMenu> {
        self.menu_map.get(&id).and_then(|&idx| self.menus.get(idx))
    }

    /// Returns a mutable reference to a top-level menu by its id.
    pub fn get_menu_mut(&mut self, id: MenuId) -> Option<&mut NativeMenu> {
        self.menu_map
            .get(&id)
            .copied()
            .and_then(|idx| self.menus.get_mut(idx))
    }

    /// Removes a top-level menu by its id. Returns `true` if found.
    pub fn remove_menu(&mut self, id: MenuId) -> bool {
        if let Some(&idx) = self.menu_map.get(&id) {
            self.menus.remove(idx);
            self.menu_map.remove(&id);
            // Re-index remaining menus.
            self.menu_map.clear();
            for (i, menu) in self.menus.iter().enumerate() {
                self.menu_map.insert(menu.id, i);
            }
            true
        } else {
            false
        }
    }

    /// Finds a menu item by id across all menus.
    pub fn find_item(&self, item_id: MenuItemId) -> Option<(&NativeMenu, &MenuItem)> {
        for menu in &self.menus {
            if let Some(item) = menu.get_item(item_id) {
                return Some((menu, item));
            }
        }
        None
    }

    /// Finds a mutable menu item by id across all menus.
    pub fn find_item_mut(&mut self, item_id: MenuItemId) -> Option<&mut MenuItem> {
        for menu in &mut self.menus {
            if let Some(item) = menu.get_item_mut(item_id) {
                return Some(item);
            }
        }
        None
    }

    /// Builds a standard macOS application menu ("App" menu with About, Quit, etc.).
    pub fn create_macos_app_menu(&mut self, app_name: &str) -> MenuId {
        let menu_id = self.alloc_menu_id();
        let mut menu = NativeMenu::new(menu_id, app_name);

        let about_id = self.alloc_item_id();
        menu.add_item(MenuItem::action(about_id, format!("About {app_name}")));

        let sep1_id = self.alloc_item_id();
        menu.add_item(MenuItem::separator(sep1_id));

        let hide_id = self.alloc_item_id();
        menu.add_item(
            MenuItem::action(hide_id, format!("Hide {app_name}"))
                .with_shortcut(MenuShortcut::cmd("H")),
        );

        let hide_others_id = self.alloc_item_id();
        menu.add_item(
            MenuItem::action(hide_others_id, "Hide Others")
                .with_shortcut(MenuShortcut::cmd_shift("H")),
        );

        let show_all_id = self.alloc_item_id();
        menu.add_item(MenuItem::action(show_all_id, "Show All"));

        let sep2_id = self.alloc_item_id();
        menu.add_item(MenuItem::separator(sep2_id));

        let quit_id = self.alloc_item_id();
        menu.add_item(
            MenuItem::action(quit_id, format!("Quit {app_name}"))
                .with_shortcut(MenuShortcut::cmd("Q")),
        );

        self.add_menu(menu)
    }

    /// Returns whether we're on macOS (menu bar is global, not per-window).
    pub fn is_global_menu_bar(&self) -> bool {
        self.platform == MenuBarPlatform::MacOS
    }

    /// Clears all menus.
    pub fn clear(&mut self) {
        self.menus.clear();
        self.menu_map.clear();
    }
}

impl Default for NativeMenuBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_empty_by_default() {
        let bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        assert_eq!(bar.menu_count(), 0);
        assert!(bar.menus().is_empty());
    }

    #[test]
    fn create_menu_and_add_items() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        let menu_id = bar.create_menu("File");
        assert_eq!(bar.menu_count(), 1);

        let menu = bar.get_menu_mut(menu_id).unwrap();
        let item_id = MenuItemId(100);
        menu.add_item(MenuItem::action(item_id, "New"));
        menu.add_item(MenuItem::action(MenuItemId(101), "Open"));
        menu.add_item(MenuItem::separator(MenuItemId(102)));
        menu.add_item(MenuItem::action(MenuItemId(103), "Quit"));

        assert_eq!(menu.item_count(), 4);
    }

    #[test]
    fn menu_item_action_defaults() {
        let item = MenuItem::action(MenuItemId(1), "Save");
        assert_eq!(item.label, "Save");
        assert!(item.enabled);
        assert!(item.shortcut.is_none());
        assert_eq!(item.tag, 0);
        assert!(!item.is_separator());
        assert!(!item.is_checked());
    }

    #[test]
    fn menu_item_checkbox_toggle() {
        let mut item = MenuItem::checkbox(MenuItemId(1), "Show Grid", false);
        assert!(!item.is_checked());
        item.toggle();
        assert!(item.is_checked());
        item.toggle();
        assert!(!item.is_checked());
    }

    #[test]
    fn toggle_on_action_is_noop() {
        let mut item = MenuItem::action(MenuItemId(1), "Action");
        item.toggle(); // Should not panic.
        assert!(!item.is_checked());
    }

    #[test]
    fn menu_item_separator() {
        let item = MenuItem::separator(MenuItemId(1));
        assert!(item.is_separator());
        assert!(item.label.is_empty());
    }

    #[test]
    fn menu_item_submenu() {
        let item = MenuItem::submenu(MenuItemId(1), "Recent Files", MenuId(10));
        assert!(matches!(
            item.kind,
            MenuItemKind::Submenu { menu_id: MenuId(10) }
        ));
    }

    #[test]
    fn menu_item_with_shortcut() {
        let item = MenuItem::action(MenuItemId(1), "Quit")
            .with_shortcut(MenuShortcut::cmd("Q"));
        let shortcut = item.shortcut.as_ref().unwrap();
        assert_eq!(shortcut.key, "Q");
        assert!(shortcut.command);
        assert!(!shortcut.shift);
    }

    #[test]
    fn shortcut_display_string_macos() {
        let s = MenuShortcut::cmd("Q");
        assert_eq!(s.display_string(true), "Cmd+Q");
    }

    #[test]
    fn shortcut_display_string_non_macos() {
        let s = MenuShortcut::cmd("Q");
        assert_eq!(s.display_string(false), "Ctrl+Q");
    }

    #[test]
    fn shortcut_cmd_shift_display() {
        let s = MenuShortcut::cmd_shift("N");
        assert_eq!(s.display_string(true), "Cmd+Shift+N");
        assert_eq!(s.display_string(false), "Ctrl+Shift+N");
    }

    #[test]
    fn shortcut_with_alt() {
        let s = MenuShortcut {
            key: "F".to_string(),
            command: true,
            shift: false,
            alt: true,
        };
        assert_eq!(s.display_string(true), "Cmd+Option+F");
        assert_eq!(s.display_string(false), "Ctrl+Alt+F");
    }

    #[test]
    fn menu_get_and_remove_item() {
        let mut menu = NativeMenu::new(MenuId(1), "Edit");
        let id1 = MenuItemId(1);
        let id2 = MenuItemId(2);
        menu.add_item(MenuItem::action(id1, "Cut"));
        menu.add_item(MenuItem::action(id2, "Copy"));

        assert!(menu.get_item(id1).is_some());
        assert_eq!(menu.get_item(id1).unwrap().label, "Cut");

        assert!(menu.remove_item(id1));
        assert!(menu.get_item(id1).is_none());
        assert_eq!(menu.item_count(), 1);
    }

    #[test]
    fn menu_clear() {
        let mut menu = NativeMenu::new(MenuId(1), "View");
        menu.add_item(MenuItem::action(MenuItemId(1), "Zoom In"));
        menu.add_item(MenuItem::action(MenuItemId(2), "Zoom Out"));
        menu.clear();
        assert_eq!(menu.item_count(), 0);
    }

    #[test]
    fn menu_bar_remove_menu() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        let file_id = bar.create_menu("File");
        let edit_id = bar.create_menu("Edit");
        assert_eq!(bar.menu_count(), 2);

        assert!(bar.remove_menu(file_id));
        assert_eq!(bar.menu_count(), 1);
        assert!(bar.get_menu(file_id).is_none());
        assert!(bar.get_menu(edit_id).is_some());
    }

    #[test]
    fn menu_bar_find_item_across_menus() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        let file_id = bar.create_menu("File");
        let edit_id = bar.create_menu("Edit");

        let save_id = MenuItemId(10);
        bar.get_menu_mut(file_id)
            .unwrap()
            .add_item(MenuItem::action(save_id, "Save"));

        let paste_id = MenuItemId(20);
        bar.get_menu_mut(edit_id)
            .unwrap()
            .add_item(MenuItem::action(paste_id, "Paste"));

        let (menu, item) = bar.find_item(save_id).unwrap();
        assert_eq!(menu.label, "File");
        assert_eq!(item.label, "Save");

        let (menu, item) = bar.find_item(paste_id).unwrap();
        assert_eq!(menu.label, "Edit");
        assert_eq!(item.label, "Paste");

        assert!(bar.find_item(MenuItemId(999)).is_none());
    }

    #[test]
    fn menu_bar_find_item_mut() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        let menu_id = bar.create_menu("File");
        let item_id = MenuItemId(1);
        bar.get_menu_mut(menu_id)
            .unwrap()
            .add_item(MenuItem::action(item_id, "Save").with_enabled(false));

        let item = bar.find_item_mut(item_id).unwrap();
        assert!(!item.enabled);
        item.enabled = true;

        let (_, item) = bar.find_item(item_id).unwrap();
        assert!(item.enabled);
    }

    #[test]
    fn macos_app_menu_structure() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::MacOS);
        let app_menu_id = bar.create_macos_app_menu("Patina Engine");
        let menu = bar.get_menu(app_menu_id).unwrap();

        assert_eq!(menu.label, "Patina Engine");
        // About, separator, Hide, Hide Others, Show All, separator, Quit
        assert_eq!(menu.item_count(), 7);
        assert_eq!(menu.items[0].label, "About Patina Engine");
        assert!(menu.items[1].is_separator());
        assert_eq!(menu.items[6].label, "Quit Patina Engine");

        // Quit should have Cmd+Q shortcut.
        let quit = &menu.items[6];
        let shortcut = quit.shortcut.as_ref().unwrap();
        assert_eq!(shortcut.key, "Q");
        assert!(shortcut.command);
    }

    #[test]
    fn is_global_menu_bar_macos() {
        let bar = NativeMenuBar::with_platform(MenuBarPlatform::MacOS);
        assert!(bar.is_global_menu_bar());
    }

    #[test]
    fn is_global_menu_bar_desktop() {
        let bar = NativeMenuBar::with_platform(MenuBarPlatform::Desktop);
        assert!(!bar.is_global_menu_bar());
    }

    #[test]
    fn menu_bar_clear() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        bar.create_menu("File");
        bar.create_menu("Edit");
        bar.clear();
        assert_eq!(bar.menu_count(), 0);
    }

    #[test]
    fn alloc_ids_are_unique() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        let m1 = bar.alloc_menu_id();
        let m2 = bar.alloc_menu_id();
        assert_ne!(m1, m2);

        let i1 = bar.alloc_item_id();
        let i2 = bar.alloc_item_id();
        assert_ne!(i1, i2);
    }

    #[test]
    fn menu_item_with_tooltip_and_tag() {
        let item = MenuItem::action(MenuItemId(1), "Export")
            .with_tooltip("Export the current project")
            .with_tag(42);
        assert_eq!(item.tooltip, "Export the current project");
        assert_eq!(item.tag, 42);
    }

    #[test]
    fn menu_item_disabled() {
        let item = MenuItem::action(MenuItemId(1), "Undo").with_enabled(false);
        assert!(!item.enabled);
    }

    #[test]
    fn remove_nonexistent_menu_returns_false() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::Headless);
        assert!(!bar.remove_menu(MenuId(999)));
    }

    #[test]
    fn remove_nonexistent_item_returns_false() {
        let mut menu = NativeMenu::new(MenuId(1), "Test");
        assert!(!menu.remove_item(MenuItemId(999)));
    }

    #[test]
    fn full_menu_bar_workflow() {
        let mut bar = NativeMenuBar::with_platform(MenuBarPlatform::MacOS);

        // Create standard macOS app menu.
        bar.create_macos_app_menu("My Game");

        // File menu.
        let file_id = bar.create_menu("File");
        let new_id = bar.next_item_id;
        bar.next_item_id += 1;
        {
            let file = bar.get_menu_mut(file_id).unwrap();
            file.add_item(
                MenuItem::action(MenuItemId(new_id), "New Project")
                    .with_shortcut(MenuShortcut::cmd("N")),
            );
        }

        let open_id = bar.next_item_id;
        bar.next_item_id += 1;
        {
            let file = bar.get_menu_mut(file_id).unwrap();
            file.add_item(
                MenuItem::action(MenuItemId(open_id), "Open...")
                    .with_shortcut(MenuShortcut::cmd("O")),
            );
        }

        // Edit menu.
        let edit_id = bar.create_menu("Edit");
        let undo_id = bar.next_item_id;
        bar.next_item_id += 1;
        {
            let edit = bar.get_menu_mut(edit_id).unwrap();
            edit.add_item(
                MenuItem::action(MenuItemId(undo_id), "Undo")
                    .with_shortcut(MenuShortcut::cmd("Z")),
            );
        }

        // View menu with checkbox.
        let view_id = bar.create_menu("View");
        let grid_id = bar.next_item_id;
        bar.next_item_id += 1;
        let view = bar.get_menu_mut(view_id).unwrap();
        view.add_item(MenuItem::checkbox(MenuItemId(grid_id), "Show Grid", true));

        assert_eq!(bar.menu_count(), 4); // App, File, Edit, View
        assert!(bar.is_global_menu_bar());

        // Toggle grid.
        let item = bar.find_item_mut(MenuItemId(grid_id)).unwrap();
        assert!(item.is_checked());
        item.toggle();
        assert!(!item.is_checked());
    }
}
