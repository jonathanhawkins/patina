//! pat-0q2kx.85 acceptance: the item-view Controls are attached to scene nodes.
//!
//! `gdscene::item_controls` held correct ItemList/OptionButton/TabBar item and
//! selection logic that **nothing instantiated for a scene node** — its own doc
//! header said signals were "surfaced as return values", `control_render.rs`
//! never imported it, and no script method reached it. So a `.tscn` `ItemList`
//! had no items, `$ItemList.add_item("a")` threw `UndefinedFunction`, and a
//! click on a row did nothing.
//!
//! These drive the runtime paths a game actually uses — a GDScript `_process`
//! calling the methods, a real `InputEvent` through `SceneTree::push_input`, and
//! the control render pass — and assert on script-observable effects, so a
//! regression to "the model exists but is unreachable" fails.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::item_views::ITEM_ROW_HEIGHT;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A Control node with an explicit rect.
///
/// `position`/`size` are what the hit test reads; the matching anchor offsets
/// are what `resolve_control_layout` re-derives the rect from during the render
/// pass. Setting both keeps the clicked rect and the drawn rect identical.
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

fn node_prop(tree: &SceneTree, node: NodeId, key: &str) -> Variant {
    tree.get_node(node).unwrap().get_property(key)
}

/// `Main -> List`, where Main's script fills the list on its first frame and
/// records `item_selected` / `item_activated` payloads.
fn item_list_scene() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let list = tree
        .add_child(main, control("List", "ItemList", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let src = "\
extends Node2D
var frame = 0
var count = 0
var selected = -1
var activated = -1
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"List\").add_item(\"a\")
        get_node(\"List\").add_item(\"b\")
        self.count = get_node(\"List\").get_item_count()
func _on_selected(index):
    self.selected = index
func _on_activated(index):
    self.activated = index
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    for (signal, method) in [
        ("item_selected", "_on_selected"),
        ("item_activated", "_on_activated"),
    ] {
        tree.connect_signal(
            list,
            signal,
            gdobject::signal::Connection::new(main.object_id(), method),
        );
    }
    (tree, main, list)
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** A script adds two items; a real click on row 1
/// selects it and delivers `item_selected(1)` to a connected script; the render
/// pass draws row 1 highlighted.
#[test]
fn item_list_click_select_e2e() {
    let (tree, main, list) = item_list_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // The script's own methods reached the item model.
    assert_eq!(
        script_var(ml.tree(), main, "count"),
        Some(Variant::Int(2)),
        "$List.add_item()/get_item_count() must reach the ItemList model"
    );

    // Row 1 spans y in [20, 40) — a real mouse event, through the GUI stage.
    ml.tree_mut()
        .push_input(&press(10.0, ITEM_ROW_HEIGHT + 5.0));

    assert_eq!(
        script_var(ml.tree(), main, "selected"),
        Some(Variant::Int(1)),
        "clicking row 1 must emit item_selected(1) into the connected script"
    );
    // The frame shows the selection: row 1 carries a highlight rect the other
    // rows do not, at the row's own y.
    let theme = Theme::default_theme();
    let items = control_draw_items(ml.tree_mut(), list, &theme, Vector2::new(120.0, 100.0));
    let row1_y = ITEM_ROW_HEIGHT;
    // Only the selected row draws a rect one row tall at its own y — the panel
    // background is the full 100px rect at y = 0.
    let highlight = items.iter().find(|i| match i {
        ControlDrawItem::Rect {
            rect, filled: true, ..
        } => (rect.position.y - row1_y).abs() < 0.5 && (rect.size.y - ITEM_ROW_HEIGHT).abs() < 0.5,
        _ => false,
    });
    assert!(
        highlight.is_some(),
        "the selected row must draw a highlight rect at its own y; drew {items:#?}"
    );
    // Both labels are drawn, so the highlight is not standing in for the rows.
    for label in ["a", "b"] {
        assert!(
            items
                .iter()
                .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text == label)),
            "row label {label:?} must be drawn"
        );
    }
}

/// A double-click promotes the row click to `item_activated`, which a single
/// click never fires (Godot `item_list.cpp` `gui_input`).
#[test]
fn double_click_activates_the_row() {
    let (tree, main, _list) = item_list_scene();
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let point = Vector2::new(10.0, 5.0);
    ml.tree_mut().push_input(&press(point.x, point.y));
    assert_eq!(
        script_var(ml.tree(), main, "activated"),
        Some(Variant::Int(-1)),
        "a single click must not emit item_activated"
    );

    assert!(
        gdscene::control::double_click_at_point(ml.tree_mut(), point).is_some(),
        "the double click must land on the ItemList"
    );
    assert_eq!(
        script_var(ml.tree(), main, "activated"),
        Some(Variant::Int(0)),
        "a double click on row 0 must emit item_activated(0)"
    );
}

/// The script setter is silent: Godot emits `item_selected` from a click, never
/// from `ItemList.select()` (`item_list.cpp` emits only under `gui_input`).
#[test]
fn script_select_does_not_emit_item_selected() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let list = tree
        .add_child(main, control("List", "ItemList", 0.0, 0.0, 120.0, 100.0))
        .unwrap();

    let src = "\
extends Node2D
var selected = -1
func _on_selected(index):
    self.selected = index
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    tree.connect_signal(
        list,
        "item_selected",
        gdobject::signal::Connection::new(main.object_id(), "_on_selected"),
    );

    // Driven from Rust, so no script is mid-call and the emit path is fully
    // live — the same connection a click delivers through in
    // `item_list_click_select_e2e`.
    for label in ["a", "b"] {
        gdscene::item_views::call_item_view_method(
            &mut tree,
            list,
            "add_item",
            &[Variant::String(label.into())],
        )
        .expect("add_item must be an ItemList method");
    }
    gdscene::item_views::call_item_view_method(&mut tree, list, "select", &[Variant::Int(1)])
        .expect("select must be an ItemList method");

    // The selection moved...
    assert_eq!(
        gdscene::item_views::call_item_view_method(&mut tree, list, "get_selected_items", &[]),
        Some(Variant::Array(vec![Variant::Int(1)].into())),
        "select(1) must move the selection"
    );
    // ...without a signal.
    assert_eq!(
        script_var(&tree, main, "selected"),
        Some(Variant::Int(-1)),
        "ItemList.select() must NOT emit item_selected — only a click does"
    );
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

/// `set_current_tab(1)` emits `tab_changed(1)` and moves the TabContainer's
/// visible child, which is what the container's layout keys off.
#[test]
fn tab_container_set_current_tab_emits_and_switches_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let tabs = tree
        .add_child(
            main,
            control("Tabs", "TabContainer", 0.0, 0.0, 300.0, 200.0),
        )
        .unwrap();
    let first = tree.add_child(tabs, Node::new("First", "Panel")).unwrap();
    let second = tree.add_child(tabs, Node::new("Second", "Panel")).unwrap();
    // The handler lives on a second node: a script that is mid-`_process` is
    // checked out of the tree's script arena, so a signal it emits back into
    // *itself* on that same frame has nowhere to land. Godot has no such
    // restriction; keeping the listener separate tests the signal, not that.
    let listener = tree
        .add_child(root, Node::new("Listener", "Node2D"))
        .unwrap();

    let src = "\
extends Node2D
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"Tabs\").set_current_tab(1)
";
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).unwrap()),
    );
    let listener_src = "\
extends Node2D
var changed = -1
func _on_changed(tab):
    self.changed = tab
