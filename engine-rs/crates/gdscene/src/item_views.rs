//! Scene-node bindings for the item-view Controls (pat-0q2kx.85).
//!
//! [`crate::item_controls`] holds the item/selection *logic* for Godot's
//! `ItemList`, `OptionButton` and `TabBar`, but nothing instantiated one for a
//! scene node: a `.tscn` `ItemList` had no items, no script method reached it,
//! and a click on it did nothing. This module is the missing wiring —
//!
//! * item state lives on the node as ordinary properties, so it round-trips
//!   through `.tscn` save/load and the inspector like every other Control
//!   property (`items` / `tabs` / `current_tab` / `selected`);
//! * [`call_item_view_method`] is the script-method bridge: it loads the model
//!   from those properties, runs the [`crate::item_controls`] operation, and
//!   stores the result back;
//! * [`item_view_click`] maps a viewport-space click onto an item/tab index and
//!   emits the signals Godot emits (`item_selected`, `item_activated`,
//!   `item_clicked`, `tab_clicked`, `tab_selected`, `tab_changed`).
//!
//! Godot ground truth: `upstream/godot/scene/gui/item_list.cpp`,
//! `option_button.cpp`, `tab_bar.cpp`. Notably, the *script* setters
//! (`ItemList.select`, `OptionButton.select`) emit nothing — only user
//! interaction does — while `TabBar.set_current_tab` emits `tab_selected`
//! always and `tab_changed` when the index actually moved.
//!
//! Geometry ([`ITEM_ROW_HEIGHT`], [`tab_spans`]) is shared with
//! [`crate::control_render`] so the pixels drawn and the rect that was clicked
//! are the same rect. Tab spans reuse the strip layout `TabContainer` already
//! draws with (`control::tab_rects`, pat-0q2kx.83) so a `TabBar` and a
//! `TabContainer` header are laid out by one rule.

use gdcore::math::{Rect2, Vector2};
use gdvariant::{Dictionary, Variant};

use crate::control;
use crate::control_render::{text_scale, text_width};
// ⚠️ `Tree` is aliased: every function here binds a `&SceneTree` to the
// name `tree`, and an unaliased import would put the control and the scene
// graph one letter apart at every call site.
use crate::item_controls::{
    ItemList, OptionButton, TabBar, Tree as TreeControl, TreeCellMode, TreeColumn,
};
use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Height of one `ItemList` row, in pixels. Godot derives this from the theme
/// font; Patina's bitmap font has one size, so the row height is fixed and
/// shared between drawing and hit-testing.
pub const ITEM_ROW_HEIGHT: f32 = 20.0;

/// Godot's `MOUSE_BUTTON_LEFT`, the button reported by `item_clicked`.
const MOUSE_BUTTON_LEFT: i64 = 1;

// ---------------------------------------------------------------------------
// Class dispatch
// ---------------------------------------------------------------------------

/// The item-view family a node belongs to, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemViewKind {
    /// `ItemList` — a flat list of rows.
    List,
    /// `OptionButton` — a button showing one of its items.
    Option,
    /// `TabBar` — a standalone strip of tabs.
    Tabs,
    /// `TabContainer` — a tab strip whose tabs *are* its children.
    TabContainer,
    /// `Tree` — a multi-column hierarchical view of foldable rows
    /// (pat-s3rrv.75).
    TreeView,
}

/// Classifies `node_id` into an [`ItemViewKind`], or `None` for anything else.
pub fn item_view_kind(tree: &SceneTree, node_id: NodeId) -> Option<ItemViewKind> {
    match tree.get_node(node_id)?.class_name() {
        "ItemList" => Some(ItemViewKind::List),
        "OptionButton" => Some(ItemViewKind::Option),
        "TabBar" => Some(ItemViewKind::Tabs),
        "TabContainer" => Some(ItemViewKind::TabContainer),
        "Tree" => Some(ItemViewKind::TreeView),
        _ => None,
    }
}

/// Whether `node_id` is one of the item-view Controls this module drives.
pub fn is_item_view(tree: &SceneTree, node_id: NodeId) -> bool {
    item_view_kind(tree, node_id).is_some()
}

// ---------------------------------------------------------------------------
// Property <-> model plumbing
// ---------------------------------------------------------------------------

fn prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Variant {
    tree.get_node(node_id)
        .map(|n| n.get_property(key))
        .unwrap_or(Variant::Nil)
}

fn set_prop(tree: &mut SceneTree, node_id: NodeId, key: &str, value: Variant) {
    if let Some(n) = tree.get_node_mut(node_id) {
        n.set_property(key, value);
    }
}

fn int_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<i64> {
    match prop(tree, node_id, key) {
        Variant::Int(i) => Some(i),
        Variant::Float(f) => Some(f as i64),
        _ => None,
    }
}

