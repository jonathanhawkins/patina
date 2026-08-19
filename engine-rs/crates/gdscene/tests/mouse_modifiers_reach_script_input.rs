//! pat-s3rrv.14 — mouse and gesture events carry the real modifier state.
//!
//! `InputEventMouseButton` inherits `InputEventMouse` inherits
//! **`InputEventWithModifiers`** (`doc/classes/InputEventMouse.xml:2`), so
//! `event.shift_pressed` on a click is real state in Godot. Patina reported all
//! four modifiers as `false` on every mouse and gesture event — the four
//! `modifier_props(false, false, false)` calls in `scene_tree.rs` — while only
//! the key arm passed real values.
//!
//! ⚠️ The consequence is not cosmetic: **no script could implement shift-click
//! or ctrl-click.** Multi-select, add-to-selection, and every other
//! modifier-qualified click read as an unmodified click.
//!
//! ⚠️ The state is mirrored from KEY events rather than carried on the mouse
//! event, because `InputEvent::MouseButton` has 71 construction sites across six
//! crates — the same reason `double_click` is a side channel (pat-s3rrv.76). A
//! modifier key press is itself a key event carrying its own flag, so holding
//! Shift and clicking with nothing in between still works — which is exactly
//! what `a_click_while_shift_is_held_reports_shift_pressed` drives.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, Key, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Records the modifier flags of every mouse click it sees.
const SRC: &str = "\
extends Node
var clicks = 0
var shift = -1
var ctrl = -1
var alt = -1
func _input(event):
    if event is InputEventMouseButton:
        self.clicks = self.clicks + 1
        self.shift = 1 if event.shift_pressed else 0
        self.ctrl = 1 if event.ctrl_pressed else 0
        self.alt = 1 if event.alt_pressed else 0
";

fn listener_loop() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(SRC, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    // One warm-up frame, matching the sibling input tests: the tree's input
    // mirrors refresh after the script phase.
    ml.step(1.0 / 60.0);
    (ml, node)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn key(k: Key, pressed: bool, shift: bool, ctrl: bool, alt: bool) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed,
        shift,
        ctrl,
        alt,
    }
}

fn click() -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(10.0, 10.0),
    }
}

/// 🔑 A click while Shift is held reports `shift_pressed`.
#[test]
fn a_click_while_shift_is_held_reports_shift_pressed() {
    let (mut ml, node) = listener_loop();

    // Holding Shift is itself a key event, and the platform stamps the modifier
    // flags onto it (the web bridge reads `shiftKey` off the browser event).
    ml.push_event(key(Key::Shift, true, true, false, false));
    ml.push_event(click());
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "clicks"),
        Variant::Int(1),
        "the click must reach _input at all",
    );
    assert_eq!(
        script_var(&ml, node, "shift"),
        Variant::Int(1),
        "shift-click must report shift_pressed = true; 0 means the mouse arm is \
         still reporting the hardcoded false, which makes shift-click \
         indistinguishable from a plain click for every script",
    );
    // ⚠️ The unheld modifiers must stay false. An implementation that stamped
    // "some modifier is down" onto all three would satisfy the assertion above.
    assert_eq!(script_var(&ml, node, "ctrl"), Variant::Int(0));
    assert_eq!(script_var(&ml, node, "alt"), Variant::Int(0));
}

/// The negative control: an unmodified click reports all three false.
///
/// Without this, hardcoding `true` would pass the test above.
#[test]
fn a_plain_click_reports_no_modifiers() {
    let (mut ml, node) = listener_loop();
    ml.push_event(click());
    ml.step(1.0 / 60.0);

    assert_eq!(script_var(&ml, node, "clicks"), Variant::Int(1));
    assert_eq!(script_var(&ml, node, "shift"), Variant::Int(0));
    assert_eq!(script_var(&ml, node, "ctrl"), Variant::Int(0));
    assert_eq!(script_var(&ml, node, "alt"), Variant::Int(0));
}

/// Releasing the modifier clears it for the next click.
///
/// ⚠️ This is what separates a mirror from a latch. A first click that reports
/// shift and a second that still reports it after release is worse than never
/// reporting it — the script would keep multi-selecting after the user let go.
#[test]
fn releasing_the_modifier_clears_it_for_the_next_click() {
    let (mut ml, node) = listener_loop();

    ml.push_event(key(Key::Shift, true, true, false, false));
    ml.push_event(click());
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "shift"), Variant::Int(1));

    // The release carries `shift: false` — the platform reports the state AFTER
    // the key went up, which is what makes a release usable as the clear.
    ml.push_event(key(Key::Shift, false, false, false, false));
    ml.push_event(click());
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "clicks"),
        Variant::Int(2),
        "the second click must have been delivered",
    );
    assert_eq!(
        script_var(&ml, node, "shift"),
        Variant::Int(0),
        "the modifier must clear on release; a latched value keeps every later \
         click looking like a shift-click",
    );
}

/// Ctrl and Alt travel the same path, and independently.
#[test]
fn ctrl_and_alt_are_reported_independently() {
    let (mut ml, node) = listener_loop();

    ml.push_event(key(Key::Ctrl, true, false, true, false));
    ml.push_event(click());
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "ctrl"), Variant::Int(1));
    assert_eq!(
        script_var(&ml, node, "shift"),
        Variant::Int(0),
        "ctrl must not set shift — a single 'any modifier' flag would fail here",
    );

    ml.push_event(key(Key::Alt, true, false, false, true));
    ml.push_event(click());
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "alt"), Variant::Int(1));
    assert_eq!(script_var(&ml, node, "ctrl"), Variant::Int(0));
}
