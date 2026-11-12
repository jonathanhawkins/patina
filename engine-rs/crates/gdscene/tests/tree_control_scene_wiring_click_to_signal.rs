//! pat-s3rrv.75 acceptance: the `Tree` control is attached to scene nodes.
//!
//! `gdscene::item_controls::Tree` was a correct arena of items with **zero
//! non-test consumers**: `item_views.rs` imported only ItemList/OptionButton/
//! TabBar, so `item_view_kind` never classified a Tree and neither
//! `call_item_view_method` nor `item_view_click` could reach one. Meanwhile
//! `control.rs` already listed `Tree` in `CONTROL_HIT_CLASSES`, so a `.tscn`
//! Tree took focus and **swallowed every click while doing nothing with any of
//! them** — the worst of the two failure modes, because it looks like a dead
//! control rather than a missing one.
//!
//! These drive the runtime paths a game uses: a GDScript `_process` calling the
//! methods, and a real `InputEvent` through `SceneTree::push_input`.
//!
//! ⚠️ **The draw arm is not part of this increment.** `control_render.rs` is
//! leased to another agent, so nothing here asserts on the draw list — see
//! `tree_row_geometry_is_shared_with_the_draw_pass` for the contract the arm
//! will consume, which is deliberately the *same* row list the click resolves
//! through so the two cannot disagree.

use gdcore::math::{Rect2, Vector2};
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::item_controls::Tree as TreeControl;
use gdscene::item_views::{
    load_tree, store_tree, tree_column_spans, tree_drop_section_at, tree_item_area_rect,
    tree_item_at, tree_rows_of, tree_visible_rows, DROP_MODE_INBETWEEN, DROP_MODE_ON_ITEM,
    ITEM_ROW_HEIGHT, TREE_ARROW_WIDTH, TREE_DROP_SECTION_NONE, TREE_INDENT,
};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A Control node with an explicit rect. `position`/`size` are what the hit test
/// reads; the anchor offsets are what the render pass re-derives the rect from,
/// so setting both keeps the clicked rect and the drawn rect identical.
fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("position", Variant::Vector2(Vector2::new(x, y)));
    n.set_property("size", Variant::Vector2(Vector2::new(w, h)));
    n.set_property("offset_left", Variant::Float(x as f64));
    n.set_property("offset_top", Variant::Float(y as f64));
    n.set_property("offset_right", Variant::Float((x + w) as f64));
    n.set_property("offset_bottom", Variant::Float((y + h) as f64));
    n
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// `Main -> Tree`, whose script builds a root plus one child on frame 1 and
/// records what each signal delivers.
///
/// ⚠️ `Tree.item_selected` takes **no arguments** — Godot reports the selection
/// through `get_selected()`, unlike `ItemList.item_selected(index)`. The handler
/// is written accordingly, and that is itself part of what this asserts: a
/// handler with the wrong arity would not run.
fn tree_scene() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let view = tree
        .add_child(main, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let src = "\
extends Node2D
var frame = 0
var root_item = -1
var child_item = -1
var selected = -1
var collapsed_item = -1
var nothing = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        self.root_item = get_node(\"Tree\").create_item()
        self.child_item = get_node(\"Tree\").create_item()
func _on_selected():
    self.selected = get_node(\"Tree\").get_selected()
func _on_collapsed(item):
    self.collapsed_item = item
func _on_nothing():
    self.nothing = self.nothing + 1
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    for (signal, method) in [
        ("item_selected", "_on_selected"),
        ("item_collapsed", "_on_collapsed"),
        ("nothing_selected", "_on_nothing"),
    ] {
        tree.connect_signal(
            view,
            signal,
            gdobject::signal::Connection::new(main.object_id(), method),
        );
    }
    (tree, main, view)
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** A script creates two items; a real click on the
/// second row selects it, delivers `item_selected` to the connected script, and
/// leaves `get_selected()` pointing at that item.
#[test]
fn tree_control_scene_wiring_click_to_signal() {
    let (tree, main, _view) = tree_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // Godot's `create_item(null)` creates the ROOT the first time and parents to
    // it afterwards, so these are item 0 and its child item 1.
    assert_eq!(
        script_var(ml.tree(), main, "root_item"),
        Some(Variant::Int(0)),
        "$Tree.create_item() must reach the Tree model — a missing method here \
         is the `UndefinedFunction` the binding exists to remove"
    );
    assert_eq!(
        script_var(ml.tree(), main, "child_item"),
        Some(Variant::Int(1)),
        "the second create_item() parents under the root, as Godot does"
    );

    // Row 1 spans y in [20, 40). x = 40 is past the root's fold arrow gutter and
    // well inside the row.
    ml.tree_mut()
        .push_input(&press(40.0, ITEM_ROW_HEIGHT + 5.0));

    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(1)),
        "clicking the second row must emit item_selected and leave \
         get_selected() on that item"
    );
}

/// Clicking a parent's fold arrow collapses it: `item_collapsed` fires, and the
/// child disappears from the row list the hit test resolves through.
#[test]
fn clicking_the_fold_arrow_collapses_and_hides_children() {
    let (tree, main, view) = tree_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        tree_visible_rows(ml.tree(), view).len(),
        2,
        "root and child are both visible before folding"
    );

    // The root is at depth 0, so its arrow occupies x in [0, 14) of row 0.
    ml.tree_mut().push_input(&press(4.0, 4.0));

    assert_eq!(
        script_var(ml.tree(), main, "collapsed_item"),
        Some(Variant::Int(0)),
        "folding the root must emit item_collapsed for that item"
    );
    // ⚠️ The collapsed row itself REMAINS — it is its descendants that vanish.
    // Dropping the parent row too would make the arrow unclickable and the tree
    // could never be expanded again.
    let rows = tree_visible_rows(ml.tree(), view);
    assert_eq!(rows.len(), 1, "the child is hidden, the root is not");
    assert_eq!(rows[0].item, 0);

    // And the hit test agrees: the child's old row now selects nothing.
    ml.tree_mut()
        .push_input(&press(40.0, ITEM_ROW_HEIGHT + 5.0));
    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(-1)),
        "a click where the child used to be must not select it while folded"
    );
}