";
    tree.attach_script(
        listener,
        Box::new(GDScriptNodeInstance::from_source(listener_src, listener).unwrap()),
    );
    tree.connect_signal(
        tabs,
        "tab_changed",
        gdobject::signal::Connection::new(listener.object_id(), "_on_changed"),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(ml.tree(), listener, "changed"),
        Some(Variant::Int(1)),
        "$Tabs.set_current_tab(1) must emit tab_changed(1)"
    );
    assert_eq!(node_prop(ml.tree(), tabs, "current_tab"), Variant::Int(1));

    // The container's layout shows child 1 and collapses child 0.
    gdscene::control::arrange_container(ml.tree_mut(), tabs, Vector2::new(300.0, 200.0));
    assert_eq!(
        gdscene::control::get_size(ml.tree(), first),
        Vector2::ZERO,
        "the non-current tab's child must be collapsed"
    );
    assert!(
        gdscene::control::get_size(ml.tree(), second).x > 0.0,
        "the current tab's child must be laid out"
    );
}

/// A standalone TabBar carries its own tabs, and a click on the second tab's
/// span changes the current tab.
#[test]
fn tab_bar_click_changes_the_current_tab() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let bar = tree
        .add_child(root, control("Bar", "TabBar", 0.0, 0.0, 200.0, 32.0))
        .unwrap();

    for title in ["One", "Two"] {
        gdscene::item_views::call_item_view_method(
            &mut tree,
            bar,
            "add_tab",
            &[Variant::String(title.into())],
        )
        .expect("add_tab must be a TabBar method");
    }
    // The first tab added becomes current, as in Godot.
    assert_eq!(node_prop(&tree, bar, "current_tab"), Variant::Int(0));

    let spans = gdscene::item_views::tab_spans(&tree, bar);
    let (x, w) = spans[1];
    tree.push_input(&press(x + w * 0.5, 10.0));

    assert_eq!(
        node_prop(&tree, bar, "current_tab"),
        Variant::Int(1),
        "clicking the second tab's span must make it current"
    );

    // And the strip draws both titles.
    let theme = Theme::default_theme();
    let items = control_draw_items(&mut tree, bar, &theme, Vector2::new(200.0, 32.0));
    for title in ["One", "Two"] {
        assert!(
            items
                .iter()
                .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text == title)),
            "tab title {title:?} must be drawn"
        );
    }
}

