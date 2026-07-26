//! pat-0q2kx.87 acceptance: LineEdit is an editable control, not a string bag.
//!
//! Godot's `LineEdit` (upstream/godot/scene/gui/line_edit.cpp `gui_input`) turns
//! key events into text edits: insert at the caret, backspace/delete, home/end,
//! shift-selection, a `max_length` clamp, secret masking, `text_changed`
//! per edit and `text_submitted` on Enter. Patina stored only the `text`
//! property (`control.rs` `set_line_edit_text`) and **never routed a key to
//! it** — no caret existed, no signal was declared, and the render pass had no
//! LineEdit arm at all, so a `.tscn` LineEdit was an invisible read-only box.
//!
//! Everything below drives the runtime path a game actually uses — real
//! `InputEvent`s through `SceneTree::push_input`, signals observed through a
//! GDScript handler, and the control render pass — so a regression to "the
//! model exists but nothing reaches it" fails rather than passing on a unit
//! test of the model alone.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, Key};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// A Control node with an explicit rect.
///
/// `position`/`size` are what the hit test reads; the matching anchor offsets
/// are what `resolve_control_layout` re-derives the rect from during the render
/// pass. Setting both keeps the focused rect and the drawn rect identical.
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

fn key_event(key: Key, pressed: bool, shift: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift,
        ctrl: false,
        alt: false,
    }
}

/// Types one key the way a keyboard does — press **and** release. Sending both
/// is the point: a release must not insert a second character.
fn type_key(tree: &mut SceneTree, key: Key) {
    tree.push_input(&key_event(key, true, false));
    tree.push_input(&key_event(key, false, false));
}

fn type_key_shift(tree: &mut SceneTree, key: Key) {
    tree.push_input(&key_event(key, true, true));
    tree.push_input(&key_event(key, false, true));
}

/// Counts thin filled bars — the caret's shape. A theme stylebox can also emit
/// narrow rects for its borders, so no test below reads this count as an
/// absolute "is there a caret?"; they compare it focused vs unfocused, which is
/// immune to whatever the background draws.
fn narrow_bar_count(items: &[ControlDrawItem]) -> usize {
    items
        .iter()
        .filter(|i| {
            matches!(
                i,
                ControlDrawItem::Rect { rect, filled, .. }
                    if *filled && rect.size.x > 0.0 && rect.size.x <= 4.0 && rect.size.y > 0.0
            )
        })
        .count()
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn int_var(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_var(tree, node, name) {
        Some(Variant::Int(n)) => n,
        other => panic!("{name} should be an Int, got {other:?}"),
    }
}

fn string_var(tree: &SceneTree, node: NodeId, name: &str) -> String {
    match script_var(tree, node, name) {
        Some(Variant::String(s)) => s,
        other => panic!("{name} should be a String, got {other:?}"),
    }
}

/// The handler script. It lives on the LineEdit itself so the connections and
/// the observed state share one instance.
const HANDLER: &str = "\
extends LineEdit
var changes = 0
var last_text = \"\"
var submits = 0
var submitted = \"\"
func _on_text_changed(new_text):
    self.changes = self.changes + 1
    self.last_text = new_text
func _on_text_submitted(t):
    self.submits = self.submits + 1
    self.submitted = t
";

/// Builds `root -> Input` (a LineEdit), scripted and signal-connected, and
/// focused. `max_length` of 0 means unlimited.
fn line_edit_scene(max_length: i64) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = control("Input", "LineEdit", 10.0, 10.0, 200.0, 24.0);
    node.set_property("max_length", Variant::Int(max_length));
    let id = tree.add_child(root, node).unwrap();

    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(HANDLER, id).unwrap()),
    );
    // Wired from Rust rather than from a `_ready` + `connect(...)`: what this
    // bead must prove is that the editing path *emits* these signals, and
    // routing that through the script's own connect would drag an unrelated
    // mechanism into the failure surface.
    for (signal, method) in [
        ("text_changed", "_on_text_changed"),
        ("text_submitted", "_on_text_submitted"),
    ] {
        tree.connect_signal(
            id,
            signal,
            gdobject::signal::Connection::new(id.object_id(), method),
        );
    }
    tree.grab_focus(id);
    (tree, id)
}