/// ⚠️ Folding must not move the selection. Godot's disclosure triangle is a
/// separate hit target, and a click that both folded and selected would make it
/// impossible to collapse a subtree without changing what is selected.
#[test]
fn folding_does_not_select_the_row() {
    let (tree, main, _view) = tree_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    ml.tree_mut().push_input(&press(4.0, 4.0));

    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(-1)),
        "the arrow click folded but must not have selected"
    );
}

/// A click on the control but below every row reports `nothing_selected`.
#[test]
fn clicking_empty_space_reports_nothing_selected() {
    let (tree, main, _view) = tree_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // Two rows occupy y in [0, 40); the control is 100 tall.
    ml.tree_mut().push_input(&press(40.0, 80.0));

    assert_eq!(
        script_var(ml.tree(), main, "nothing"),
        Some(Variant::Int(1)),
        "an empty click inside the control must emit nothing_selected"
    );
    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(-1)),
        "and must not select anything"
    );
}

// ---------------------------------------------------------------------------
// The contract the draw arm consumes
// ---------------------------------------------------------------------------

/// The row geometry is derived ONCE and shared.
///
/// The acceptance criterion is that the drawn row and the hit row are the same
/// rect. This asserts the shared source rather than the drawing: `control_render`
/// lays rows out from `tree_visible_rows`, which is the list `item_view_click`
/// resolves a click through, so a drift between them is not expressible.
#[test]
fn tree_row_geometry_is_shared_with_the_draw_pass() {
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let child = model.create_item(Some(root));
    let grandchild = model.create_item(Some(child));
    let rect = Rect2::new(Vector2::new(5.0, 7.0), Vector2::new(120.0, 100.0));

    let rows = tree_rows_of(&model, rect, false);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter().map(|r| r.item).collect::<Vec<_>>(),
        vec![root, child, grandchild],
        "depth-first pre-order: a child follows its parent, not the next sibling"
    );
    assert_eq!(
        rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );

    // Rows stack from the control's own origin, one ITEM_ROW_HEIGHT apart.
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.rect.position.x, rect.position.x);
        assert_eq!(
            row.rect.position.y,
            rect.position.y + i as f32 * ITEM_ROW_HEIGHT
        );
        assert_eq!(row.rect.size.x, rect.size.x);
    }

    // Only a row with children has an arrow, and text starts past the gutter so
    // it never overlaps a foldable sibling's triangle.
    assert!(rows[0].arrow_rect().is_some(), "the root has children");
    assert!(rows[2].arrow_rect().is_none(), "the leaf has none");
    assert_eq!(
        rows[1].text_x(),
        rect.position.x + TREE_INDENT + TREE_ARROW_WIDTH,
        "a depth-1 row indents once and clears the arrow gutter"
    );
}

/// ⚠️ `hide_root` suppresses the root ROW while keeping its children visible and
/// un-indented. A tree authored with `hide_root = true` whose root still
/// occupied row 0 would offset every hit test by exactly one row.
#[test]
fn hiding_the_root_shifts_the_rows_up_and_removes_an_indent_level() {
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let child = model.create_item(Some(root));
    let rect = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(120.0, 100.0));

    let rows = tree_rows_of(&model, rect, true);
    assert_eq!(rows.len(), 1, "the root row is gone");
    assert_eq!(rows[0].item, child);
    assert_eq!(rows[0].rect.position.y, 0.0, "the child is now row 0");
    assert_eq!(
        rows[0].depth, 0,
        "and is not indented under a hidden parent"
    );
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// A Tree round-trips through the node's `items` property.
///
/// The click path stores after every change, so a write with no matching read
/// would mean an authored tree is replaced by an empty default the first time a
/// user clicks it. Round-trip identity is what makes storing safe.
#[test]
fn the_arena_round_trips_through_the_node_properties() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(2);
    let root = model.create_item(None);
    let child = model.create_item(Some(root));
    model.set_text(root, 0, "parent");
    model.set_text(root, 1, "second column");
    model.set_text(child, 0, "leaf");
    model.set_collapsed(root, true);
    model.set_selectable(child, false);
    model.select(root);

    store_tree(&mut tree, view, &model);
    let back = load_tree(&tree, view);

    assert_eq!(back.column_count(), 2, "the column count survives");
    assert_eq!(back.item_count(), 2);
    assert_eq!(back.get_text(root, 0), "parent");
    assert_eq!(
        back.get_text(root, 1),
        "second column",
        "⚠️ every column, not just the first — a single-`text` codec would drop \
         this silently"
    );
    assert_eq!(back.get_text(child, 0), "leaf");
    assert_eq!(
        back.get_item(child).map(|it| it.parent),
        Some(Some(root)),
        "the parent link is what makes it a tree rather than a list"
    );
    assert!(back.is_collapsed(root), "fold state survives");
    assert!(!back.get_item(child).unwrap().selectable);
    assert_eq!(back.get_selected(), Some(root), "and the selection");
}

// ---------------------------------------------------------------------------
// CHECK cells — ported from upstream/godot/scene/gui/tree.cpp:3175-3253
// ---------------------------------------------------------------------------

/// A scene whose Tree has one item with a CHECK cell in column 0.
///
/// `editable` is set explicitly because Godot's default is **false**, and it is
/// the gate the whole toggle path hangs off.
fn check_scene(editable: bool) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let view = tree
        .add_child(main, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    let item = model.create_item(None);
    model.set_cell_mode(item, 0, gdscene::item_controls::TreeCellMode::Check);
    model.set_editable(item, 0, editable);
    store_tree(&mut tree, view, &model);

    let src = "\
extends Node2D
var edited = 0
var selected = -1
var activated = 0
func _on_edited():
    self.edited = self.edited + 1
func _on_selected():
    self.selected = get_node(\"Tree\").get_selected()
func _on_activated():
    self.activated = self.activated + 1
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    for (signal, method) in [
        ("item_edited", "_on_edited"),
        ("item_selected", "_on_selected"),
        ("item_activated", "_on_activated"),
    ] {
        tree.connect_signal(
            view,
            signal,
            gdobject::signal::Connection::new(main.object_id(), method),
        );
    }
    (tree, main, view)
}

fn is_checked(tree: &SceneTree, view: NodeId) -> bool {
    load_tree(tree, view).is_checked(0, 0)
}

/// **The ordering `tree.cpp` corrected.** A click on an editable CHECK cell
/// SELECTS first and toggles second — `select_single_item` runs above the
/// cell-mode switch (tree.cpp:3196), with `if (!c.editable) return -1;` between
/// them. Toggling *instead of* selecting would be the intuitive guess and is
/// wrong.
#[test]
fn clicking_an_editable_check_cell_selects_and_toggles() {
    let (tree, main, view) = check_scene(true);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert!(!is_checked(ml.tree(), view), "starts unchecked");

    ml.tree_mut().push_input(&press(40.0, 5.0));

    assert!(is_checked(ml.tree(), view), "the cell must have toggled");
    assert_eq!(
        script_var(ml.tree(), main, "edited"),
        Some(Variant::Int(1)),
        "and reported item_edited"
    );
    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(0)),
        "⚠️ the row is ALSO selected — selection happens before editing"
    );

    // A second click flips it back: the toggle reads the current state rather
    // than latching on.
    ml.tree_mut().push_input(&press(40.0, 5.0));
    assert!(!is_checked(ml.tree(), view), "clicking again unchecks");
}

