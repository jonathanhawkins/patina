//! pat-s3rrv.14 — the `InputEvent` **method** surface answers from a script.
//!
//! The class family and object delivery landed in earlier passes: a script's
//! `_input(event)` already receives an object and `event is InputEventKey`
//! already type-checks. The METHODS did not exist anywhere. `is_pressed`,
//! `is_echo`, `as_text`, `is_action_type` and — above all —
//! `event.is_action_pressed("ui_accept")` were registered in ClassDB as
//! metadata and dispatched by nothing: grepping the workspace for `"is_echo"`,
//! `"as_text"` or `"is_action_type"` returned zero implementation sites.
//!
//! `is_action_pressed` is the single most common line in any Godot `_input`
//! body, and it needs something the engine did not previously expose: the
//! action's real BINDINGS. `SceneTree` mirrored only a bound-event *count*, and
//! a count cannot answer "does this event match this action". The mirror now
//! carries the bindings themselves.
//!
//! ⚠️ Every rule asserted here is transcribed from
//! `upstream/godot/core/input/input_event.cpp` and `input_map.cpp`, not
//! inferred from the method names. Several are counter-intuitive and a
//! plausible guess gets them wrong — see the individual tests.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use gdcore::math::Vector2;

/// A loop whose InputMap binds `ui_accept` to Enter and `fire` to the left
/// mouse button.
fn listener_loop(src: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);

    let mut map = InputMap::new();
    map.add_action("ui_accept", 0.5);
    map.action_add_event("ui_accept", ActionBinding::KeyBinding(Key::Enter));
    map.add_action("fire", 0.5);
    map.action_add_event("fire", ActionBinding::MouseBinding(MouseButton::Left));
    // An action with no binding at all — a real state, since
    // `InputMap.add_action` creates exactly that.
    map.add_action("unbound", 0.5);
    ml.input_state_mut().set_input_map(map);
    // ⚠️ One warm-up frame BEFORE any event is pushed. `MainLoop::step`
    // refreshes the tree's InputMap mirror *after* the script phase — its own
    // comment says a script reads engine truth "next frame" — so an event
    // pushed into the very first step would be matched against an empty map.
    // Without this every is_action_* assertion below would fail for a reason
    // that has nothing to do with what it is testing.
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

fn press(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn release(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

/// **The acceptance test**: `event.is_action_pressed("ui_accept")` answers.
///
/// This is the idiom the whole bead exists for. It needs three things that were
/// each missing: an event object (earlier pass), a dispatch table for the
/// method, and the action's real bindings reaching the interpreter.
#[test]
fn input_event_methods_answer_from_script() {
    const SRC: &str = "\
extends Node
var accepted = -1
var fired = -1
var other = -1
var unbound = -1
func _input(event):
    if event is InputEventKey:
        self.accepted = 1 if event.is_action_pressed(\"ui_accept\") else 0
        self.other = 1 if event.is_action_pressed(\"fire\") else 0
        self.unbound = 1 if event.is_action_pressed(\"unbound\") else 0
    if event is InputEventMouseButton:
        self.fired = 1 if event.is_action_pressed(\"fire\") else 0
";

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::Enter));
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "accepted"),
        Variant::Int(1),
        "Enter is bound to ui_accept, so `event.is_action_pressed(\"ui_accept\")` \
         must be true — -1 means the method never dispatched at all, which is \
         the bug this bead exists for",
    );
    assert_eq!(
        script_var(&ml, node, "other"),
        Variant::Int(0),
        "…and the SAME event must answer false for an action it is not bound \
         to. Without this the test would pass against an implementation that \
         ignored the event and reported on global input state",
    );
    assert_eq!(
        script_var(&ml, node, "unbound"),
        Variant::Int(0),
        "an action that exists but has no binding matches nothing",
    );

    // -- the mouse half, through a different binding kind --------------------
    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(4.0, 4.0),
    });
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "fired"),
        Variant::Int(1),
        "a left click must satisfy the action bound to MOUSE_BUTTON_LEFT",
    );
}

