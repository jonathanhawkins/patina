//! pat-0q2kx.18: the GUI stage of `SceneTree::push_input`.
//!
//! Godot's `Viewport::push_input` runs a GUI stage between `_input` and the
//! shortcut/unhandled stages (upstream/godot/scene/main/viewport.cpp
//! `_gui_input_event`). Patina had the hit-test math (`control_at_point`,
//! `click_at_point`) with green unit tests but **no runtime caller**:
//! `push_input` went straight from `_input` to `_shortcut_input`, so a real
//! mouse event never reached a Control. `gui_input`, `mouse_entered`,
//! `mouse_exited` and button presses could not fire from input at all.
//!
//! These push real `InputEvent`s through `push_input` and assert on
//! script-observable effects, so "the hit test works but nothing calls it"
//! fails.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A Control node with an explicit rect at `(x, y)` sized `(w, h)`.
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

fn release(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(x, y),
    }
}

fn motion(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMotion {
        position: Vector2::new(x, y),
        relative: Vector2::ZERO,
    }
}

fn script_var(tree: &SceneTree, node: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// `Main -> Button`, with `Main`'s script connected to the button's signals.
fn button_scene(disabled: bool) -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut btn = control("Play", "Button", 10.0, 10.0, 100.0, 40.0);
    if disabled {
        btn.set_property("disabled", Variant::Bool(true));
    }
    let btn_id = tree.add_child(main, btn).unwrap();

    // The handler lives on the button itself so `_gui_input` and the signal
    // connections share one script.
    let src = "\
extends Button
var clicks = 0
var entered = 0
var exited = 0
var gui_events = 0
func _on_pressed():
    self.clicks = self.clicks + 1
func _on_entered():
    self.entered = self.entered + 1
func _on_exited():
    self.exited = self.exited + 1
func _gui_input(event):
    self.gui_events = self.gui_events + 1
";
    tree.attach_script(
        btn_id,
        Box::new(GDScriptNodeInstance::from_source(src, btn_id).unwrap()),
    );
    // Wire the signals from Rust rather than from a `_ready` + `connect(...)`
    // in GDScript. What this bead must prove is that the GUI stage *emits*
    // these signals; routing that assertion through the script's own connect
    // path would make an unrelated mechanism part of the failure surface.
    for (signal, method) in [
        ("pressed", "_on_pressed"),
        ("mouse_entered", "_on_entered"),
        ("mouse_exited", "_on_exited"),
    ] {
        tree.connect_signal(
            btn_id,
            signal,
            gdobject::signal::Connection::new(btn_id.object_id(), method),
        );
    }
    (tree, main, btn_id)
}

/// **The core wiring test.** A press+release inside a Button's rect reaches the
/// GUI stage, runs `_gui_input`, and fires `pressed` through to the script.
#[test]
fn gui_input_button_click_routes_to_script() {
    let (mut tree, _main, btn) = button_scene(false);

    tree.push_input(&press(50.0, 25.0));
    tree.push_input(&release(50.0, 25.0));

    // Assert the GUI stage itself FIRST. `_gui_input` is called directly by the
    // stage and needs no signal wiring, so it isolates "the event reached the
    // control" from "the script's `connect` in `_ready` worked" — two different
    // mechanisms that the previous ordering conflated.
    assert!(
        matches!(script_var(&tree, btn, "gui_events"), Some(Variant::Int(n)) if n >= 2),
        "stage: the button's `_gui_input` must receive both the press and the \
         release (this needs no connect/_ready), got {:?}",
        script_var(&tree, btn, "gui_events")
    );
    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(1)),
        "signal: press+release inside the rect must fire `pressed` through the \
         script's connect()"
    );
}

/// A click outside the rect hits nothing — the stage must not fire blindly.
#[test]
fn click_outside_the_control_does_nothing() {
    let (mut tree, _main, btn) = button_scene(false);

    tree.push_input(&press(500.0, 500.0));
    tree.push_input(&release(500.0, 500.0));

    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(0)),
        "a click over empty space must not press the button"
    );
    assert_eq!(
        script_var(&tree, btn, "gui_events"),
        Some(Variant::Int(0)),
        "_gui_input must not run for a control that was not hit"
    );
}

/// A disabled button swallows the click, as Godot does.
#[test]
fn disabled_button_emits_nothing() {
    let (mut tree, _main, btn) = button_scene(true);

    tree.push_input(&press(50.0, 25.0));
    tree.push_input(&release(50.0, 25.0));

    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(0)),
        "a disabled button must not emit `pressed`"
    );
}

/// Releasing off the control cancels the click — press and release must land on
/// the same Control.
#[test]
fn release_outside_cancels_the_click() {
    let (mut tree, _main, btn) = button_scene(false);

    tree.push_input(&press(50.0, 25.0));
    tree.push_input(&release(500.0, 500.0));

    assert_eq!(
        script_var(&tree, btn, "clicks"),
        Some(Variant::Int(0)),
        "dragging off the button before release must cancel the click"
    );
}

/// Hover transitions fire once per crossing, not per event.
#[test]
fn hover_transitions_emit_entered_and_exited_once() {
    let (mut tree, _main, btn) = button_scene(false);

    // Move in, then again inside: one `mouse_entered`.
    tree.push_input(&motion(50.0, 25.0));
    tree.push_input(&motion(60.0, 30.0));
    assert_eq!(
        script_var(&tree, btn, "entered"),
        Some(Variant::Int(1)),
        "mouse_entered fires once on entry, not per motion event"
    );
    assert_eq!(tree.hovered_control(), Some(btn));

    // Move out: one `mouse_exited`, and the hover clears.
    tree.push_input(&motion(500.0, 500.0));
    assert_eq!(
        script_var(&tree, btn, "exited"),
        Some(Variant::Int(1)),
        "mouse_exited fires when the pointer leaves the control"
    );
    assert_eq!(tree.hovered_control(), None);
}

/// `MOUSE_FILTER_PASS` bubbles the event to the ancestor Control; the default
/// `STOP` does not.
#[test]
fn mouse_filter_pass_bubbles_to_the_ancestor() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Parent panel covering the child.
    let panel = tree
        .add_child(root, control("Panel", "Panel", 0.0, 0.0, 200.0, 200.0))
        .unwrap();
    let mut child = control("Inner", "ColorRect", 10.0, 10.0, 50.0, 50.0);
    // MouseFilter::Pass == 1 (Godot MOUSE_FILTER_PASS).
    child.set_property("mouse_filter", Variant::Int(1));
    let child_id = tree.add_child(panel, child).unwrap();

    let counter = "\
extends Control
var gui_events = 0
func _gui_input(event):
    self.gui_events = self.gui_events + 1
";
    tree.attach_script(
        panel,
        Box::new(GDScriptNodeInstance::from_source(counter, panel).unwrap()),
    );
    tree.attach_script(
        child_id,
        Box::new(GDScriptNodeInstance::from_source(counter, child_id).unwrap()),
    );

    tree.push_input(&press(20.0, 20.0));

    assert!(
        matches!(script_var(&tree, child_id, "gui_events"), Some(Variant::Int(n)) if n >= 1),
        "the PASS child receives the event first"
    );
    assert!(
        matches!(script_var(&tree, panel, "gui_events"), Some(Variant::Int(n)) if n >= 1),
        "a PASS child must bubble the event to its ancestor Control"
    );
}
