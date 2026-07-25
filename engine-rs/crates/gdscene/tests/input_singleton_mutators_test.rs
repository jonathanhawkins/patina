//! pat-0q2kx.24 — `Input` singleton mutators from a script reach the engine.
//!
//! In Godot, `Input.action_press` injects action state that `is_action_pressed`
//! sees the same frame, `set_mouse_mode` changes capture, and
//! `start_joy_vibration` drives rumble (upstream/godot/core/input/input.cpp).
//! Patina had the real `InputState` implementations but no path to them: the
//! `SceneAccess` trait methods were empty default bodies and `SceneTreeAccessor`
//! never overrode them, so every one of these calls silently did nothing while a
//! scene ran.
//!
//! These tests drive a real `MainLoop` — not a mock — and assert the engine state
//! afterwards, plus the same-frame visibility Godot guarantees.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Node A drives every mutator on frame 1. `MOUSE_MODE_CAPTURED` is 2.
const DRIVER: &str = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        Input.action_press(\"jump\")
        Input.set_mouse_mode(2)
        Input.start_joy_vibration(0, 0.5, 0.9, 1.0)
        Input.warp_mouse(Vector2(120.0, 45.0))
";

/// Node B observes the injected action *in the same frame*, which is what Godot
/// guarantees and what a mock cannot prove.
const OBSERVER: &str = "\
extends Node
var saw_just_pressed = false
var saw_pressed = false
var mode_seen = -1
func _process(delta):
    if Input.is_action_just_pressed(\"jump\"):
        self.saw_just_pressed = true
    if Input.is_action_pressed(\"jump\"):
        self.saw_pressed = true
    self.mode_seen = Input.get_mouse_mode()
";

fn script_prop(tree: &SceneTree, node: gdscene::node::NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

#[test]
fn input_singleton_mutators_affect_runtime() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Driver first so it runs before the observer in the same script phase.
    let driver = tree.add_child(root, Node::new("Driver", "Node")).unwrap();
    let observer = tree.add_child(root, Node::new("Observer", "Node")).unwrap();

    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(DRIVER, driver).expect("driver parses")),
    );
    tree.attach_script(
        observer,
        Box::new(GDScriptNodeInstance::from_source(OBSERVER, observer).expect("observer parses")),
    );

    let mut ml = MainLoop::new(tree);

    // Baseline: nothing set yet, so a pass cannot come from default state.
    assert_eq!(
        ml.input_state().mouse_mode(),
        gdplatform::input::MouseMode::Visible
    );
    assert!(ml.input_state().joy_vibration(0).is_none());

    ml.step(1.0 / 60.0);

    // -- same frame: the observer saw the injected action -------------------
    assert_eq!(
        script_prop(ml.tree(), observer, "saw_just_pressed"),
        Variant::Bool(true),
        "Input.action_press must be visible to is_action_just_pressed in the same frame"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "saw_pressed"),
        Variant::Bool(true),
        "Input.action_press must be visible to is_action_pressed in the same frame"
    );

    // -- engine state after the frame --------------------------------------
    assert_eq!(
        ml.input_state().mouse_mode(),
        gdplatform::input::MouseMode::Captured,
        "Input.set_mouse_mode(2) must reach the engine InputState"
    );
    assert_eq!(
        ml.input_state().joy_vibration(0),
        Some((0.5, 0.9, 1.0)),
        "Input.start_joy_vibration must reach the engine InputState"
    );
    assert_eq!(
        ml.input_state().get_mouse_position(),
        gdcore::math::Vector2::new(120.0, 45.0),
        "Input.warp_mouse must move the engine's cursor position"
    );
    assert!(
        ml.input_state().is_action_pressed("jump"),
        "the pressed action must be real engine state, not just a snapshot fiction"
    );

    // -- next frame: still pressed, and the mode read back through a script --
    ml.step(1.0 / 60.0);
    assert!(
        ml.input_state().is_action_pressed("jump"),
        "a pressed action stays pressed until released"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "mode_seen"),
        Variant::Int(2),
        "Input.get_mouse_mode must report the mode a script set"
    );
    assert_eq!(
        ml.tree().pending_input_command_count(),
        0,
        "queued commands must be drained, not accumulated every frame"
    );
}

/// `action_release` must undo a press, so a script can model a tap.
#[test]
fn action_release_clears_the_injected_press() {
    const TAP: &str = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        Input.action_press(\"fire\")
    if self.frame == 2:
        Input.action_release(\"fire\")
";
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Tapper", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(TAP, node).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    assert!(
        ml.input_state().is_action_pressed("fire"),
        "press did not reach the engine"
    );
    ml.step(1.0 / 60.0);
    assert!(
        !ml.input_state().is_action_pressed("fire"),
        "Input.action_release must clear the engine's action state"
    );
}
