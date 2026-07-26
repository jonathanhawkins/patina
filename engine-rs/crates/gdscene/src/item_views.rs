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
use crate::item_controls::{ItemList, OptionButton, TabBar};
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
}

/// Classifies `node_id` into an [`ItemViewKind`], or `None` for anything else.
pub fn item_view_kind(tree: &SceneTree, node_id: NodeId) -> Option<ItemViewKind> {
    match tree.get_node(node_id)?.class_name() {
        "ItemList" => Some(ItemViewKind::List),
        "OptionButton" => Some(ItemViewKind::Option),
        "TabBar" => Some(ItemViewKind::Tabs),
        "TabContainer" => Some(ItemViewKind::TabContainer),
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
