//! PopupMenu item model (pat-u3l8c.73).
//!
//! Godot's `PopupMenu` is an ordered list of items — normal, checkable,
//! separator, or submenu-opening — that emits `id_pressed(id)` when an enabled,
//! non-separator item is activated. This module provides that item model (the
//! logic layer); the popup *layer* rendering and modal input capture build on
//! top of it.

/// A single `PopupMenu` entry, mirroring Godot's PopupMenu item surface.
#[derive(Debug, Clone, PartialEq)]
pub struct PopupMenuItem {
    /// Display label.
    pub text: String,
    /// The id emitted by `id_pressed` when this item is activated.
    pub id: i64,
    /// When true the item cannot be activated (greyed out).
    pub disabled: bool,
    /// When true the item is a non-interactive separator/divider.
    pub separator: bool,
    /// When true the item shows a check box / radio state.
    pub checkable: bool,
    /// Current checked state (only meaningful when `checkable`).
    pub checked: bool,
    /// Name of the submenu this item opens instead of emitting `id_pressed`.
    pub submenu: Option<String>,
    /// Optional keyboard-shortcut label (display + accelerator lookup).
    pub shortcut: Option<String>,
}

impl PopupMenuItem {
    fn normal(text: String, id: i64) -> Self {
        Self {
            text,
            id,
            disabled: false,
            separator: false,
            checkable: false,
            checked: false,
            submenu: None,
            shortcut: None,
        }
    }
}

/// An ordered `PopupMenu`. Activating an enabled, non-separator, non-submenu
/// item yields its id (the `id_pressed` payload).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PopupMenu {
    items: Vec<PopupMenuItem>,
}

impl PopupMenu {
    /// Creates an empty popup menu.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Adds a normal item and returns its index. When `id < 0` the id defaults
    /// to the item's index (Godot's implicit-id behaviour).
    pub fn add_item(&mut self, text: impl Into<String>, id: i64) -> usize {
        let idx = self.items.len();
        let id = if id < 0 { idx as i64 } else { id };
        self.items.push(PopupMenuItem::normal(text.into(), id));
        idx
    }

    /// Adds a checkable item (defaults unchecked). Returns its index.
    pub fn add_check_item(&mut self, text: impl Into<String>, id: i64) -> usize {
        let idx = self.add_item(text, id);
        self.items[idx].checkable = true;
        idx
    }

    /// Adds a submenu-opening item. Activating it opens `submenu` rather than
    /// emitting `id_pressed`. Returns its index.
    pub fn add_submenu_item(
        &mut self,
        text: impl Into<String>,
        submenu: impl Into<String>,
    ) -> usize {
        let idx = self.add_item(text, -1);
        self.items[idx].submenu = Some(submenu.into());
        idx
    }

    /// Adds a non-interactive separator and returns its index.
    pub fn add_separator(&mut self) -> usize {
        let idx = self.items.len();
        let mut item = PopupMenuItem::normal(String::new(), -1);
        item.separator = true;
        item.disabled = true;
        self.items.push(item);
        idx
    }

    /// Number of items (including separators).
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Borrows the item at `idx`.
    pub fn get_item(&self, idx: usize) -> Option<&PopupMenuItem> {
        self.items.get(idx)
    }

    /// Enables or disables the item at `idx`.
    pub fn set_item_disabled(&mut self, idx: usize, disabled: bool) {
        if let Some(it) = self.items.get_mut(idx) {
            it.disabled = disabled;
        }
    }

    /// Sets the display shortcut label on the item at `idx`.
    pub fn set_item_shortcut(&mut self, idx: usize, shortcut: impl Into<String>) {
        if let Some(it) = self.items.get_mut(idx) {
            it.shortcut = Some(shortcut.into());
        }
    }

    /// Toggles a checkable item's checked state, returning the new state (or
    /// `false` for a missing / non-checkable item).
    pub fn toggle_item_checked(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(it) if it.checkable => {
                it.checked = !it.checked;
                it.checked
            }
            _ => false,
        }
    }

    /// Activates the item at `idx` (as a click would).
    ///
    /// Returns `Some(id)` — the `id_pressed` payload — for an enabled,
    /// non-separator, non-submenu item; `None` otherwise (disabled, separator,
    /// submenu-opener, or out of range). A checkable item also toggles its
    /// checked state.
    pub fn activate_item(&mut self, idx: usize) -> Option<i64> {
        let it = self.items.get(idx)?;
        if it.disabled || it.separator || it.submenu.is_some() {
            return None;
        }
        let id = it.id;
        let checkable = it.checkable;
        if checkable {
            self.toggle_item_checked(idx);
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_item_defaults_id_to_index_and_activates() {
        let mut menu = PopupMenu::new();
        let a = menu.add_item("Open", -1); // implicit id = index 0
        let b = menu.add_item("Save", 42); // explicit id
        assert_eq!(menu.item_count(), 2);
        assert_eq!(menu.activate_item(a), Some(0));
        assert_eq!(menu.activate_item(b), Some(42));
    }

    #[test]
    fn separators_disabled_and_submenu_items_do_not_emit() {
        let mut menu = PopupMenu::new();
        let normal = menu.add_item("Copy", 1);
        let sep = menu.add_separator();
        let disabled = menu.add_item("Paste", 2);
        menu.set_item_disabled(disabled, true);
        let sub = menu.add_submenu_item("Recent", "recent_menu");

        assert_eq!(menu.activate_item(normal), Some(1));
        assert_eq!(menu.activate_item(sep), None);
        assert_eq!(menu.activate_item(disabled), None);
        assert_eq!(menu.activate_item(sub), None);
        assert_eq!(menu.activate_item(999), None); // out of range
    }

    #[test]
    fn check_item_toggles_on_activate() {
        let mut menu = PopupMenu::new();
        let c = menu.add_check_item("Word Wrap", 7);
        assert!(!menu.get_item(c).unwrap().checked);
        assert_eq!(menu.activate_item(c), Some(7));
        assert!(
            menu.get_item(c).unwrap().checked,
            "activating toggles the check"
        );
        assert_eq!(menu.activate_item(c), Some(7));
        assert!(
            !menu.get_item(c).unwrap().checked,
            "activating again toggles off"
        );
    }

    #[test]
    fn item_shortcut_is_stored() {
        let mut menu = PopupMenu::new();
        let s = menu.add_item("Save", 3);
        menu.set_item_shortcut(s, "Ctrl+S");
        assert_eq!(
            menu.get_item(s).unwrap().shortcut.as_deref(),
            Some("Ctrl+S")
        );
    }
}
