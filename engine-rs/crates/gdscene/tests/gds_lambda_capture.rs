//! pat-s3rrv.67 acceptance: lambda captures survive to a LATER FRAME, driven
//! through a real `SceneTree` rather than through the interpreter directly.
//!
//! `lambda_captures_enclosing_locals` (gdscript-interop) already pins the
//! capture semantics where the logic lives. This file is the wiring requirement
//! the bead asks for on top of it: *"prove it through a stored-then-later-
//! invoked callable driven by the SceneTree"*. The distinction is the point —
//! a lambda that works inside one `run()` call can still fail once it is parked
//! in a signal connection and fired on a subsequent frame, because by then the
//! environment it was written in is long gone. That is the failure Godot's
//! `GDScriptLambdaCallable` exists to prevent (`gdscript_lambda_callable.cpp`).
//!
//! ⚠️ Each assertion is paired with a control, because "the captured value is
//! wrong", "the callable never ran" and "`_ready` never ran" all produce the
//! SAME reading on a single property. `ran` proves `_ready` executed, and
//! `result` is checked BEFORE the invocation to prove that merely storing the
//! lambda did not run it — so the final assertion can only be about whether the
//! capture survived the frame boundary.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn attach(tree: &mut SceneTree, id: NodeId, src: &str) {
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
}

fn prop(ml: &MainLoop, id: NodeId, name: &str) -> Option<Variant> {
    ml.tree().get_script(id).and_then(|s| s.get_property(name))
}

fn ints(v: Option<Variant>) -> Vec<i64> {
    match v {
        Some(Variant::Array(items)) => items
            .iter()
            .map(|i| match i {
                Variant::Int(n) => *n,
                Variant::Float(f) => *f as i64,
                _ => i64::MIN,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `_ready` writes a local and PARKS a lambda that closes over it in a member,
/// to be invoked several frames later.
///
/// ⚠️ The lambda body here deliberately contains no `self`, so that a failure
/// is unambiguously about the frame boundary. The bead's literal wording —
/// `func(): self.result = msg` connected to a signal — is covered separately
/// below by `a_lambda_connected_to_a_signal_writes_through_self`, where `self`
/// resolves because the dispatch runs through the connecting node's own script.
const LATER_FRAME: &str = "\
extends Node
var cb = null
var ran = false
var result = \"unset\"

func _ready():
    var msg = \"hi\"
    self.cb = func(): return msg
    self.ran = true

func fire():
    self.result = self.cb.call()
";

/// **The acceptance clause.** A local captured in `_ready` is still readable
/// when the callable runs on a later frame.
#[test]
fn a_captured_local_survives_to_a_later_frame() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    attach(&mut tree, main, LATER_FRAME);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main);

    // CONTROL: `_ready` actually ran. Without this, a tree that never fired
    // `_ready` would leave `result` at its initial value and read exactly like
    // a capture that failed.
    assert_eq!(
        prop(&ml, main, "ran"),
        Some(Variant::Bool(true)),
        "`_ready` must have run, else nothing below is about capture"
    );
    assert_eq!(
        prop(&ml, main, "result"),
        Some(Variant::String("unset".to_string())),
        "storing a lambda must not invoke it"
    );

    // Advance real frames so `_ready`'s environment is well and truly gone.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    ml.tree_mut().call_script_method(main, "fire", &[]);

    assert_eq!(
        prop(&ml, main, "result"),
        Some(Variant::String("hi".to_string())),
        "the parked lambda must still see the `msg` it was written beside, \
         three frames after `_ready` returned. `unset` means the callable \
         never ran; an empty or missing value means the captured local did not \
         survive the frame boundary, which is exactly what a lambda holding a \
         reference to a dead scope would produce"
    );
}

/// The bead's second clause, at scene level: three lambdas built in a loop each
/// remember THEIR OWN loop variable.
const LOOP_CAPTURE: &str = "\
extends Node
var results = []

func build():
    var cbs = []
    for i in range(3):
        cbs.append(func(): return i)
    var out = []
    for cb in cbs:
        out.append(cb.call())
    self.results = out
";

#[test]
fn three_lambdas_each_capture_their_own_loop_variable() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    attach(&mut tree, main, LOOP_CAPTURE);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main);
    ml.tree_mut().call_script_method(main, "build", &[]);

    assert_eq!(
        ints(prop(&ml, main, "results")),
        vec![0, 1, 2],
        "each lambda must remember the `i` it was created with — [2, 2, 2] \
         means they all read the loop variable at call time, and an empty vec \
         means the call raised or the property never landed"
    );
}

