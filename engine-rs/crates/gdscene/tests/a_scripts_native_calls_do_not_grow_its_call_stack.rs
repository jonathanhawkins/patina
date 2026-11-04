//! A script's bare native call (`get_position()`, `move_and_slide()`) must not
//! leave anything on the GDScript call stack.
//!
//! `call_self_func_or_native` tries the script's own function first and falls
//! back to the node's native method on `UndefinedFunction`. The lookup failed
//! AFTER `call_user_func_impl` had pushed a `StackFrame` and replaced
//! `current_function`, and the early return undid neither. On the platformer
//! demo that leaked ~22 frames per game frame — 481k live allocations after a
//! few minutes of Play — and `get_stack()` grew by one entry per native call.

use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

#[test]
fn bare_native_calls_leave_the_call_stack_as_they_found_it() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("Mover", "Node2D")).unwrap();
    let src = "\
extends Node2D
var depth = -1
var top = \"\"
var max_depth = 0
func _physics_process(_delta):
    helper()
func helper():
    for i in range(10):
        get_position()
        set_position(Vector2(i, 0))
    var s = get_stack()
    depth = s.size()
    if s.size() > max_depth:
        max_depth = s.size()
    top = s[0][\"function\"]
";
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(src, id).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    let script = ml.tree().get_script(id).unwrap();
    let get = |name: &str| script.get_property(name);
    // Not vacuous: the native calls ran (the last set_position landed).
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => assert_eq!((v.x, v.y), (9.0, 0.0), "set_position must have run"),
        other => panic!("position: {other:?}"),
    }
    // `helper` is the only user frame on the stack while it runs (the entry
    // `_physics_process` is dispatched by the host, which pushes none).
    assert_eq!(get("depth"), Some(Variant::Int(1)), "get_stack() inside helper");
    assert_eq!(
        get("max_depth"),
        Some(Variant::Int(1)),
        "20 native calls a frame for 120 frames must not grow the stack"
    );
    assert_eq!(get("top"), Some(Variant::String("helper".into())), "the top frame is helper");
}
