//! Item models for Tree / ItemList / OptionButton (pat-u3l8c.74).
//!
//! These are the logic layers (items, selection, activation) the widget
//! rendering + input dispatch build on. Signals are surfaced as return values:
//! `ItemList::select` / `Tree::select` return whether the selection changed
//! (the `item_selected` trigger), `ItemList::activate` returns the activated
//! index (`item_activated`), and `OptionButton::pick_item` returns the chosen
//! index (`item_selected`).

// ---------------------------------------------------------------------------
// ItemList
// ---------------------------------------------------------------------------

/// A single [`ItemList`] entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemListItem {
    /// Display label.
    pub text: String,
    /// Optional icon resource path.
    pub icon: Option<String>,
    /// Whether the item can be selected.
    pub selectable: bool,
    /// Whether the item is disabled (not selectable/activatable).
    pub disabled: bool,
    /// Current selection state.
    pub selected: bool,
}

impl ItemListItem {
    fn new(text: String, icon: Option<String>) -> Self {
        Self {
            text,
            icon,
            selectable: true,
            disabled: false,
            selected: false,
        }
    }
}

/// A flat list of selectable items (Godot `ItemList`), supporting single- or
/// multi-select and item activation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemList {
    items: Vec<ItemListItem>,
    multi_select: bool,
}

impl ItemList {
    /// Creates an empty single-select item list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables/disables multi-selection (Godot `select_mode`).
    pub fn set_multi_select(&mut self, on: bool) {
        self.multi_select = on;
    }

    /// Adds a text item, returning its index.
    pub fn add_item(&mut self, text: impl Into<String>) -> usize {
        let idx = self.items.len();
        self.items.push(ItemListItem::new(text.into(), None));
        idx
    }

    /// Adds an item with an icon, returning its index.
    pub fn add_icon_item(&mut self, text: impl Into<String>, icon: impl Into<String>) -> usize {
        let idx = self.items.len();
        self.items
            .push(ItemListItem::new(text.into(), Some(icon.into())));
        idx
    }

    /// Number of items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Borrows the item at `idx`.
    pub fn get_item(&self, idx: usize) -> Option<&ItemListItem> {
        self.items.get(idx)
    }

    /// Sets the text of the item at `idx` (Godot `set_item_text`).
    pub fn set_item_text(&mut self, idx: usize, text: impl Into<String>) {
        if let Some(it) = self.items.get_mut(idx) {
            it.text = text.into();
        }
    }

    /// Marks the item at `idx` selectable or not (Godot `set_item_selectable`).
    /// A non-selectable item also loses any selection it held.
    pub fn set_item_selectable(&mut self, idx: usize, selectable: bool) {
        if let Some(it) = self.items.get_mut(idx) {
            it.selectable = selectable;
            if !selectable {
                it.selected = false;
            }
        }
    }

    /// Removes every item (Godot `clear`).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Marks the item at `idx` disabled (also clears its selection).
    pub fn set_item_disabled(&mut self, idx: usize, disabled: bool) {
        if let Some(it) = self.items.get_mut(idx) {
            it.disabled = disabled;
            if disabled {
                it.selected = false;
            }
        }
    }

    /// Selects the item at `idx`. In single-select mode this deselects every
    /// other item. Non-selectable / disabled / out-of-range indices are ignored.
    /// Returns `true` when the set of selected items changed (`item_selected`).
    pub fn select(&mut self, idx: usize) -> bool {
        match self.items.get(idx) {
            Some(it) if it.selectable && !it.disabled => {}
            _ => return false,
        }
        let before = self.get_selected_items();
        if !self.multi_select {
            for it in self.items.iter_mut() {
                it.selected = false;
            }
        }
        self.items[idx].selected = true;
        self.get_selected_items() != before
    }

    /// Deselects the item at `idx`.
    pub fn deselect(&mut self, idx: usize) {
        if let Some(it) = self.items.get_mut(idx) {
            it.selected = false;
        }
    }

    /// Deselects every item.
    pub fn deselect_all(&mut self) {
        for it in self.items.iter_mut() {
            it.selected = false;
        }
    }

    /// Whether the item at `idx` is selected.
    pub fn is_selected(&self, idx: usize) -> bool {
        self.items.get(idx).map(|it| it.selected).unwrap_or(false)
    }

