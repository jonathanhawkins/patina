//! pat-0q2kx.21: keyboard focus on the SceneTree, and click-grabs-focus in the
//! GUI input stage.
//!
//! Before this there was no focus state anywhere — `grep grab_focus` across the
//! non-test sources returned zero hits — so `Control.has_focus()` had no answer
//! and clicking a Button could not move focus.
//!
//! The GUI stage itself (hit-test, `_gui_input`, hover, button press/release)
//! landed in pat-0q2kx.18; these cover only the focus layer added on top.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, Key, MouseButton};
use gdscene::control::FocusSide;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("position", Variant::Vector2(Vector2::new(x, y)));
    n.set_property("size", Variant::Vector2(Vector2::new(w, h)));
    n
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

/// `Main -> {Button, LineEdit, Label}`, laid out side by side.
fn focus_scene() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let btn = tree
        .add_child(main, control("Play", "Button", 0.0, 0.0, 80.0, 30.0))
        .unwrap();
    let edit = tree
        .add_child(main, control("Name", "LineEdit", 100.0, 0.0, 80.0, 30.0))
        .unwrap();
    // A Label is FOCUS_NONE in Godot — clicking it must not take focus.
    let label = tree
        .add_child(main, control("Title", "Label", 200.0, 0.0, 80.0, 30.0))
        .unwrap();
    (tree, btn, edit, label)
}

#[test]
fn grab_and_release_focus_emit_signals_and_track_owner() {
    let (mut tree, btn, edit, _label) = focus_scene();

    assert_eq!(tree.focus_owner(), None, "nothing is focused initially");
    assert!(!tree.has_focus(btn));

    tree.grab_focus(btn);
    assert_eq!(tree.focus_owner(), Some(btn));
    assert!(tree.has_focus(btn));

    // Moving focus transfers ownership exactly once.
    tree.grab_focus(edit);
    assert_eq!(tree.focus_owner(), Some(edit));
    assert!(!tree.has_focus(btn), "the previous owner loses focus");

    tree.release_focus();
    assert_eq!(tree.focus_owner(), None, "release drops focus entirely");
}

/// Re-grabbing on the current owner is a no-op, so repeated clicks do not
/// re-fire `focus_entered`.
#[test]
fn regrabbing_focus_on_the_current_owner_is_a_noop() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    tree.grab_focus(btn);
    tree.grab_focus(btn);
    assert_eq!(tree.focus_owner(), Some(btn));
}

/// **The wiring test.** A click inside a focusable Control's rect grabs focus
/// through the GUI input stage — not via a direct `grab_focus` call.
#[test]
fn clicking_a_control_grabs_focus_through_push_input() {
    let (mut tree, btn, edit, _label) = focus_scene();

    tree.push_input(&press(40.0, 15.0));
    assert!(
        tree.has_focus(btn),
        "clicking a Button must grab focus, got {:?}",
        tree.focus_owner()
    );

    tree.push_input(&press(140.0, 15.0));
    assert!(
        tree.has_focus(edit),
        "clicking the LineEdit must move focus to it, got {:?}",
        tree.focus_owner()
    );
}

/// 🔑 Clicking a Label **keeps** the previous focus owner. Godot does not
/// release focus for a click that lands on no control.
///
/// ⚠️ This test previously asserted the opposite — `focus_owner() == None`,
/// "matching Godot" — and failed against correct engine behaviour (pat-21oab).
/// Two things it had wrong:
///
/// 1. A Label is `MOUSE_FILTER_IGNORE` by default
///    (`scene/gui/label.cpp:1477`), so the click never resolves to the Label.
///    Its `FOCUS_NONE` mode never enters the picture, which is why the old name
///    described a mechanism that does not run.
/// 2. With no control under the cursor, `Viewport::_gui_input_event`
///    (`scene/main/viewport.cpp:1918-1926`) sets `gui.hide_focus = true`,
///    queues a redraw, and **returns** — `gui.key_focus` is untouched. Its
///    comment says so: "Focus should be hidden on click even if the focus
///    holder didn't change."
///
/// So the observable is: the Button stays the focus owner, and the Label never
/// becomes it. Godot additionally hides the focus *ring* via `hide_focus`;
/// Patina has no such flag yet, which is a real gap but a visual one and not
/// what this test measures.
#[test]
fn clicking_a_label_keeps_the_previous_focus_owner() {
    let (mut tree, btn, _edit, label) = focus_scene();

    tree.push_input(&press(40.0, 15.0));
    assert!(tree.has_focus(btn));

    // A Label is MOUSE_FILTER_IGNORE, so this press finds no control.
    tree.push_input(&press(240.0, 15.0));
    assert_eq!(
        tree.focus_owner(),
        Some(btn),
        "a click that lands on no control must leave the focus owner alone \
         (viewport.cpp:1918-1926 hides the ring and returns)"
    );
    assert!(
        !tree.has_focus(label),
        "the Label must never take focus — it is both FOCUS_NONE and \
         MOUSE_FILTER_IGNORE"
    );
}