/// ⚠️ Godot's default for a cell is `editable = false`, and
/// `propagate_mouse_event` returns before the CHECK branch on a non-editable
/// cell. So the row selects and the box does NOT move — a cell that toggled
/// anyway would make every read-only checkbox writable.
#[test]
fn a_non_editable_check_cell_selects_but_does_not_toggle() {
    let (tree, main, view) = check_scene(false);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    ml.tree_mut().push_input(&press(40.0, 5.0));

    assert!(
        !is_checked(ml.tree(), view),
        "a non-editable cell must not toggle"
    );
    assert_eq!(
        script_var(ml.tree(), main, "edited"),
        Some(Variant::Int(0)),
        "and must not report item_edited"
    );
    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(0)),
        "but it is still selected — only the EDIT half is gated"
    );
}

/// `set_edit_checkbox_cell_only_when_checkbox_is_pressed(true)` narrows the
/// toggle target from the whole cell to the checkbox glyph (tree.cpp:3246).
/// Default is false, which is why the test above toggles from x = 40.
#[test]
fn the_checkbox_only_flag_narrows_the_toggle_target() {
    let (tree, _main, view) = check_scene(true);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    ml.tree_mut().get_node_mut(view).unwrap().set_property(
        "edit_checkbox_cell_only_when_checkbox_is_pressed",
        Variant::Bool(true),
    );

    // x = 60 is deep inside the cell but far outside the checkbox glyph.
    ml.tree_mut().push_input(&press(60.0, 5.0));
    assert!(
        !is_checked(ml.tree(), view),
        "with the flag on, a click away from the box must not toggle"
    );

    // The glyph starts at the cell's content x — past the indent/arrow gutter.
    let rows = tree_visible_rows(ml.tree(), view);
    let spans = tree_column_spans(
        &load_tree(ml.tree(), view),
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(120.0, 100.0)),
    );
    let box_rect = rows[0].check_rect(0, &spans);
    ml.tree_mut()
        .push_input(&press(box_rect.position.x + 2.0, box_rect.position.y + 2.0));
    assert!(
        is_checked(ml.tree(), view),
        "a click inside the checkbox must still toggle"
    );
}

/// ⚠️ A double-click ACTIVATES rather than selecting, and only on a
/// **non-editable** cell — `propagate_mouse_event` sets
/// `propagate_mouse_activated` and returns before the selection block
/// (tree.cpp:3180). An editable cell double-clicked opens its editor instead,
/// so it must not activate.
#[test]
fn double_click_activates_only_a_non_editable_cell() {
    for (editable, expected) in [(false, 1), (true, 0)] {
        let (tree, main, view) = check_scene(editable);
        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        gdscene::item_views::item_view_click(ml.tree_mut(), view, Vector2::new(40.0, 5.0), true);

        assert_eq!(
            script_var(ml.tree(), main, "activated"),
            Some(Variant::Int(expected)),
            "editable = {editable} must {} item_activated",
            if expected == 1 { "emit" } else { "not emit" }
        );
    }
}

/// Godot emits BOTH signals for a click on the control but on no row:
/// `empty_clicked` from propagation reaching the root unconsumed
/// (tree.cpp:3380) and `nothing_selected` from the unhandled click
/// (tree.cpp:4226). A handler connected to either must fire.
#[test]
fn an_empty_click_reports_both_empty_clicked_and_nothing_selected() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let view = tree
        .add_child(main, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let src = "\
extends Node2D
var empty = 0
var nothing = 0
var pos_x = -1.0
func _on_empty(at, button):
    self.empty = self.empty + 1
    self.pos_x = at.x
func _on_nothing():
    self.nothing = self.nothing + 1
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    for (signal, method) in [
        ("empty_clicked", "_on_empty"),
        ("nothing_selected", "_on_nothing"),
    ] {
        tree.connect_signal(
            view,
            signal,
            gdobject::signal::Connection::new(main.object_id(), method),
        );
    }

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    ml.tree_mut().push_input(&press(40.0, 60.0));

    assert_eq!(script_var(ml.tree(), main, "empty"), Some(Variant::Int(1)));
    assert_eq!(
        script_var(ml.tree(), main, "nothing"),
        Some(Variant::Int(1))
    );
    // ⚠️ The position is the control's OWN coordinates, not the viewport's —
    // Godot passes `get_local_mouse_position()`.
    assert_eq!(
        script_var(ml.tree(), main, "pos_x"),
        Some(Variant::Float(40.0)),
        "empty_clicked carries the position local to the control"
    );
}

/// The per-cell state survives the `.tscn` round trip. Without this the click
/// path's store would write a checkbox that the next load silently reset.
#[test]
fn cell_mode_checked_and_editable_round_trip() {
    use gdscene::item_controls::TreeCellMode;

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(2);
    let item = model.create_item(None);
    model.set_cell_mode(item, 1, TreeCellMode::Check);
    model.set_checked(item, 1, true);
    model.set_editable(item, 1, true);

    store_tree(&mut tree, view, &model);
    let back = load_tree(&tree, view);

    assert_eq!(back.get_cell_mode(item, 1), TreeCellMode::Check);
    assert!(back.is_checked(item, 1));
    assert!(back.is_editable(item, 1));
    // Column 0 keeps Godot's defaults, so the arrays are positional rather than
    // "whatever the last written column was".
    assert_eq!(back.get_cell_mode(item, 0), TreeCellMode::Text);
    assert!(!back.is_checked(item, 0));
    assert!(
        !back.is_editable(item, 0),
        "Godot's default is NOT editable"
    );
}

