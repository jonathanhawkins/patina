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
/// ⚠️ The lambda bodies here deliberately contain NO `self`. The bead's literal
/// wording is `func(): self.result = msg`, but `self` capture is the one piece
/// of this bead still undelivered — `CallableRef::Lambda` has a `captures` map
/// and no `self_id` — so a lambda writing through `self` would be testing that
/// gap rather than the frame-boundary property this file is for. Returning the
/// captured value and letting the ORDINARY method store it keeps the assertion
/// on the delivered behaviour, and keeps a failure here meaningful.
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