// ===========================================================================
// pat-0q2kx.84: focus traversal (Tab / Shift-Tab), `focus_neighbor_*` arrows,
// key routing to the focus owner, and the `has_focus` script bridge.
//
// pat-0q2kx.21 (above) gave the tree a focus *owner*. Nothing moved it: Tab did
// nothing, `focus_next` / `focus_neighbor_*` were inert properties no code read,
// key events never reached the focused Control, and a script could not ask
// `has_focus()`. These cover the layer that makes the stored properties act.
// ===========================================================================

fn key_press(k: Key, shift: bool) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed: true,
        shift,
        ctrl: false,
        alt: false,
    }
}

fn key_release(k: Key) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

/// Reads a script member by name — how these tests observe GDScript state.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// A Button that records its own focus signals and presses in GDScript.
///
/// The signals are connected from Rust rather than from a `_ready` +
/// `connect(...)`, so a failure means "the runtime never emitted it" and not
/// "the script's own connect call broke" — the same split the pat-0q2kx.18
/// tests use.
fn scripted_button(tree: &mut SceneTree, parent: NodeId, name: &str, x: f32) -> NodeId {
    let id = tree
        .add_child(parent, control(name, "Button", x, 0.0, 80.0, 30.0))
        .unwrap();
    let src = "\
extends Button
var focus_in = 0
var focus_out = 0
var presses = 0
func _on_focus_entered():
    self.focus_in = self.focus_in + 1
func _on_focus_exited():
    self.focus_out = self.focus_out + 1
func _on_pressed():
    self.presses = self.presses + 1
";
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(src, id).unwrap()),
    );
    for (signal, method) in [
        ("focus_entered", "_on_focus_entered"),
        ("focus_exited", "_on_focus_exited"),
        ("pressed", "_on_pressed"),
    ] {
        tree.connect_signal(
            id,
            signal,
            gdobject::signal::Connection::new(id.object_id(), method),
        );
    }
    id
}

/// **The acceptance test.** `Main -> {A, B, C}` buttons, driven entirely by real
/// input events through a stepped `MainLoop`, observed through GDScript:
///
/// 1. clicking A focuses it and fires `focus_entered`;
/// 2. Tab moves focus to B, firing `focus_exited` on A;
/// 3. a `focus_next` NodePath override on A redirects Tab straight to C,
///    skipping B;
/// 4. `ui_accept` (Enter) while C is focused presses C — and only C.
#[test]
fn gui_focus_traversal_e2e() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let a = scripted_button(&mut tree, main, "A", 0.0);
    let b = scripted_button(&mut tree, main, "B", 100.0);
    let c = scripted_button(&mut tree, main, "C", 200.0);

    let mut ml = MainLoop::new(tree);
    let dt = 1.0 / 60.0;

    // 1. Click A.
    ml.push_event(press(40.0, 15.0));
    ml.step(dt);
    assert!(
        ml.tree().has_focus(a),
        "clicking A must focus it, got {:?}",
        ml.tree().focus_owner()
    );
    assert_eq!(
        script_var(ml.tree(), a, "focus_in"),
        Some(Variant::Int(1)),
        "A's script must see `focus_entered`"
    );

    // 2. Tab walks to the next Control in tree order.
    ml.push_event(key_press(Key::Tab, false));
    ml.step(dt);
    assert!(
        ml.tree().has_focus(b),
        "Tab must move focus A -> B, got {:?}",
        ml.tree().focus_owner()
    );
    assert_eq!(
        script_var(ml.tree(), a, "focus_out"),
        Some(Variant::Int(1)),
        "A must see `focus_exited` when Tab takes focus away"
    );
    assert_eq!(
        script_var(ml.tree(), b, "focus_in"),
        Some(Variant::Int(1)),
        "B must see `focus_entered`"
    );

    // 3. With `focus_next` set, Tab from A must jump past B to C.
    gdscene::control::set_focus_next(ml.tree_mut(), a, "../C");
    ml.push_event(press(40.0, 15.0));
    ml.step(dt);
    assert!(ml.tree().has_focus(a), "click A again to re-arm the walk");
    ml.push_event(key_press(Key::Tab, false));
    ml.step(dt);
    assert!(
        ml.tree().has_focus(c),
        "`focus_next` must beat tree order, got {:?}",
        ml.tree().focus_owner()
    );
    assert_eq!(
        script_var(ml.tree(), b, "focus_in"),
        Some(Variant::Int(1)),
        "B must be skipped by the override, not visited a second time"
    );

    // 4. `ui_accept` operates the focused button. Godot's default
    // ACTION_MODE_BUTTON_RELEASE fires `pressed` on the release half.
    ml.push_event(key_press(Key::Enter, false));
    ml.push_event(key_release(Key::Enter));
    ml.step(dt);
    assert_eq!(
        script_var(ml.tree(), c, "presses"),
        Some(Variant::Int(1)),
        "Enter on the focused button must emit `pressed`"
    );
    assert_eq!(
        script_var(ml.tree(), a, "presses"),
        Some(Variant::Int(0)),
        "only the focus owner is pressed by ui_accept"
    );
}