// ---------------------------------------------------------------------------
// propagate_check — ported from upstream/godot/scene/gui/tree.cpp:282-386
// ---------------------------------------------------------------------------

/// `root ─┬─ a ─┬─ a1`, plus a sibling `b`, all in one column.
/// `        │    └─ a2`
/// `        └─ b`
fn propagate_model() -> (TreeControl, [usize; 5]) {
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let a = model.create_item(Some(root));
    let a1 = model.create_item(Some(a));
    let a2 = model.create_item(Some(a));
    let b = model.create_item(Some(root));
    for item in [root, a, a1, a2, b] {
        model.set_cell_mode(item, 0, gdscene::item_controls::TreeCellMode::Check);
    }
    (model, [root, a, a1, a2, b])
}

/// ⚠️ **The two setters are coupled** (`tree.cpp:361-386`): a real change to
/// `checked` clears `indeterminate` and vice versa, but setting the value a cell
/// already has changes NOTHING — upstream's comment calls that out as
/// deliberate ("Prevent uncheck if indeterminate set to false twice"). Two
/// independent booleans, which is the obvious model, gets `propagate_check`
/// wrong in both directions.
#[test]
fn checked_and_indeterminate_are_mutually_exclusive_but_idempotent() {
    let (mut model, [root, ..]) = propagate_model();

    model.set_checked(root, 0, true);
    assert!(model.is_checked(root, 0));

    model.set_indeterminate(root, 0, true);
    assert!(model.is_indeterminate(root, 0));
    assert!(
        !model.is_checked(root, 0),
        "going indeterminate clears checked"
    );

    model.set_checked(root, 0, true);
    assert!(model.is_checked(root, 0));
    assert!(
        !model.is_indeterminate(root, 0),
        "and a real check clears indeterminate"
    );

    // The idempotent path. From a definitively checked cell whose indeterminate
    // flag is already clear...
    assert!(model.is_checked(root, 0) && !model.is_indeterminate(root, 0));
    // ...clearing that already-clear flag must change nothing. Without the
    // early-out it would fall through to `checked = false` and silently uncheck
    // the cell — which is precisely what upstream's comment warns about.
    model.set_indeterminate(root, 0, false);
    assert!(
        model.is_checked(root, 0),
        "clearing an already-clear indeterminate must not uncheck the cell"
    );
}

/// Checking a middle node pushes DOWN to its whole subtree and recomputes
/// UPWARD, leaving the root in the mixed state because its other child is not
/// checked.
#[test]
fn propagate_check_pushes_down_and_recomputes_up() {
    let (mut model, [root, a, a1, a2, b]) = propagate_model();

    model.set_checked(a, 0, true);
    let touched = model.propagate_check(a, 0);

    assert!(model.is_checked(a1, 0), "the subtree takes a's state");
    assert!(model.is_checked(a2, 0));
    assert!(!model.is_checked(b, 0), "an unrelated sibling is untouched");
    // root has one checked child and one unchecked one → mixed.
    assert!(model.is_indeterminate(root, 0));
    assert!(
        !model.is_checked(root, 0),
        "and going indeterminate cleared its checked flag"
    );

    // Godot's emission order: self, then descendants depth-first, then
    // ancestors upward.
    assert_eq!(touched, vec![a, a1, a2, root]);
}

/// Once every child agrees, the parent stops being mixed and becomes checked —
/// which only works because `set_checked` clears `indeterminate`.
#[test]
fn propagate_check_resolves_a_mixed_parent_when_children_agree() {
    let (mut model, [root, a, _a1, _a2, b]) = propagate_model();

    model.set_checked(a, 0, true);
    model.propagate_check(a, 0);
    assert!(model.is_indeterminate(root, 0), "precondition: mixed");

    model.set_checked(b, 0, true);
    let touched = model.propagate_check(b, 0);

    assert!(model.is_checked(root, 0), "all children checked → checked");
    assert!(!model.is_indeterminate(root, 0), "and no longer mixed");
    assert_eq!(touched, vec![b, root]);
}

/// ⚠️ An indeterminate child makes its parent indeterminate too — the
/// mixed state cascades all the way up, which is the whole point of the third
/// state. This also exercises upstream's early `break`, which fires only for an
/// UNCHECKED indeterminate child.
#[test]
fn indeterminate_cascades_to_ancestors() {
    let (mut model, [root, a, a1, _a2, b]) = propagate_model();

    // Everything under root checked first.
    model.set_checked(a, 0, true);
    model.propagate_check(a, 0);
    model.set_checked(b, 0, true);
    model.propagate_check(b, 0);
    assert!(model.is_checked(root, 0), "precondition: all checked");

    // Now uncheck ONE leaf.
    model.set_checked(a1, 0, false);
    let touched = model.propagate_check(a1, 0);

    assert!(
        model.is_indeterminate(a, 0),
        "a has one checked and one unchecked child"
    );
    assert!(
        model.is_indeterminate(root, 0),
        "and root inherits the mixed state from a"
    );
    assert!(!model.is_checked(root, 0));
    assert_eq!(touched, vec![a1, a, root]);
}

