//! pat-0q2kx.8 — synchronous cross-node script method calls.
//!
//! In Godot, `$Enemy.take_damage(5)` runs `take_damage` on the *enemy*
//! (upstream/godot/core/object/object.cpp `Object::callv`), `obj.has_method`
//! reports script and native methods, and a `Callable(obj, "m")` always calls
//! `m` on `obj` (core/variant/callable.cpp).
//!
//! Before this bead Patina had none of that: an unknown method on an object ref
//! threw `UndefinedFunction("ObjectId.take_damage")`, there were no
//! `call`/`callv`/`has_method` arms for object refs, and — worst — a Callable
//! dropped its `target_id` and ran the method on *the calling script*, so
//! `Callable(enemy, "take_damage").call(5)` silently damaged the caller.
//!
//! These tests drive a real scene through the script layer and assert the
//! receiver, the return value, and the target's mutated state.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Node B: holds state and returns a value the caller can observe.
const B_SCRIPT: &str = "\
extends Node
var hp = 100
func take_damage(amount):
    self.hp = self.hp - amount
    return self.hp
";

/// Node A: reaches B four different ways, then asks what B responds to.
/// (`+=`/`elif` avoided for parser-independence, as the sibling script tests do.)
const A_SCRIPT: &str = "\
extends Node
var ret_direct = 0
var ret_call = 0
var ret_callv = 0
var ret_callable = 0
var argc = -99
var has_it = false
var has_missing = true
func _ready():
    self.ret_direct = $\"../B\".take_damage(5)
    self.ret_call = $\"../B\".call(\"take_damage\", 5)
    self.ret_callv = $\"../B\".callv(\"take_damage\", [5])
    self.ret_callable = Callable($\"../B\", \"take_damage\").call(5)
    self.argc = $\"../B\".get_method_argument_count(\"take_damage\")
    self.has_it = $\"../B\".has_method(\"take_damage\")
    self.has_missing = $\"../B\".has_method(\"no_such_method\")
";

/// Builds `Root -> {A, B}` with both scripts attached and fires tree entry, so
/// A's `_ready` has run. Returns the tree and the two node ids.
fn scene_with_a_and_b(
    a_src: &str,
    b_src: &str,
) -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Root", "Node")).unwrap();
    let a = tree.add_child(parent, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(parent, Node::new("B", "Node")).unwrap();

    let b_script = GDScriptNodeInstance::from_source(b_src, b).expect("B script parses");
    tree.attach_script(b, Box::new(b_script));
    let a_script = GDScriptNodeInstance::from_source(a_src, a).expect("A script parses");
    tree.attach_script(a, Box::new(a_script));

    LifecycleManager::enter_tree(&mut tree, parent);
    (tree, a, b)
}

fn script_prop(tree: &SceneTree, node: gdscene::node::NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

#[test]
fn cross_object_script_method_call() {
    let (tree, a, b) = scene_with_a_and_b(A_SCRIPT, B_SCRIPT);

    // Each call lands on B and damages it in turn: 100 -> 95 -> 90 -> 85 -> 80.
    // The return values therefore also prove the *order* and that every form
    // reached the same object rather than a copy.
    assert_eq!(
        script_prop(&tree, a, "ret_direct"),
        Variant::Int(95),
        "$\"../B\".take_damage(5) must run on B and return its hp"
    );
    assert_eq!(
        script_prop(&tree, a, "ret_call"),
        Variant::Int(90),
        "obj.call(\"take_damage\", 5) must run on B"
    );
    assert_eq!(
        script_prop(&tree, a, "ret_callv"),
        Variant::Int(85),
        "obj.callv(\"take_damage\", [5]) must run on B"
    );
    assert_eq!(
        script_prop(&tree, a, "ret_callable"),
        Variant::Int(80),
        "Callable(B, \"take_damage\").call(5) must run on B, not on the caller"
    );

    // B's own state carries every hit, and the script->node property sync makes
    // it readable as a node property too.
    assert_eq!(script_prop(&tree, b, "hp"), Variant::Int(80));
    assert_eq!(
        tree.get_node(b).unwrap().get_property("hp"),
        Variant::Int(80),
        "the target's script vars sync onto its node after a cross-object call"
    );

    // has_method sees B's script method, and only real methods.
    assert_eq!(script_prop(&tree, a, "has_it"), Variant::Bool(true));
    assert_eq!(
        script_prop(&tree, a, "has_missing"),
        Variant::Bool(false),
        "has_method must be false for a name nothing defines"
    );
    assert_eq!(
        script_prop(&tree, a, "argc"),
        Variant::Int(1),
        "get_method_argument_count reports take_damage(amount)'s one parameter"
    );
}

/// The regression this bead exists for: a Callable used to drop its target and
/// run on whichever script invoked it. Both nodes define `whoami`, so a
/// wrong-receiver call is not an error — it silently returns the caller's answer.
#[test]
fn callable_runs_on_its_target_not_the_caller() {
    let a_src = "\
extends Node
var who = \"unset\"
func whoami():
    return \"A\"
func _ready():
    self.who = Callable($\"../B\", \"whoami\").call()
";
    let b_src = "\
extends Node
func whoami():
    return \"B\"
";
    let (tree, a, _b) = scene_with_a_and_b(a_src, b_src);
    assert_eq!(
        script_prop(&tree, a, "who"),
        Variant::String("B".into()),
        "Callable(B, \"whoami\") must call B's method even though A defines one too"
    );
}

/// A Callable bound to the running script itself still calls locally — the
/// target-routing change must not break `Callable(self, "m")`, which is how
/// tween/signal callbacks are written.
#[test]
fn callable_on_self_still_calls_locally() {
    let a_src = "\
extends Node
var who = \"unset\"
func whoami():
    return \"A\"
func _ready():
    self.who = Callable(self, \"whoami\").call()
";
    let b_src = "\
extends Node
func whoami():
    return \"B\"
";
    let (tree, a, _b) = scene_with_a_and_b(a_src, b_src);
    assert_eq!(
        script_prop(&tree, a, "who"),
        Variant::String("A".into()),
        "Callable(self, ...) must stay local"
    );
}

/// The wiring requirement: the same cross-object call works in a scene driven by
/// `MainLoop::step`, from `_process`, not just at tree entry.
#[test]
fn cross_object_call_from_process_under_main_loop() {
    let a_src = "\
extends Node
var ret = 0
func _process(delta):
    self.ret = $\"../B\".take_damage(5)
";
    let (tree, a, b) = scene_with_a_and_b(a_src, B_SCRIPT);
    // Tree entry alone must not have damaged B — the call lives in _process.
    assert_eq!(script_prop(&tree, b, "hp"), Variant::Int(100));

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), a, "ret"),
        Variant::Int(95),
        "a frame of MainLoop must carry the cross-object call"
    );
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), b, "hp"),
        Variant::Int(90),
        "each frame damages B once more"
    );
}
