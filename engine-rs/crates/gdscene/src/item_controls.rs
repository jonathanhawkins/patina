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

/// How one [`TreeItem`] cell behaves (Godot `TreeItem::TreeCellMode`).
///
/// ⚠️ The discriminants are Godot's wire values (`tree.h`), because they travel
/// through `.tscn` storage and `set_cell_mode(column, mode)` from a script.
/// `RANGE = 2`, `ICON = 3` and `CUSTOM = 4` are **not modelled yet** and are
/// read back as [`TreeCellMode::Text`] — the numbering is reserved so adding
/// them later cannot renumber what is already saved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TreeCellMode {
    /// `CELL_MODE_STRING = 0` — plain text.
    #[default]
    Text,
    /// `CELL_MODE_CHECK = 1` — a checkbox beside the text.
    Check,
}

impl TreeCellMode {
    /// Godot's integer for this mode.
    pub fn godot_value(self) -> i64 {
        match self {
            TreeCellMode::Text => 0,
            TreeCellMode::Check => 1,
        }
    }

    /// Reads Godot's integer. Unmodelled modes fall back to
    /// [`TreeCellMode::Text`] — a cell that renders as text is wrong but inert,
    /// where guessing `Check` would put a toggle on a cell that has none.
    pub fn from_godot_value(value: i64) -> Self {
        match value {
            1 => TreeCellMode::Check,
            _ => TreeCellMode::Text,
        }
    }
}

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
    /// Whether the item has a row at all (Godot `TreeItem.visible`,
    /// pat-s3rrv.75).
    ///
    /// ⚠️ Distinct from `collapsed`: a collapsed item still shows its OWN row
    /// and hides its descendants, while an invisible one hides itself and its
    /// descendants both.
    pub visible: bool,
    /// Per-column cell mode (pat-s3rrv.75). Same length as `columns`.
    pub modes: Vec<TreeCellMode>,
    /// Per-column checkbox state; meaningful only for a
    /// [`TreeCellMode::Check`] cell.
    pub checked: Vec<bool>,
    /// Per-column INDETERMINATE state — Godot's third checkbox state, set on a
    /// parent whose children disagree (pat-s3rrv.75).
    ///
    /// ⚠️ Independent of [`checked`](Self::checked) rather than a third enum
    /// value, because that is how `TreeItem` stores it: `set_indeterminate` and
    /// `set_checked` are separate setters and a cell can carry both flags.
    pub indeterminate: Vec<bool>,
    /// Per-column editability.
    ///
    /// ⚠️ Godot's default is **false** (`tree.cpp` `TreeItem::Cell::editable`),
    /// and it gates the check toggle: `propagate_mouse_event` returns early on
    /// `!c.editable` before ever reaching the CHECK branch. A cell defaulting to
    /// editable would make every checkbox toggle on the first click.
    pub editable: Vec<bool>,
}

/// Per-column layout state (Godot `Tree::ColumnInfo`, pat-s3rrv.75).
///
/// The defaults are Godot's: no custom minimum, expanding, ratio 1 — which
/// distributes the control's width evenly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeColumn {
    /// Column header text (`set_column_title`).
    pub title: String,
    /// Floor for this column's width, in pixels (`set_column_custom_minimum_width`).
    pub custom_min_width: i32,
    /// Share of the leftover width this column claims when expanding.
    pub expand_ratio: i32,
    /// Whether the column grows past its minimum at all.
    pub expand: bool,
}

impl Default for TreeColumn {
    fn default() -> Self {
        Self {
            title: String::new(),
            custom_min_width: 0,
            expand_ratio: 1,
            expand: true,
        }
    }
}

/// A multi-column hierarchical item control (Godot `Tree`), stored as an arena
/// of [`TreeItem`]s so parent/child links and selection are index-based.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    columns: usize,
    /// Per-column layout, always `columns` long.
    column_info: Vec<TreeColumn>,
    items: Vec<TreeItem>,
    selected: Option<usize>,
}

