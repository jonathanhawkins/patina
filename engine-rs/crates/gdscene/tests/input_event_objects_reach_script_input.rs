//! Acceptance test for pat-s3rrv.14: scripts receive real `InputEvent`
//! **objects**, not an ad-hoc Dictionary.
//!
//! Godot's `Viewport::push_input` delivers a RefCounted `InputEvent` subclass,
//! and essentially every ported project is written against that shape:
//!
//! ```gdscript
//! func _input(event):
//!     if event is InputEventKey and event.keycode == KEY_W and event.pressed:
//!         ...
//! ```
//!
//! Patina handed those virtuals a `Dictionary` with ad-hoc keys and the WRONG
//! TYPES — `scene_tree.rs` stored `keycode` as the name string `"W"` where
//! Godot's is the int `KEY_W == 87`, and `button` as the Rust debug string
//! `"Left"` where Godot exposes `button_index == MOUSE_BUTTON_LEFT == 1`. None
//! of the 17 `InputEvent*` classes was registered in ClassDB either, so
//! `event is InputEventKey` could not type-check and every ported `_input` body
//! was silently dead code.
//!
//! These tests drive the real path — `MainLoop::push_event` ->
//! `SceneTree::push_input` -> the script virtual — and assert with Godot's own
//! idiom rather than by inspecting the payload from Rust.

use gdplatform::input::{InputEvent, Key, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use gdcore::math::Vector2;

/// The Godot idiom the bead names, verbatim.
const LISTENER: &str = "\
extends Node
var hit = 0
var code = 0
var btn = 0
func _input(event):
    if event is InputEventKey and event.keycode == KEY_W and event.pressed:
        self.hit = 1
        self.code = event.keycode
    elif event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
        self.hit = 2
        self.btn = event.button_index
";

fn listener_loop(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    (MainLoop::new(tree), node)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// **The acceptance test.** `event is InputEventKey` + `event.keycode == KEY_W`
/// holds for a pushed W key, and the mouse branch fires for a left press.
#[test]
fn input_event_objects_reach_script_input() {
    // -- key half ----------------------------------------------------------
    let (mut ml, node) = listener_loop(LISTENER);
    ml.push_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "hit"),
        Variant::Int(1),
        "`event is InputEventKey and event.keycode == KEY_W and event.pressed` \
         must hold for a pushed W key — this is THE Godot input idiom"
    );
    assert_eq!(
        script_var(&ml, node, "code"),
        Variant::Int(87),
        "InputEventKey.keycode must be the int KEY_W (87), not the name string"
    );

    // -- mouse half --------------------------------------------------------
    let (mut ml, node) = listener_loop(LISTENER);
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(10.0, 10.0),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "hit"),
        Variant::Int(2),
        "the `elif event is InputEventMouseButton and event.button_index == \
         MOUSE_BUTTON_LEFT` branch must fire for a left press"
    );
    assert_eq!(
        script_var(&ml, node, "btn"),
        Variant::Int(1),
        "InputEventMouseButton.button_index must be MOUSE_BUTTON_LEFT (1), not \
         the debug string \"Left\""
    );
}

/// The type test is exclusive: a mouse event must NOT satisfy
/// `is InputEventKey`. Without this, a blanket-true `is` would pass the test
/// above for the wrong reason.
#[test]
fn a_mouse_event_is_not_an_input_event_key() {
    const PROBE: &str = "\
extends Node
var as_key = false
var as_mouse = false
func _input(event):
    if event is InputEventKey:
        self.as_key = true
    if event is InputEventMouseButton:
        self.as_mouse = true
";
    let (mut ml, node) = listener_loop(PROBE);
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
        position: Vector2::new(5.0, 5.0),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "as_mouse"),
        Variant::Bool(true),
        "a mouse press must be an InputEventMouseButton"
    );
    assert_eq!(
        script_var(&ml, node, "as_key"),
        Variant::Bool(false),
        "and must NOT also satisfy `is InputEventKey` — otherwise the acceptance \
         test above would pass for any event at all"
    );
}

/// Modifier flags use Godot's names and reach the script as bools.
#[test]
fn key_events_carry_godot_modifier_properties() {
    const PROBE: &str = "\
extends Node
var shift = false
var ctrl = false
var meta = false
func _input(event):
    if event is InputEventKey:
        self.shift = event.shift_pressed
        self.ctrl = event.ctrl_pressed
        self.meta = event.meta_pressed
";
    let (mut ml, node) = listener_loop(PROBE);
    ml.push_event(InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: true,
        ctrl: false,
        alt: false,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "shift"),
        Variant::Bool(true),
        "InputEventWithModifiers.shift_pressed must reflect the event"
    );
    assert_eq!(
        script_var(&ml, node, "ctrl"),
        Variant::Bool(false),
        "and ctrl_pressed must be false here"
    );
    // Patina has no meta source yet, but the property must still be a bool —
    // a script reading it must not get Nil.
    assert_eq!(
        script_var(&ml, node, "meta"),
        Variant::Bool(false),
        "meta_pressed must be present as a bool, not missing"
    );
}