/// The node's `items` array, as dictionaries. Entries that are not dictionaries
/// are skipped rather than shifting every later index.
fn item_dicts(tree: &SceneTree, node_id: NodeId, key: &str) -> Vec<Dictionary> {
    match prop(tree, node_id, key) {
        Variant::Array(items) => items
            .into_iter()
            .filter_map(|v| match v {
                Variant::Dictionary(d) => Some(d),
                // A bare string is accepted as shorthand for `{"text": s}` so a
                // hand-written `.tscn` can say `items = ["A", "B"]`.
                Variant::String(s) => {
                    let mut d = Dictionary::new();
                    d.insert(Variant::String("text".into()), Variant::String(s));
                    Some(d)
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn dict_string(d: &Dictionary, key: &str) -> String {
    match d.get(Variant::String(key.into())) {
        Some(Variant::String(s)) => s.clone(),
        Some(Variant::StringName(s)) => s.as_str().to_string(),
        _ => String::new(),
    }
}

fn dict_bool(d: &Dictionary, key: &str, default: bool) -> bool {
    match d.get(Variant::String(key.into())) {
        Some(Variant::Bool(b)) => *b,
        _ => default,
    }
}

fn dict_int(d: &Dictionary, key: &str, default: i64) -> i64 {
    match d.get(Variant::String(key.into())) {
        Some(Variant::Int(i)) => *i,
        Some(Variant::Float(f)) => *f as i64,
        _ => default,
    }
}

fn arg_string(args: &[Variant], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Variant::String(s)) => Some(s.clone()),
        Some(Variant::StringName(s)) => Some(s.as_str().to_string()),
        _ => None,
    }
}

fn arg_int(args: &[Variant], i: usize) -> Option<i64> {
    match args.get(i) {
        Some(Variant::Int(v)) => Some(*v),
        Some(Variant::Float(v)) => Some(*v as i64),
        _ => None,
    }
}

fn arg_bool(args: &[Variant], i: usize, default: bool) -> bool {
    match args.get(i) {
        Some(Variant::Bool(b)) => *b,
        _ => default,
    }
}

/// A non-negative index argument, as `usize`. `None` for a missing or negative
/// one, which every caller treats as "ignore this call" — Godot's own
/// out-of-range behaviour.
fn arg_index(args: &[Variant], i: usize) -> Option<usize> {
    match arg_int(args, i) {
        Some(v) if v >= 0 => Some(v as usize),
        _ => None,
    }
}

// -- ItemList ---------------------------------------------------------------

/// Rebuilds the [`ItemList`] model from `node_id`'s properties.
pub fn load_item_list(tree: &SceneTree, node_id: NodeId) -> ItemList {
    let mut list = ItemList::new();
    // Godot `SELECT_SINGLE = 0`, `SELECT_MULTI = 1`.
    list.set_multi_select(int_prop(tree, node_id, "select_mode").unwrap_or(0) != 0);
    let dicts = item_dicts(tree, node_id, "items");
    for d in &dicts {
        list.add_item(dict_string(d, "text"));
    }
    // Flags apply after every item exists so `select` sees the final list.
    for (i, d) in dicts.iter().enumerate() {
        if !dict_bool(d, "selectable", true) {
            list.set_item_selectable(i, false);
        }
        if dict_bool(d, "disabled", false) {
            list.set_item_disabled(i, true);
        }
    }
    for (i, d) in dicts.iter().enumerate() {
        if dict_bool(d, "selected", false) {
            list.select(i);
        }
    }
    list
}

/// Builds a [`PopupMenu`](crate::popup::PopupMenu) from `node_id`'s `items`
/// property (pat-s3rrv.72).
///
/// Same storage shape as [`load_item_list`] — an Array of Dictionaries — so a
/// PopupMenu round-trips through `.tscn` exactly the way an ItemList does, and
/// one reader handles both.
///
/// ⚠️ A missing `id` becomes `-1`, which `PopupMenu::add_item` reads as "use the
/// index". That is Godot's own rule for `add_item(label)` without an explicit
/// id, so an authored menu that omits ids still emits the indices a script
/// expects rather than a run of `-1`s.
pub fn load_popup_menu(tree: &SceneTree, node_id: NodeId) -> crate::popup::PopupMenu {
    let mut menu = crate::popup::PopupMenu::new();
    for d in item_dicts(tree, node_id, "items") {
        // A separator carries no text or id, and must stay non-activatable.
        if dict_bool(&d, "separator", false) {
            menu.add_separator();
            continue;
        }
        let text = dict_string(&d, "text");
        let id = dict_int(&d, "id", -1);
        let idx = if dict_bool(&d, "checkable", false) {
            menu.add_check_item(text, id)
        } else {
            menu.add_item(text, id)
        };
        if dict_bool(&d, "disabled", false) {
            menu.set_item_disabled(idx, true);
        }
        // Applied after creation: `add_check_item` starts unchecked, and the
        // authored state is what the .tscn recorded.
        if dict_bool(&d, "checked", false) {
            menu.toggle_item_checked(idx);
        }
    }
    menu
}

/// Writes the [`ItemList`] model back onto `node_id`'s `items` property.
pub fn store_item_list(tree: &mut SceneTree, node_id: NodeId, list: &ItemList) {
    let items: Vec<Variant> = (0..list.item_count())
        .filter_map(|i| list.get_item(i))
        .map(|it| {
            let mut d = Dictionary::new();
            d.insert(
                Variant::String("text".into()),
                Variant::String(it.text.clone()),
            );
            if let Some(icon) = &it.icon {
                d.insert(
                    Variant::String("icon".into()),
                    Variant::String(icon.clone()),
                );
            }
            d.insert(
                Variant::String("selectable".into()),
                Variant::Bool(it.selectable),
            );
            d.insert(
                Variant::String("disabled".into()),
                Variant::Bool(it.disabled),
            );
            d.insert(
                Variant::String("selected".into()),
                Variant::Bool(it.selected),
            );
            Variant::Dictionary(d)
        })
        .collect();
    set_prop(tree, node_id, "items", Variant::Array(items));
}

// -- Tree -------------------------------------------------------------------

/// Horizontal indent per nesting level, in pixels.
///
/// Shared by the fold arrow, the cell text and the hit test, so a row's arrow is
/// exactly where the click path looks for it (pat-s3rrv.75).
pub const TREE_INDENT: f32 = 16.0;

/// Godot's "no drop target here" answer from
/// [`tree_drop_section_at`] (`Tree.get_drop_section_at_position`).
///
/// ⚠️ Deliberately not `-1`, which already means "above this row". A caller
/// that conflates them drops at the top of the tree whenever the pointer is
/// outside it.
pub const TREE_DROP_SECTION_NONE: i64 = -100;

/// `Tree.drop_mode_flags` — drop onto an item (Godot `DROP_MODE_ON_ITEM`).
pub const DROP_MODE_ON_ITEM: i64 = 1;
/// `Tree.drop_mode_flags` — drop between items (Godot `DROP_MODE_INBETWEEN`).
pub const DROP_MODE_INBETWEEN: i64 = 2;

/// Side of a CHECK cell's checkbox glyph, in pixels.
pub const TREE_CHECK_SIZE: f32 = 14.0;

/// Width of a row's fold arrow box, in pixels. Only a row with children has one.
pub const TREE_ARROW_WIDTH: f32 = 14.0;

/// One visible row of a [`TreeControl`], resolved against the node's on-screen
/// rect (pat-s3rrv.75).
///
/// ⚠️ The draw pass and the hit test MUST consume this same list. Deriving row
/// order or row rects twice is how a click comes to select a different item than
/// the one under the cursor — and with folding in play the two derivations
/// disagree the moment a subtree is collapsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeRow {
    /// Arena index of the item this row shows.
    pub item: usize,
    /// Nesting depth; 0 for a root item.
    pub depth: usize,
    /// The row's full-width rect, in viewport-global coordinates.
    pub rect: Rect2,
    /// Whether the item has children, and therefore a fold arrow.
    pub has_children: bool,
}

impl TreeRow {
    /// The fold arrow's rect, or `None` when the row has no children.
    pub fn arrow_rect(&self) -> Option<Rect2> {
        if !self.has_children {
            return None;
        }
        Some(Rect2::new(
            Vector2::new(
                self.rect.position.x + self.depth as f32 * TREE_INDENT,
                self.rect.position.y,
            ),
            Vector2::new(TREE_ARROW_WIDTH, self.rect.size.y),
        ))
    }

    /// Where this row's first cell's text starts — past the indent and the
    /// arrow gutter, so text never overlaps the arrow of a foldable sibling.
    pub fn text_x(&self) -> f32 {
        self.rect.position.x + self.depth as f32 * TREE_INDENT + TREE_ARROW_WIDTH
    }

    /// Where `column`'s content starts, given the column spans.
    ///
    /// ⚠️ Only column 0 is indented and cleared of the arrow gutter — the tree
    /// structure lives in the first column, exactly as in Godot, so indenting
    /// the others would push their content out of their own span.
    pub fn cell_content_x(&self, column: usize, spans: &[(f32, f32)]) -> f32 {
        match column {
            0 => self.text_x(),
            _ => spans.get(column).map(|(x, _)| *x).unwrap_or(self.text_x()),
        }
    }

    /// The rect a cell occupies within this row.
    pub fn cell_rect(&self, column: usize, spans: &[(f32, f32)]) -> Rect2 {
        let x = self.cell_content_x(column, spans);
        let end = spans
            .get(column)
            .map(|(sx, w)| sx + w)
            .unwrap_or(self.rect.position.x + self.rect.size.x);
        Rect2::new(
            Vector2::new(x, self.rect.position.y),
            Vector2::new((end - x).max(0.0), self.rect.size.y),
        )
    }

    /// The checkbox glyph's rect for a CHECK cell, at the start of its content.
    pub fn check_rect(&self, column: usize, spans: &[(f32, f32)]) -> Rect2 {
        let cell = self.cell_rect(column, spans);
        Rect2::new(
            cell.position,
            Vector2::new(TREE_CHECK_SIZE.min(cell.size.x), cell.size.y),
        )
    }
}

/// The `(x, width)` span of each column, in viewport-global coordinates.
///
/// Widths come from [`TreeControl::column_width`], which is Godot's own integer
/// distribution over `custom_min_width` / `expand` / `expand_ratio`. Exported so
/// the draw pass and the hit test share one answer — the same rule the row rects
/// follow.
///
/// ⚠️ The spans need not fill `rect`: Godot's arithmetic truncates, and a
/// non-expanding column stops at its minimum. The remainder is empty space on
/// the right, which is what Godot draws too.
pub fn tree_column_spans(model: &TreeControl, rect: Rect2) -> Vec<(f32, f32)> {
    let content = rect.size.x.max(0.0) as i32;
    let mut x = rect.position.x;
    (0..model.column_count())
        .map(|c| {
            let w = model.column_width(c, content).max(0) as f32;
            let span = (x, w);
            x += w;
            span
        })
        .collect()
}

/// The `(item, column)` under a control-local point, or `None` (pat-s3rrv.75).
///
/// The single resolver behind `get_item_at_position`, `get_column_at_position`
/// and [`tree_drop_section_at`] — Godot funnels all three through
/// `_find_item_at_pos` for the same reason: three separate walks would answer
/// three different things for the same pixel.
///
/// ⚠️ `local_point` is in the CONTROL's own coordinates, as Godot's are.
///
/// ⚠️ A point past the LAST column is `None` even when it is inside the
/// control and level with a real row. Godot's column loop falls through and
/// returns nullptr, and with truncating column widths that remainder exists on
/// most trees — so a caller that treats "inside the control" as "on an item"
/// targets a row the user did not point at.
pub fn tree_item_at(
    tree: &SceneTree,
    node_id: NodeId,
    local_point: Vector2,
) -> Option<(usize, usize)> {
    let rect = control::get_global_rect(tree, node_id);
    let global_point = local_point + rect.position;
    if !control::rect_has_point(rect.position, rect.size, global_point) {
        return None;
    }
    let row = tree_visible_rows(tree, node_id)
        .into_iter()
        .find(|r| control::rect_has_point(r.rect.position, r.rect.size, global_point))?;
    let column = tree_column_spans(&load_tree(tree, node_id), rect)
        .into_iter()
        .position(|(x, w)| global_point.x >= x && global_point.x < x + w)?;
    Some((row.item, column))
}

/// The rect `item` occupies, in the CONTROL's own coordinates — the whole row
/// for `column = None`, or that column's cell (pat-s3rrv.75).
///
/// Empty when the item has no row: it may be hidden, or inside a collapsed
/// subtree. That is the honest answer — an item that is not laid out has no
/// area — and it is what a caller positioning a popup needs to branch on.
///
/// ⚠️ The whole-row rect spans the FULL control width, not the sum of the column
/// widths. Godot uses `get_size().width` here, and the two differ whenever the
/// columns do not fill the control — which the truncating width arithmetic makes
/// common.
pub fn tree_item_area_rect(
    tree: &SceneTree,
    node_id: NodeId,
    item: usize,
    column: Option<usize>,
) -> Rect2 {
    let rect = control::get_global_rect(tree, node_id);
    let empty = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0));
    let Some(row) = tree_visible_rows(tree, node_id)
        .into_iter()
        .find(|r| r.item == item)
    else {
        return empty;
    };
    // Rows carry viewport-global positions; every public position API on Tree
    // speaks the control's own coordinates.
    let y = row.rect.position.y - rect.position.y;

    match column {
        None => Rect2::new(
            Vector2::new(0.0, y),
            Vector2::new(rect.size.x, row.rect.size.y),
        ),
        Some(column) => {
            let spans = tree_column_spans(&load_tree(tree, node_id), rect);
            match spans.get(column) {
                Some((x, w)) => Rect2::new(
                    Vector2::new(x - rect.position.x, y),
                    Vector2::new(*w, row.rect.size.y),
                ),
                None => empty,
            }
        }
    }
}

