//! pat-0q2kx.91 acceptance: `SceneTree::push_input` propagates in Godot's order,
//! with Godot's filtering.
//!
//! Godot ground truth: `SceneTree::_call_input_pause`
//! (upstream/godot/scene/main/scene_tree.cpp:1435-1447) walks each stage's node
//! group **in reverse**, skips any node failing `can_process()`, and re-checks
//! `is_input_handled()` before every node. `Viewport::_push_unhandled_input_internal`
//! (viewport.cpp:3558-3576) admits only Key/Shortcut/JoypadButton events to
//! `_shortcut_input` and only Key events to `_unhandled_key_input`.
//!
//! Patina had none of the three: `push_input` iterated tree order **forward**,
//! never consulted `process_mode`, and ran `_shortcut_input` for every event
//! type — so a parent got first refusal over its own children, a paused scene
//! still reacted to input, and moving the mouse fired shortcut handlers.
//!
//! Order is observed by having each node report into a shared `Log` node's
//! script, so the assertions read the *sequence* rather than per-node flags that
//! could not tell "C ran first" from "only C ran".

use gdplatform::input::{InputEvent, Key, MouseButton};
use gdscene::node::{Node, NodeId, ProcessMode};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// `Main -> {Log, P -> C}`. `P` and `C` run `body_p` / `body_c`; both report
/// into `Log.note()`, which appends to a single string.
fn scene(body_p: &str, body_c: &str) -> (SceneTree, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let log = tree.add_child(main, Node::new("Log", "Node2D")).unwrap();
    let log_src = "\
extends Node2D
var seq = \"\"
func note(who):
    self.seq = self.seq + who
";
    tree.attach_script(
        log,
        Box::new(GDScriptNodeInstance::from_source(log_src, log).unwrap()),
    );

    let p = tree.add_child(main, Node::new("P", "Node2D")).unwrap();
    tree.attach_script(
        p,
        Box::new(GDScriptNodeInstance::from_source(body_p, p).unwrap()),
    );

    let c = tree.add_child(p, Node::new("C", "Node2D")).unwrap();
    tree.attach_script(
        c,
        Box::new(GDScriptNodeInstance::from_source(body_c, c).unwrap()),
    );

    (tree, log, c)
}

fn seq(tree: &SceneTree, log: NodeId) -> String {
    let value = tree
        .get_script(log)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == "seq")
                .map(|p| p.default_value)
        })
        .unwrap_or(Variant::Nil);
    match value {
        Variant::String(s) => s,
        other => panic!("Log.seq should be a String, got {other:?}"),
    }
}

fn key(k: Key) -> InputEvent {
    InputEvent::Key {
        key: k,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn motion() -> InputEvent {
    InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(5.0, 5.0),
        relative: gdcore::math::Vector2::ZERO,
    }
}

/// `_input` on P and on C, each reporting its own name.
const P_INPUT: &str = "\
extends Node2D
func _input(event):
    get_node(\"../Log\").note(\"P\")
";
const C_INPUT: &str = "\
extends Node2D
func _input(event):
    get_node(\"../../Log\").note(\"C\")
";

/// **The acceptance test.** Reverse order, handled short-circuit, `can_process`
/// gating, and the shortcut-stage event filter.
#[test]
fn input_propagation_order_parity() {
    // (1) The deeper, later node runs FIRST. Forward tree order would give
    // "PC" — this is the whole point of Godot's reversed group walk.
    let (mut tree, log, _c) = scene(P_INPUT, C_INPUT);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "CP",
        "the child must receive `_input` before its parent (reversed group walk)"
    );

    // (2) `set_input_as_handled()` in the child stops the parent's `_input` and
    // every later stage — not merely the stages after this one.
    let c_handles = "\
extends Node2D
func _input(event):
    get_node(\"../../Log\").note(\"C\")
    set_input_as_handled()
";
    let p_also_unhandled = "\
extends Node2D
func _input(event):
    get_node(\"../Log\").note(\"P\")
func _unhandled_input(event):
    get_node(\"../Log\").note(\"U\")
";
    let (mut tree, log, _c) = scene(p_also_unhandled, c_handles);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "C",
        "a handled event must reach neither the parent's `_input` nor any later stage"
    );

    // (3) A node whose `process_mode` disables it receives no input at all.
    let (mut tree, log, c) = scene(P_INPUT, C_INPUT);
    tree.get_node_mut(c)
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "P",
        "a process_mode = DISABLED node must be skipped by the input walk"
    );

    // (4) Mouse motion must never reach `_shortcut_input`.
    let p_shortcut = "\
extends Node2D
func _input(event):
    get_node(\"../Log\").note(\"P\")
func _shortcut_input(event):
    get_node(\"../Log\").note(\"S\")
";
    let c_noop = "extends Node2D\n";
    let (mut tree, log, _c) = scene(p_shortcut, c_noop);
    tree.push_input(&motion());
    assert_eq!(
        seq(&tree, log),
        "P",
        "MouseMotion must not run `_shortcut_input` — Godot admits only \
         Key/Shortcut/JoypadButton to that stage"
    );

    // ... but a key event still does, so the gate is not simply off.
    let (mut tree, log, _c) = scene(p_shortcut, c_noop);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "PS",
        "a key event must still reach `_shortcut_input` after `_input`"
    );
}

/// A paused tree stops input for `PAUSABLE` nodes but not for `ALWAYS` ones —
/// the other half of `can_process`, which a DISABLED-only test would not cover.
#[test]
fn pause_gates_input_by_process_mode() {
    let (mut tree, log, c) = scene(P_INPUT, C_INPUT);
    tree.set_paused(true);
    // C keeps running while paused; P inherits PAUSABLE and must go quiet.
    tree.get_node_mut(c)
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "C",
        "while paused, only a process_mode = ALWAYS node may receive input"
    );
}

/// `_unhandled_key_input` is keyboard-only; `_unhandled_input` still sees
/// everything that reaches it.
#[test]
fn unhandled_key_stage_is_keyboard_only() {
    let p = "\
extends Node2D
func _unhandled_key_input(event):
    get_node(\"../Log\").note(\"K\")
func _unhandled_input(event):
    get_node(\"../Log\").note(\"U\")
";
    let c_noop = "extends Node2D\n";

    let (mut tree, log, _c) = scene(p, c_noop);
    tree.push_input(&key(Key::A));
    assert_eq!(
        seq(&tree, log),
        "KU",
        "a key event runs `_unhandled_key_input` before `_unhandled_input`"
    );

    // A mouse button reaches `_unhandled_input` only — this is the case Godot
    // calls out as the reason the key stage exists at all.
    let (mut tree, log, _c) = scene(p, c_noop);
    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::new(500.0, 500.0),
    });
    assert_eq!(
        seq(&tree, log),
        "U",
        "a non-key event must skip `_unhandled_key_input`"
    );
}
