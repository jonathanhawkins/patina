//! pat-0q2kx.92 — the `Input` singleton's script surface.
//!
//! Godot's `Input` exposes 59 methods; Patina's interpreter answered 17, and
//! two of the gaps were not "missing feature" but "idiomatic Godot code is a
//! hard runtime error":
//!
//! * `Input.is_key_pressed(KEY_A)` — Godot takes a `Key` **int**, Patina took
//!   only its own key-name strings, so the constant form TypeError'd;
//! * `Input.is_action_pressed("ui_accept", true)` — Godot's optional
//!   `exact_match` argument was rejected on arity.
//!
//! Plus `get_mouse_button_mask`, `is_anything_pressed`,
//! `get_last_mouse_velocity`, `is_physical_key_pressed` and
//! `parse_input_event`, none of which existed.
//!
//! Every assertion here runs a **stepped scene** against the live
//! `MainLoop`-owned `InputState`, not a mock, so "the method parses but reads
//! nothing" fails.

use gdcore::math::Vector2;
use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, Key, MouseButton,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads the `Input` surface exactly as a game script would — through the Godot
/// constants (`KEY_A`, `JOY_BUTTON_A`, `JOY_AXIS_LEFT_X`), not raw ints.
const OBSERVER: &str = "\
extends Node
var key_a = false
var physical_a = false
var exact_accept = false
var button_mask = 0
var anything = false
var joy_a = false
var joy_axis = 0.0
var accept_just = false
func _process(delta):
    self.key_a = Input.is_key_pressed(KEY_A)
    self.physical_a = Input.is_physical_key_pressed(KEY_A)
    self.exact_accept = Input.is_action_pressed(\"ui_accept\", true)
    self.button_mask = Input.get_mouse_button_mask()
    self.anything = Input.is_anything_pressed()
    self.joy_a = Input.is_joy_button_pressed(0, JOY_BUTTON_A)
    self.joy_axis = Input.get_joy_axis(0, JOY_AXIS_LEFT_X)
    if Input.is_action_just_pressed(\"ui_accept\"):
        self.accept_just = true
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// A one-node scene with [`OBSERVER`] attached and `ui_accept` bound to Space,
/// as Godot's builtin table binds it.
fn observer_scene() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let observer = tree.add_child(root, Node::new("Observer", "Node")).unwrap();
    tree.attach_script(
        observer,
        Box::new(GDScriptNodeInstance::from_source(OBSERVER, observer).expect("observer parses")),
    );

    let mut map = InputMap::new();
    map.add_action("ui_accept", 0.5);
    map.action_add_event("ui_accept", ActionBinding::KeyBinding(Key::Space));

    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);
    (ml, observer)
}

fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core surface test.** Every clause runs against the live engine state
/// inside a stepped scene.
#[test]
fn input_singleton_script_surface() {
    let (mut ml, observer) = observer_scene();

    // Baseline: nothing held, so a later pass cannot come from default state.
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "key_a"),
        Variant::Bool(false),
        "no key held yet"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "anything"),
        Variant::Bool(false),
        "is_anything_pressed is false on an idle frame"
    );

    // -- Input.is_key_pressed(KEY_A): the Key-int form ---------------------
    ml.push_event(key_event(Key::A, true));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "key_a"),
        Variant::Bool(true),
        "Input.is_key_pressed(KEY_A) must accept the Key int and read live \
         engine state — this used to be a TypeError"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "physical_a"),
        Variant::Bool(true),
        "Input.is_physical_key_pressed(KEY_A) answers from the same key state"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "anything"),
        Variant::Bool(true),
        "a held key makes is_anything_pressed true"
    );
    ml.push_event(key_event(Key::A, false));

    // -- exact_match is accepted, and still answers ------------------------
    ml.push_event(key_event(Key::Space, true));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "exact_accept"),
        Variant::Bool(true),
        "Input.is_action_pressed(action, exact_match) must be accepted on arity \
         AND report the action — accepting the argument but answering false \
         would be no better than the old error"
    );
    ml.push_event(key_event(Key::Space, false));
    ml.step(1.0 / 60.0);

    // -- get_mouse_button_mask reflects a held left button -----------------
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(10.0, 10.0),
    });
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "button_mask"),
        Variant::Int(1),
        "MOUSE_BUTTON_MASK_LEFT is bit 0, so a held left button reads 1"
    );

    // -- joypad polling reaches the same live state ------------------------
    ml.push_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    ml.push_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: -0.75,
        gamepad_id: 0,
    });
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "joy_a"),
        Variant::Bool(true),
        "Input.is_joy_button_pressed(0, JOY_BUTTON_A) reflects injected pad state"
    );
    match script_prop(ml.tree(), observer, "joy_axis") {
        Variant::Float(v) => assert!(
            (v - -0.75).abs() < 1e-4,
            "Input.get_joy_axis(0, JOY_AXIS_LEFT_X) must report the deflection, got {v}"
        ),
        other => panic!("joy_axis should be a float, got {other:?}"),
    }

    // -- parse_input_event(InputEventAction) fires the action ---------------
    parse_input_event_fires_the_action();
}