/// The script surface: `propagate_check` emits `check_propagated_to_item` once
/// per touched item. The signal was registered in ClassDB and emitted by
/// nothing before this.
#[test]
fn propagate_check_emits_once_per_touched_item() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let view = tree
        .add_child(main, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let (mut model, [_root, a, ..]) = propagate_model();
    model.set_checked(a, 0, true);
    store_tree(&mut tree, view, &model);

    let src = "\
extends Node2D
var hits = 0
var last = -1
func _on_propagated(item, column):
    self.hits = self.hits + 1
    self.last = item
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    tree.connect_signal(
        view,
        "check_propagated_to_item",
        gdobject::signal::Connection::new(main.object_id(), "_on_propagated"),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    gdscene::item_views::call_item_view_method(
        ml.tree_mut(),
        view,
        "propagate_check",
        &[Variant::Int(a as i64), Variant::Int(0)],
    )
    .expect("propagate_check must be a Tree method");

    assert_eq!(
        script_var(ml.tree(), main, "hits"),
        Some(Variant::Int(4)),
        "a, its two children, and the root it made indeterminate"
    );
    assert_eq!(
        script_var(ml.tree(), main, "last"),
        Some(Variant::Int(0)),
        "the root is emitted last — ancestors come after descendants"
    );
    // And the state reached the node, not just the signal.
    let back = load_tree(ml.tree(), view);
    assert!(back.is_indeterminate(0, 0), "the tri-state persisted");
}

// ---------------------------------------------------------------------------
// TreeItem.visible and the drop-section API
// ---------------------------------------------------------------------------

/// ⚠️ `visible` is NOT `collapsed`. A collapsed item keeps its own row and hides
/// its descendants; an invisible one hides itself *and* its descendants, which
/// is what `_find_item_at_pos`'s "Do not try children" bail does for both.
#[test]
fn hiding_an_item_removes_it_and_its_subtree_from_the_rows() {
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let a = model.create_item(Some(root));
    let _a1 = model.create_item(Some(a));
    let b = model.create_item(Some(root));
    let rect = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(120.0, 100.0));

    assert_eq!(tree_rows_of(&model, rect, false).len(), 4);

    model.set_item_visible(a, false);
    let rows: Vec<usize> = tree_rows_of(&model, rect, false)
        .iter()
        .map(|r| r.item)
        .collect();
    assert_eq!(
        rows,
        vec![root, b],
        "a AND its child a1 are gone; a collapsed `a` would have left its own row"
    );

    // The rows below close the gap rather than leaving a hole.
    let laid = tree_rows_of(&model, rect, false);
    assert_eq!(laid[1].rect.position.y, ITEM_ROW_HEIGHT);
}

/// ⚠️ Hiding an item DESELECTS it (`TreeItem::set_visible`, tree.cpp:392).
/// Otherwise `get_selected()` keeps naming a row that is not on screen, and the
/// next click compares against a phantom.
#[test]
fn hiding_the_selected_item_clears_the_selection() {
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let other = model.create_item(Some(root));
    model.select(other);
    assert_eq!(model.get_selected(), Some(other));

    model.set_item_visible(other, false);
    assert_eq!(
        model.get_selected(),
        None,
        "a hidden item cannot stay selected"
    );

    // Hiding an item that is NOT selected leaves the selection alone.
    model.select(root);
    let third = model.create_item(Some(root));
    model.set_item_visible(third, false);
    assert_eq!(model.get_selected(), Some(root));
}

/// The default drop mode splits a row into quarters: the middle half is "onto
/// this item", the outer quarters are "between".
#[test]
fn the_default_drop_mode_splits_a_row_into_quarters() {
    let (tree, _main, view) = tree_scene_for_drop();
    // Row 0 spans y in [0, 20).
    for (y, expected) in [
        (1.0, -1),
        (4.0, -1),
        (6.0, 0),
        (10.0, 0),
        (16.0, 1),
        (19.0, 1),
    ] {
        assert_eq!(
            tree_drop_section_at(&tree, view, Vector2::new(40.0, y)),
            expected,
            "y = {y} within the first row"
        );
    }
}

/// `DROP_MODE_ON_ITEM` answers "onto" everywhere in the row — there is no
/// between-band at all.
#[test]
fn drop_mode_on_item_never_reports_a_between_section() {
    let (mut tree, _main, view) = tree_scene_for_drop();
    tree.get_node_mut(view)
        .unwrap()
        .set_property("drop_mode_flags", Variant::Int(DROP_MODE_ON_ITEM));

    for y in [1.0, 10.0, 19.0] {
        assert_eq!(
            tree_drop_section_at(&tree, view, Vector2::new(40.0, y)),
            0,
            "ON_ITEM must answer 0 at y = {y}"
        );
    }
}

/// `DROP_MODE_INBETWEEN` splits at the MIDPOINT and never answers "onto".
#[test]
fn drop_mode_inbetween_splits_at_the_midpoint() {
    let (mut tree, _main, view) = tree_scene_for_drop();
    tree.get_node_mut(view)
        .unwrap()
        .set_property("drop_mode_flags", Variant::Int(DROP_MODE_INBETWEEN));

    // The quarters that answer 0 by default must now answer -1 / 1.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 4.0)),
        -1
    );
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 9.0)),
        -1
    );
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 11.0)),
        1
    );
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 16.0)),
        1
    );
}

/// ⚠️ A miss answers `-100`, not `-1`. `-1` already means "above this row", so a
/// caller that treats a miss as `-1` inserts at the top of the tree every time
/// the pointer leaves the control.
#[test]
fn a_position_on_no_row_reports_the_none_sentinel() {
    let (tree, _main, view) = tree_scene_for_drop();

    // Inside the control, below every row.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 80.0)),
        TREE_DROP_SECTION_NONE
    );
    // Outside the control entirely.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(40.0, 500.0)),
        TREE_DROP_SECTION_NONE
    );
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(-5.0, 5.0)),
        TREE_DROP_SECTION_NONE
    );
}

/// ⚠️ The position is in the CONTROL's own coordinates, matching Godot's
/// `get_drop_section_at_position(get_local_mouse_position())`. A Tree away from
/// the origin proves the conversion is applied — with a viewport-global point
/// the same y would read a different row.
#[test]
fn the_drop_position_is_control_local_not_viewport_global() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 50.0, 30.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    model.create_item(None);
    model.create_item(None);
    store_tree(&mut tree, view, &model);

    // Local y = 10 is the middle of row 0 wherever the control sits.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(10.0, 10.0)),
        0
    );
    // Local y = 25 is the middle of row 1.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(10.0, 25.0)),
        0
    );
    // ⚠️ 40 is what a caller would pass if it wrongly handed over the
    // viewport-global y of row 0 (the control top, 30, plus 10). Locally that is
    // past both rows, so the correct answer is a MISS; if the offset were
    // applied twice it would read as row 0 and answer 0.
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(10.0, 40.0)),
        TREE_DROP_SECTION_NONE,
        "a viewport-global y must not resolve to a row"
    );
}

/// `visible` survives the `.tscn` round trip, so a hidden row stays hidden.
#[test]
fn item_visibility_round_trips() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let hidden = model.create_item(Some(root));
    model.set_item_visible(hidden, false);

    store_tree(&mut tree, view, &model);
    let back = load_tree(&tree, view);

    assert!(back.is_item_visible(root));
    assert!(!back.is_item_visible(hidden));
    assert_eq!(
        tree_visible_rows(&tree, view).len(),
        1,
        "and the hidden row is absent from the laid-out rows"
    );
}

