//! pat-m9ig: GDScript get_instance_id callable parity tests.
//!
//! Verifies that `get_instance_id()` is callable from GDScript in both
//! self-call and receiver-call forms, returning positive stable integers
//! matching the structural contract from node_instance_id_parity_test.

use gdscript_interop::bindings::SceneAccess;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

// ── Minimal mock SceneAccess ────────────────────────────────────────

struct StubAccess;

impl SceneAccess for StubAccess {
    fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
        None
    }
    fn get_parent(&self, _node: u64) -> Option<u64> {
        None
    }
    fn get_children(&self, _node: u64) -> Vec<u64> {
        vec![]
    }
    fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
        Variant::Nil
    }
    fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
    fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
    fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
    fn get_node_name(&self, _node: u64) -> Option<String> {
        None
    }
}

// ── Self-call: get_instance_id() in a class method ──────────────────

#[test]
fn self_get_instance_id_returns_positive_int() {
    // Godot: calling get_instance_id() inside a script method returns the
    // object's own instance ID as a positive integer.
    let src = "\
extends Node2D
func check_id():
    return get_instance_id()
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");

    let node_id: u64 = 42;
    interp.set_scene_access(Box::new(StubAccess), node_id);

    let result = interp
        .call_instance_method(&mut inst, "check_id", &[])
        .expect("method call failed");

    assert_eq!(result, Variant::Int(node_id as i64));
}

#[test]
fn self_get_instance_id_returns_stable_value_across_calls() {
    // Godot: get_instance_id() is stable — calling it multiple times returns
    // the same value.
    let src = "\
extends Node
func check():
    var a = get_instance_id()
    var b = get_instance_id()
    return a == b
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    interp.set_scene_access(Box::new(StubAccess), 99);

    let result = interp
        .call_instance_method(&mut inst, "check", &[])
        .expect("method call failed");
    assert_eq!(result, Variant::Bool(true));
}

#[test]
fn self_get_instance_id_stored_in_var() {
    // Practical pattern: scripts store their ID for later comparisons.
    let src = "\
extends Node2D
var my_id = 0
func _ready():
    my_id = get_instance_id()
func get_my_id():
    return my_id
";
    let node_id: u64 = 777;
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    interp.set_scene_access(Box::new(StubAccess), node_id);

    // Simulate _ready storing the ID.
    interp
        .call_instance_method(&mut inst, "_ready", &[])
        .expect("_ready failed");

    let result = interp
        .call_instance_method(&mut inst, "get_my_id", &[])
        .expect("get_my_id failed");
    assert_eq!(result, Variant::Int(node_id as i64));
}

// ── Receiver-call: obj.get_instance_id() on an ObjectId variant ─────

#[test]
fn receiver_get_instance_id_returns_same_id() {
    // Godot: var other = get_node(".."); other.get_instance_id() returns
    // that node's instance ID.
    let src = "\
extends Node
func check(obj_id):
    return obj_id.get_instance_id()
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");

    let raw_id: u64 = 12345;
    let obj = Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw_id));

    let result = interp
        .call_instance_method(&mut inst, "check", &[obj])
        .expect("method call failed");
    assert_eq!(result, Variant::Int(raw_id as i64));
}

#[test]
fn receiver_get_instance_id_different_objects_different_ids() {
    // Godot: two different objects have different instance IDs.
    let src = "\
extends Node
func compare(a, b):
    return a.get_instance_id() != b.get_instance_id()
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");

    let a = Variant::ObjectId(gdcore::id::ObjectId::from_raw(100));
    let b = Variant::ObjectId(gdcore::id::ObjectId::from_raw(200));

    let result = interp
        .call_instance_method(&mut inst, "compare", &[a, b])
        .expect("method call failed");
    assert_eq!(result, Variant::Bool(true));
}

// ── Error path: no scene access ─────────────────────────────────────

#[test]
fn self_get_instance_id_without_scene_access_errors() {
    // Without scene access set, get_instance_id() should return an error,
    // not panic or return garbage.
    let src = "\
extends Node
func check():
    return get_instance_id()
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("class parse failed");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("instantiate failed");
    // Deliberately do NOT set scene access.

    let result = interp.call_instance_method(&mut inst, "check", &[]);
    assert!(
        result.is_err(),
        "get_instance_id() without scene access should error"
    );
}
