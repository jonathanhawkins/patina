//! pat-s3rrv.14 item (d) — touch, drag and MIDI events reach `_input` as real
//! `InputEvent` objects.
//!
//! `input_event_to_variant` builds `InputEventScreenTouch`,
//! `InputEventScreenDrag` and `InputEventMIDI`, but **no test drove one of them
//! through a script**. The three classes that were covered — Key, MouseButton,
//! MouseMotion — are also the three that were rewritten first, so the delivery
//! path was only ever exercised for the events whose payloads had already been
//! debugged.
//!
//! Every value below is deliberately ASYMMETRIC and distinct. A fixture using
//! 0/1 or repeated numbers cannot catch a field that is populated from its
//! neighbour: `relative` vs `velocity` and `pitch` vs `velocity` are adjacent in
//! their constructors and identical in type.

use gdplatform::input::InputEvent;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use gdcore::math::Vector2;

/// Reads each class through the Godot idiom, and records which branch fired so
/// a silently-undelivered event cannot masquerade as a zero value.
const LISTENER: &str = "\
extends Node
var saw_touch = false
var saw_drag = false
var saw_midi = false
var touch_index = -1
var touch_pressed = false
var touch_double = false
var drag_index = -1
var drag_rel_x = -1.0
var drag_vel_x = -1.0
var drag_pressure = -1.0
var midi_channel = -1
var midi_pitch = -1
var midi_velocity = -1
var midi_is_note_on = false
var touch_was_a_drag = false
func _input(event):
    if event is InputEventScreenTouch:
        self.saw_touch = true
        self.touch_index = event.index
        self.touch_pressed = event.pressed
        self.touch_double = event.double_tap
        if event is InputEventScreenDrag:
            self.touch_was_a_drag = true
    elif event is InputEventScreenDrag:
        self.saw_drag = true
        self.drag_index = event.index
        self.drag_rel_x = event.relative.x
        self.drag_vel_x = event.velocity.x
        self.drag_pressure = event.pressure
    elif event is InputEventMIDI:
        self.saw_midi = true
        self.midi_channel = event.channel
        self.midi_pitch = event.pitch
        self.midi_velocity = event.velocity
        if event.message == MIDI_MESSAGE_NOTE_ON:
            self.midi_is_note_on = true
";

fn listener() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(LISTENER, node).expect("script parses")),
    );
    (MainLoop::new(tree), node)
}

fn var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn num(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    match var(ml, node, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("expected a number for `{name}`, got {other:?}"),
    }
}

/// A finger press arrives as an `InputEventScreenTouch` with its own fields.
#[test]
fn a_screen_touch_reaches_script_input_as_an_object() {
    let (mut ml, node) = listener();
    ml.push_event(InputEvent::ScreenTouch {
        index: 2,
        position: Vector2::new(11.0, 13.0),
        pressed: true,
        double_tap: true,
        canceled: false,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "saw_touch"),
        Variant::Bool(true),
        "`event is InputEventScreenTouch` must hold — a false here means the \
         event never reached `_input`, not that a field was wrong"
    );
    assert_eq!(num(&ml, node, "touch_index"), 2.0, "index is the finger id");
    assert_eq!(var(&ml, node, "touch_pressed"), Variant::Bool(true));
    assert_eq!(
        var(&ml, node, "touch_double"),
        Variant::Bool(true),
        "double_tap must survive delivery; it is computed by InputState and is \
         the field most easily dropped on the way through"
    );

    // ⚠️ Exclusivity: ScreenTouch and ScreenDrag are SIBLINGS under
    // InputEventFromWindow, not parent and child. If `is` ever answered true
    // for both, the elif chain in any real project would route drags into the
    // touch branch and silently swallow them.
    assert_eq!(
        var(&ml, node, "touch_was_a_drag"),
        Variant::Bool(false),
        "a touch must NOT also satisfy `is InputEventScreenDrag`"
    );
    assert_eq!(
        var(&ml, node, "saw_drag"),
        Variant::Bool(false),
        "and must not have taken the drag branch"
    );
}

/// A drag carries the motion fields, and `relative` must not be confused with
/// `velocity` — they are adjacent, same-typed, and both plausible.
#[test]
fn a_screen_drag_reaches_script_input_with_distinct_motion_fields() {
    let (mut ml, node) = listener();
    ml.push_event(InputEvent::ScreenDrag {
        index: 3,
        position: Vector2::new(21.0, 23.0),
        relative: Vector2::new(5.0, 7.0),
        velocity: Vector2::new(31.0, 37.0),
        pressure: 0.5,
        tilt: Vector2::new(0.25, -0.5),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "saw_drag"),
        Variant::Bool(true),
        "`event is InputEventScreenDrag` must hold"
    );
    assert_eq!(num(&ml, node, "drag_index"), 3.0);
    assert_eq!(
        num(&ml, node, "drag_rel_x"),
        5.0,
        "relative.x is 5 — reading 31 means `velocity` was written into \
         `relative`"
    );
    assert_eq!(
        num(&ml, node, "drag_vel_x"),
        31.0,
        "velocity.x is 31 — reading 5 means the two are swapped"
    );
    assert!(
        (num(&ml, node, "drag_pressure") - 0.5).abs() < 1e-6,
        "pen pressure must survive as a float, got {}",
        num(&ml, node, "drag_pressure")
    );
}

/// A MIDI note-on reaches the script, and `message` compares equal to the
/// `@GlobalScope` constant a real project switches on.
#[test]
fn a_midi_note_on_reaches_script_input() {
    let (mut ml, node) = listener();
    ml.push_event(InputEvent::Midi {
        channel: 5,
        // MIDI_MESSAGE_NOTE_ON
        message: 9,
        pitch: 60,
        velocity: 100,
        controller_number: 0,
        controller_value: 0,
        instrument: 0,
        pressure: 0,
        device: 0,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "saw_midi"),
        Variant::Bool(true),
        "`event is InputEventMIDI` must hold"
    );
    assert_eq!(num(&ml, node, "midi_channel"), 5.0);
    assert_eq!(
        num(&ml, node, "midi_pitch"),
        60.0,
        "pitch is 60 — reading 100 means `velocity` landed in `pitch`; the two \
         are adjacent in the constructor and both small ints"
    );
    assert_eq!(num(&ml, node, "midi_velocity"), 100.0);

    // ⚠️ Compared against the NAMED constant, not the literal 9. Godot's
    // MIDI_MESSAGE_* are the status nibble, not the raw status byte, so a
    // script that switches on them only works if the delivered `message`
    // carries the same encoding the constant does.
    assert_eq!(
        var(&ml, node, "midi_is_note_on"),
        Variant::Bool(true),
        "`event.message == MIDI_MESSAGE_NOTE_ON` must hold — this is how a \
         project dispatches MIDI, and it is the whole point of storing the \
         @GlobalScope integer rather than a raw byte"
    );
}