/// A release answers `is_action_released`, not `is_action_pressed`
/// (pat-s3rrv.14).
///
/// `event_get_action_status` reports the event's own press state; the two
/// predicates read opposite sides of it. An implementation that returned
/// "matches the action" for both would pass a pressed-only test.
#[test]
fn a_release_is_released_not_pressed() {
    const SRC: &str = "\
extends Node
var pressed = -1
var released = -1
var strength = -1.0
func _input(event):
    if event is InputEventKey:
        self.pressed = 1 if event.is_action_pressed(\"ui_accept\") else 0
        self.released = 1 if event.is_action_released(\"ui_accept\") else 0
        self.strength = event.get_action_strength(\"ui_accept\")
";

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(release(Key::Enter));
    ml.step(1.0 / 60.0);

    assert_eq!(script_var(&ml, node, "pressed"), Variant::Int(0));
    assert_eq!(
        script_var(&ml, node, "released"),
        Variant::Int(1),
        "an Enter RELEASE must satisfy is_action_released(\"ui_accept\")",
    );
    assert_eq!(
        script_var(&ml, node, "strength"),
        Variant::Float(0.0),
        "a released digital action has zero strength, not Nil — a script doing \
         arithmetic on it must not get a type error",
    );

    // …and a press is the mirror image.
    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::Enter));
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "pressed"), Variant::Int(1));
    assert_eq!(script_var(&ml, node, "released"), Variant::Int(0));
    assert_eq!(
        script_var(&ml, node, "strength"),
        Variant::Float(1.0),
        "a pressed digital binding is full strength",
    );
}

/// `is_pressed` / `is_released` / `is_echo` / `is_canceled` (pat-s3rrv.14).
///
/// ⚠️ `is_released()` is `!pressed && !canceled` upstream, **not**
/// `!is_pressed()`. The distinction only shows on a canceled event, where both
/// predicates are false together — a natural implementation as `!is_pressed()`
/// would report a canceled event as released.
#[test]
fn the_state_predicates_follow_godots_definitions() {
    const SRC: &str = "\
extends Node
var pressed = -1
var released = -1
var echo = -1
var canceled = -1
var action_type = -1
func _input(event):
    if event is InputEventKey:
        self.pressed = 1 if event.is_pressed() else 0
        self.released = 1 if event.is_released() else 0
        self.echo = 1 if event.is_echo() else 0
        self.canceled = 1 if event.is_canceled() else 0
        self.action_type = 1 if event.is_action_type() else 0
";

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::W));
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "pressed"), Variant::Int(1));
    assert_eq!(script_var(&ml, node, "released"), Variant::Int(0));
    assert_eq!(
        script_var(&ml, node, "echo"),
        Variant::Int(0),
        "a fresh press is not an echo",
    );
    assert_eq!(script_var(&ml, node, "canceled"), Variant::Int(0));
    assert_eq!(
        script_var(&ml, node, "action_type"),
        Variant::Int(1),
        "InputEventKey is one of the five classes Godot overrides \
         is_action_type() to true on",
    );

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(release(Key::W));
    ml.step(1.0 / 60.0);
    assert_eq!(script_var(&ml, node, "pressed"), Variant::Int(0));
    assert_eq!(script_var(&ml, node, "released"), Variant::Int(1));
}

/// Mouse MOTION is not an action type (pat-s3rrv.14).
///
/// Godot overrides `is_action_type()` to true on exactly five classes, and
/// InputEventMouseMotion is not among them — nor, less obviously, is
/// InputEventScreenTouch. Assuming "anything you can press" is an action type
/// gets this wrong.
#[test]
fn mouse_motion_is_not_an_action_type() {
    const SRC: &str = "\
extends Node
var motion_action_type = -1
var button_action_type = -1
func _input(event):
    if event is InputEventMouseMotion:
        self.motion_action_type = 1 if event.is_action_type() else 0
    if event is InputEventMouseButton:
        self.button_action_type = 1 if event.is_action_type() else 0
";

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(InputEvent::MouseMotion {
        position: Vector2::new(3.0, 4.0),
        relative: Vector2::new(1.0, 1.0),
    });
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(3.0, 4.0),
    });
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "motion_action_type"),
        Variant::Int(0),
        "InputEventMouseMotion does NOT override is_action_type()",
    );
    assert_eq!(
        script_var(&ml, node, "button_action_type"),
        Variant::Int(1),
        "…but InputEventMouseButton does — so a blanket answer fails one of \
         these two whichever way it is written",
    );
}

