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
    /// Height of one popup row, in pixels (pat-s3rrv.72).
    ///
    /// The popup layer needs ONE constant shared by the hit-test and the draw
    /// pass: if the row height used to map a click to an index differs from the
    /// one used to lay the rows out, every click below the first row lands on
    /// the wrong item — and it looks correct until you click the last one.
    ///
    /// ⚠️ Godot derives this from the theme (font size + `v_separation`) and
    /// draws separators SHORTER than normal rows. Patina has no popup theme
    /// yet, so rows are uniform; when theme metrics arrive, `index_at` and the
    /// draw pass must change together or they will disagree in exactly the way
    /// this constant exists to prevent.
    pub const ROW_HEIGHT: f32 = 24.0;

    /// Total height of the popup panel for the current items.
    pub fn height(&self) -> f32 {
        self.items.len() as f32 * Self::ROW_HEIGHT
    }

    /// The item index at `local_y` pixels below the panel's top edge, or `None`
    /// when the offset falls outside the panel.
    ///
    /// ⚠️ Returns an index for separators and disabled rows too — hovering one
    /// is meaningful (Godot highlights nothing, but the row is still "where the
    /// cursor is"), and refusing them here would silently turn a click on a
    /// disabled row into a click on whatever is underneath. Activation is the
    /// gate: [`activate_item`](Self::activate_item) already answers `None` for
    /// disabled, separator and submenu rows.
    pub fn index_at(&self, local_y: f32) -> Option<usize> {
        if local_y < 0.0 {
            return None;
        }
        let idx = (local_y / Self::ROW_HEIGHT) as usize;
        (idx < self.items.len()).then_some(idx)
    }

    /// Whether activating `idx` would yield an id — the same rule
    /// [`activate_item`](Self::activate_item) applies, without mutating.
    ///
    /// Lets the input layer decide whether a release should close the popup
    /// without toggling a checkbox as a side effect of asking.
    pub fn is_activatable(&self, idx: usize) -> bool {
        self.items
            .get(idx)
            .is_some_and(|it| !it.disabled && !it.separator && it.submenu.is_none())
    }

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

    /// The hit-test and the layout must agree, so `index_at` is defined purely
    /// in terms of the shared row height.
    #[test]
    fn index_at_maps_a_y_offset_to_the_row_under_it() {
        let mut menu = PopupMenu::new();
        menu.add_item("A", 1);
        menu.add_item("B", 2);
        menu.add_item("C", 3);

        let h = PopupMenu::ROW_HEIGHT;
        assert_eq!(menu.index_at(0.0), Some(0), "the very top edge is row 0");
        assert_eq!(menu.index_at(h - 0.01), Some(0), "just above the seam");
        assert_eq!(
            menu.index_at(h),
            Some(1),
            "the seam itself belongs to row 1"
        );
        assert_eq!(menu.index_at(2.5 * h), Some(2));

        // Outside the panel in both directions — a click there must fall
        // through to whatever is underneath rather than hitting the last row.
        assert_eq!(menu.index_at(-1.0), None, "above the panel");
        assert_eq!(
            menu.index_at(3.0 * h),
            None,
            "one pixel past the last row is outside, not row 2 — an inclusive \
             bound here would make the popup swallow clicks below it"
        );
        assert_eq!(menu.height(), 3.0 * h);
    }

    /// `index_at` deliberately answers for rows that cannot be activated.
    #[test]
    fn a_disabled_row_is_hit_tested_but_not_activatable() {
        let mut menu = PopupMenu::new();
        menu.add_item("Enabled", 1);
        menu.add_separator();
        let dis = menu.add_item("Disabled", 3);
        menu.set_item_disabled(dis, true);

        let h = PopupMenu::ROW_HEIGHT;
        assert_eq!(
            menu.index_at(1.5 * h),
            Some(1),
            "the separator row is still WHERE the cursor is; returning None \
             would let the click reach the control underneath the popup"
        );
        assert_eq!(menu.index_at(2.5 * h), Some(2));

        assert!(menu.is_activatable(0));
        assert!(!menu.is_activatable(1), "a separator is not activatable");
        assert!(
            !menu.is_activatable(2),
            "a disabled item is not activatable"
        );
        assert!(!menu.is_activatable(99), "out of range is not activatable");
    }

    /// `is_activatable` must agree with `activate_item` and must not mutate —
    /// asking whether a checkbox can be activated may not toggle it.
    #[test]
    fn is_activatable_agrees_with_activate_item_without_toggling() {
        let mut menu = PopupMenu::new();
        let c = menu.add_check_item("Sound", 7);

        assert!(menu.is_activatable(c));
        assert!(
            menu.is_activatable(c),
            "asking twice must not change anything"
        );
        assert_eq!(
            menu.get_item(c).map(|i| i.checked),
            Some(false),
            "is_activatable must not toggle the check state"
        );

        assert_eq!(menu.activate_item(c), Some(7));
        assert_eq!(
            menu.get_item(c).map(|i| i.checked),
            Some(true),
            "activation is what toggles it"
        );
    }

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