    /// The indices of all selected items, ascending.
    pub fn get_selected_items(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].selected)
            .collect()
    }

    /// Activates the item at `idx` (double-click / Enter). Returns `Some(idx)`
    /// — the `item_activated` payload — for an enabled item, `None` otherwise.
    pub fn activate(&mut self, idx: usize) -> Option<usize> {
        match self.items.get(idx) {
            Some(it) if it.selectable && !it.disabled => Some(idx),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// OptionButton
// ---------------------------------------------------------------------------

/// A single [`OptionButton`] dropdown entry.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionButtonItem {
    /// Display label.
    pub text: String,
    /// Item id (Godot `get_item_id`). Defaults to the item's index.
    pub id: i64,
    /// Whether the item is disabled in the dropdown.
    pub disabled: bool,
}

/// A drop-down single-choice control (Godot `OptionButton`).
#[derive(Debug, Clone, PartialEq)]
pub struct OptionButton {
    items: Vec<OptionButtonItem>,
    /// Index of the selected item, or `-1` for none.
    selected: i64,
}

impl Default for OptionButton {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: -1,
        }
    }
}

impl OptionButton {
    /// Creates an empty option button with nothing selected.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an item; `id < 0` defaults the id to the item's index. The first
    /// item added becomes the selection (Godot auto-selects index 0).
    pub fn add_item(&mut self, text: impl Into<String>, id: i64) -> usize {
        let idx = self.items.len();
        let id = if id < 0 { idx as i64 } else { id };
        self.items.push(OptionButtonItem {
            text: text.into(),
            id,
            disabled: false,
        });
        if self.selected < 0 {
            self.selected = idx as i64;
        }
        idx
    }

    /// Number of items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Borrows the item at `idx`.
    pub fn get_item(&self, idx: usize) -> Option<&OptionButtonItem> {
        self.items.get(idx)
    }

    /// Sets the item at `idx` disabled in the dropdown.
    pub fn set_item_disabled(&mut self, idx: usize, disabled: bool) {
        if let Some(it) = self.items.get_mut(idx) {
            it.disabled = disabled;
        }
    }

    /// Sets the selected index without emitting a signal (Godot `select`).
    pub fn select(&mut self, idx: usize) {
        if idx < self.items.len() {
            self.selected = idx as i64;
        }
    }

    /// Sets the selected index, accepting Godot's `-1` ("nothing selected").
    /// Out-of-range positive indices are ignored, as [`select`](Self::select)
    /// ignores them. Needed to restore a persisted `selected = -1` that
    /// [`add_item`](Self::add_item)'s auto-select would otherwise overwrite.
    pub fn set_selected_index(&mut self, idx: i64) {
        if idx < 0 {
            self.selected = -1;
        } else if (idx as usize) < self.items.len() {
            self.selected = idx;
        }
    }

    /// Sets the text of the item at `idx` (Godot `set_item_text`).
    pub fn set_item_text(&mut self, idx: usize, text: impl Into<String>) {
        if let Some(it) = self.items.get_mut(idx) {
            it.text = text.into();
        }
    }

    /// Removes every item and clears the selection (Godot `clear`).
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected = -1;
    }

    /// The selected item index, or `-1`.
    pub fn get_selected(&self) -> i64 {
        self.selected
    }

    /// The id of the selected item, or `-1` when nothing is selected.
    pub fn get_selected_id(&self) -> i64 {
        if self.selected >= 0 {
            self.items
                .get(self.selected as usize)
                .map(|it| it.id)
                .unwrap_or(-1)
        } else {
            -1
        }
    }

    /// Picks an item as the user would from the dropdown: selects it and returns
    /// its index (the `item_selected` payload) for an enabled item, `None`
    /// otherwise.
    pub fn pick_item(&mut self, idx: usize) -> Option<i64> {
        match self.items.get(idx) {
            Some(it) if !it.disabled => {}
            _ => return None,
        }
        self.selected = idx as i64;
        Some(idx as i64)
    }
}

// ---------------------------------------------------------------------------
// TabBar
// ---------------------------------------------------------------------------

/// A single tab in a [`TabBar`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// The tab's display title.
    pub title: String,
}