/// Where a drop at `local_point` would land: `-1` above the row, `0` on it, `1`
/// below it, or [`TREE_DROP_SECTION_NONE`] for no row at all (pat-s3rrv.75).
///
/// ⚠️ `local_point` is in the CONTROL's own coordinates, matching Godot's
/// `get_drop_section_at_position(get_local_mouse_position())`. Passing a
/// viewport-global point silently reads the wrong row for any Tree that is not
/// at the origin.
///
/// The bands come from `drop_mode_flags`: ON_ITEM answers `0` everywhere,
/// INBETWEEN splits at the row's midpoint and never answers `0`, and the
/// default splits at the quarters so the middle half is "onto this item".
pub fn tree_drop_section_at(tree: &SceneTree, node_id: NodeId, local_point: Vector2) -> i64 {
    // ⚠️ Resolved through the same helper as `get_item_at_position`, which is
    // what applies Godot's COLUMN gate: `_find_item_at_pos` returns nullptr when
    // the x falls past the last column, and `get_drop_section_at_position`
    // answers its section only for a non-null item. Checking the row rect alone
    // reported a real section for a drop in the right-hand remainder — and that
    // remainder exists whenever the column widths do not divide evenly, which
    // the truncating arithmetic makes common.
    let Some((item, _column)) = tree_item_at(tree, node_id, local_point) else {
        return TREE_DROP_SECTION_NONE;
    };
    let rect = control::get_global_rect(tree, node_id);
    let global_point = local_point + rect.position;
    let Some(row) = tree_visible_rows(tree, node_id)
        .into_iter()
        .find(|r| r.item == item)
    else {
        return TREE_DROP_SECTION_NONE;
    };

    let flags = int_prop(tree, node_id, "drop_mode_flags").unwrap_or(0);
    let y = global_point.y - row.rect.position.y;
    let h = row.rect.size.y;
    if flags == DROP_MODE_ON_ITEM {
        0
    } else if flags == DROP_MODE_INBETWEEN {
        // ⚠️ No "onto" band at all in this mode — the row splits in half.
        if y < h / 2.0 {
            -1
        } else {
            1
        }
    } else if y < h / 4.0 {
        -1
    } else if y >= h * 3.0 / 4.0 {
        1
    } else {
        0
    }
}

/// Rebuilds the [`TreeControl`] model from `node_id`'s properties.
///
/// Same storage shape as [`load_item_list`] — an Array of Dictionaries under
/// `items` — so a Tree round-trips through `.tscn` the way every other item view
/// does. Each entry carries `parent` (an index, `-1` for none), the per-column
/// `texts`, and the `collapsed`/`selectable`/`selected` flags.
pub fn load_tree(tree: &SceneTree, node_id: NodeId) -> TreeControl {
    let columns = int_prop(tree, node_id, "columns").unwrap_or(1).max(1) as usize;
    let mut model = TreeControl::new(columns);

    // ⚠️ Read BEFORE the items: `store_tree` runs on every click, so a layout a
    // script configured that the round trip dropped would be erased the first
    // time the user clicked a row.
    for (c, entry) in item_dicts(tree, node_id, "column_info").iter().enumerate() {
        let Some(info) = model.column_mut(c) else {
            break;
        };
        *info = TreeColumn {
            title: dict_string(entry, "title"),
            custom_min_width: dict_int(entry, "custom_min_width", 0) as i32,
            expand_ratio: dict_int(entry, "expand_ratio", 1) as i32,
            expand: dict_bool(entry, "expand", true),
        };
    }

    let dicts = item_dicts(tree, node_id, "items");

    // ⚠️ Two passes. `create_item` takes a parent INDEX, so an entry may name a
    // parent that does not exist yet; every item is created first and the
    // parent links are read from the stored order, which is the order this
    // module writes and therefore already topological for anything we saved.
    for d in &dicts {
        let parent = match dict_int(d, "parent", -1) {
            p if p >= 0 => Some(p as usize),
            _ => None,
        };
        let idx = model.create_item(parent);
        match d.get(Variant::String("texts".into())) {
            Some(Variant::Array(cells)) => {
                for (col, cell) in cells.iter().enumerate() {
                    if let Variant::String(s) = cell {
                        model.set_text(idx, col, s.clone());
                    }
                }
            }
            // A single-column tree may store just `text`, matching the ItemList
            // shorthand a hand-written `.tscn` uses.
            _ => model.set_text(idx, 0, dict_string(d, "text")),
        }
    }
    // Flags after every item exists, so `select` sees the final arena.
    for (i, d) in dicts.iter().enumerate() {
        if !dict_bool(d, "selectable", true) {
            model.set_selectable(i, false);
        }
        if dict_bool(d, "collapsed", false) {
            model.set_collapsed(i, true);
        }
        if !dict_bool(d, "visible", true) {
            model.set_item_visible(i, false);
        }
        // Per-column cell state (pat-s3rrv.75). Parallel arrays rather than a
        // dictionary per cell: the columns are already positional, and a sparse
        // map would have to encode the index twice.
        for (key, apply) in [
            ("modes", 0u8),
            ("checked", 1),
            ("editable", 2),
            ("indeterminate", 3),
        ] {
            let Some(Variant::Array(values)) = d.get(Variant::String(key.into())) else {
                continue;
            };
            for (col, v) in values.iter().enumerate() {
                match apply {
                    0 => model.set_cell_mode(
                        i,
                        col,
                        TreeCellMode::from_godot_value(match v {
                            Variant::Int(n) => *n,
                            _ => 0,
                        }),
                    ),
                    1 => model.set_checked(i, col, matches!(v, Variant::Bool(true))),
                    2 => model.set_editable(i, col, matches!(v, Variant::Bool(true))),
                    _ => model.set_indeterminate(i, col, matches!(v, Variant::Bool(true))),
                }
            }
        }
    }
    for (i, d) in dicts.iter().enumerate() {
        if dict_bool(d, "selected", false) {
            model.select(i);
        }
    }
    model
}