/// A mouse motion arrives as `InputEventMouseMotion` with Godot's `relative`.
#[test]
fn mouse_motion_arrives_as_input_event_mouse_motion() {
    const PROBE: &str = "\
extends Node
var moved = false
var rel_x = 0.0
func _input(event):
    if event is InputEventMouseMotion:
        self.moved = true
        self.rel_x = event.relative.x
";
    let (mut ml, node) = listener_loop(PROBE);
    ml.push_event(InputEvent::MouseMotion {
        position: Vector2::new(30.0, 40.0),
        relative: Vector2::new(7.0, 0.0),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "moved"),
        Variant::Bool(true),
        "a motion event must satisfy `is InputEventMouseMotion`"
    );
    match script_var(&ml, node, "rel_x") {
        Variant::Float(f) => assert!(
            (f - 7.0).abs() < 1e-6,
            "InputEventMouseMotion.relative must carry the delta, got {f}"
        ),
        other => panic!("relative.x should be a float, got {other:?}"),
    }
}

/// **Acceptance clause 3.** ClassDB reports the core `InputEvent` classes with
/// Godot's inheritance chain — which is what lets `event is InputEvent` hold for
/// a concrete key event, not just the exact leaf name.
#[test]
fn class_db_reports_the_input_event_hierarchy() {
    // ClassDB is a process-global table that unit tests do not populate on
    // their own; this is the same entry point the runner and editor boot
    // through, and it is idempotent.
    gdobject::class_db::register_all_classes();

    // The 12 core classes the bead names.
    for class in [
        "InputEvent",
        "InputEventFromWindow",
        "InputEventWithModifiers",
        "InputEventKey",
        "InputEventMouse",
        "InputEventMouseButton",
        "InputEventMouseMotion",
        "InputEventJoypadButton",
        "InputEventJoypadMotion",
        "InputEventScreenTouch",
        "InputEventScreenDrag",
        "InputEventAction",
    ] {
        assert!(
            gdobject::class_db::class_exists(class),
            "{class} must be registered in ClassDB"
        );
        assert!(
            gdobject::class_db::is_parent_class(class, "InputEvent") || class == "InputEvent",
            "{class} must descend from InputEvent"
        );
    }

    // The chain itself, not just membership: these are the links that make the
    // modifier and mouse properties resolve on the concrete classes.
    for (child, parent) in [
        ("InputEvent", "RefCounted"),
        ("InputEventFromWindow", "InputEvent"),
        ("InputEventWithModifiers", "InputEventFromWindow"),
        ("InputEventKey", "InputEventWithModifiers"),
        ("InputEventMouse", "InputEventWithModifiers"),
        ("InputEventMouseButton", "InputEventMouse"),
        ("InputEventMouseMotion", "InputEventMouse"),
        ("InputEventScreenTouch", "InputEventFromWindow"),
        ("InputEventScreenDrag", "InputEventFromWindow"),
        ("InputEventJoypadButton", "InputEvent"),
        ("InputEventJoypadMotion", "InputEvent"),
        ("InputEventAction", "InputEvent"),
    ] {
        assert!(
            gdobject::class_db::is_parent_class(child, parent),
            "{child} must inherit {parent} (Godot's InputEvent hierarchy)"
        );
    }

    // A mouse button is NOT a key, so the hierarchy cannot be a blanket yes.
    assert!(
        !gdobject::class_db::is_parent_class("InputEventMouseButton", "InputEventKey"),
        "the hierarchy must stay exclusive between the key and mouse branches"
    );
}

/// With the hierarchy registered, the BASE-class test holds for a concrete
/// event — Godot's `event is InputEvent` idiom, which a leaf-name-only match
/// would answer false.
#[test]
fn a_key_event_is_also_an_input_event() {
    gdobject::class_db::register_all_classes();
    const PROBE: &str = "\
extends Node
var base = false
var leaf = false
func _input(event):
    if event is InputEvent:
        self.base = true
    if event is InputEventKey:
        self.leaf = true
";
    let (mut ml, node) = listener_loop(PROBE);
    ml.push_event(InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "leaf"),
        Variant::Bool(true),
        "the concrete class must match"
    );
    assert_eq!(
        script_var(&ml, node, "base"),
        Variant::Bool(true),
        "and so must the base class — `event is InputEvent` is ordinary Godot"
    );
}

/// The legacy subscript spelling still reads the same value, so a script
/// written against the older dictionary-shaped event keeps working.
#[test]
fn subscript_access_still_reads_event_properties() {
    const PROBE: &str = "\
extends Node
var code = 0
func _input(event):
    self.code = event[\"keycode\"]
";
    let (mut ml, node) = listener_loop(PROBE);
    ml.push_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "code"),
        Variant::Int(87),
        "`event[\"keycode\"]` must still resolve, now against the object's \
         properties (Godot's Object supports both spellings)"
    );
}