/// The `parse_input_event` clause of the acceptance criteria, on its own scene.
///
/// Two things force that. The observer **latches** `accept_just` the first time
/// it sees `ui_accept` just-pressed, and the `exact_match` clause above presses
/// Space — which is bound to `ui_accept` — so reusing that scene would assert
/// against a latch the test itself had already set.
///
/// And the injector has to run **before** the observer in tree order. Godot
/// makes a parsed event visible immediately; Patina models that by injecting
/// into the frame's live snapshot (`SceneTree::inject_action_press`) as well as
/// queueing the engine mutation. The just-pressed *edge* is cleared by
/// `InputState::flush_frame` at the end of the same step, so a later frame
/// cannot see it — the reader must be downstream of the writer within the frame,
/// which is exactly Godot's ordering guarantee too.
fn parse_input_event_fires_the_action() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Injector first: tree order is process order.
    let injector = tree.add_child(root, Node::new("Injector", "Node")).unwrap();
    let injector_src = "\
extends Node
var done = false
func _process(delta):
    if not self.done:
        self.done = true
        Input.parse_input_event({\"class\": \"InputEventAction\", \"action\": \"ui_accept\", \"pressed\": true})
";
    tree.attach_script(
        injector,
        Box::new(
            GDScriptNodeInstance::from_source(injector_src, injector).expect("injector parses"),
        ),
    );

    let observer = tree.add_child(root, Node::new("Observer", "Node")).unwrap();
    tree.attach_script(
        observer,
        Box::new(GDScriptNodeInstance::from_source(OBSERVER, observer).expect("observer parses")),
    );

    let mut map = InputMap::new();
    map.add_action("ui_accept", 0.5);
    map.action_add_event("ui_accept", ActionBinding::KeyBinding(Key::Space));
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(map);

    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), injector, "done"),
        Variant::Bool(true),
        "the injector ran"
    );
    assert_eq!(
        script_prop(ml.tree(), observer, "accept_just"),
        Variant::Bool(true),
        "Input.parse_input_event(InputEventAction ui_accept) must make \
         is_action_just_pressed(\"ui_accept\") true for a script that runs after it"
    );
}

// ---------------------------------------------------------------------------
// Individual behaviours
// ---------------------------------------------------------------------------

/// The mask is a bitfield, not a count: left+right reads 3.
#[test]
fn mouse_button_mask_is_a_bitfield() {
    let (mut ml, observer) = observer_scene();
    for button in [MouseButton::Left, MouseButton::Right] {
        ml.push_event(InputEvent::MouseButton {
            button,
            pressed: true,
            position: Vector2::ZERO,
        });
    }
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), observer, "button_mask"),
        Variant::Int(0b11),
        "LEFT (bit 0) + RIGHT (bit 1) = 3"
    );
}

/// `get_last_mouse_velocity` reports pixels **per second**, so it depends on the
/// frame delta rather than being the raw displacement.
#[test]
fn last_mouse_velocity_is_pixels_per_second() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Reader", "Node")).unwrap();
    let src = "\
extends Node
var vel = Vector2(0, 0)
func _process(delta):
    self.vel = Input.get_last_mouse_velocity()
";
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("reader parses")),
    );
    let mut ml = MainLoop::new(tree);

    // 10px of motion over a 0.1s frame is 100 px/s.
    ml.push_event(InputEvent::MouseMotion {
        position: Vector2::new(10.0, 0.0),
        relative: Vector2::new(10.0, 0.0),
    });
    ml.step(0.1);
    match script_prop(ml.tree(), node, "vel") {
        Variant::Vector2(v) => assert!(
            (v.x - 100.0).abs() < 1e-3 && v.y.abs() < 1e-3,
            "10px over 0.1s is 100 px/s, got {v:?}"
        ),
        other => panic!("velocity should be a Vector2, got {other:?}"),
    }
}

/// An unknown `Input` method is still an error — the new arms must not have
/// turned the dispatch into a silent catch-all.
#[test]
fn unknown_input_method_still_errors() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Bad", "Node")).unwrap();
    let src = "\
extends Node
var ran = false
func _process(delta):
    Input.no_such_method()
    self.ran = true
";
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), node, "ran"),
        Variant::Bool(false),
        "the call must fail, so the statement after it never runs"
    );
}
