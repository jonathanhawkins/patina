//! pat-s3rrv.14 — analog action strength reaches a running script.
//!
//! # What was wrong
//!
//! `gdplatform::input::InputState` computes `get_action_strength` and
//! `get_action_raw_strength` correctly, deadzone remap and all — and **nothing
//! read them**. The script-facing `InputSnapshot` carried key names, digital
//! action sets and raw axis values, but no per-action strengths, so the
//! `SceneAccess` trait DEFAULTS answered every query with a digital `1.0`/`0.0`.
//!
//! The engine `InputState` belongs to `MainLoop`, which a running script cannot
//! reach, so an override on the accessor alone could not have fixed it: there
//! was no analog data on that side of the bridge to return. The fix carries the
//! pair across `MainLoop`'s `InputState -> InputSnapshot` conversion.
//!
//! ⚠️ Why this needs a GAMEPAD to test at all: `strength` and `raw_strength`
//! differ **only** for analog bindings, and only once a deadzone applies. A
//! digital binding reports `1.0` from both, so an implementation that simply
//! aliases the two passes every keyboard-shaped test and is wrong on every pad.

use gdplatform::input::{ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// The stick deflection and deadzone used throughout, kept as constants so the
/// expected strength below is DERIVED from Godot's remap rather than pasted in
/// as a magic 0.56.
const DEFLECTION: f32 = 0.6;
const DEADZONE: f32 = 0.1;

/// Godot's deadzone remap: strength rises from 0 AT the deadzone to 1 at full
/// deflection, instead of jumping to the deadzone's own value
/// (`gdplatform::input::axis_match`).
fn expected_strength() -> f32 {
    (DEFLECTION - DEADZONE) / (1.0 - DEADZONE)
}

const OBSERVER: &str = "\
extends Node
var aim_strength = -1.0
var aim_raw = -1.0
var jump_strength = -1.0
var jump_raw = -1.0
func _process(delta):
    self.aim_strength = Input.get_action_strength(\"aim\")
    self.aim_raw = Input.get_action_raw_strength(\"aim\")
    self.jump_strength = Input.get_action_strength(\"jump\")
    self.jump_raw = Input.get_action_raw_strength(\"jump\")
";

fn num(tree: &SceneTree, node: NodeId, prop: &str) -> f32 {
    match tree
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
    {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        other => panic!("expected a number for `{prop}`, got {other:?}"),
    }
}

/// "aim" is ANALOG (a stick direction under a deadzone); "jump" is DIGITAL (a
/// face button). Both in one scene so the analog and digital cases are observed
/// through the identical accessor.
fn scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let observer = tree.add_child(root, Node::new("Observer", "Node")).unwrap();
    tree.attach_script(
        observer,
        Box::new(GDScriptNodeInstance::from_source(OBSERVER, observer).expect("observer parses")),
    );

    let mut map = InputMap::new();
    map.add_action("aim", DEADZONE);
    map.action_add_event(
        "aim",
        ActionBinding::GamepadAxisDirection {
            axis: GamepadAxis::LeftStickX,
            positive: true,
        },
    );
    map.add_action("jump", 0.5);
    map.action_add_event(
        "jump",
        ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
    );

    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);
    (ml, observer)
}

/// **The headline.** A stick at 0.6 under a 0.1 deadzone reports the remapped
/// strength and the raw deflection, and the two DIFFER.
#[test]
fn an_analog_action_reports_a_deadzone_remapped_strength_to_a_script() {
    let (mut ml, observer) = scene();

    // Baseline first: an idle pad must read 0.0, or a pass below could come
    // from default state rather than from the stick.
    ml.step(1.0 / 60.0);
    assert_eq!(num(ml.tree(), observer, "aim_strength"), 0.0, "idle stick");
    assert_eq!(num(ml.tree(), observer, "aim_raw"), 0.0, "idle stick, raw");

    ml.push_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: DEFLECTION,
        gamepad_id: 0,
    });
    ml.step(1.0 / 60.0);

    let strength = num(ml.tree(), observer, "aim_strength");
    let raw = num(ml.tree(), observer, "aim_raw");

    assert!(
        (strength - expected_strength()).abs() < 1e-4,
        "strength must be the deadzone-remapped (0.6-0.1)/(1-0.1) = {}, got {strength}. \
         Reading 1.0 means the SceneAccess digital default answered — the snapshot \
         never carried the analog value",
        expected_strength()
    );
    assert!(
        (raw - DEFLECTION).abs() < 1e-4,
        "raw strength must IGNORE the deadzone and report the deflection {DEFLECTION}, \
         got {raw}"
    );

    // ⚠️ The assertion that actually catches an aliased implementation: the two
    // must not be equal here. Every other assertion in this file would still
    // pass if `get_action_raw_strength` simply forwarded to `get_action_strength`
    // and the remap were skipped.
    assert!(
        (strength - raw).abs() > 1e-3,
        "strength ({strength}) and raw ({raw}) must DIVERGE for an analog binding \
         under a deadzone; equal values mean one accessor is forwarding to the other"
    );
}

/// The digital control: a face button reports 1.0 from BOTH accessors.
///
/// This is what keeps the test above honest in the other direction — it shows
/// the divergence is a property of analog bindings, not of the two accessors
/// disagreeing generally, and it exercises the fallback path for an action the
/// snapshot carries no strength entry for.
#[test]
fn a_digital_action_reports_one_from_both_accessors() {
    let (mut ml, observer) = scene();
    ml.push_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        num(ml.tree(), observer, "jump_strength"),
        1.0,
        "a held digital action is full strength"
    );
    assert_eq!(
        num(ml.tree(), observer, "jump_raw"),
        1.0,
        "and reports the same raw strength — a digital binding has no deadzone \
         remap to apply"
    );
}
