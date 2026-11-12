//! pat-0q2kx.88 — a browser gamepad reaches a running scene.
//!
//! Godot's web build polls `navigator.getGamepads()` each frame and converts
//! standard-mapping state into JoypadButton / JoypadMotion events that feed the
//! action system, `Input.is_joy_button_pressed()` and `Input.get_joy_axis()`
//! (platform/web/js/libs/library_godot_input.js, core/input/input.cpp).
//!
//! Patina had the normalizer (`InputState::apply_standard_gamepad_frame`) with
//! green unit tests and **nothing downstream of it**:
//!
//! * `InputSnapshot` — the only thing a script can read — carried no gamepad
//!   state at all, so `Input.is_joy_button_pressed` had nothing to answer from;
//! * its action map was built from **key names**, so an action bound to a
//!   gamepad button could never report pressed;
//! * and `MainLoop::step` only built a snapshot when a key, a mouse button or a
//!   mouse position was present — so a **gamepad-only frame produced no
//!   snapshot whatsoever** and a script saw nothing however well the transport
//!   worked.
//!
//! These drive a real `MainLoop` and assert what a script observes.

use gdplatform::input::{ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Godot `JoyButton` / `JoyAxis` values (core/input/input_enums.h).
const JOY_BUTTON_A: i64 = 0;
const JOY_BUTTON_DPAD_UP: i64 = 11;
const JOY_AXIS_LEFT_X: i64 = 0;

/// Reads `Input` through the script-facing API a real game would use.
const OBSERVER: &str = "\
extends Node
var a_pressed = false
var dpad_up_pressed = false
var axis_x = 0.0
var jump_just = false
var input_events = 0
func _process(delta):
    if Input.is_joy_button_pressed(0, 0):
        self.a_pressed = true
    if Input.is_joy_button_pressed(0, 11):
        self.dpad_up_pressed = true
    self.axis_x = Input.get_joy_axis(0, 0)
    if Input.is_action_just_pressed(\"jump\"):
        self.jump_just = true
func _input(event):
    self.input_events = self.input_events + 1
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Builds a one-node scene whose script is [`OBSERVER`], with "jump" bound to
/// the pad's A button — the binding a gamepad game actually ships.
fn scene_with_observer() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let observer = tree.add_child(root, Node::new("Observer", "Node")).unwrap();
    tree.attach_script(
        observer,
        Box::new(GDScriptNodeInstance::from_source(OBSERVER, observer).expect("observer parses")),
    );

    let mut map = InputMap::new();
    map.add_action("jump", 0.5);
    map.action_add_event(
        "jump",
        ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
    );

    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);
    (ml, observer)
}

/// The headline: a standard-mapping button press reaches the script's
/// `Input.is_joy_button_pressed`, the bound action, and `_input`.
#[test]
fn gamepad_button_reaches_script_queries_actions_and_input() {
    let (mut ml, observer) = scene_with_observer();

    // Baseline — nothing pressed, so a pass cannot come from default state.
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "a_pressed"),
        Variant::Bool(false),
        "no pad activity yet"
    );

    // One poll of the producer route's normalizer, plus the edge event the
    // route pushes for the transition.
    ml.push_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    ml.input_state_mut()
        .apply_standard_gamepad_frame(0, &[true], &[]);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_prop(ml.tree(), observer, "a_pressed"),
        Variant::Bool(true),
        "Input.is_joy_button_pressed(0, JOY_BUTTON_A) must be true for a held A button"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "jump_just"),
        Variant::Bool(true),
        "an action bound to JOY_BUTTON_A must report just_pressed to the script"
    );
    // Exactly one event was pushed this frame, so a single `_input` call is
    // proof the JoypadButton event was dispatched to the script.
    assert_eq!(
        script_prop(ml.tree(), observer, "input_events"),
        Variant::Int(1),
        "_input must receive the JoypadButton event exactly once"
    );
}