// ---------------------------------------------------------------------------
// OptionButton
// ---------------------------------------------------------------------------

/// `OptionButton` items round-trip through the node, and the selected label is
/// what the button draws — Godot's `_select` mirrors it onto `text`.
#[test]
fn option_button_items_reach_the_node_and_the_frame() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ob = tree
        .add_child(root, control("Pick", "OptionButton", 0.0, 0.0, 120.0, 30.0))
        .unwrap();

    let call = |tree: &mut SceneTree, method: &str, args: &[Variant]| {
        gdscene::item_views::call_item_view_method(tree, ob, method, args)
            .unwrap_or_else(|| panic!("{method} must be an OptionButton method"))
    };
    call(
        &mut tree,
        "add_item",
        &[Variant::String("Red".into()), Variant::Int(10)],
    );
    call(
        &mut tree,
        "add_item",
        &[Variant::String("Green".into()), Variant::Int(20)],
    );
    assert_eq!(
        call(&mut tree, "get_selected_id", &[]),
        Variant::Int(10),
        "the first item added is auto-selected, as in Godot"
    );

    call(&mut tree, "select", &[Variant::Int(1)]);
    assert_eq!(call(&mut tree, "get_selected", &[]), Variant::Int(1));
    assert_eq!(call(&mut tree, "get_selected_id", &[]), Variant::Int(20));

    let theme = Theme::default_theme();
    let items = control_draw_items(&mut tree, ob, &theme, Vector2::new(120.0, 30.0));
    assert!(
        items
            .iter()
            .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text == "Green")),
        "the button must draw the selected item's label"
    );
}

/// The classes carry their signals in ClassDB, so `has_signal` answers before
/// anything connects to them.
#[test]
fn item_view_signals_are_registered() {
    gdobject::class_db::register_2d_classes();
    for (class, signal) in [
        ("ItemList", "item_selected"),
        ("ItemList", "item_activated"),
        ("TabBar", "tab_changed"),
        ("TabContainer", "tab_changed"),
        ("OptionButton", "item_selected"),
    ] {
        assert!(
            gdobject::class_db::class_has_signal(class, signal),
            "{class} must declare {signal}"
        );
    }
}