impl Tree {
    /// Creates a tree with `columns` columns (at least 1).
    pub fn new(columns: usize) -> Self {
        let columns = columns.max(1);
        Self {
            columns,
            column_info: vec![TreeColumn::default(); columns],
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
            visible: true,
            modes: vec![TreeCellMode::default(); self.columns],
            checked: vec![false; self.columns],
            indeterminate: vec![false; self.columns],
            editable: vec![false; self.columns],
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

    /// Clears the selection without removing anything (Godot `deselect_all`).
    pub fn deselect_all(&mut self) {
        self.selected = None;
    }

    /// Godot's root item: the first item created with no parent.
    ///
    /// ⚠️ Godot's `Tree` has exactly ONE root — `create_item(null)` creates it
    /// the first time and parents to it afterwards. The arena permits several
    /// parentless items, so this reports the first, which is the one every
    /// later `create_item(null)` hangs off (pat-s3rrv.75).
    pub fn get_root(&self) -> Option<usize> {
        (0..self.items.len()).find(|&i| self.items[i].parent.is_none())
    }

    /// Whether `item` has at least one child — the test that decides whether a
    /// row draws a fold arrow at all.
    pub fn has_children(&self, item: usize) -> bool {
        self.items.iter().any(|it| it.parent == Some(item))
    }

    /// Sets whether `item` can be selected.
    pub fn set_selectable(&mut self, item: usize, selectable: bool) {
        if let Some(it) = self.items.get_mut(item) {
            it.selectable = selectable;
        }
    }

    /// Removes every item and the selection (Godot `Tree::clear`).
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected = None;
    }

    /// Sets the column count (at least 1), resizing every existing item's cells.
    ///
    /// ⚠️ Existing cells are resized rather than dropped: Godot's `set_columns`
    /// keeps the text of the columns that survive, and rebuilding the rows would
    /// silently erase an authored tree when a script widened it.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.column_info.resize(self.columns, TreeColumn::default());
        for it in &mut self.items {
            it.columns.resize(self.columns, String::new());
            it.modes.resize(self.columns, TreeCellMode::default());
            it.checked.resize(self.columns, false);
            it.indeterminate.resize(self.columns, false);
            it.editable.resize(self.columns, false);
        }
    }

    /// Borrows a column's layout state.
    pub fn column(&self, column: usize) -> Option<&TreeColumn> {
        self.column_info.get(column)
    }

    /// Mutably borrows a column's layout state, for the `set_column_*` setters.
    pub fn column_mut(&mut self, column: usize) -> Option<&mut TreeColumn> {
        self.column_info.get_mut(column)
    }

    /// The width of `column` within a content area `content_width` wide, in
    /// Godot's own integer arithmetic (`tree.cpp:5662`).
    ///
    /// ⚠️ Two details are ported rather than improved. The guard compares the
    /// leftover PIXELS against the sum of RATIOS — so when space is tight, no
    /// column expands at all. And the division truncates, so the columns need
    /// not fill the width exactly; the remainder is empty space on the right.
    ///
    /// ⚠️ Only `custom_min_width` contributes the minimum. Godot also grows a
    /// column to fit its widest cell and its title, which needs a content pass
    /// with font metrics and belongs with the draw pass.
    pub fn column_width(&self, column: usize, content_width: i32) -> i32 {
        let Some(info) = self.column_info.get(column) else {
            return 0;
        };
        let mut width = info.custom_min_width;
        if !info.expand {
            return width;
        }
        let mut expand_area = content_width;
        let mut expanding_total = 0i32;
        for c in &self.column_info {
            expand_area -= c.custom_min_width;
            if c.expand {
                expanding_total += c.expand_ratio;
            }
        }
        if expand_area >= expanding_total && expanding_total > 0 {
            width += expand_area * info.expand_ratio / expanding_total;
        }
        width
    }

    /// Sets a cell's mode.
    pub fn set_cell_mode(&mut self, item: usize, column: usize, mode: TreeCellMode) {
        if let Some(slot) = self
            .items
            .get_mut(item)
            .and_then(|it| it.modes.get_mut(column))
        {
            *slot = mode;
        }
    }

    /// A cell's mode, or [`TreeCellMode::Text`] when the cell does not exist.
    pub fn get_cell_mode(&self, item: usize, column: usize) -> TreeCellMode {
        self.items
            .get(item)
            .and_then(|it| it.modes.get(column))
            .copied()
            .unwrap_or_default()
    }