/// Writes the [`TreeControl`] model back onto `node_id`'s `items` property.
pub fn store_tree(tree: &mut SceneTree, node_id: NodeId, model: &TreeControl) {
    let selected = model.get_selected();
    let items: Vec<Variant> = (0..model.item_count())
        .filter_map(|i| model.get_item(i).map(|it| (i, it)))
        .map(|(i, it)| {
            let mut d = Dictionary::new();
            d.insert(
                Variant::String("parent".into()),
                Variant::Int(it.parent.map(|p| p as i64).unwrap_or(-1)),
            );
            d.insert(
                Variant::String("texts".into()),
                Variant::Array(
                    it.columns
                        .iter()
                        .map(|c| Variant::String(c.clone()))
                        .collect(),
                ),
            );
            d.insert(
                Variant::String("collapsed".into()),
                Variant::Bool(it.collapsed),
            );
            d.insert(
                Variant::String("modes".into()),
                Variant::Array(
                    it.modes
                        .iter()
                        .map(|m| Variant::Int(m.godot_value()))
                        .collect(),
                ),
            );
            d.insert(
                Variant::String("checked".into()),
                Variant::Array(it.checked.iter().map(|c| Variant::Bool(*c)).collect()),
            );
            d.insert(
                Variant::String("editable".into()),
                Variant::Array(it.editable.iter().map(|e| Variant::Bool(*e)).collect()),
            );
            d.insert(
                Variant::String("indeterminate".into()),
                Variant::Array(it.indeterminate.iter().map(|i| Variant::Bool(*i)).collect()),
            );
            d.insert(
                Variant::String("selectable".into()),
                Variant::Bool(it.selectable),
            );
            d.insert(Variant::String("visible".into()), Variant::Bool(it.visible));
            d.insert(
                Variant::String("selected".into()),
                Variant::Bool(selected == Some(i)),
            );
            Variant::Dictionary(d)
        })
        .collect();
    set_prop(tree, node_id, "items", Variant::Array(items));
    set_prop(
        tree,
        node_id,
        "columns",
        Variant::Int(model.column_count() as i64),
    );
    let column_info: Vec<Variant> = (0..model.column_count())
        .filter_map(|c| model.column(c))
        .map(|info| {
            let mut d = Dictionary::new();
            d.insert(
                Variant::String("title".into()),
                Variant::String(info.title.clone()),
            );
            d.insert(
                Variant::String("custom_min_width".into()),
                Variant::Int(info.custom_min_width as i64),
            );
            d.insert(
                Variant::String("expand_ratio".into()),
                Variant::Int(info.expand_ratio as i64),
            );
            d.insert(Variant::String("expand".into()), Variant::Bool(info.expand));
            Variant::Dictionary(d)
        })
        .collect();
    set_prop(tree, node_id, "column_info", Variant::Array(column_info));
}

/// The visible rows of the Tree at `node_id`, top to bottom, in viewport-global
/// coordinates (pat-s3rrv.75).
///
/// This is the single source of row geometry: `item_view_click` resolves a click
/// through it, and the `control_render` draw arm lays rows out from it, so the
/// drawn row and the hit row are the same rect by construction.
///
/// ⚠️ Godot's `hide_root` suppresses the root ROW while keeping its children
/// visible and un-indented; a tree authored with `hide_root = true` whose root
/// still occupied row 0 would offset every hit test by one row.
pub fn tree_visible_rows(tree: &SceneTree, node_id: NodeId) -> Vec<TreeRow> {
    let model = load_tree(tree, node_id);
    tree_rows_of(
        &model,
        control::get_global_rect(tree, node_id),
        hide_root(tree, node_id),
    )
}

/// Whether the node hides its root row (Godot `Tree.hide_root`, default false).
fn hide_root(tree: &SceneTree, node_id: NodeId) -> bool {
    matches!(prop(tree, node_id, "hide_root"), Variant::Bool(true))
}

/// Lays a model out into rows within `rect`. Split from
/// [`tree_visible_rows`] so it can be exercised without a scene.
pub fn tree_rows_of(model: &TreeControl, rect: Rect2, hide_root: bool) -> Vec<TreeRow> {
    let root = model.get_root();
    model
        .visible_items()
        .into_iter()
        .filter(|(idx, _)| !(hide_root && Some(*idx) == root))
        .enumerate()
        .map(|(row, (item, depth))| TreeRow {
            item,
            // A hidden root must not leave its children indented under nothing.
            depth: if hide_root {
                depth.saturating_sub(1)
            } else {
                depth
            },
            rect: Rect2::new(
                Vector2::new(
                    rect.position.x,
                    rect.position.y + row as f32 * ITEM_ROW_HEIGHT,
                ),
                Vector2::new(rect.size.x, ITEM_ROW_HEIGHT),
            ),
            has_children: model.has_children(item),
        })
        .collect()
}