/// A two-row Tree at the origin, for the drop-section cases.
fn tree_scene_for_drop() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .unwrap();
    let view = tree
        .add_child(main, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    model.create_item(None);
    model.create_item(None);
    store_tree(&mut tree, view, &model);
    (tree, main, view)
}

// ---------------------------------------------------------------------------
// Column layout — ported from upstream/godot/scene/gui/tree.cpp:5662
// ---------------------------------------------------------------------------

/// With every default in place (min 0, expanding, ratio 1) the width divides
/// evenly — so this change cannot move any tree that never configures columns.
#[test]
fn default_columns_divide_the_width_evenly() {
    let model = TreeControl::new(4);
    assert_eq!(model.column_width(0, 120), 30);
    assert_eq!(model.column_width(3, 120), 30);

    let spans = tree_column_spans(
        &model,
        Rect2::new(Vector2::new(10.0, 0.0), Vector2::new(120.0, 40.0)),
    );
    assert_eq!(
        spans,
        vec![(10.0, 30.0), (40.0, 30.0), (70.0, 30.0), (100.0, 30.0)],
        "spans start at the control's own x and abut"
    );
}

/// `expand_ratio` splits the leftover space proportionally — the column with
/// ratio 3 gets three times the share of the one with ratio 1.
#[test]
fn expand_ratio_splits_the_leftover_space() {
    let mut model = TreeControl::new(2);
    model.column_mut(0).unwrap().expand_ratio = 3;

    // expand_area = 120, expanding_total = 4 → 120*3/4 and 120*1/4.
    assert_eq!(model.column_width(0, 120), 90);
    assert_eq!(model.column_width(1, 120), 30);
}

/// A non-expanding column stops at its minimum, and the space it declines goes
/// to the ones that do expand.
#[test]
fn a_non_expanding_column_keeps_only_its_minimum() {
    let mut model = TreeControl::new(2);
    {
        let c = model.column_mut(0).unwrap();
        c.expand = false;
        c.custom_min_width = 40;
    }

    assert_eq!(model.column_width(0, 120), 40, "fixed at its minimum");
    // expand_area = 120 - 40 - 0 = 80, expanding_total = 1 → 0 + 80.
    assert_eq!(model.column_width(1, 120), 80);

    let spans = tree_column_spans(
        &model,
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(120.0, 40.0)),
    );
    assert_eq!(spans, vec![(0.0, 40.0), (40.0, 80.0)]);
}

/// A minimum is a floor the expansion is added to, not a cap.
#[test]
fn a_custom_minimum_is_a_floor_the_expansion_adds_to() {
    let mut model = TreeControl::new(2);
    model.column_mut(0).unwrap().custom_min_width = 50;

    // expand_area = 120 - 50 = 70, expanding_total = 2 → each adds 70*1/2 = 35.
    assert_eq!(model.column_width(0, 120), 85, "50 floor + 35 share");
    assert_eq!(model.column_width(1, 120), 35, "0 floor + 35 share");
}

/// ⚠️ **Godot's guard compares PIXELS against a sum of RATIOS.** It reads like a
/// bug and is the shipped behaviour: when the leftover space is smaller than the
/// ratio total, NO column expands and every one sits at its minimum. Reproduced
/// rather than "fixed", because a tidied version diverges from Godot on exactly
/// the cramped layouts where it fires.
#[test]
fn no_column_expands_when_the_leftover_is_smaller_than_the_ratio_total() {
    let mut model = TreeControl::new(2);
    model.column_mut(0).unwrap().expand_ratio = 100;
    model.column_mut(1).unwrap().expand_ratio = 100;
    // expanding_total = 200, but only 20px of content to share.
    assert_eq!(
        model.column_width(0, 20),
        0,
        "the guard blocks all expansion"
    );
    assert_eq!(model.column_width(1, 20), 0);

    // One pixel more than the total and the distribution runs again.
    assert_eq!(model.column_width(0, 200), 100);
}

/// ⚠️ The arithmetic is integer and truncating, so the columns need not fill the
/// control — the remainder is empty space on the right, as in Godot.
#[test]
fn truncation_can_leave_a_gap_at_the_right_edge() {
    let model = TreeControl::new(3);
    // 100 / 3 = 33 each, so 99 of 100 pixels are covered.
    assert_eq!(model.column_width(0, 100), 33);

    let spans = tree_column_spans(
        &model,
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(100.0, 40.0)),
    );
    let covered: f32 = spans.iter().map(|(_, w)| *w).sum();
    assert_eq!(covered, 99.0, "one pixel of remainder, not redistributed");
}

/// The column layout must survive the round trip. `store_tree` runs on every
/// click, so a layout the store dropped would be erased the first time a user
/// clicked a row.
#[test]
fn the_column_layout_round_trips() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(2);
    {
        let c = model.column_mut(0).unwrap();
        c.title = "Name".to_string();
        c.custom_min_width = 40;
        c.expand = false;
    }
    model.column_mut(1).unwrap().expand_ratio = 5;

    store_tree(&mut tree, view, &model);
    let back = load_tree(&tree, view);

    assert_eq!(back.column(0).unwrap().title, "Name");
    assert_eq!(back.column(0).unwrap().custom_min_width, 40);
    assert!(!back.column(0).unwrap().expand);
    assert_eq!(back.column(1).unwrap().expand_ratio, 5);
    // And the widths it implies are the same on both sides.
    assert_eq!(back.column_width(0, 120), model.column_width(0, 120));
    assert_eq!(back.column_width(1, 120), model.column_width(1, 120));
}

// ---------------------------------------------------------------------------
// Position queries — tree.cpp:6413 / :6479, sharing _find_item_at_pos
// ---------------------------------------------------------------------------

/// A Tree whose single column stops at 40px, leaving 80px of the control
/// uncovered — the "remainder" Godot's column loop falls through on.
fn tree_with_a_short_column() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    model.create_item(None);
    model.create_item(None);
    {
        let c = model.column_mut(0).unwrap();
        c.expand = false;
        c.custom_min_width = 40;
    }
    store_tree(&mut tree, view, &model);
    (tree, view)
}