    /// Sets a cell's checkbox state.
    ///
    /// ⚠️ Applies whatever the cell's mode is, exactly as Godot's
    /// `TreeItem::set_checked` does — the mode decides whether the checkbox is
    /// *shown*, not whether the flag exists. Refusing to store it on a text cell
    /// would silently drop state a script set before switching the mode.
    pub fn set_checked(&mut self, item: usize, column: usize, checked: bool) {
        let Some(it) = self.items.get_mut(item) else {
            return;
        };
        // ⚠️ The early-out is load-bearing, not an optimisation: setting the
        // value it already has must NOT clear `indeterminate` (tree.cpp:361).
        match it.checked.get_mut(column) {
            Some(slot) if *slot != checked => *slot = checked,
            _ => return,
        }
        // A real change makes the cell definitively checked or unchecked, so it
        // is no longer the mixed state.
        if let Some(ind) = it.indeterminate.get_mut(column) {
            *ind = false;
        }
    }

    /// Whether a cell's checkbox is ticked.
    pub fn is_checked(&self, item: usize, column: usize) -> bool {
        self.items
            .get(item)
            .and_then(|it| it.checked.get(column))
            .copied()
            .unwrap_or(false)
    }

    /// Sets a cell's indeterminate flag.
    pub fn set_indeterminate(&mut self, item: usize, column: usize, value: bool) {
        let Some(it) = self.items.get_mut(item) else {
            return;
        };
        // Upstream's own comment: "Prevent uncheck if indeterminate set to
        // false twice." Clearing an already-clear flag must not uncheck the
        // cell (tree.cpp:375).
        match it.indeterminate.get_mut(column) {
            Some(slot) if *slot != value => *slot = value,
            _ => return,
        }
        if let Some(checked) = it.checked.get_mut(column) {
            *checked = false;
        }
    }

    /// Whether a cell is indeterminate.
    pub fn is_indeterminate(&self, item: usize, column: usize) -> bool {
        self.items
            .get(item)
            .and_then(|it| it.indeterminate.get(column))
            .copied()
            .unwrap_or(false)
    }

    /// Propagates `item`'s check state through its subtree and back up its
    /// ancestors, returning every item touched in Godot's own emission order —
    /// self, then descendants depth-first, then ancestors upward
    /// (`tree.cpp:282-357`).
    ///
    /// The caller emits `check_propagated_to_item(item, column)` for each,
    /// because the model has no signal channel of its own.
    ///
    /// # The tri-state rule, verbatim from the C++
    ///
    /// A parent becomes INDETERMINATE when any child is indeterminate, or when
    /// its children disagree. It clears indeterminate only when it was
    /// indeterminate and nothing is checked. Otherwise its checked state is
    /// simply "any child is checked".
    ///
    /// ⚠️ The upstream scan breaks out early on an indeterminate child **only
    /// inside the `!is_checked` branch**, so a child that is both checked and
    /// indeterminate does not short-circuit it. That is reproduced here rather
    /// than tidied: tidying it changes the result for exactly the mixed trees
    /// this function exists to handle.
    pub fn propagate_check(&mut self, item: usize, column: usize) -> Vec<usize> {
        if self.items.get(item).is_none() {
            return Vec::new();
        }
        let checked = self.is_checked(item, column);
        let mut touched = vec![item];

        // Down: every descendant takes this item's state, depth-first.
        let mut stack: Vec<usize> = self.get_children(Some(item)).into_iter().rev().collect();
        let mut guard = 0usize;
        while let Some(child) = stack.pop() {
            // A malformed arena (a parent cycle) must terminate rather than
            // spin; the arena can hold at most `item_count` distinct items.
            guard += 1;
            if guard > self.items.len() {
                break;
            }
            self.set_checked(child, column, checked);
            touched.push(child);
            for grandchild in self.get_children(Some(child)).into_iter().rev() {
                stack.push(grandchild);
            }
        }

        // Up: each ancestor recomputes from its own direct children.
        let mut current = self.items.get(item).and_then(|it| it.parent);
        let mut steps = 0usize;
        while let Some(parent) = current {
            steps += 1;
            if steps > self.items.len() {
                break;
            }
            let mut any_checked = false;
            let mut any_unchecked = false;
            let mut any_indeterminate = false;
            for child in self.get_children(Some(parent)) {
                if !self.is_checked(child, column) {
                    any_unchecked = true;
                    if self.is_indeterminate(child, column) {
                        any_indeterminate = true;
                        break;
                    }
                } else {
                    any_checked = true;
                }
            }

            if any_indeterminate || (any_checked && any_unchecked) {
                self.set_indeterminate(parent, column, true);
            } else if self.is_indeterminate(parent, column) && !any_checked {
                self.set_indeterminate(parent, column, false);
            } else {
                self.set_checked(parent, column, any_checked);
            }
            touched.push(parent);
            current = self.items.get(parent).and_then(|it| it.parent);
        }
        touched
    }