/// Tab walks the Controls in tree order, and wraps at the end.
#[test]
fn tab_walks_tree_order_and_wraps() {
    let (mut tree, btn, edit, _label) = focus_scene();

    tree.grab_focus(btn);
    assert!(tree.focus_next(), "Tab must find the LineEdit");
    assert!(tree.has_focus(edit));

    // Only Button and LineEdit are focusable (the Label is FOCUS_NONE), so the
    // next step wraps back to the Button rather than stalling on the Label.
    assert!(tree.focus_next());
    assert!(
        tree.has_focus(btn),
        "the tab ring must wrap past the FOCUS_NONE Label"
    );
}

/// Shift-Tab is the reverse walk.
#[test]
fn shift_tab_walks_backwards() {
    let (mut tree, btn, edit, _label) = focus_scene();

    tree.grab_focus(edit);
    assert!(tree.focus_prev());
    assert!(tree.has_focus(btn), "Shift-Tab steps back to the Button");

    assert!(tree.focus_prev());
    assert!(tree.has_focus(edit), "and wraps around the end backwards");
}

/// A `FOCUS_CLICK` control can be clicked into but is not in the tab ring —
/// Godot's `find_next_valid_focus` accepts only `FOCUS_ALL`.
#[test]
fn tab_skips_focus_click_controls() {
    let (mut tree, btn, edit, _label) = focus_scene();
    gdscene::control::set_focus_mode(&mut tree, edit, gdscene::control::FocusMode::Click);

    tree.grab_focus(btn);
    assert!(
        !tree.focus_next(),
        "with the LineEdit click-only and the Label FOCUS_NONE, Tab has nowhere \
         to go"
    );
    assert!(tree.has_focus(btn), "focus must stay put, not be dropped");

    // The same control is still clickable into.
    tree.push_input(&press(140.0, 15.0));
    assert!(tree.has_focus(edit), "FOCUS_CLICK still takes click focus");
}

/// A hidden control is out of the tab ring (Godot checks `is_visible_in_tree`).
#[test]
fn tab_skips_hidden_controls() {
    let (mut tree, btn, edit, _label) = focus_scene();
    gdscene::node2d::set_visible(&mut tree, edit, false);

    tree.grab_focus(btn);
    assert!(!tree.focus_next(), "a hidden LineEdit is not a tab target");
    assert!(!tree.has_focus(edit));
}

/// An override pointing at something that cannot take focus is ignored, and the
/// walk falls back to tree order — Godot only *attempts* the override.
#[test]
fn focus_next_override_to_a_non_focusable_falls_back_to_tree_order() {
    let (mut tree, btn, edit, _label) = focus_scene();
    // The Label is FOCUS_NONE, so this override cannot be honored.
    gdscene::control::set_focus_next(&mut tree, btn, "../Title");

    tree.grab_focus(btn);
    assert!(tree.focus_next());
    assert!(
        tree.has_focus(edit),
        "an unusable `focus_next` must not black-hole the tab walk"
    );
}

/// With nothing focused, Tab focuses the first focusable Control.
#[test]
fn tab_with_nothing_focused_lands_on_the_first_focusable() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    assert_eq!(tree.focus_owner(), None);

    tree.push_input(&key_press(Key::Tab, false));
    assert!(
        tree.has_focus(btn),
        "Tab into a fresh window focuses the first"
    );
}

/// Arrow keys walk geometrically: the controls sit left-to-right, so `Right`
/// from the Button lands on the LineEdit, and `Left` walks back the other way.
#[test]
fn arrow_keys_navigate_to_the_nearest_neighbor() {
    let (mut tree, btn, edit, _label) = focus_scene();
    tree.grab_focus(btn);

    tree.push_input(&key_press(Key::Right, false));
    assert!(
        tree.has_focus(edit),
        "Right must reach the control to the right, got {:?}",
        tree.focus_owner()
    );

    // The return leg uses two Buttons rather than walking back out of the
    // LineEdit. Godot's LineEdit consumes Left/Right itself for caret movement
    // and calls `accept_event()` unconditionally — see
    // upstream/godot/scene/gui/line_edit.cpp:1010-1023, where
    // `ui_text_caret_left`/`_right` both `accept_event(); return;` with no
    // boundary exemption — so an arrow never reaches the focus ring while a
    // text field holds focus (pat-0q2kx.87 made Patina match). The geometric
    // claim this test is about is unchanged; only the control that proves the
    // leftward half is.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let left = tree
        .add_child(main, control("Back", "Button", 0.0, 0.0, 80.0, 30.0))
        .unwrap();
    let right = tree
        .add_child(main, control("Next", "Button", 100.0, 0.0, 80.0, 30.0))
        .unwrap();
    tree.grab_focus(right);

    tree.push_input(&key_press(Key::Left, false));
    assert!(
        tree.has_focus(left),
        "Left must come back to the control on the left, got {:?}",
        tree.focus_owner()
    );
}

