//! pat-0q2kx.81 — first-class `Signal` values: `emit`, `connect(Callable)`,
//! `disconnect`, and `await`.
//!
//! Godot 4 scripts use `hit.emit(dmg)`, `button.pressed.connect(_on_pressed)`
//! and `await other.done` as the primary signal API (`doc/classes/Signal.xml`;
//! `Object::connect` takes a Callable).
//!
//! ⚠️ The bead is labelled MISSING, but at HEAD `Variant::Signal { object_id,
//! name }` exists, member access yields it, and `emit`/`connect`/`disconnect`/
//! `is_connected` are dispatched on it. What was missing is this test — so the
//! whole surface could regress without a failure. These drive the real
//! `MainLoop`, so a pass means a script reached the signal table, not that a
//! Rust helper exists.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn attach(tree: &mut SceneTree, id: gdscene::node::NodeId, src: &str) {
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
}

fn prop(ml: &MainLoop, id: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    ml.tree().get_script(id).and_then(|s| s.get_property(name))
}

/// `A` declares the signal and emits it; `B` connects a Callable to it.
const EMITTER: &str = "\
extends Node
signal scored(points)

func fire():
    scored.emit(3)
";

const LISTENER: &str = "\
extends Node
var got = -1

func _on_scored(points):
    self.got = points

func hook():
    var a = get_node(\"../A\")
    a.scored.connect(_on_scored)
";

fn two_node_tree() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    attach(&mut tree, a, EMITTER);
    attach(&mut tree, b, LISTENER);
    (tree, a, b)
}

/// **The acceptance test.** `b` connects to `a.scored` through the Signal value,
/// `a` emits it, and `b`'s handler runs with the emitted argument.
#[test]
fn signal_value_emit_connect_await() {
    let (tree, a, b) = two_node_tree();
    let mut ml = MainLoop::new(tree);

    ml.tree_mut().call_script_method(b, "hook", &[]);
    // Nothing emitted yet — a handler that ran here would mean the connect
    // itself fired it.
    assert_eq!(
        prop(&ml, b, "got"),
        Some(Variant::Int(-1)),
        "connecting must not invoke the handler"
    );

    ml.tree_mut().call_script_method(a, "fire", &[]);
    assert_eq!(
        prop(&ml, b, "got"),
        Some(Variant::Int(3)),
        "`scored.emit(3)` must reach the connected Callable with its argument"
    );
}

/// Godot 4's `Object.connect(StringName, Callable)` signature — the shape the
/// old Godot-3 `(signal, object, method)` arm could not accept.
#[test]
fn object_connect_takes_a_callable() {
    let (mut tree, a, b) = two_node_tree();
    attach(
        &mut tree,
        b,
        "\
extends Node
var got = -1

func _on_scored(points):
    self.got = points

func hook():
    var a = get_node(\"../A\")
    a.connect(\"scored\", _on_scored)
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(b, "hook", &[]);
    ml.tree_mut().call_script_method(a, "fire", &[]);

    assert_eq!(
        prop(&ml, b, "got"),
        Some(Variant::Int(3)),
        "the 4.x connect(name, Callable) signature must work on the object too"
    );
}

/// `disconnect` and `is_connected` operate on the same Signal value.
#[test]
fn signal_value_disconnect_and_is_connected() {
    let (mut tree, a, b) = two_node_tree();
    attach(
        &mut tree,
        b,
        "\
extends Node
var got = -1
var linked_before = false
var linked_after = true

func _on_scored(points):
    self.got = points

func hook():
    var a = get_node(\"../A\")
    a.scored.connect(_on_scored)
    self.linked_before = a.scored.is_connected(_on_scored)
    a.scored.disconnect(_on_scored)
    self.linked_after = a.scored.is_connected(_on_scored)
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(b, "hook", &[]);

    assert_eq!(
        prop(&ml, b, "linked_before"),
        Some(Variant::Bool(true)),
        "is_connected must see the connection it just made"
    );
    assert_eq!(
        prop(&ml, b, "linked_after"),
        Some(Variant::Bool(false)),
        "…and must not see it after disconnect"
    );

    // ⚠️ The behavioural half: a disconnected handler must not fire. Asserting
    // only on `is_connected` would pass even if disconnect updated bookkeeping
    // without unhooking anything.
    ml.tree_mut().call_script_method(a, "fire", &[]);
    assert_eq!(
        prop(&ml, b, "got"),
        Some(Variant::Int(-1)),
        "a disconnected handler must not run"
    );
}

/// `await <Signal value>` parks the frame and resumes with the emitted value.
#[test]
fn await_on_a_signal_value_resumes_with_the_argument() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let w = tree.add_child(root, Node::new("W", "Node")).unwrap();
    attach(&mut tree, a, EMITTER);
    attach(
        &mut tree,
        w,
        "\
extends Node
var seen = -1

func watch():
    var a = get_node(\"../A\")
    var v = await a.scored
    self.seen = v
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(w, "watch", &[]);
    assert_eq!(
        prop(&ml, w, "seen"),
        Some(Variant::Int(-1)),
        "the await must suspend before the signal fires"
    );

    ml.tree_mut().call_script_method(a, "fire", &[]);
    assert_eq!(
        prop(&ml, w, "seen"),
        Some(Variant::Int(3)),
        "awaiting a Signal value must resume with the emitted argument"
    );
}

// NOTE (pat-0q2kx.81): a "scriptless emitter wakes an await" test was written
// here and removed — its premise is false. `a.ping` only becomes a
// `Variant::Signal` when `node_has_signal` says the node HAS that signal, and
// that check consults the attached script's declarations. A node with no script
// declares nothing, so `a.ping` reads as a plain (absent) property, the await
// never suspends, and the test failed on its own setup rather than on the
// engine. The `emit_signal` broadcast is still deliberately outside the
// `scripts.contains_key(&source)` guard so an engine-side emit can wake an
// await — that path is simply not reachable from GDScript until a scriptless
// node can expose a signal name.

/// An object-bound `await` must NOT wake on a same-named signal from a
/// different object — the broadcast added above offers the signal to every
/// script, so this pins that the host still filters by source.
#[test]
fn await_bound_to_one_object_ignores_another_objects_signal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let other = tree.add_child(root, Node::new("Other", "Node")).unwrap();
    let w = tree.add_child(root, Node::new("W", "Node")).unwrap();
    attach(&mut tree, a, EMITTER);
    attach(&mut tree, other, EMITTER);
    attach(
        &mut tree,
        w,
        "\
extends Node
var seen = -1

func watch():
    var a = get_node(\"../A\")
    var v = await a.scored
    self.seen = v
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(w, "watch", &[]);

    // The WRONG object emits the same signal name.
    ml.tree_mut().call_script_method(other, "fire", &[]);
    assert_eq!(
        prop(&ml, w, "seen"),
        Some(Variant::Int(-1)),
        "`await a.scored` must ignore Other.scored"
    );

    // The right one wakes it.
    ml.tree_mut().call_script_method(a, "fire", &[]);
    assert_eq!(
        prop(&ml, w, "seen"),
        Some(Variant::Int(3)),
        "…and still wakes for its own object"
    );
}
