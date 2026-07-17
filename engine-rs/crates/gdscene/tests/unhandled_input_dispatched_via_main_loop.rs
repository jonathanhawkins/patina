//! pat-nzgdu.83 — input events propagate to script virtuals through MainLoop.
//!
//! Before this bead there was no input dispatch at all: `MainLoop::step` only
//! bridged `InputState` into a pollable `InputSnapshot`, so a script could ask
//! `Input.is_action_pressed()` but `_input`/`_unhandled_input` were never
//! called, and `set_input_as_handled()` did not exist. Godot instead routes each
//! event through `Viewport::push_input` in stage order — `_input` →
//! `_shortcut_input` → `_unhandled_key_input` → `_unhandled_input` — with
//! `set_input_as_handled()` short-circuiting the later stages.
//!
//! These tests drive real GDScript through the real frame loop (`push_event` →
//! `MainLoop::step` → `SceneTree::push_input`), which is the path the runner and
//! editor take, rather than calling the dispatcher directly.

use gdplatform::input::{InputEvent, Key};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A key press, the event kind that reaches every propagation stage.
fn key_press(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

/// Reads a script variable off the node's attached script instance.
fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
}

/// Acceptance (a): `_unhandled_input(event)` fires on a scripted node when a key
/// event is pushed and `MainLoop::step` runs, and (b) a sibling calling
/// `set_input_as_handled()` from `_input` stops that same event from ever
/// reaching `_unhandled_input`.
#[test]
fn unhandled_input_dispatched_via_main_loop() {
    // -- (a) the event reaches _unhandled_input through a real frame --------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let listener_id = tree
        .add_child(root, Node::new("Listener", "Node"))
        .unwrap();

    // Records that the virtual ran and what the event payload carried.
    let listener_src = "\
extends Node
var saw = false
var keycode = \"\"
func _unhandled_input(event):
    self.saw = true
    self.keycode = event[\"keycode\"]
";
    tree.attach_script(
        listener_id,
        Box::new(GDScriptNodeInstance::from_source(listener_src, listener_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.push_event(key_press(Key::A));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, listener_id, "saw"),
        Some(Variant::Bool(true)),
        "_unhandled_input must fire when a key event is pushed and step() runs",
    );
    assert_eq!(
        script_var(&ml, listener_id, "keycode"),
        Some(Variant::String("A".to_string())),
        "_unhandled_input must receive the real event, not an empty placeholder",
    );

    // -- (b) set_input_as_handled() in _input suppresses the sibling --------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let blocker_id = tree.add_child(root, Node::new("Blocker", "Node")).unwrap();
    let listener_id = tree
        .add_child(root, Node::new("Listener", "Node"))
        .unwrap();

    let blocker_src = "\
extends Node
var ran = false
func _input(event):
    self.ran = true
    set_input_as_handled()
";
    tree.attach_script(
        blocker_id,
        Box::new(GDScriptNodeInstance::from_source(blocker_src, blocker_id).unwrap()),
    );
    tree.attach_script(
        listener_id,
        Box::new(GDScriptNodeInstance::from_source(listener_src, listener_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.push_event(key_press(Key::A));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, blocker_id, "ran"),
        Some(Variant::Bool(true)),
        "_input must run (otherwise the suppression below proves nothing)",
    );
    assert_eq!(
        script_var(&ml, listener_id, "saw"),
        Some(Variant::Bool(false)),
        "set_input_as_handled() in _input must stop the sibling's _unhandled_input",
    );
}

/// Without a blocker the same sibling layout DOES reach `_unhandled_input`.
/// This is the control for the suppression assertion above: it proves the
/// sibling is silenced by `set_input_as_handled()` and not by a dispatch bug.
#[test]
fn unhandled_input_reaches_sibling_when_not_handled() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let quiet_id = tree.add_child(root, Node::new("Quiet", "Node")).unwrap();
    let listener_id = tree
        .add_child(root, Node::new("Listener", "Node"))
        .unwrap();

    // Same shape as the blocker, but it does NOT mark the event handled.
    let quiet_src = "\
extends Node
var ran = false
func _input(event):
    self.ran = true
";
    let listener_src = "\
extends Node
var saw = false
func _unhandled_input(event):
    self.saw = true
";
    tree.attach_script(
        quiet_id,
        Box::new(GDScriptNodeInstance::from_source(quiet_src, quiet_id).unwrap()),
    );
    tree.attach_script(
        listener_id,
        Box::new(GDScriptNodeInstance::from_source(listener_src, listener_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.push_event(key_press(Key::A));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, quiet_id, "ran"),
        Some(Variant::Bool(true)),
        "_input must run",
    );
    assert_eq!(
        script_var(&ml, listener_id, "saw"),
        Some(Variant::Bool(true)),
        "an unhandled event must still reach _unhandled_input",
    );
}

/// Events are consumed by the step that dispatches them: a second step with no
/// new events must not re-deliver the old one (Godot does not replay events).
#[test]
fn input_events_are_not_redelivered_on_later_steps() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let counter_id = tree
        .add_child(root, Node::new("Counter", "Node"))
        .unwrap();

    let counter_src = "\
extends Node
var count = 0
func _unhandled_input(event):
    self.count = self.count + 1
";
    tree.attach_script(
        counter_id,
        Box::new(GDScriptNodeInstance::from_source(counter_src, counter_id).unwrap()),
    );

    let mut ml = MainLoop::new(tree);
    ml.push_event(key_press(Key::A));
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, counter_id, "count"),
        Some(Variant::Int(1)),
        "a pushed event must be dispatched exactly once, not once per step",
    );
}