/// `as_text()` and the modifier helpers (pat-s3rrv.14).
///
/// ⚠️ Two traps. The modifier order is **Ctrl+Alt+Shift+Meta**, not the
/// Ctrl/Shift/Alt a reader would assume from how keyboards are usually
/// described. And the key names are Godot's `keycode_get_string` — `"Space"`,
/// `"Up"` — NOT `gdplatform::input::Key::name()`, which returns browser
/// `KeyboardEvent.key` names (`" "`, `"ArrowUp"`) and would have produced text
/// that looks plausible and matches nothing Godot prints.
#[test]
fn as_text_matches_godots_spelling() {
    const SRC: &str = "\
extends Node
var text = \"unset\"
var mods = 0
func _input(event):
    if event is InputEventKey:
        self.text = event.as_text()
        self.mods = event.get_modifiers_mask()
";

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::W));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "text"),
        Variant::String("W".to_string()),
        "a bare W prints as \"W\"",
    );

    // Ctrl+Shift+W — the order is Godot's, not the order the flags were set.
    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: true,
        ctrl: true,
        alt: false,
    });
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "text"),
        Variant::String("Ctrl+Shift+W".to_string()),
        "modifiers print Ctrl, Alt, Shift, Meta in THAT order \
         (input_event.cpp:257) — \"Shift+Ctrl+W\" would be wrong",
    );
    // KeyModifierMask: CTRL = 1<<28, SHIFT = 1<<25.
    assert_eq!(
        script_var(&ml, node, "mods"),
        Variant::Int((1 << 28) + (1 << 25)),
        "get_modifiers_mask must use Godot's KeyModifierMask bit values",
    );

    // A special key uses Godot's name, not the browser's.
    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::Space));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "text"),
        Variant::String("Space".to_string()),
        "Godot prints \"Space\"; Key::name() would have given a single blank",
    );

    let (mut ml, node) = listener_loop(SRC);
    ml.push_event(press(Key::Up));
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, node, "text"),
        Variant::String("Up".to_string()),
        "Godot prints \"Up\"; Key::name() would have given \"ArrowUp\"",
    );
}

/// `InputMap.action_get_events` returns real events, and `action_has_event`
/// reads the event it is given (pat-s3rrv.14).
///
/// Both used to ignore their event argument entirely: `action_has_event`
/// answered `count > 0` for anything at all, and `action_get_events` returned
/// an array of the right length whose every element was `null`.
#[test]
fn input_map_answers_with_real_events() {
    const SRC: &str = "\
extends Node
var count = -1
var first_class = \"unset\"
var first_code = -1
var has_enter = -1
var has_escape = -1
func _process(delta):
    var events = InputMap.action_get_events(\"ui_accept\")
    self.count = events.size()
    if self.count > 0:
        self.first_class = events[0].get_class()
        self.first_code = events[0].keycode
        self.has_enter = 1 if InputMap.action_has_event(\"ui_accept\", events[0]) else 0
        self.has_escape = 1 if InputMap.action_has_event(\"fire\", events[0]) else 0
";

    let (mut ml, node) = listener_loop(SRC);
    ml.tree_mut().process_script_ready(node);
    // The helper already ran the warm-up frame that populates the mirror.
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, node, "count"),
        Variant::Int(1),
        "ui_accept has one binding",
    );
    assert_eq!(
        script_var(&ml, node, "first_class"),
        Variant::String("InputEventKey".to_string()),
        "the array must hold real InputEventKey objects — it used to hold \
         nulls, on which get_class() cannot even be called",
    );
    assert_eq!(
        script_var(&ml, node, "first_code"),
        Variant::Int(4194309),
        "…carrying Godot's KEY_ENTER (SPECIAL|0x05)",
    );
    assert_eq!(
        script_var(&ml, node, "has_enter"),
        Variant::Int(1),
        "action_has_event must find the event it was just handed",
    );
    assert_eq!(
        script_var(&ml, node, "has_escape"),
        Variant::Int(0),
        "…and must NOT claim the mouse action holds a key event. This is the \
         assertion the old count-only implementation failed: it answered true \
         for every event, for every action that had any binding at all",
    );
}