/// A point over a row resolves to that row and its column.
#[test]
fn a_point_over_a_row_resolves_to_the_item_and_column() {
    let (tree, view) = tree_with_a_short_column();

    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(10.0, 5.0)),
        Some((0, 0))
    );
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(10.0, 25.0)),
        Some((1, 0)),
        "row 1 spans y in [20, 40)"
    );
}

/// ⚠️ **The column gate.** A point past the LAST column is a miss even though it
/// sits inside the control and level with a real row — Godot's column loop falls
/// through and returns nullptr. With truncating column widths that remainder
/// exists on most trees, so treating "inside the control" as "on an item" targets
/// a row the user never pointed at.
#[test]
fn a_point_past_the_last_column_is_a_miss() {
    let (tree, view) = tree_with_a_short_column();

    // x = 39 is the last pixel of the 40-wide column.
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(39.0, 5.0)),
        Some((0, 0))
    );
    // x = 40 onward is inside the control, level with row 0, and covered by no
    // column at all.
    assert_eq!(tree_item_at(&tree, view, Vector2::new(40.0, 5.0)), None);
    assert_eq!(tree_item_at(&tree, view, Vector2::new(100.0, 5.0)), None);
}

/// The same gate governs the drop section — `get_drop_section_at_position`
/// answers only for a non-null item. This is a REGRESSION test: the first
/// version of `tree_drop_section_at` checked the row rect (full width) and
/// reported a real section for a drop in the remainder.
#[test]
fn a_drop_past_the_last_column_reports_no_section() {
    let (tree, view) = tree_with_a_short_column();

    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(10.0, 10.0)),
        0
    );
    assert_eq!(
        tree_drop_section_at(&tree, view, Vector2::new(60.0, 10.0)),
        TREE_DROP_SECTION_NONE,
        "the right-hand remainder is not a drop target"
    );
}

/// Multi-column resolution: the x picks the column out of the spans.
#[test]
fn the_column_comes_from_the_spans() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();
    let mut model = TreeControl::new(2);
    model.create_item(None);
    store_tree(&mut tree, view, &model);

    // Two default columns over 120px: 60 each.
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(10.0, 5.0)),
        Some((0, 0))
    );
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(59.0, 5.0)),
        Some((0, 0))
    );
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(60.0, 5.0)),
        Some((0, 1))
    );
    assert_eq!(
        tree_item_at(&tree, view, Vector2::new(119.0, 5.0)),
        Some((0, 1))
    );
}

/// The script surface. ⚠️ The two miss values differ and are Godot's own:
/// `get_item_at_position` answers `null` (Nil here), `get_column_at_position`
/// answers `-1`, and `get_drop_section_at_position` answers `-100`.
#[test]
fn the_position_queries_are_reachable_from_a_script() {
    let (mut tree, view) = tree_with_a_short_column();

    let hit = gdscene::item_views::call_item_view_method(
        &mut tree,
        view,
        "get_item_at_position",
        &[Variant::Vector2(Vector2::new(10.0, 25.0))],
    );
    assert_eq!(hit, Some(Variant::Int(1)), "row 1 by its arena index");

    let column = gdscene::item_views::call_item_view_method(
        &mut tree,
        view,
        "get_column_at_position",
        &[Variant::Vector2(Vector2::new(10.0, 25.0))],
    );
    assert_eq!(column, Some(Variant::Int(0)));

    // A miss in the remainder, through each of the three queries.
    let miss = Vector2::new(80.0, 25.0);
    assert_eq!(
        gdscene::item_views::call_item_view_method(
            &mut tree,
            view,
            "get_item_at_position",
            &[Variant::Vector2(miss)]
        ),
        Some(Variant::Nil),
    );
    assert_eq!(
        gdscene::item_views::call_item_view_method(
            &mut tree,
            view,
            "get_column_at_position",
            &[Variant::Vector2(miss)]
        ),
        Some(Variant::Int(-1)),
    );
    assert_eq!(
        gdscene::item_views::call_item_view_method(
            &mut tree,
            view,
            "get_drop_section_at_position",
            &[Variant::Vector2(miss)]
        ),
        Some(Variant::Int(TREE_DROP_SECTION_NONE)),
    );
}

// ---------------------------------------------------------------------------
// get_item_area_rect — tree.cpp:5839
// ---------------------------------------------------------------------------

/// The whole-row rect, in the control's own coordinates.
///
/// ⚠️ It spans the FULL control width, not the sum of the column widths. Godot
/// uses `get_size().width`, and the two differ whenever the columns do not fill
/// the control — which a non-expanding column or the truncating arithmetic makes
/// common.
#[test]
fn the_whole_row_rect_spans_the_control_not_the_columns() {
    let (tree, view) = tree_with_a_short_column();

    let row0 = tree_item_area_rect(&tree, view, 0, None);
    assert_eq!(row0.position, Vector2::new(0.0, 0.0));
    assert_eq!(
        row0.size,
        Vector2::new(120.0, ITEM_ROW_HEIGHT),
        "120 is the control's width; the single column is only 40 wide"
    );

    let row1 = tree_item_area_rect(&tree, view, 1, None);
    assert_eq!(
        row1.position,
        Vector2::new(0.0, ITEM_ROW_HEIGHT),
        "the second row sits one row height down"
    );
}

/// A cell rect uses that column's span.
#[test]
fn a_cell_rect_uses_its_own_column_span() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();
    let mut model = TreeControl::new(2);
    model.create_item(None);
    store_tree(&mut tree, view, &model);

    // Two default columns over 120px: 60 each.
    assert_eq!(
        tree_item_area_rect(&tree, view, 0, Some(0)),
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(60.0, ITEM_ROW_HEIGHT))
    );
    assert_eq!(
        tree_item_area_rect(&tree, view, 0, Some(1)),
        Rect2::new(Vector2::new(60.0, 0.0), Vector2::new(60.0, ITEM_ROW_HEIGHT))
    );
}

/// ⚠️ The rect is control-LOCAL, so a Tree away from the origin reports the same
/// rect as one at it. Returning viewport-global coordinates would place every
/// popup at double the control's offset.
#[test]
fn the_area_rect_is_control_local() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 50.0, 30.0, 120.0, 100.0))
        .unwrap();
    let mut model = TreeControl::new(1);
    model.create_item(None);
    store_tree(&mut tree, view, &model);

    assert_eq!(
        tree_item_area_rect(&tree, view, 0, None).position,
        Vector2::new(0.0, 0.0),
        "the first row starts at the control's own origin, not at (50, 30)"
    );
}