/// A frame with *only* gamepad activity must still produce a snapshot. This is
/// the regression that made every other clause unobservable: `has_input` used
/// to test keys, mouse buttons and mouse position only.
#[test]
fn gamepad_only_frame_still_produces_a_snapshot() {
    let (mut ml, observer) = scene_with_observer();

    // No key, no mouse button, no mouse movement — only a pad.
    ml.input_state_mut()
        .apply_standard_gamepad_frame(0, &[true], &[]);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_prop(ml.tree(), observer, "a_pressed"),
        Variant::Bool(true),
        "a gamepad-only frame must still reach the script"
    );
}

/// Stick movement drives `Input.get_joy_axis` with the analog value.
#[test]
fn gamepad_axis_value_reaches_script() {
    let (mut ml, observer) = scene_with_observer();

    ml.input_state_mut()
        .apply_standard_gamepad_frame(0, &[], &[0.8, 0.0]);
    ml.step(1.0 / 60.0);

    let axis = script_prop(ml.tree(), observer, "axis_x");
    match axis {
        Variant::Float(v) => assert!(
            (v - 0.8).abs() < 1e-5,
            "get_joy_axis(0, JOY_AXIS_LEFT_X) should be 0.8, got {v}"
        ),
        other => panic!("axis_x should be a float, got {other:?}"),
    }
}

/// The browser's `buttons[]` order is **not** Godot's `JoyButton` order:
/// standard-mapping index 12 is D-pad Up, which Godot numbers 11. Reading the
/// browser index straight through would silently bind the wrong button.
#[test]
fn standard_mapping_index_is_translated_to_godot_joybutton() {
    // buttons[12] is D-pad Up in the W3C standard mapping.
    let mut buttons = vec![false; 16];
    buttons[12] = true;

    let (mut ml, observer) = scene_with_observer();
    ml.input_state_mut()
        .apply_standard_gamepad_frame(0, &buttons, &[]);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_prop(ml.tree(), observer, "dpad_up_pressed"),
        Variant::Bool(true),
        "browser buttons[12] must reach the script as JOY_BUTTON_DPAD_UP (11)"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "a_pressed"),
        Variant::Bool(false),
        "and must NOT be mistaken for JOY_BUTTON_A"
    );
}

/// The enum conversions themselves, against
/// upstream/godot/core/input/input_enums.h.
#[test]
fn godot_joy_enum_values_match_upstream() {
    assert_eq!(GamepadButton::FaceA.to_godot_index(), Some(JOY_BUTTON_A));
    assert_eq!(GamepadButton::FaceB.to_godot_index(), Some(1));
    assert_eq!(GamepadButton::Select.to_godot_index(), Some(4)); // BACK
    assert_eq!(GamepadButton::Guide.to_godot_index(), Some(5));
    assert_eq!(GamepadButton::Start.to_godot_index(), Some(6));
    assert_eq!(GamepadButton::LeftShoulder.to_godot_index(), Some(9));
    assert_eq!(
        GamepadButton::DPadUp.to_godot_index(),
        Some(JOY_BUTTON_DPAD_UP)
    );
    assert_eq!(GamepadButton::DPadRight.to_godot_index(), Some(14));
    // Godot models the analog triggers as axes, not buttons.
    assert_eq!(GamepadButton::LeftTrigger.to_godot_index(), None);
    assert_eq!(GamepadButton::RightTrigger.to_godot_index(), None);

    // Round trip.
    for index in 0..=14 {
        let button = GamepadButton::from_godot_index(index).expect("valid JoyButton");
        assert_eq!(button.to_godot_index(), Some(index));
    }
    assert_eq!(GamepadButton::from_godot_index(99), None);

    assert_eq!(GamepadAxis::LeftStickX.to_godot_index(), JOY_AXIS_LEFT_X);
    assert_eq!(GamepadAxis::RightStickY.to_godot_index(), 3);
    assert_eq!(GamepadAxis::LeftTriggerAnalog.to_godot_index(), 4);
    assert_eq!(GamepadAxis::RightTriggerAnalog.to_godot_index(), 5);
    for index in 0..=5 {
        let axis = GamepadAxis::from_godot_index(index).expect("valid JoyAxis");
        assert_eq!(axis.to_godot_index(), index);
    }
    assert_eq!(GamepadAxis::from_godot_index(99), None);
}
