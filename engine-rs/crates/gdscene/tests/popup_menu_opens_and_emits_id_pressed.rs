//! Acceptance test for pat-s3rrv.72: the popup layer is reachable.
//!
//! `popup.rs` (items, separators, checkables, `activate_item`) and `menu.rs`
//! (open/hover state) both existed and **neither had a caller** — `popup::`
//! matched one doc comment workspace-wide and `menu::` matched nothing. A
//! MenuButton was a button that did nothing, and `item_views::item_view_click`
//! returned false for OptionButton with a TODO pointing here.
//!
//! Godot's `PopupMenu` is a `Popup` <- `Window`: it draws above everything,
//! GRABS input while open, and activates on release
//! (`upstream/godot/scene/gui/popup_menu.cpp`).

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, Theme};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::{Dictionary, Variant};

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

fn release(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(x, y),
    }
}

/// One authored menu entry, in the Array-of-Dictionaries shape `.tscn` carries
/// (the same storage `item_views` uses for an ItemList).
fn item(text: &str, id: i64, disabled: bool, separator: bool) -> Variant {
    let mut d = Dictionary::new();
    d.insert(Variant::String("text".into()), Variant::String(text.into()));
    d.insert(Variant::String("id".into()), Variant::Int(id));
    if disabled {
        d.insert(Variant::String("disabled".into()), Variant::Bool(true));
    }
    if separator {
        d.insert(Variant::String("separator".into()), Variant::Bool(true));
    }
    Variant::Dictionary(d.into())
}

/// A MenuButton at (10,10) sized 80x24, owning a PopupMenu of
/// `Open` / `Save` / separator / disabled `Quit`.
///
/// The popup therefore opens at y = 34 and its rows are 24px tall.
fn menu_scene() -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = tree
        .add_child(root, Node::new("Menu", "MenuButton"))
        .unwrap();
    gdscene::control::set_position(&mut tree, btn, Vector2::new(10.0, 10.0));
    gdscene::control::set_size(&mut tree, btn, Vector2::new(80.0, 24.0));

    let popup = tree
        .add_child(btn, Node::new("Popup", "PopupMenu"))
        .unwrap();
    let items = Variant::Array(
        vec![
            item("Open", 10, false, false),
            item("Save", 20, false, false),
            item("", 0, false, true),
            item("Quit", 30, true, false),
        ]
        .into(),
    );
    tree.get_node_mut(popup)
        .unwrap()
        .set_property("items", items);

    // Capture `id_pressed` in a script, wired from Rust: what this bead must
    // prove is that the popup EMITS it, and routing through a `_ready` +
    // `connect(...)` would drag an unrelated mechanism into the failure surface.
    let src = "extends PopupMenu\nvar got = -1\nvar idx = -1\n\
               func _on_id(id):\n    self.got = id\n\
               func _on_index(i):\n    self.idx = i\n";
    tree.attach_script(
        popup,
        Box::new(GDScriptNodeInstance::from_source(src, popup).unwrap()),
    );
    for (signal, method) in [("id_pressed", "_on_id"), ("index_pressed", "_on_index")] {
        tree.connect_signal(
            popup,
            signal,
            gdobject::signal::Connection::new(popup.object_id(), method),
        );
    }
    (tree, btn, popup)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn draw(tree: &mut SceneTree) -> Vec<gdscene::control_render::ControlDrawItem> {
    let root = tree.root_id();
    gdscene::control_render::control_draw_items(
        tree,
        root,
        &Theme::default(),
        Vector2::new(400.0, 300.0),
    )
}