/// **The acceptance test.** Every clause of the bead's "prove it", in order.
#[test]
fn line_edit_typing_signals_e2e() {
    let (mut tree, input) = line_edit_scene(5);

    // -- typing 'h','i' produces "hi" and exactly two text_changed ----------
    type_key(&mut tree, Key::H);
    type_key(&mut tree, Key::I);

    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "hi",
        "key events routed to the focused LineEdit must insert at the caret"
    );
    assert_eq!(
        int_var(&tree, input, "changes"),
        2,
        "text_changed fires once per edit — a key RELEASE must not count as a \
         second edit"
    );
    assert_eq!(
        string_var(&tree, input, "last_text"),
        "hi",
        "text_changed carries the new text"
    );
    assert_eq!(
        gdscene::line_edit::caret_column(&tree, input),
        2,
        "the caret advances past what was typed"
    );

    // -- max_length 5 stops typing at 5 -------------------------------------
    let changes_before = int_var(&tree, input, "changes");
    for k in [Key::A, Key::B, Key::C, Key::D, Key::E] {
        type_key(&mut tree, k);
    }
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "hiabc",
        "typing past max_length must stop at the cap, keeping the first 5 chars"
    );
    assert_eq!(
        int_var(&tree, input, "changes") - changes_before,
        3,
        "only the three keystrokes that fit may emit text_changed; the two \
         rejected ones change nothing and must stay silent"
    );

    // -- Enter emits text_submitted with the current text --------------------
    type_key(&mut tree, Key::Enter);
    assert_eq!(
        int_var(&tree, input, "submits"),
        1,
        "Enter must emit text_submitted exactly once"
    );
    assert_eq!(
        string_var(&tree, input, "submitted"),
        "hiabc",
        "text_submitted carries the current text"
    );
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "hiabc",
        "Godot's LineEdit does not clear itself on submit"
    );

    // -- backspace removes a character --------------------------------------
    type_key(&mut tree, Key::Backspace);
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "hiab",
        "backspace deletes the character before the caret"
    );
    assert_eq!(
        gdscene::line_edit::caret_column(&tree, input),
        4,
        "backspace pulls the caret back with it"
    );

    // -- the rendered frame shows the text and a caret -----------------------
    let theme = Theme::default_theme();
    let items = control_draw_items(&mut tree, input, &theme, Vector2::new(320.0, 200.0));

    let drew_text = items
        .iter()
        .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text == "hiab"));
    assert!(
        drew_text,
        "the render pass must draw the LineEdit's text, got {items:?}"
    );

    // The caret is a thin filled bar. Asserting it *differentially* — one more
    // narrow bar while focused than while not — pins it to the caret rather
    // than to whatever narrow rects the background stylebox happens to draw.
    tree.release_focus();
    let unfocused = control_draw_items(&mut tree, input, &theme, Vector2::new(320.0, 200.0));
    assert_eq!(
        narrow_bar_count(&items),
        narrow_bar_count(&unfocused) + 1,
        "a focused LineEdit must draw exactly one caret bar that an unfocused \
         one does not; focused={items:?} unfocused={unfocused:?}"
    );
}

/// The caret is focus state, not a permanent decoration, and the text draws
/// either way. Without this, "always draw a bar" would satisfy the acceptance
/// test above.
#[test]
fn caret_tracks_focus_and_text_draws_regardless() {
    let (mut tree, input) = line_edit_scene(0);
    type_key(&mut tree, Key::H);

    let theme = Theme::default_theme();
    let size = Vector2::new(320.0, 200.0);
    let focused = control_draw_items(&mut tree, input, &theme, size);
    tree.release_focus();
    let unfocused = control_draw_items(&mut tree, input, &theme, size);

    for (label, items) in [("focused", &focused), ("unfocused", &unfocused)] {
        assert!(
            items
                .iter()
                .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text == "h")),
            "the text must draw while {label}, got {items:?}"
        );
    }
    assert!(
        narrow_bar_count(&focused) > narrow_bar_count(&unfocused),
        "dropping focus must remove the caret bar; focused={focused:?} \
         unfocused={unfocused:?}"
    );
}

/// Shift produces the upper-case/punctuation form, and `secret` masks what is
/// drawn without touching the stored text.
#[test]
fn shift_typing_and_secret_masking() {
    let (mut tree, input) = line_edit_scene(0);
    type_key_shift(&mut tree, Key::H);
    type_key(&mut tree, Key::I);
    type_key_shift(&mut tree, Key::Num1);

    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "Hi!",
        "shift must produce the upper-case letter and the shifted number-row \
         punctuation, not a capital digit"
    );

    // Secret masks the DISPLAY only.
    if let Some(node) = tree.get_node_mut(input) {
        node.set_property("secret", Variant::Bool(true));
    }
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "Hi!",
        "secret must not alter the stored text a script reads"
    );
    assert_eq!(
        gdscene::line_edit::display_text(&tree, input),
        "•••",
        "secret masks every character for display"
    );

    let theme = Theme::default_theme();
    let items = control_draw_items(&mut tree, input, &theme, Vector2::new(320.0, 200.0));
    assert!(
        !items
            .iter()
            .any(|i| matches!(i, ControlDrawItem::Text { text, .. } if text.contains("Hi"))),
        "a secret LineEdit must never draw the real text, got {items:?}"
    );
}

