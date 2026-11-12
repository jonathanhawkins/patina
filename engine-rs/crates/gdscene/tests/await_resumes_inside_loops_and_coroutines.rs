//! pat-0q2kx.78 — `await` resumes at the exact suspension point, including
//! inside loops, and awaiting a coroutine yields its return value.
//!
//! Godot's VM resumes a `GDScriptFunctionState` at the instruction it left,
//! preserving loop counters and branch position (`gdscript_vm.cpp`). Patina
//! parked only `remaining = body[i+1..]` of the TOP-LEVEL statement list, and
//! `While`/`For` propagated `Suspend` outward exactly like `Return` — so
//! `for e in list: spawn(e); await timer` spawned ONE enemy and silently
//! abandoned the rest.
//!
//! ⚠️ That bug was invisible to every existing test because a TOP-LEVEL await
//! has no enclosing construct to lose: the flat `remaining` was sufficient, and
//! the suite was green. These drive the real `MainLoop` tick, mirroring
//! `gdscript_await_resumes_at_runtime.rs`.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Builds a one-node tree running `src` and returns the loop plus the node id.
fn run(src: &str) -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(src, main_id).expect("script parses");
    tree.attach_script(main_id, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main_id);
    (ml, main_id)
}

fn prop(ml: &MainLoop, id: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    ml.tree().get_script(id).and_then(|s| s.get_property(name))
}

/// **The acceptance test.** A `for` body that awaits must run every iteration.
#[test]
fn await_resumes_inside_loops_and_coroutines() {
    // Three iterations, each awaiting a fresh 0.05s timer.
    let (mut ml, id) = run("\
extends Node
var count = 0
func _ready():
    for i in 3:
        self.count = self.count + 1
        var t = get_tree().create_timer(0.05)
        await t.timeout
");

    // The first iteration ran and then parked: exactly one increment so far.
    assert_eq!(
        prop(&ml, id, "count"),
        Some(Variant::Int(1)),
        "the loop must suspend after its first iteration, not run to completion"
    );

    // Tick well past 3 x 0.05s. Each resume finishes the current iteration and
    // re-enters the loop for the next item.
    for _ in 0..40 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        prop(&ml, id, "count"),
        Some(Variant::Int(3)),
        "every loop iteration must run — this is the `spawns ONE enemy` bug"
    );
}

/// `var v = await helper()` where the helper itself awaits: the caller must park
/// on the callee and take its return value.
#[test]
fn awaiting_a_coroutine_yields_its_return_value() {
    let (mut ml, id) = run("\
extends Node
signal go
var v = -1
func helper():
    await go
    return 7
func _ready():
    var r = await helper()
    self.v = r
");

    // Parked inside `helper`, which is itself parked on `go`.
    assert_eq!(
        prop(&ml, id, "v"),
        Some(Variant::Int(-1)),
        "the caller must wait for the callee, not take its immediate Nil"
    );

    ml.tree_mut().emit_signal(id, "go", &[]);

    assert_eq!(
        prop(&ml, id, "v"),
        Some(Variant::Int(7)),
        "awaiting a coroutine must deliver the value it returned"
    );
}

/// A `while` that awaits mid-body keeps looping, and the statements after the
/// await still run each pass.
#[test]
fn await_inside_while_keeps_looping_and_finishes_the_body() {
    let (mut ml, id) = run("\
extends Node
var n = 0
var after = 0
func _ready():
    while self.n < 3:
        self.n = self.n + 1
        var t = get_tree().create_timer(0.05)
        await t.timeout
        self.after = self.after + 1
");

    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        prop(&ml, id, "n"),
        Some(Variant::Int(3)),
        "the while condition must be re-tested after each resume"
    );
    // ⚠️ The statement AFTER the await is the half a flat `remaining` loses
    // first: it belongs to the loop body, not the function body.
    assert_eq!(
        prop(&ml, id, "after"),
        Some(Variant::Int(3)),
        "the rest of the loop body must run on every resume"
    );
}

/// An await nested inside an `if` inside a `for`: both enclosing frames have to
/// be replayed, innermost first.
#[test]
fn await_nested_in_if_inside_for_resumes_both_frames() {
    let (mut ml, id) = run("\
extends Node
var hits = 0
var tail = 0
func _ready():
    for i in 3:
        if true:
            var t = get_tree().create_timer(0.05)
            await t.timeout
            self.hits = self.hits + 1
    self.tail = 99
");

    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        prop(&ml, id, "hits"),
        Some(Variant::Int(3)),
        "the if-body tail must run inside every loop iteration"
    );
    // The function body's own remaining statement runs only after the loop is
    // exhausted — proving the outer frame is replayed last, not dropped.
    assert_eq!(
        prop(&ml, id, "tail"),
        Some(Variant::Int(99)),
        "statements after the loop must still run once it finishes"
    );
}

/// Regression guard: a top-level await (no enclosing construct) still works.
/// This is the shape that was already green, and the continuation stack must not
/// disturb it.
#[test]
fn top_level_await_still_resumes() {
    let (mut ml, id) = run("\
extends Node
var done = false
func _ready():
    var t = get_tree().create_timer(0.05)
    await t.timeout
    self.done = true
");

    for _ in 0..20 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        prop(&ml, id, "done"),
        Some(Variant::Bool(true)),
        "a plain top-level await must be unaffected"
    );
}