fn texts(items: &[gdscene::control_render::ControlDrawItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            gdscene::control_render::ControlDrawItem::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// **The acceptance case**: the button opens the menu, the menu draws, and
/// choosing a row emits `id_pressed` with that row's id and closes.
#[test]
fn popup_menu_opens_and_emits_id_pressed() {
    let (mut tree, btn, popup) = menu_scene();

    // Nothing is drawn from the popup before it opens.
    assert!(
        !texts(&draw(&mut tree)).contains(&"Open".to_string()),
        "the menu must not draw until it is opened"
    );

    tree.push_input(&press(50.0, 20.0));
    let after_open = texts(&draw(&mut tree));
    assert!(
        after_open.contains(&"Open".to_string()) && after_open.contains(&"Save".to_string()),
        "pressing the MenuButton must open the popup and draw its rows; got {after_open:?}"
    );

    // Row 1 ("Save", id 20): the popup starts at y = 10 + 24 = 34, rows are
    // ROW_HEIGHT tall, so row 1 spans 58..82 — 70 is its middle.
    tree.push_input(&release(50.0, 70.0));

    assert_eq!(
        script_var(&tree, popup, "got"),
        Variant::Int(20),
        "choosing `Save` must emit id_pressed with its authored id 20, not its \
         index — an implementation that emitted the index would answer 1 here"
    );
    assert_eq!(
        script_var(&tree, popup, "idx"),
        Variant::Int(1),
        "…and index_pressed with the row index"
    );

    let after_choose = texts(&draw(&mut tree));
    assert!(
        !after_choose.contains(&"Open".to_string()),
        "activating an item must close the popup; got {after_choose:?}"
    );
    let _ = btn;
}

/// A separator and a disabled row are not activatable: the menu stays open and
/// nothing is emitted.
#[test]
fn a_separator_and_a_disabled_row_do_not_activate() {
    let (mut tree, _btn, popup) = menu_scene();
    tree.push_input(&press(50.0, 20.0));

    // Row 2 is the separator (34 + 2*24 = 82..106).
    tree.push_input(&release(50.0, 94.0));
    assert_eq!(
        script_var(&tree, popup, "got"),
        Variant::Int(-1),
        "a separator must not emit id_pressed"
    );
    assert!(
        texts(&draw(&mut tree)).contains(&"Open".to_string()),
        "…and must leave the menu open rather than dismissing it as if chosen"
    );

    // Row 3 is the disabled `Quit` (106..130).
    tree.push_input(&release(50.0, 118.0));
    assert_eq!(
        script_var(&tree, popup, "got"),
        Variant::Int(-1),
        "a disabled row must not emit id_pressed"
    );
    assert!(
        texts(&draw(&mut tree)).contains(&"Open".to_string()),
        "…and must leave the menu open"
    );
}

/// 🔑 The popup is MODAL: a click inside its rect never reaches the control
/// underneath.
///
/// Without this the menu is cosmetic — every choice would also press whatever
/// the popup happens to cover.
#[test]
fn a_click_inside_the_popup_never_reaches_the_control_underneath() {
    let (mut tree, _btn, popup) = menu_scene();

    // A Button parked directly under the popup's second row, recording presses.
    let root = tree.root_id();
    let under = tree.add_child(root, Node::new("Under", "Button")).unwrap();
    gdscene::control::set_position(&mut tree, under, Vector2::new(10.0, 58.0));
    gdscene::control::set_size(&mut tree, under, Vector2::new(80.0, 24.0));
    let src = "extends Button\nvar hits = 0\nfunc _on_pressed():\n    self.hits += 1\n";
    tree.attach_script(
        under,
        Box::new(GDScriptNodeInstance::from_source(src, under).unwrap()),
    );
    tree.connect_signal(
        under,
        "pressed",
        gdobject::signal::Connection::new(under.object_id(), "_on_pressed"),
    );

    tree.push_input(&press(50.0, 20.0));
    tree.push_input(&press(50.0, 70.0));
    tree.push_input(&release(50.0, 70.0));

    assert_eq!(
        script_var(&tree, under, "hits"),
        Variant::Int(0),
        "the Button under the popup must NOT have been pressed — the popup grabs \
         input while open, and this is the whole difference between a menu and \
         a decoration"
    );
    assert_eq!(
        script_var(&tree, popup, "got"),
        Variant::Int(20),
        "…while the popup itself still activated its row"
    );
}