/// An item with no row — hidden, or inside a collapsed subtree — has no area.
/// An out-of-range column likewise, rather than silently answering column 0.
#[test]
fn an_item_without_a_row_has_an_empty_area() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();
    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let child = model.create_item(Some(root));
    let hidden = model.create_item(Some(root));
    model.set_item_visible(hidden, false);
    model.set_collapsed(root, true);
    store_tree(&mut tree, view, &model);

    let empty = Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(0.0, 0.0));
    assert_eq!(
        tree_item_area_rect(&tree, view, child, None),
        empty,
        "a child of a collapsed parent is not laid out"
    );
    assert_eq!(tree_item_area_rect(&tree, view, hidden, None), empty);
    assert_eq!(
        tree_item_area_rect(&tree, view, root, Some(7)),
        empty,
        "an out-of-range column is empty, not column 0"
    );
    // The collapsed parent itself still has a row.
    assert_eq!(
        tree_item_area_rect(&tree, view, root, None).size,
        Vector2::new(120.0, ITEM_ROW_HEIGHT)
    );
}

/// The script surface. ⚠️ A `button_index` is refused rather than guessed:
/// sizing a button needs its texture and no buttons are modelled, so answering
/// the cell rect for one would be quietly wrong.
#[test]
fn get_item_area_rect_is_reachable_from_a_script() {
    let (mut tree, view) = tree_with_a_short_column();

    let whole = gdscene::item_views::call_item_view_method(
        &mut tree,
        view,
        "get_item_area_rect",
        &[Variant::Int(0)],
    );
    assert_eq!(
        whole,
        Some(Variant::Rect2(Rect2::new(
            Vector2::new(0.0, 0.0),
            Vector2::new(120.0, ITEM_ROW_HEIGHT)
        ))),
        "no column argument means the whole row"
    );

    let cell = gdscene::item_views::call_item_view_method(
        &mut tree,
        view,
        "get_item_area_rect",
        &[Variant::Int(0), Variant::Int(0)],
    );
    assert_eq!(
        cell,
        Some(Variant::Rect2(Rect2::new(
            Vector2::new(0.0, 0.0),
            Vector2::new(40.0, ITEM_ROW_HEIGHT)
        ))),
        "the column is only 40 wide"
    );

    let with_button = gdscene::item_views::call_item_view_method(
        &mut tree,
        view,
        "get_item_area_rect",
        &[Variant::Int(0), Variant::Int(0), Variant::Int(0)],
    );
    assert_eq!(
        with_button,
        Some(Variant::Rect2(Rect2::new(
            Vector2::new(0.0, 0.0),
            Vector2::new(0.0, 0.0)
        ))),
        "a button index is refused while no buttons are modelled"
    );
}

// ---------------------------------------------------------------------------
// The draw arm — the bead's remaining acceptance criterion
// ---------------------------------------------------------------------------

/// **The acceptance criterion.** One text primitive per visible row, each at the
/// same y the click path resolves that row to.
///
/// ⚠️ Asserted against `tree_visible_rows` rather than against hardcoded
/// coordinates: the whole point is that the draw pass and the hit test consume
/// one row list, so the test compares them to each other. Hardcoding the y would
/// still pass if the two drifted apart in the same direction.
#[test]
fn the_draw_pass_emits_one_row_per_visible_item_at_the_hit_y() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let a = model.create_item(Some(root));
    model.set_text(root, 0, "parent");
    model.set_text(a, 0, "child");
    store_tree(&mut tree, view, &model);

    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(
        &mut tree,
        view,
        &theme,
        Vector2::new(120.0, 100.0),
    );
    let strings: Vec<(String, f32)> = items
        .iter()
        .filter_map(|i| match i {
            gdscene::control_render::ControlDrawItem::Text { text, position, .. } => {
                Some((text.clone(), position.y))
            }
            _ => None,
        })
        .collect();

    let rows = tree_visible_rows(&tree, view);
    assert_eq!(
        strings.len(),
        rows.len(),
        "one text primitive per visible row; got {strings:?}"
    );
    assert_eq!(strings[0].0, "parent");
    assert_eq!(strings[1].0, "child");
    for (i, row) in rows.iter().enumerate() {
        assert!(
            strings[i].1 >= row.rect.position.y
                && strings[i].1 < row.rect.position.y + row.rect.size.y,
            "row {i}'s text must sit inside the rect the click resolves to"
        );
    }
}

/// Collapsing a parent removes its children from the DRAW list as well as the
/// hit test — the acceptance criterion's second half.
#[test]
fn a_collapsed_subtree_disappears_from_the_draw_list() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let mut model = TreeControl::new(1);
    let root = model.create_item(None);
    let a = model.create_item(Some(root));
    model.set_text(root, 0, "parent");
    model.set_text(a, 0, "child");
    model.set_collapsed(root, true);
    store_tree(&mut tree, view, &model);

    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(
        &mut tree,
        view,
        &theme,
        Vector2::new(120.0, 100.0),
    );
    let texts: Vec<String> = items
        .iter()
        .filter_map(|i| match i {
            gdscene::control_render::ControlDrawItem::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(texts.contains(&"parent".to_string()));
    assert!(
        !texts.contains(&"child".to_string()),
        "a collapsed parent's child must not be drawn"
    );
}

/// ⚠️ `Tree` must be in `CONTROL_CLASSES` as well as `CONTROL_HIT_CLASSES`. With
/// only the hit entry — the state this bead started in — the node absorbs clicks
/// and draws nothing.
#[test]
fn a_tree_draws_something_at_all() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let view = tree
        .add_child(root_id, control("Tree", "Tree", 0.0, 0.0, 120.0, 100.0))
        .unwrap();
    let mut model = TreeControl::new(1);
    model.create_item(None);
    store_tree(&mut tree, view, &model);

    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(
        &mut tree,
        view,
        &theme,
        Vector2::new(120.0, 100.0),
    );
    assert!(
        !items.is_empty(),
        "an empty draw list means Tree is missing from CONTROL_CLASSES"
    );
}