fn tree_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let mut model = load_tree(tree, node_id);
    // Read-only queries answer without a store, so they cannot perturb state.
    //
    // ⚠️ Godot returns a `TreeItem` OBJECT from `create_item`/`get_root`/
    // `get_selected`; Patina has no TreeItem instance type wired to a script, so
    // these return the arena INDEX (and `Nil` for "none"), the same way this
    // module reports ItemList rows. A script therefore holds an index and passes
    // it back as one — self-consistent, but not Godot's object handle.
    match method {
        "get_root" => {
            return Some(match model.get_root() {
                Some(i) => Variant::Int(i as i64),
                None => Variant::Nil,
            })
        }
        "get_selected" => {
            return Some(match model.get_selected() {
                Some(i) => Variant::Int(i as i64),
                None => Variant::Nil,
            })
        }
        // Single-select only so far, so the selected column is always the
        // first. Reported rather than omitted because a script that reads it
        // expects a column number, not a missing method.
        "get_selected_column" => {
            return Some(Variant::Int(if model.get_selected().is_some() {
                0
            } else {
                -1
            }))
        }
        "get_columns" => return Some(Variant::Int(model.column_count() as i64)),
        "get_column_title" => {
            let col = arg_index(args, 0)?;
            return Some(Variant::String(
                model
                    .column(col)
                    .map(|c| c.title.clone())
                    .unwrap_or_default(),
            ));
        }
        "get_column_expand" => {
            let col = arg_index(args, 0)?;
            return Some(Variant::Bool(
                model.column(col).map(|c| c.expand).unwrap_or(false),
            ));
        }
        "get_column_expand_ratio" => {
            let col = arg_index(args, 0)?;
            return Some(Variant::Int(
                model
                    .column(col)
                    .map(|c| c.expand_ratio as i64)
                    .unwrap_or(0),
            ));
        }
        "get_column_custom_minimum_width" => {
            let col = arg_index(args, 0)?;
            return Some(Variant::Int(
                model
                    .column(col)
                    .map(|c| c.custom_min_width as i64)
                    .unwrap_or(0),
            ));
        }
        "get_column_width" => {
            let col = arg_index(args, 0)?;
            let width = control::get_global_rect(tree, node_id).size.x.max(0.0) as i32;
            return Some(Variant::Int(model.column_width(col, width) as i64));
        }
        "get_item_count" => return Some(Variant::Int(model.item_count() as i64)),
        "is_collapsed" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(model.is_collapsed(idx)));
        }
        "get_item_text" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1).unwrap_or(0);
            return Some(Variant::String(model.get_text(idx, col).to_string()));
        }
        "is_checked" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1).unwrap_or(0);
            return Some(Variant::Bool(model.is_checked(idx, col)));
        }
        "is_indeterminate" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1).unwrap_or(0);
            return Some(Variant::Bool(model.is_indeterminate(idx, col)));
        }
        "is_item_visible" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(model.is_item_visible(idx)));
        }
        "get_drop_section_at_position" => {
            let Some(Variant::Vector2(point)) = args.first() else {
                return Some(Variant::Int(TREE_DROP_SECTION_NONE));
            };
            return Some(Variant::Int(tree_drop_section_at(tree, node_id, *point)));
        }
        // ⚠️ Godot returns a TreeItem object here and `null` for a miss; with no
        // TreeItem instance type wired to scripts this hands back the arena
        // index, and `Nil` for the miss — the same handle `get_selected` uses.
        "get_item_at_position" => {
            let Some(Variant::Vector2(point)) = args.first() else {
                return Some(Variant::Nil);
            };
            return Some(match tree_item_at(tree, node_id, *point) {
                Some((item, _)) => Variant::Int(item as i64),
                None => Variant::Nil,
            });
        }
        // Godot's signature is `get_item_area_rect(item, column = -1,
        // button_index = -1)`. A button index is REFUSED rather than guessed:
        // sizing a button needs its texture, and no buttons are modelled yet, so
        // answering the cell rect for one would be quietly wrong.
        "get_item_area_rect" => {
            let item = arg_index(args, 0)?;
            let empty = Variant::Rect2(Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0)));
            if matches!(args.get(2), Some(Variant::Int(b)) if *b >= 0) {
                return Some(empty);
            }
            let column = match args.get(1) {
                Some(Variant::Int(c)) if *c >= 0 => Some(*c as usize),
                _ => None,
            };
            return Some(Variant::Rect2(tree_item_area_rect(
                tree, node_id, item, column,
            )));
        }
        // Godot's own miss value here is `-1`, not the drop section's `-100`.
        "get_column_at_position" => {
            let Some(Variant::Vector2(point)) = args.first() else {
                return Some(Variant::Int(-1));
            };
            return Some(Variant::Int(
                tree_item_at(tree, node_id, *point)
                    .map(|(_, column)| column as i64)
                    .unwrap_or(-1),
            ));
        }
        "is_editable" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1).unwrap_or(0);
            return Some(Variant::Bool(model.is_editable(idx, col)));
        }
        "get_cell_mode" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1).unwrap_or(0);
            return Some(Variant::Int(model.get_cell_mode(idx, col).godot_value()));
        }
        "get_edit_checkbox_cell_only_when_checkbox_is_pressed" => {
            return Some(Variant::Bool(matches!(
                prop(
                    tree,
                    node_id,
                    "edit_checkbox_cell_only_when_checkbox_is_pressed"
                ),
                Variant::Bool(true)
            )))
        }
        // Patina draws every row and never scrolls, so there is nothing to
        // bring into view. Answered rather than left unhandled so a port of a
        // real Godot script does not abort on a missing method.
        "ensure_cursor_is_visible" => return Some(Variant::Nil),
        _ => {}
    }

    // Handled ahead of the mutating tail because it EMITS: Godot fires
    // `check_propagated_to_item(item, column)` once per item the propagation
    // touches, and the shared tail below only stores.
    if method == "propagate_check" {
        let idx = arg_index(args, 0)?;
        let col = arg_index(args, 1).unwrap_or(0);
        let touched = model.propagate_check(idx, col);
        store_tree(tree, node_id, &model);
        for item in touched {
            tree.emit_signal(
                node_id,
                "check_propagated_to_item",
                &[Variant::Int(item as i64), Variant::Int(col as i64)],
            );
        }
        return Some(Variant::Nil);
    }

    let result = match method {
        // Godot: a null parent means "under the root", creating the root the
        // first time. `get_root()` is None until then, so one expression covers
        // both cases.
        "create_item" => {
            let parent = match args.first() {
                Some(Variant::Int(i)) if *i >= 0 => Some(*i as usize),
                _ => model.get_root(),
            };
            Variant::Int(model.create_item(parent) as i64)
        }
        "set_columns" => {
            model.set_columns(arg_index(args, 0)?);
            Variant::Nil
        }
        "set_column_title" => {
            let col = arg_index(args, 0)?;
            let title = arg_string(args, 1)?;
            model.column_mut(col)?.title = title;
            Variant::Nil
        }
        "set_column_expand" => {
            let col = arg_index(args, 0)?;
            model.column_mut(col)?.expand = !matches!(args.get(1), Some(Variant::Bool(false)));
            Variant::Nil
        }
        "set_column_expand_ratio" => {
            let col = arg_index(args, 0)?;
            let ratio = match args.get(1) {
                Some(Variant::Int(n)) => *n as i32,
                _ => 1,
            };
            model.column_mut(col)?.expand_ratio = ratio;
            Variant::Nil
        }
        "set_column_custom_minimum_width" => {
            let col = arg_index(args, 0)?;
            let min = match args.get(1) {
                Some(Variant::Int(n)) => *n as i32,
                _ => 0,
            };
            // Godot clamps a negative minimum to zero rather than rejecting it.
            model.column_mut(col)?.custom_min_width = min.max(0);
            Variant::Nil
        }
        "set_item_text" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1)?;
            model.set_text(idx, col, arg_string(args, 2)?);
            Variant::Nil
        }
        "set_collapsed" => {
            let idx = arg_index(args, 0)?;
            model.set_collapsed(idx, matches!(args.get(1), Some(Variant::Bool(true))));
            Variant::Nil
        }
        "set_selectable" => {
            let idx = arg_index(args, 0)?;
            model.set_selectable(idx, !matches!(args.get(1), Some(Variant::Bool(false))));
            Variant::Nil
        }
        "set_item_visible" => {
            let idx = arg_index(args, 0)?;
            model.set_item_visible(idx, !matches!(args.get(1), Some(Variant::Bool(false))));
            Variant::Nil
        }
        "set_cell_mode" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1)?;
            let mode = match args.get(2) {
                Some(Variant::Int(n)) => *n,
                _ => 0,
            };
            model.set_cell_mode(idx, col, TreeCellMode::from_godot_value(mode));
            Variant::Nil
        }
        "set_checked" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1)?;
            model.set_checked(idx, col, matches!(args.get(2), Some(Variant::Bool(true))));
            Variant::Nil
        }
        "set_indeterminate" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1)?;
            model.set_indeterminate(idx, col, matches!(args.get(2), Some(Variant::Bool(true))));
            Variant::Nil
        }
        "set_editable" => {
            let idx = arg_index(args, 0)?;
            let col = arg_index(args, 1)?;
            model.set_editable(idx, col, matches!(args.get(2), Some(Variant::Bool(true))));
            Variant::Nil
        }
        // A Tree-level flag rather than per-cell state, so it lives on the node
        // beside `hide_root` instead of in the arena.
        "set_edit_checkbox_cell_only_when_checkbox_is_pressed" => {
            let on = matches!(args.first(), Some(Variant::Bool(true)));
            set_prop(
                tree,
                node_id,
                "edit_checkbox_cell_only_when_checkbox_is_pressed",
                Variant::Bool(on),
            );
            Variant::Nil
        }
        "select" => {
            let idx = arg_index(args, 0)?;
            Variant::Bool(model.select(idx))
        }
        "deselect_all" => {
            model.deselect_all();
            Variant::Nil
        }
        "clear" => {
            model.clear();
            Variant::Nil
        }
        _ => return None,
    };
    store_tree(tree, node_id, &model);
    Some(result)
}

// -- OptionButton -----------------------------------------------------------

/// Rebuilds the [`OptionButton`] model from `node_id`'s properties.
pub fn load_option_button(tree: &SceneTree, node_id: NodeId) -> OptionButton {
    let mut ob = OptionButton::new();
    let dicts = item_dicts(tree, node_id, "items");
    for (i, d) in dicts.iter().enumerate() {
        ob.add_item(dict_string(d, "text"), dict_int(d, "id", i as i64));
        if dict_bool(d, "disabled", false) {
            ob.set_item_disabled(i, true);
        }
    }
    // Restored last: `add_item` auto-selects the first item, which would
    // otherwise overwrite a persisted `selected` (including `-1`).
    if let Some(sel) = int_prop(tree, node_id, "selected") {
        ob.set_selected_index(sel);
    }
    ob
}

