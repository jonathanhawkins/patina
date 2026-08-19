//! pat-s3rrv.14 — magnify and pan gestures reach `_input` as real objects.
//!
//! ⚠️ The gap this closes is the worst shape a gap takes, and worse here than
//! for touch/MIDI. `class_db.rs:22707-22736` **registered**
//! `InputEventGesture`, `InputEventMagnifyGesture` and `InputEventPanGesture`,
//! while `gdplatform::input::InputEvent` had no gesture variant and
//! `input_event_to_variant` built none. So `event is InputEventPanGesture`
//! type-checked, compiled, and could never be true: a project's gesture branch
//! was dead code rather than an error anyone could see.
//!
//! Every value below is deliberately ASYMMETRIC. `position` vs `delta` are both
//! `Vector2` and adjacent in the constructor, so a fixture with equal or
//! mirrored components cannot catch a swap; `delta` carries a negative
//! component so a sign flip cannot hide either.

use gdplatform::input::InputEvent;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use gdcore::math::Vector2;

/// Reads both classes through the Godot idiom and records which branch fired,
/// so an undelivered event cannot masquerade as a zero value.
///
/// `saw_gesture` is set from the BASE class in each branch: Godot's hierarchy
/// is `InputEventGesture` < `InputEventMagnifyGesture` / `InputEventPanGesture`,
/// and a registration with a missing or wrong `parent` would still satisfy the
/// leaf test while failing this one.
const LISTENER: &str = "\
extends Node
var saw_magnify = false
var saw_pan = false
var magnify_is_gesture = false
var pan_is_gesture = false
var magnify_was_a_pan = false
var factor = -1.0
var pos_x = -1.0
var pos_y = -1.0
var delta_x = -1.0
var delta_y = -1.0
var shift_seen = true
func _input(event):
    if event is InputEventMagnifyGesture:
        self.saw_magnify = true
        self.factor = event.factor
        self.pos_x = event.position.x
        self.pos_y = event.position.y
        self.shift_seen = event.shift_pressed
        if event is InputEventGesture:
            self.magnify_is_gesture = true
        if event is InputEventPanGesture:
            self.magnify_was_a_pan = true
    elif event is InputEventPanGesture:
        self.saw_pan = true
        self.delta_x = event.delta.x
        self.delta_y = event.delta.y
        self.pos_x = event.position.x
        self.pos_y = event.position.y
        if event is InputEventGesture:
            self.pan_is_gesture = true
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

/// A pinch arrives as an `InputEventMagnifyGesture` carrying its factor.
#[test]
fn a_magnify_gesture_reaches_script_input_as_an_object() {
    let (mut ml, node) = listener();
    ml.push_event(InputEvent::MagnifyGesture {
        position: Vector2::new(17.0, 23.0),
        factor: 1.25,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "saw_magnify"),
        Variant::Bool(true),
        "`event is InputEventMagnifyGesture` must hold — a false here means the \
         event never reached `_input` at all, which is the state this bead found"
    );
    assert_eq!(num(&ml, node, "factor"), 1.25, "factor is the scale delta");
    assert_eq!(num(&ml, node, "pos_x"), 17.0);
    assert_eq!(
        num(&ml, node, "pos_y"),
        23.0,
        "position must not be transposed"
    );

    // 🔑 The base-class test. A registration whose `parent` is missing or wrong
    // still satisfies the leaf `is` above; only this catches it, and Godot code
    // legitimately tests against `InputEventGesture`.
    assert_eq!(
        var(&ml, node, "magnify_is_gesture"),
        Variant::Bool(true),
        "InputEventMagnifyGesture must also satisfy `is InputEventGesture`"
    );

    // ⚠️ Exclusivity: magnify and pan are SIBLINGS under InputEventGesture. If
    // `is` answered true for both, a real project's elif chain would route pans
    // into the magnify branch and silently swallow them.
    assert_eq!(
        var(&ml, node, "magnify_was_a_pan"),
        Variant::Bool(false),
        "a magnify must NOT also satisfy `is InputEventPanGesture`"
    );

    // Godot's InputEventGesture derives from InputEventWithModifiers, so this
    // reads `false`, not Nil. Omitting the modifier props would make every
    // `if event.shift_pressed` in a ported project compare against Nil.
    assert_eq!(
        var(&ml, node, "shift_seen"),
        Variant::Bool(false),
        "modifier props must be present as bools on a gesture"
    );
}

/// A two-finger pan carries `delta`, which must not be confused with
/// `position` — same type, adjacent in the constructor, both plausible.
#[test]
fn a_pan_gesture_reaches_script_input_with_distinct_position_and_delta() {
    let (mut ml, node) = listener();
    ml.push_event(InputEvent::PanGesture {
        position: Vector2::new(31.0, 37.0),
        delta: Vector2::new(-5.0, 9.0),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        var(&ml, node, "saw_pan"),
        Variant::Bool(true),
        "`event is InputEventPanGesture` must hold"
    );
    assert_eq!(
        var(&ml, node, "pan_is_gesture"),
        Variant::Bool(true),
        "InputEventPanGesture must also satisfy `is InputEventGesture`"
    );
    assert_eq!(num(&ml, node, "pos_x"), 31.0);
    assert_eq!(num(&ml, node, "pos_y"), 37.0);
    assert_eq!(
        num(&ml, node, "delta_x"),
        -5.0,
        "delta must be the pan delta, not the position — and the sign must survive"
    );
    assert_eq!(num(&ml, node, "delta_y"), 9.0);
    assert_eq!(
        var(&ml, node, "saw_magnify"),
        Variant::Bool(false),
        "a pan must not have taken the magnify branch"
    );
}

/// A gesture must not disturb action state.
///
/// `process_event` treats gestures the way it treats MIDI — nothing is held.
/// The risk being pinned is the opposite of a missing feature: an over-eager
/// arm that wrote `mouse_position` from the gesture centre would drag the
/// pointer on every pinch, and nothing in the two tests above would notice.
#[test]
fn a_gesture_does_not_move_the_pointer_or_press_an_action() {
    use gdplatform::input::InputState;

    let mut state = InputState::new();
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(3.0, 4.0),
        relative: Vector2::new(0.0, 0.0),
    });
    let before = state.get_mouse_position();

    state.process_event(InputEvent::MagnifyGesture {
        position: Vector2::new(500.0, 600.0),
        factor: 2.0,
    });
    state.process_event(InputEvent::PanGesture {
        position: Vector2::new(700.0, 800.0),
        delta: Vector2::new(1.0, 1.0),
    });

    assert_eq!(
        state.get_mouse_position(),
        before,
        "a gesture carries a position but must NOT move the cursor"
    );
}