/// Model for Godot's `TabBar`: an ordered list of tabs with a current index.
/// [`set_current_tab`](TabBar::set_current_tab) returns whether the current tab
/// actually changed — the `tab_changed(idx)` trigger.
#[derive(Debug, Clone)]
pub struct TabBar {
    tabs: Vec<Tab>,
    current: i64,
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl TabBar {
    /// Creates an empty tab bar (no tabs, current index `-1`).
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            current: -1,
        }
    }

    /// Appends a tab with `title`; the first tab added becomes current. Returns
    /// the new tab's index.
    pub fn add_tab(&mut self, title: impl Into<String>) -> usize {
        let idx = self.tabs.len();
        self.tabs.push(Tab {
            title: title.into(),
        });
        if self.current < 0 {
            self.current = idx as i64;
        }
        idx
    }

    /// Number of tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The current tab index, or `-1` when there are no tabs.
    pub fn get_current_tab(&self) -> i64 {
        self.current
    }

    /// The title of the tab at `idx`, if it exists.
    pub fn get_tab_title(&self, idx: usize) -> Option<&str> {
        self.tabs.get(idx).map(|t| t.title.as_str())
    }

    /// Renames the tab at `idx` (Godot `set_tab_title`).
    pub fn set_tab_title(&mut self, idx: usize, title: impl Into<String>) {
        if let Some(t) = self.tabs.get_mut(idx) {
            t.title = title.into();
        }
    }

    /// Sets the current tab **without** reporting a change — the restore path
    /// for a persisted index, including Godot's `-1` ("no current tab") that
    /// [`add_tab`](Self::add_tab)'s auto-select would otherwise overwrite.
    pub fn set_current_index(&mut self, idx: i64) {
        if idx < 0 {
            self.current = -1;
        } else if (idx as usize) < self.tabs.len() {
            self.current = idx;
        }
    }

    /// Removes every tab (Godot `clear_tabs`).
    pub fn clear_tabs(&mut self) {
        self.tabs.clear();
        self.current = -1;
    }

    /// Sets the current tab to `idx` when in range. Returns `true` when the
    /// current tab actually changed — Godot's `tab_changed(idx)` signal.
    pub fn set_current_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() || self.current == idx as i64 {
            return false;
        }
        self.current = idx as i64;
        true
    }
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

/// A single [`Tree`] item, stored in the tree's arena.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeItem {
    /// Arena index of the parent item; `None` for a root child.
    pub parent: Option<usize>,
    /// Per-column text.
    pub columns: Vec<String>,
    /// Whether this item's subtree is collapsed (children hidden).
    pub collapsed: bool,
    /// Whether the item can be selected.
    pub selectable: bool,
}

/// A multi-column hierarchical item control (Godot `Tree`), stored as an arena
/// of [`TreeItem`]s so parent/child links and selection are index-based.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    columns: usize,
    items: Vec<TreeItem>,
    selected: Option<usize>,
}

impl Tree {
    /// Creates a tree with `columns` columns (at least 1).
    pub fn new(columns: usize) -> Self {
        Self {
            columns: columns.max(1),
            items: Vec::new(),
            selected: None,
        }
    }

    /// Number of columns.
    pub fn column_count(&self) -> usize {
        self.columns
    }

    /// Creates an item under `parent` (`None` = a root child) and returns its
    /// arena index. Columns start empty; the item is expanded and selectable.
    pub fn create_item(&mut self, parent: Option<usize>) -> usize {
        let idx = self.items.len();
        self.items.push(TreeItem {
            parent,
            columns: vec![String::new(); self.columns],
            collapsed: false,
            selectable: true,
        });
        idx
    }

    /// Total item count.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Borrows the item at `idx`.
    pub fn get_item(&self, idx: usize) -> Option<&TreeItem> {
        self.items.get(idx)
    }

    /// Sets the text of `column` on `item` (ignored if out of range).
    pub fn set_text(&mut self, item: usize, column: usize, text: impl Into<String>) {
        if let Some(it) = self.items.get_mut(item) {
            if let Some(cell) = it.columns.get_mut(column) {
                *cell = text.into();
            }
        }
    }