/// Writes the [`OptionButton`] model back onto `node_id`.
///
/// Also mirrors the selected item's label onto the node's `text` property, the
/// way Godot's `OptionButton::_select` calls `set_text` — that is what the
/// button draws.
pub fn store_option_button(tree: &mut SceneTree, node_id: NodeId, ob: &OptionButton) {
    let items: Vec<Variant> = (0..ob.item_count())
        .filter_map(|i| ob.get_item(i))
        .map(|it| {
            let mut d = Dictionary::new();
            d.insert(
                Variant::String("text".into()),
                Variant::String(it.text.clone()),
            );
            d.insert(Variant::String("id".into()), Variant::Int(it.id));
            d.insert(
                Variant::String("disabled".into()),
                Variant::Bool(it.disabled),
            );
            Variant::Dictionary(d)
        })
        .collect();
    set_prop(tree, node_id, "items", Variant::Array(items));
    set_prop(tree, node_id, "selected", Variant::Int(ob.get_selected()));
    let text = if ob.get_selected() >= 0 {
        ob.get_item(ob.get_selected() as usize)
            .map(|it| it.text.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    set_prop(tree, node_id, "text", Variant::String(text));
}

// -- TabBar / TabContainer --------------------------------------------------

/// The tab titles of `node_id`.
///
/// A `TabBar` carries its own `tabs` array; a `TabContainer`'s tabs *are* its
/// children, so its titles come from [`control::get_tab_title`] (node name, or
/// the `_tab_name` override) — Godot's `TabContainer::get_tab_title`.
pub fn tab_titles(tree: &SceneTree, node_id: NodeId) -> Vec<String> {
    match item_view_kind(tree, node_id) {
        Some(ItemViewKind::TabContainer) => {
            let count = tree
                .get_node(node_id)
                .map(|n| n.children().len())
                .unwrap_or(0);
            (0..count)
                .map(|i| control::get_tab_title(tree, node_id, i))
                .collect()
        }
        _ => match prop(tree, node_id, "tabs") {
            Variant::Array(items) => items
                .into_iter()
                .map(|v| match v {
                    Variant::String(s) => s,
                    Variant::Dictionary(d) => dict_string(&d, "title"),
                    _ => String::new(),
                })
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// Rebuilds the [`TabBar`] model for `node_id` (a `TabBar` or a `TabContainer`).
pub fn load_tab_bar(tree: &SceneTree, node_id: NodeId) -> TabBar {
    let mut bar = TabBar::new();
    for title in tab_titles(tree, node_id) {
        bar.add_tab(title);
    }
    // `current_tab` is the same property `control::arrange_container` reads to
    // decide which TabContainer child is visible, so the model and the layout
    // never disagree.
    if let Some(current) = int_prop(tree, node_id, "current_tab") {
        bar.set_current_index(current);
    }
    bar
}

/// Writes the [`TabBar`] model back onto `node_id`.
///
/// A `TabContainer`'s tabs are its children, so only `current_tab` is stored for
/// one — renaming or adding tabs there means renaming or adding children.
pub fn store_tab_bar(tree: &mut SceneTree, node_id: NodeId, bar: &TabBar) {
    if item_view_kind(tree, node_id) != Some(ItemViewKind::TabContainer) {
        let tabs: Vec<Variant> = (0..bar.tab_count())
            .map(|i| Variant::String(bar.get_tab_title(i).unwrap_or_default().to_string()))
            .collect();
        set_prop(tree, node_id, "tabs", Variant::Array(tabs));
        set_prop(
            tree,
            node_id,
            "tab_count",
            Variant::Int(bar.tab_count() as i64),
        );
    }
    set_prop(
        tree,
        node_id,
        "current_tab",
        Variant::Int(bar.get_current_tab()),
    );
}

// ---------------------------------------------------------------------------
// Geometry — shared by the renderer and the click mapping
// ---------------------------------------------------------------------------

/// The rect of each of `count` `ItemList` rows inside `rect`, in `rect`'s own
/// coordinate space. Rows past the bottom edge are still returned (the list
/// scrolls in Godot); [`item_view_click`] rejects those separately.
pub fn item_row_rects(count: usize, rect: Rect2) -> Vec<Rect2> {
    (0..count)
        .map(|i| {
            Rect2::new(
                Vector2::new(
                    rect.position.x,
                    rect.position.y + i as f32 * ITEM_ROW_HEIGHT,
                ),
                Vector2::new(rect.size.x, ITEM_ROW_HEIGHT),
            )
        })
        .collect()
}

/// The `(x, width)` span of each tab of `node_id`, in the node's own coordinate
/// space.
///
/// One layout rule for both classes: each tab is as wide as its title plus
/// [`control::TAB_H_PADDING`] on each side, laid left to right — the same rule
/// `TabContainer` draws its header with, so a `TabBar` strip and a container
/// header line up and hit-test identically.
pub fn tab_spans(tree: &SceneTree, node_id: NodeId) -> Vec<(f32, f32)> {
    if item_view_kind(tree, node_id) == Some(ItemViewKind::TabContainer) {
        return control::tab_rects(tree, node_id);
    }
    let scale = text_scale(control::get_font_size(tree, node_id));
    let mut x = 0.0f32;
    tab_titles(tree, node_id)
        .iter()
        .map(|t| {
            let w = text_width(t, scale) + control::TAB_H_PADDING * 2.0;
            let span = (x, w);
            x += w;
            span
        })
        .collect()
}

/// The index of the tab under the viewport-global `point`, or `None`.
///
/// A `TabContainer`'s strip is only its header band, which
/// [`control::tab_at_point`] already resolves; a `TabBar` *is* its strip.
pub fn tab_at_point(tree: &SceneTree, node_id: NodeId, point: Vector2) -> Option<usize> {
    if item_view_kind(tree, node_id) == Some(ItemViewKind::TabContainer) {
        return control::tab_at_point(tree, node_id, point);
    }
    let rect = control::get_global_rect(tree, node_id);
    let local = point - rect.position;
    if local.y < 0.0 || local.y >= rect.size.y || local.x < 0.0 {
        return None;
    }
    tab_spans(tree, node_id)
        .into_iter()
        .position(|(x, w)| local.x >= x && local.x < x + w)
}

// ---------------------------------------------------------------------------
// Script method bridge
// ---------------------------------------------------------------------------

/// Runs an item-view method on `node_id`, or returns `None` when the node is not
/// an item view or does not implement `method`.
///
/// This is the half of Godot's `Object::callp` that reaches the engine's own
/// method for the class: `crate::scripting`'s `call_node_method` forwards here,
/// so `$ItemList.add_item("a")` from GDScript lands on the real model.
pub fn call_item_view_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    match item_view_kind(tree, node_id)? {
        ItemViewKind::List => item_list_method(tree, node_id, method, args),
        ItemViewKind::Option => option_button_method(tree, node_id, method, args),
        ItemViewKind::Tabs | ItemViewKind::TabContainer => tab_method(tree, node_id, method, args),
        ItemViewKind::TreeView => tree_method(tree, node_id, method, args),
    }
}

fn item_list_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let mut list = load_item_list(tree, node_id);
    // Read-only queries answer without a store, so they cannot perturb state.
    match method {
        "get_item_count" => return Some(Variant::Int(list.item_count() as i64)),
        "get_item_text" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::String(
                list.get_item(idx)
                    .map(|i| i.text.clone())
                    .unwrap_or_default(),
            ));
        }
        "is_item_disabled" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(
                list.get_item(idx).map(|i| i.disabled).unwrap_or(false),
            ));
        }
        "is_item_selectable" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(
                list.get_item(idx).map(|i| i.selectable).unwrap_or(false),
            ));
        }
        "is_selected" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(list.is_selected(idx)));
        }
        "get_selected_items" => {
            return Some(Variant::Array(
                list.get_selected_items()
                    .into_iter()
                    .map(|i| Variant::Int(i as i64))
                    .collect(),
            ));
        }
        _ => {}
    }

    let result = match method {
        // Godot: `int add_item(text, icon = null, selectable = true)`.
        "add_item" => {
            let text = arg_string(args, 0)?;
            let idx = list.add_item(text);
            if !arg_bool(args, 1, true) {
                list.set_item_selectable(idx, false);
            }
            Variant::Int(idx as i64)
        }
        "add_icon_item" => {
            let icon = arg_string(args, 0)?;
            let text = arg_string(args, 1).unwrap_or_default();
            Variant::Int(list.add_icon_item(text, icon) as i64)
        }
        "set_item_text" => {
            let idx = arg_index(args, 0)?;
            list.set_item_text(idx, arg_string(args, 1)?);
            Variant::Nil
        }
        "set_item_disabled" => {
            let idx = arg_index(args, 0)?;
            list.set_item_disabled(idx, arg_bool(args, 1, true));
            Variant::Nil
        }
        "set_item_selectable" => {
            let idx = arg_index(args, 0)?;
            list.set_item_selectable(idx, arg_bool(args, 1, true));
            Variant::Nil
        }
        // The script setter changes the selection *silently*: Godot emits
        // `item_selected` only from a click (item_list.cpp `gui_input`).
        "select" => {
            let idx = arg_index(args, 0)?;
            list.select(idx);
            Variant::Nil
        }
        "deselect" => {
            let idx = arg_index(args, 0)?;
            list.deselect(idx);
            Variant::Nil
        }
        "deselect_all" => {
            list.deselect_all();
            Variant::Nil
        }
        "clear" => {
            list.clear();
            Variant::Nil
        }
        _ => return None,
    };
    store_item_list(tree, node_id, &list);
    Some(result)
}