/// While a text field has focus, Left/Right move the caret rather than walking
/// the focus ring — the ordering claim the key-stage wiring makes.
#[test]
fn arrows_move_the_caret_not_the_focus_ring() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let input = tree
        .add_child(root, control("Input", "LineEdit", 0.0, 0.0, 100.0, 24.0))
        .unwrap();
    // A second focusable control, so a stray focus move has somewhere to go.
    let button = tree
        .add_child(root, control("Next", "Button", 0.0, 40.0, 100.0, 24.0))
        .unwrap();
    tree.grab_focus(input);

    type_key(&mut tree, Key::H);
    type_key(&mut tree, Key::I);
    assert_eq!(gdscene::line_edit::caret_column(&tree, input), 2);

    tree.push_input(&key_event(Key::Left, true, false));
    assert_eq!(
        gdscene::line_edit::caret_column(&tree, input),
        1,
        "Left must move the caret inside the focused LineEdit"
    );
    assert_eq!(
        tree.focus_owner(),
        Some(input),
        "the focus ring must not move while a text field is focused"
    );
    assert_ne!(tree.focus_owner(), Some(button));

    tree.push_input(&key_event(Key::Home, true, false));
    assert_eq!(gdscene::line_edit::caret_column(&tree, input), 0);
    tree.push_input(&key_event(Key::End, true, false));
    assert_eq!(gdscene::line_edit::caret_column(&tree, input), 2);

    // Tab is NOT consumed by a LineEdit — Godot leaves focus navigation intact.
    tree.push_input(&key_event(Key::Tab, true, false));
    assert_eq!(
        tree.focus_owner(),
        Some(button),
        "Tab must still walk the focus ring out of a LineEdit"
    );
}

/// Shift-arrow builds a selection; typing replaces it; Delete removes it.
#[test]
fn shift_selection_replace_and_delete() {
    let (mut tree, input) = line_edit_scene(0);
    for k in [Key::H, Key::I, Key::A, Key::B] {
        type_key(&mut tree, k);
    }
    assert_eq!(gdscene::line_edit::text(&tree, input), "hiab");

    // Select the last two characters: caret is at 4, shift-Left twice.
    tree.push_input(&key_event(Key::Left, true, true));
    tree.push_input(&key_event(Key::Left, true, true));
    assert_eq!(
        gdscene::line_edit::selection(&tree, input),
        Some((2, 4)),
        "shift-Left extends a selection back from the caret"
    );
    assert_eq!(gdscene::line_edit::selected_text(&tree, input), "ab");

    // Typing over a selection replaces it.
    type_key(&mut tree, Key::Z);
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "hiz",
        "typing with a selection must replace it, not insert alongside it"
    );
    assert!(
        !gdscene::line_edit::has_selection(&tree, input),
        "the selection is consumed by the replacement"
    );

    // Delete with a selection removes the selection.
    tree.push_input(&key_event(Key::Home, true, false));
    tree.push_input(&key_event(Key::Right, true, true));
    assert_eq!(gdscene::line_edit::selected_text(&tree, input), "h");
    tree.push_input(&key_event(Key::Delete, true, false));
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "iz",
        "Delete with a selection removes the selection, not the next character"
    );
}

/// `editable = false` blocks every mutation but still consumes the keystroke,
/// as Godot's read-only LineEdit does.
#[test]
fn non_editable_line_edit_rejects_edits() {
    let (mut tree, input) = line_edit_scene(0);
    type_key(&mut tree, Key::H);
    if let Some(node) = tree.get_node_mut(input) {
        node.set_property("editable", Variant::Bool(false));
    }
    let changes_before = int_var(&tree, input, "changes");

    type_key(&mut tree, Key::I);
    type_key(&mut tree, Key::Backspace);

    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "h",
        "a non-editable LineEdit must accept neither insertions nor deletions"
    );
    assert_eq!(
        int_var(&tree, input, "changes"),
        changes_before,
        "a blocked edit must not emit text_changed"
    );
}

/// A TextEdit is the same machine with one fork: Enter inserts a newline
/// instead of submitting.
#[test]
fn text_edit_enter_inserts_a_newline() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let editor = tree
        .add_child(root, control("Doc", "TextEdit", 0.0, 0.0, 200.0, 80.0))
        .unwrap();
    tree.grab_focus(editor);

    type_key(&mut tree, Key::H);
    type_key(&mut tree, Key::Enter);
    type_key(&mut tree, Key::I);

    assert_eq!(
        gdscene::line_edit::text(&tree, editor),
        "h\ni",
        "Enter in a multi-line TextEdit inserts a newline rather than submitting"
    );
}

/// A LineEdit with no focus must not eat keystrokes meant for gameplay.
#[test]
fn unfocused_line_edit_ignores_typing() {
    let (mut tree, input) = line_edit_scene(0);
    tree.release_focus();
    type_key(&mut tree, Key::H);
    assert_eq!(
        gdscene::line_edit::text(&tree, input),
        "",
        "keys must only reach a text field that holds focus"
    );
}