/// Connects a lambda to a signal on the PARENT node, so the emit can be driven
/// from Rust on a later frame while the lambda's target — and therefore its
/// `self` — is still this node's own script.
///
/// ⚠️ `get_parent().connect(...)`, not `self.connect(...)`. `self` inside a
/// script body is the `ClassInstance`, which does not route to the ObjectId
/// method dispatch where `connect` lives; `get_parent()` yields a real
/// ObjectId. The *target* is unaffected by that choice — `callable_target`
/// resolves a lambda against `current_node_id`, the CONNECTING node, whatever
/// object the signal is emitted on.
///
/// ⚠️ **The lambda bodies are method CALLS, not assignments, and that is forced
/// by the parser rather than chosen.** A single-line lambda body is parsed as
/// one *expression* with an implicit return (`parser.rs`, `Token::Func`), so
/// the bead's literal `func(): self.result = msg` does not parse at all —
/// `unexpected token += at line 8` / `expected expression` is what an earlier
/// draft of this fixture got. GDScript assignment is a statement, and the
/// multi-line `parse_block()` branch for lambda bodies has no fixture anywhere
/// in this repo, so it is not exercised here. `func(): self._note_msg(msg)`
/// keeps the whole claim intact — the lambda still reaches `self`, and still
/// carries the captured `msg` — using the single-expression form
/// `lambda_captures_enclosing_locals` already proves.
///
/// ⚠️ Each lambda is also bound to a local before being connected rather than
/// written inline in the argument list, which is the shape the sibling tests in
/// this file prove. Nothing about the connection depends on that spelling.
const SIGNAL_CONNECT: &str = "\
extends Node
var ran = false
var hits = 0
var result = \"unset\"

func _note_hit():
    self.hits += 1

func _note_msg(v):
    self.result = v

func _ready():
    var msg = \"hi\"
    var bump = func(): self._note_hit()
    var store = func(): self._note_msg(msg)
    get_parent().connect(\"ping\", bump)
    get_parent().connect(\"ping\", store)
    self.ran = true
";

/// Builds `Parent(Node) -> Main(Node)` with `SIGNAL_CONNECT` on `Main`, runs
/// `_ready`, then advances three frames so the environment `_ready` ran in is
/// gone before anything is emitted. Returns `(loop, parent, main)`.
fn connected_scene() -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
    let main = tree.add_child(parent, Node::new("Main", "Node")).unwrap();
    attach(&mut tree, main, SIGNAL_CONNECT);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main);
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    (ml, parent, main)
}

/// 🔑 THE CONNECTION ITSELF — the positive control for the test below.
///
/// ⚠️ Split out deliberately. `sig.connect(func(): ...)` used to raise
/// `connect() Callable has no bindable target/method` outright: a
/// `gdobject::signal::Connection` stores `(target_id, method NAME)`, and a
/// lambda has no name, so `callable_target` returned `None` and the connect
/// never happened. With only the `self`-writing test below, that failure and a
/// failure of `self`/capture would produce the SAME reading — an unchanged
/// `result` — and the difference is what tells you which half broke.
#[test]
fn a_lambda_can_be_connected_to_a_signal_and_fires() {
    let (mut ml, parent, main) = connected_scene();

    assert_eq!(
        prop(&ml, main, "ran"),
        Some(Variant::Bool(true)),
        "`_ready` must have completed — if `connect()` raised, it stopped short \
         of setting `ran` and nothing below is about lambdas"
    );
    assert_eq!(
        prop(&ml, main, "hits"),
        Some(Variant::Int(0)),
        "connecting must not invoke the lambda"
    );

    let _ = ml.tree_mut().emit_signal(parent, "ping", &[]);

    assert_eq!(
        prop(&ml, main, "hits"),
        Some(Variant::Int(1)),
        "the connected lambda must run exactly once per emit"
    );

    // And it is a live connection, not a one-shot: a second emit fires it again.
    let _ = ml.tree_mut().emit_signal(parent, "ping", &[]);
    assert_eq!(
        prop(&ml, main, "hits"),
        Some(Variant::Int(2)),
        "a second emit must fire the lambda again — 1 here means the connection \
         was treated as one-shot"
    );
}

/// **The bead's acceptance clause.** A lambda connected to a node signal,
/// fired on a LATER frame, carries its captured `msg` through `self` and lands
/// `"hi"` on the connecting instance.
///
/// ⚠️ Spelled `func(): self._note_msg(msg)` rather than the bead's literal
/// `func(): self.result = msg` — see `SIGNAL_CONNECT`; the literal form is not
/// parseable as a single-line lambda body. The semantics asserted are the
/// bead's: captured local + `self` + a later frame.
///
/// ⚠️ Two independent things must hold for this to pass, which is why the
/// control above exists: the captured `msg` has to survive the frame boundary,
/// AND `self` inside the lambda has to be the instance that connected it. The
/// second is not a capture at all — `CallableRef::Lambda` still carries no
/// `self_id`. It works because the connection's target is the connecting node,
/// so the emit dispatches through THAT node's script and installs it as
/// `self_instance` before the lambda body runs, then writes the mutated
/// properties back out. A lambda connected to some other object's signal and
/// dispatched against a different instance would still see the wrong `self`;
/// that case is not what this bead asks for and is not claimed here.
#[test]
fn a_lambda_connected_to_a_signal_writes_through_self() {
    let (mut ml, parent, main) = connected_scene();

    assert_eq!(
        prop(&ml, main, "result"),
        Some(Variant::String("unset".to_string())),
        "storing the connection must not run the lambda"
    );

    let _ = ml.tree_mut().emit_signal(parent, "ping", &[]);

    assert_eq!(
        prop(&ml, main, "result"),
        Some(Variant::String("hi".to_string())),
        "`self.result = msg` in a signal-connected lambda must land `hi` on the \
         connecting instance. `unset` means either the lambda never ran (check \
         `a_lambda_can_be_connected_to_a_signal_and_fires` — if that also \
         fails, the connect is the problem) or the write went to some other \
         `self`; a missing/empty value means `msg` did not survive the three \
         frames between `_ready` and the emit"
    );
}