fn option_button_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let mut ob = load_option_button(tree, node_id);
    match method {
        "get_item_count" => return Some(Variant::Int(ob.item_count() as i64)),
        "get_selected" => return Some(Variant::Int(ob.get_selected())),
        "get_selected_id" => return Some(Variant::Int(ob.get_selected_id())),
        "get_item_text" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::String(
                ob.get_item(idx).map(|i| i.text.clone()).unwrap_or_default(),
            ));
        }
        "get_item_id" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Int(ob.get_item(idx).map(|i| i.id).unwrap_or(-1)));
        }
        "is_item_disabled" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::Bool(
                ob.get_item(idx).map(|i| i.disabled).unwrap_or(false),
            ));
        }
        _ => {}
    }

    let result = match method {
        // Godot: `void add_item(label, id = -1)`, `id < 0` meaning "use the index".
        "add_item" => {
            let text = arg_string(args, 0)?;
            ob.add_item(text, arg_int(args, 1).unwrap_or(-1));
            Variant::Nil
        }
        "set_item_text" => {
            let idx = arg_index(args, 0)?;
            ob.set_item_text(idx, arg_string(args, 1)?);
            Variant::Nil
        }
        "set_item_disabled" => {
            let idx = arg_index(args, 0)?;
            ob.set_item_disabled(idx, arg_bool(args, 1, true));
            Variant::Nil
        }
        // `select()` is the silent setter — `OptionButton::select` calls
        // `_select(idx, false)`, i.e. emit = false (option_button.cpp).
        "select" => {
            ob.set_selected_index(arg_int(args, 0)?);
            Variant::Nil
        }
        "clear" => {
            ob.clear();
            Variant::Nil
        }
        _ => return None,
    };
    store_option_button(tree, node_id, &ob);
    Some(result)
}

fn tab_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let mut bar = load_tab_bar(tree, node_id);
    let is_container = item_view_kind(tree, node_id) == Some(ItemViewKind::TabContainer);
    match method {
        "get_tab_count" => return Some(Variant::Int(bar.tab_count() as i64)),
        "get_current_tab" => return Some(Variant::Int(bar.get_current_tab())),
        "get_tab_title" => {
            let idx = arg_index(args, 0)?;
            return Some(Variant::String(
                bar.get_tab_title(idx).unwrap_or_default().to_string(),
            ));
        }
        // Godot `TabContainer::get_current_tab_control` — the visible child.
        "get_current_tab_control" if is_container => {
            let current = bar.get_current_tab();
            let child = if current >= 0 {
                tree.get_node(node_id)
                    .and_then(|n| n.children().get(current as usize).copied())
            } else {
                None
            };
            return Some(match child {
                Some(c) => Variant::ObjectId(c.object_id()),
                None => Variant::Nil,
            });
        }
        // The one mutator that emits: `TabBar::set_current_tab` fires
        // `tab_selected` on every call and `tab_changed` only on a real move
        // (upstream/godot/scene/gui/tab_bar.cpp).
        "set_current_tab" => {
            let idx = arg_index(args, 0)?;
            let changed = bar.set_current_tab(idx);
            store_tab_bar(tree, node_id, &bar);
            tree.emit_signal(node_id, "tab_selected", &[Variant::Int(idx as i64)]);
            if changed {
                tree.emit_signal(node_id, "tab_changed", &[Variant::Int(idx as i64)]);
            }
            return Some(Variant::Nil);
        }
        _ => {}
    }

    // A TabContainer's tabs are its children; only a TabBar owns a tab list.
    if is_container {
        return None;
    }
    let result = match method {
        "add_tab" => {
            bar.add_tab(arg_string(args, 0).unwrap_or_default());
            Variant::Nil
        }
        "set_tab_title" => {
            let idx = arg_index(args, 0)?;
            bar.set_tab_title(idx, arg_string(args, 1)?);
            Variant::Nil
        }
        "clear_tabs" => {
            bar.clear_tabs();
            Variant::Nil
        }
        _ => return None,
    };
    store_tab_bar(tree, node_id, &bar);
    Some(result)
}

// ---------------------------------------------------------------------------
// Click mapping
// ---------------------------------------------------------------------------