    /// Shows or hides `item` and, with it, its whole subtree.
    ///
    /// ⚠️ Hiding also DESELECTS (`TreeItem::set_visible`, tree.cpp:392): a
    /// hidden item must never remain the selection, or `get_selected()` keeps
    /// pointing at a row that is not on screen and the next click compares
    /// against a phantom.
    pub fn set_item_visible(&mut self, item: usize, visible: bool) {
        let Some(it) = self.items.get_mut(item) else {
            return;
        };
        if it.visible == visible {
            return;
        }
        it.visible = visible;
        if !visible && self.selected == Some(item) {
            self.selected = None;
        }
    }

    /// Whether `item` has a row of its own.
    ///
    /// ⚠️ Answers this item's own flag, not Godot's recursive
    /// `is_visible_in_tree`. [`visible_items`](Self::visible_items) skips a
    /// hidden item's descendants anyway, so the recursive question never has to
    /// be asked separately.
    pub fn is_item_visible(&self, item: usize) -> bool {
        self.items.get(item).map(|it| it.visible).unwrap_or(false)
    }

    /// Sets whether a cell can be edited — which is also what allows a CHECK
    /// cell to toggle on click.
    pub fn set_editable(&mut self, item: usize, column: usize, editable: bool) {
        if let Some(slot) = self
            .items
            .get_mut(item)
            .and_then(|it| it.editable.get_mut(column))
        {
            *slot = editable;
        }
    }

    /// Whether a cell can be edited (Godot default: false).
    pub fn is_editable(&self, item: usize, column: usize) -> bool {
        self.items
            .get(item)
            .and_then(|it| it.editable.get(column))
            .copied()
            .unwrap_or(false)
    }

    /// The rows a viewer actually shows, in top-to-bottom order, as
    /// `(item index, depth)` — depth 0 for a root item (pat-s3rrv.75).
    ///
    /// # Why this belongs on the model
    ///
    /// Both the draw pass and the hit test need the SAME row order, or a click
    /// selects a different item than the one under the cursor. Deriving it twice
    /// is how those two drift apart, so it is derived once, here.
    ///
    /// ⚠️ A collapsed item still appears — it is its DESCENDANTS that vanish.
    /// Dropping the collapsed row itself would make the fold arrow unclickable,
    /// so the tree could never be expanded again.
    pub fn visible_items(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        // Iterative rather than recursive: a malformed arena (an item whose
        // parent chain loops) must not blow the stack, and the visited guard
        // below makes that a bounded walk instead.
        let mut stack: Vec<(usize, usize)> = self
            .get_children(None)
            .into_iter()
            .rev()
            .map(|i| (i, 0))
            .collect();
        let mut visited = vec![false; self.items.len()];
        while let Some((idx, depth)) = stack.pop() {
            if visited.get(idx).copied().unwrap_or(true) {
                continue;
            }
            visited[idx] = true;
            // ⚠️ An invisible item contributes NO row and stops the descent, the
            // same way a collapsed one stops it — `_find_item_at_pos` treats the
            // two identically ("Do not try children"). A collapsed item differs
            // in that it still contributes its own row.
            if !self.is_item_visible(idx) {
                continue;
            }
            out.push((idx, depth));
            if !self.is_collapsed(idx) {
                for child in self.get_children(Some(idx)).into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
        }
        out
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