    /// The text of `column` on `item`, or `""`.
    pub fn get_text(&self, item: usize, column: usize) -> &str {
        self.items
            .get(item)
            .and_then(|it| it.columns.get(column))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Collapses/expands `item`'s subtree.
    pub fn set_collapsed(&mut self, item: usize, collapsed: bool) {
        if let Some(it) = self.items.get_mut(item) {
            it.collapsed = collapsed;
        }
    }

    /// Whether `item` is collapsed.
    pub fn is_collapsed(&self, item: usize) -> bool {
        self.items.get(item).map(|it| it.collapsed).unwrap_or(false)
    }

    /// The direct children of `parent` (`None` = root children), ascending.
    pub fn get_children(&self, parent: Option<usize>) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].parent == parent)
            .collect()
    }

    /// Selects `item`. Returns `true` when the selection changed
    /// (`item_selected`). Non-selectable / out-of-range items are ignored.
    pub fn select(&mut self, item: usize) -> bool {
        match self.items.get(item) {
            Some(it) if it.selectable => {}
            _ => return false,
        }
        let changed = self.selected != Some(item);
        self.selected = Some(item);
        changed
    }

    /// The selected item index, if any.
    pub fn get_selected(&self) -> Option<usize> {
        self.selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- ItemList -----------------------------------------------------------

    #[test]
    fn item_list_single_select_clears_others() {
        let mut list = ItemList::new();
        let a = list.add_item("A");
        let b = list.add_item("B");
        assert!(list.select(a));
        assert!(list.select(b));
        // Single-select: only B remains selected.
        assert!(!list.is_selected(a));
        assert!(list.is_selected(b));
        assert_eq!(list.get_selected_items(), vec![b]);
        // Re-selecting the same item is not a change.
        assert!(!list.select(b));
    }

    #[test]
    fn item_list_multi_select_keeps_multiple() {
        let mut list = ItemList::new();
        list.set_multi_select(true);
        let a = list.add_item("A");
        let b = list.add_item("B");
        assert!(list.select(a));
        assert!(list.select(b));
        assert_eq!(list.get_selected_items(), vec![a, b]);
    }

    #[test]
    fn item_list_disabled_is_not_selectable_and_activate_reports_index() {
        let mut list = ItemList::new();
        let a = list.add_icon_item("A", "res://a.png");
        list.set_item_disabled(a, true);
        assert!(!list.select(a));
        assert_eq!(list.activate(a), None);

        let b = list.add_item("B");
        assert_eq!(list.activate(b), Some(b));
        assert_eq!(
            list.get_item(a).unwrap().icon.as_deref(),
            Some("res://a.png")
        );
    }

    // -- OptionButton -------------------------------------------------------

    #[test]
    fn option_button_auto_selects_first_and_picks_emit_index() {
        let mut ob = OptionButton::new();
        let a = ob.add_item("Red", 10); // auto-selected (index 0)
        let _b = ob.add_item("Green", 20);
        assert_eq!(ob.get_selected(), a as i64);
        assert_eq!(ob.get_selected_id(), 10);
        // Picking index 1 selects it and returns its index (item_selected).
        assert_eq!(ob.pick_item(1), Some(1));
        assert_eq!(ob.get_selected(), 1);
        assert_eq!(ob.get_selected_id(), 20);
    }

    #[test]
    fn option_button_disabled_item_is_not_pickable() {
        let mut ob = OptionButton::new();
        ob.add_item("A", -1); // implicit id 0
        let b = ob.add_item("B", -1); // implicit id 1
        ob.set_item_disabled(b, true);
        assert_eq!(ob.pick_item(b), None);
        assert_eq!(ob.get_item(b).unwrap().id, 1);
    }

    // -- Tree ---------------------------------------------------------------

    #[test]
    fn tree_hierarchy_columns_and_collapse() {
        let mut tree = Tree::new(2);
        let root = tree.create_item(None);
        let child_a = tree.create_item(Some(root));
        let child_b = tree.create_item(Some(root));
        tree.set_text(child_a, 0, "Name");
        tree.set_text(child_a, 1, "Value");

        assert_eq!(tree.get_children(None), vec![root]);
        assert_eq!(tree.get_children(Some(root)), vec![child_a, child_b]);
        assert_eq!(tree.get_text(child_a, 0), "Name");
        assert_eq!(tree.get_text(child_a, 1), "Value");
        assert_eq!(tree.get_text(child_a, 5), ""); // out-of-range column

        assert!(!tree.is_collapsed(root));
        tree.set_collapsed(root, true);
        assert!(tree.is_collapsed(root));
    }

    #[test]
    fn tree_select_reports_change() {
        let mut tree = Tree::new(1);
        let a = tree.create_item(None);
        let b = tree.create_item(None);
        assert!(tree.select(a));
        assert_eq!(tree.get_selected(), Some(a));
        assert!(!tree.select(a)); // no change
        assert!(tree.select(b));
        assert_eq!(tree.get_selected(), Some(b));
    }
}