/// Routes a click at `global_point` (viewport space) onto `node_id`'s item or
/// tab, emitting the signals Godot emits. Returns `true` when the click landed
/// on an item/tab.
///
/// `double_click` promotes an `ItemList` row click to `item_activated`, which is
/// Godot's double-click action.
pub fn item_view_click(
    tree: &mut SceneTree,
    node_id: NodeId,
    global_point: Vector2,
    double_click: bool,
) -> bool {
    let Some(kind) = item_view_kind(tree, node_id) else {
        return false;
    };
    let global = control::get_global_rect(tree, node_id);
    let local = global_point - global.position;
    match kind {
        ItemViewKind::List => {
            let mut list = load_item_list(tree, node_id);
            let rects = item_row_rects(list.item_count(), global);
            let Some(idx) = rects
                .iter()
                .position(|r| control::rect_has_point(r.position, r.size, global_point))
            else {
                // Godot's `empty_clicked(at_position, mouse_button_index)`.
                tree.emit_signal(
                    node_id,
                    "empty_clicked",
                    &[Variant::Vector2(local), Variant::Int(MOUSE_BUTTON_LEFT)],
                );
                return false;
            };
            let selectable = list
                .get_item(idx)
                .map(|i| i.selectable && !i.disabled)
                .unwrap_or(false);
            // item_list.cpp: a selectable, not-already-selected row is selected
            // and reports `item_selected`; `item_clicked` fires either way.
            if selectable && !list.is_selected(idx) {
                list.select(idx);
                store_item_list(tree, node_id, &list);
                tree.emit_signal(node_id, "item_selected", &[Variant::Int(idx as i64)]);
            }
            tree.emit_signal(
                node_id,
                "item_clicked",
                &[
                    Variant::Int(idx as i64),
                    Variant::Vector2(local),
                    Variant::Int(MOUSE_BUTTON_LEFT),
                ],
            );
            // `ItemList::activate` is the model's own "may this row activate?"
            // answer and yields the `item_activated` payload.
            if double_click {
                if let Some(activated) = list.activate(idx) {
                    tree.emit_signal(node_id, "item_activated", &[Variant::Int(activated as i64)]);
                }
            }
            true
        }
        // A Tree resolves a click against the SAME row list the draw pass
        // lays out (pat-s3rrv.75). Order matters inside a row: the fold arrow
        // is checked before selection, because Godot folds without selecting —
        // clicking a disclosure triangle must not move the selection.
        ItemViewKind::TreeView => {
            let mut model = load_tree(tree, node_id);
            let rows = tree_rows_of(&model, global, hide_root(tree, node_id));
            let Some(row) = rows
                .iter()
                .find(|r| control::rect_has_point(r.rect.position, r.rect.size, global_point))
            else {
                // Godot emits BOTH for a click that lands on the control but on
                // no row: `empty_clicked` from `propagate_mouse_event` reaching
                // the root unconsumed (tree.cpp:3380), and `nothing_selected`
                // from the un-handled click (tree.cpp:4226). A handler
                // connected to either one must fire.
                tree.emit_signal(
                    node_id,
                    "empty_clicked",
                    &[Variant::Vector2(local), Variant::Int(MOUSE_BUTTON_LEFT)],
                );
                tree.emit_signal(node_id, "nothing_selected", &[]);
                return false;
            };

            if let Some(arrow) = row.arrow_rect() {
                if control::rect_has_point(arrow.position, arrow.size, global_point) {
                    let collapsed = !model.is_collapsed(row.item);
                    model.set_collapsed(row.item, collapsed);
                    store_tree(tree, node_id, &model);
                    // ⚠️ Godot passes the TreeItem object; Patina has no
                    // TreeItem instance type wired to scripts, so the arena
                    // index travels instead — the same handle `get_selected`
                    // hands back.
                    tree.emit_signal(node_id, "item_collapsed", &[Variant::Int(row.item as i64)]);
                    return true;
                }
            }

            // Which column was hit; the cell's own mode and editability decide
            // everything below (tree.cpp:3175-3253).
            let spans = tree_column_spans(&model, global);
            let column = spans
                .iter()
                .position(|(x, w)| global_point.x >= *x && global_point.x < *x + *w)
                .unwrap_or(0);
            let editable = model.is_editable(row.item, column);
            let checkbox_only = matches!(
                prop(
                    tree,
                    node_id,
                    "edit_checkbox_cell_only_when_checkbox_is_pressed"
                ),
                Variant::Bool(true)
            );

            // ⚠️ A double-click ACTIVATES instead of selecting, and only when
            // the cell is not editable — `propagate_mouse_event` sets
            // `propagate_mouse_activated` and returns before the selection
            // block. Emitting it after selecting, as the first pass did, made
            // every double-click both select and activate.
            if double_click && !editable {
                tree.emit_signal(node_id, "item_activated", &[]);
                return true;
            }

            let selectable = model
                .get_item(row.item)
                .map(|it| it.selectable)
                .unwrap_or(false);
            if selectable {
                let changed = model.select(row.item);
                store_tree(tree, node_id, &model);
                if changed {
                    tree.emit_signal(node_id, "item_selected", &[]);
                }
                // Godot pairs every selecting click with this, carrying the
                // position in the control's own coordinates.
                tree.emit_signal(
                    node_id,
                    "item_mouse_selected",
                    &[Variant::Vector2(local), Variant::Int(MOUSE_BUTTON_LEFT)],
                );
            }

            // ⚠️ Editing comes AFTER selection, and only for an editable cell —
            // `if (!c.editable) return -1;` sits between the two in tree.cpp. A
            // non-editable CHECK cell is therefore selected and NOT toggled.
            // ⚠️ Godot's `set_edit_checkbox_cell_only_when_checkbox_is_pressed`
            // (tree.cpp:3246, default FALSE) decides whether the whole cell is a
            // toggle target or only the checkbox glyph. With it off — the
            // default — clicking anywhere in a CHECK cell flips it, which is
            // surprising enough that it has to be the documented behaviour
            // rather than an accident.
            let on_checkbox = !checkbox_only
                || control::rect_has_point(
                    row.check_rect(column, &spans).position,
                    row.check_rect(column, &spans).size,
                    global_point,
                );
            if editable
                && on_checkbox
                && model.get_cell_mode(row.item, column) == TreeCellMode::Check
            {
                let flipped = !model.is_checked(row.item, column);
                model.set_checked(row.item, column, flipped);
                store_tree(tree, node_id, &model);
                tree.emit_signal(node_id, "item_edited", &[]);
            }
            true
        }
        ItemViewKind::Tabs | ItemViewKind::TabContainer => {
            let Some(idx) = tab_at_point(tree, node_id, global_point) else {
                return false;
            };
            tree.emit_signal(node_id, "tab_clicked", &[Variant::Int(idx as i64)]);
            let mut bar = load_tab_bar(tree, node_id);
            let changed = bar.set_current_tab(idx);
            store_tab_bar(tree, node_id, &bar);
            tree.emit_signal(node_id, "tab_selected", &[Variant::Int(idx as i64)]);
            if changed {
                tree.emit_signal(node_id, "tab_changed", &[Variant::Int(idx as i64)]);
            }
            true
        }
        // Clicking an OptionButton opens its dropdown, which needs the popup
        // layer — stage 2 of pat-0q2kx.85. The button still emits `pressed`
        // through the normal BaseButton path.
        ItemViewKind::Option => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn view(class: &str) -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("View", class);
        n.set_property("position", Variant::Vector2(Vector2::ZERO));
        n.set_property("size", Variant::Vector2(Vector2::new(120.0, 100.0)));
        let id = tree.add_child(root, n).unwrap();
        (tree, id)
    }

    fn call(tree: &mut SceneTree, id: NodeId, method: &str, args: &[Variant]) -> Option<Variant> {
        call_item_view_method(tree, id, method, args)
    }

    /// `call` for a method invoked for its effect. Asserts the bridge accepted
    /// it, so a typo in a method name fails loudly instead of silently doing
    /// nothing.
    fn run(tree: &mut SceneTree, id: NodeId, method: &str, args: &[Variant]) {
        assert!(
            call_item_view_method(tree, id, method, args).is_some(),
            "{method} must be handled by the item-view bridge"
        );
    }

    #[test]
    fn item_list_methods_round_trip_through_node_properties() {
        let (mut tree, id) = view("ItemList");
        assert_eq!(
            call(&mut tree, id, "add_item", &[Variant::String("a".into())]),
            Some(Variant::Int(0))
        );
        assert_eq!(
            call(&mut tree, id, "add_item", &[Variant::String("b".into())]),
            Some(Variant::Int(1))
        );
        assert_eq!(
            call(&mut tree, id, "get_item_count", &[]),
            Some(Variant::Int(2))
        );
        // The state lives on the node, not in a side table.
        assert!(
            matches!(prop(&tree, id, "items"), Variant::Array(a) if a.len() == 2),
            "items must persist as a node property"
        );
        run(&mut tree, id, "select", &[Variant::Int(1)]);
        assert_eq!(
            call(&mut tree, id, "get_selected_items", &[]),
            Some(Variant::Array(vec![Variant::Int(1)]))
        );
        assert_eq!(
            call(&mut tree, id, "get_item_text", &[Variant::Int(1)]),
            Some(Variant::String("b".into()))
        );
    }

    #[test]
    fn option_button_select_mirrors_text_and_id() {
        let (mut tree, id) = view("OptionButton");
        run(
            &mut tree,
            id,
            "add_item",
            &[Variant::String("Red".into()), Variant::Int(10)],
        );
        run(
            &mut tree,
            id,
            "add_item",
            &[Variant::String("Green".into()), Variant::Int(20)],
        );
        // Godot auto-selects the first item added.
        assert_eq!(
            call(&mut tree, id, "get_selected_id", &[]),
            Some(Variant::Int(10))
        );
        run(&mut tree, id, "select", &[Variant::Int(1)]);
        assert_eq!(
            call(&mut tree, id, "get_selected_id", &[]),
            Some(Variant::Int(20))
        );
        // `text` is what the button draws — Godot's `_select` sets it.
        assert_eq!(prop(&tree, id, "text"), Variant::String("Green".into()));
    }

    #[test]
    fn item_list_click_selects_the_row_under_the_point() {
        let (mut tree, id) = view("ItemList");
        run(&mut tree, id, "add_item", &[Variant::String("a".into())]);
        run(&mut tree, id, "add_item", &[Variant::String("b".into())]);

        // Row 1 spans y in [20, 40).
        assert!(item_view_click(
            &mut tree,
            id,
            Vector2::new(10.0, ITEM_ROW_HEIGHT + 5.0),
            false
        ));
        assert_eq!(
            call(&mut tree, id, "get_selected_items", &[]),
            Some(Variant::Array(vec![Variant::Int(1)]))
        );
        // Below the last row is empty space, not row 1 again.
        assert!(!item_view_click(
            &mut tree,
            id,
            Vector2::new(10.0, 90.0),
            false
        ));
    }

    #[test]
    fn disabled_row_click_does_not_select() {
        let (mut tree, id) = view("ItemList");
        run(&mut tree, id, "add_item", &[Variant::String("a".into())]);
        run(
            &mut tree,
            id,
            "set_item_disabled",
            &[Variant::Int(0), Variant::Bool(true)],
        );
        item_view_click(&mut tree, id, Vector2::new(10.0, 5.0), false);
        assert_eq!(
            call(&mut tree, id, "get_selected_items", &[]),
            Some(Variant::Array(Vec::new()))
        );
    }

    #[test]
    fn tab_bar_click_maps_x_to_the_tab_under_it() {
        let (mut tree, id) = view("TabBar");
        run(&mut tree, id, "add_tab", &[Variant::String("One".into())]);
        run(&mut tree, id, "add_tab", &[Variant::String("Two".into())]);
        let spans = tab_spans(&tree, id);
        let (x, w) = spans[1];
        let mid = Vector2::new(x + w * 0.5, 5.0);

        assert!(item_view_click(&mut tree, id, mid, false));
        assert_eq!(
            call(&mut tree, id, "get_current_tab", &[]),
            Some(Variant::Int(1))
        );
    }

    #[test]
    fn tab_container_titles_come_from_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("Tabs", "TabContainer");
        n.set_property("size", Variant::Vector2(Vector2::new(300.0, 200.0)));
        let tabs = tree.add_child(root, n).unwrap();
        tree.add_child(tabs, Node::new("First", "Panel")).unwrap();
        tree.add_child(tabs, Node::new("Second", "Panel")).unwrap();

        assert_eq!(
            tab_titles(&tree, tabs),
            vec!["First".to_string(), "Second".to_string()]
        );
        assert_eq!(
            call(&mut tree, tabs, "get_tab_count", &[]),
            Some(Variant::Int(2))
        );
        run(&mut tree, tabs, "set_current_tab", &[Variant::Int(1)]);
        assert_eq!(prop(&tree, tabs, "current_tab"), Variant::Int(1));
    }
}