/// A `focus_neighbor_*` override beats geometry — the LineEdit is to the
/// *right*, but the override sends `Down` to it.
#[test]
fn focus_neighbor_override_beats_geometry() {
    let (mut tree, btn, edit, _label) = focus_scene();
    gdscene::control::set_focus_neighbor(&mut tree, btn, FocusSide::Bottom, "../Name");

    tree.grab_focus(btn);
    tree.push_input(&key_press(Key::Down, false));
    assert!(
        tree.has_focus(edit),
        "`focus_neighbor_bottom` must redirect Down, got {:?}",
        tree.focus_owner()
    );
}

/// An arrow with no neighbor must fall through to `_unhandled_input`: a game
/// that reads movement keys cannot go dead just because a Control holds focus.
#[test]
fn arrow_with_no_neighbor_is_left_for_gameplay() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    tree.grab_focus(btn);

    tree.push_input(&key_press(Key::Left, false));
    assert!(
        !tree.is_input_handled(),
        "nothing is left of the Button, so the GUI stage must not swallow Left"
    );
    assert!(tree.has_focus(btn));

    tree.push_input(&key_press(Key::Right, false));
    assert!(
        tree.is_input_handled(),
        "a completed focus move does consume the event"
    );
}

/// Key events go to the focus owner, not to whatever the mouse is over —
/// Godot's `gui.key_focus` routing.
#[test]
fn key_events_reach_the_focus_owners_gui_input() {
    let (mut tree, btn, edit, _label) = focus_scene();
    let src = "\
extends LineEdit
var keys = 0
func _gui_input(event):
    self.keys = self.keys + 1
";
    tree.attach_script(
        edit,
        Box::new(GDScriptNodeInstance::from_source(src, edit).unwrap()),
    );

    // Hover the Button, focus the LineEdit: the key must follow focus.
    tree.push_input(&press(40.0, 15.0));
    assert!(tree.has_focus(btn));
    tree.grab_focus(edit);
    tree.push_input(&key_press(Key::A, false));

    assert!(
        matches!(script_var(&tree, edit, "keys"), Some(Variant::Int(n)) if n >= 1),
        "the focused LineEdit must receive `_gui_input` for the key, got {:?}",
        script_var(&tree, edit, "keys")
    );
}

/// The script bridge: a GDScript UI controller can ask `has_focus()` and call
/// `grab_focus()` on itself, the standard Godot idiom for focusing a menu's
/// default button on load.
#[test]
fn scripts_can_query_and_grab_focus() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let btn = tree
        .add_child(main, control("Play", "Button", 0.0, 0.0, 80.0, 30.0))
        .unwrap();
    let other = tree
        .add_child(main, control("Quit", "Button", 100.0, 0.0, 80.0, 30.0))
        .unwrap();

    let src = "\
extends Button
var grabbed = 0
var focused_frames = 0
func _process(delta):
    if self.grabbed == 0:
        self.grabbed = 1
        self.grab_focus()
    if self.has_focus():
        self.focused_frames = self.focused_frames + 1
";
    tree.attach_script(
        btn,
        Box::new(GDScriptNodeInstance::from_source(src, btn).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert!(
        ml.tree().has_focus(btn),
        "`grab_focus()` from a script must move the viewport's focus owner"
    );
    assert!(
        matches!(script_var(ml.tree(), btn, "focused_frames"), Some(Variant::Int(n)) if n >= 1),
        "`has_focus()` must report true right after the script grabbed it, got {:?}",
        script_var(ml.tree(), btn, "focused_frames")
    );
    assert!(!ml.tree().has_focus(other));
}

/// An explicit `focus_mode = 0` overrides the class-based default, so a Button
/// authored as FOCUS_NONE does not take focus.
#[test]
fn explicit_focus_mode_none_overrides_the_class_default() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    tree.get_node_mut(btn)
        .unwrap()
        .set_property("focus_mode", Variant::Int(0));

    tree.push_input(&press(40.0, 15.0));
    assert_eq!(
        tree.focus_owner(),
        None,
        "an explicit focus_mode = FOCUS_NONE must beat the class default"
    );
}
