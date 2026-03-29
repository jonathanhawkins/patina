//! pat-ra42: func dispatch via object method table.
//!
//! Validates that:
//! 1. ScriptInstance::call_method dispatches to user-defined functions
//! 2. ClassDB method registration and lookup works
//! 3. ScriptInstance::has_method returns correct results
//! 4. Method argument forwarding works correctly
//! 5. Return values propagate through the dispatch chain
//! 6. Calling undefined methods returns appropriate errors
//! 7. ScriptBridge routes method calls to attached scripts

use std::sync::Mutex;

use gdcore::id::ObjectId;
use gdscript_interop::bindings::{ScriptError, ScriptInstance};
use gdscript_interop::bridge::{NativeScript, ScriptBridge};
use gdvariant::Variant;

static CLASSDB_LOCK: Mutex<()> = Mutex::new(());

// ===========================================================================
// 1. ClassDB method registration and lookup
// ===========================================================================

#[test]
fn ra42_classdb_method_registration() {
    let _g = CLASSDB_LOCK.lock().unwrap();
    gdobject::class_db::clear_for_testing();

    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Object"),
    );
    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Node")
            .parent("Object")
            .method(gdobject::class_db::MethodInfo::new("get_name", 0))
            .method(gdobject::class_db::MethodInfo::new("queue_free", 0)),
    );
    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Node2D")
            .parent("Node")
            .method(gdobject::class_db::MethodInfo::new("get_position", 0))
            .method(gdobject::class_db::MethodInfo::new("set_position", 1)),
    );

    let methods = gdobject::class_db::get_method_list("Node2D");
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"get_position"));
    assert!(method_names.contains(&"set_position"));
}

#[test]
fn ra42_classdb_inherited_method_lookup() {
    let _g = CLASSDB_LOCK.lock().unwrap();
    gdobject::class_db::clear_for_testing();

    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Object"),
    );
    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Node")
            .parent("Object")
            .method(gdobject::class_db::MethodInfo::new("get_name", 0)),
    );
    gdobject::class_db::register_class(
        gdobject::class_db::ClassRegistration::new("Node2D")
            .parent("Node")
            .method(gdobject::class_db::MethodInfo::new("get_position", 0)),
    );

    let methods = gdobject::class_db::get_method_list("Node2D");
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"get_name"), "Node2D should inherit get_name from Node");
}

// ===========================================================================
// 2. NativeScript call_method dispatch
// ===========================================================================

#[test]
fn ra42_native_script_call_method() {
    let mut script = NativeScript::builder("TestScript")
        .method("greet", |_args| Ok(Variant::String("hello".into())))
        .build();

    let result = script.call_method("greet", &[]);
    assert_eq!(result.unwrap(), Variant::String("hello".into()));
}

#[test]
fn ra42_native_script_call_method_with_args() {
    let mut script = NativeScript::builder("MathScript")
        .method("add", |args| {
            let a = match &args[0] {
                Variant::Int(i) => *i,
                _ => 0,
            };
            let b = match &args[1] {
                Variant::Int(i) => *i,
                _ => 0,
            };
            Ok(Variant::Int(a + b))
        })
        .build();

    let result = script.call_method("add", &[Variant::Int(3), Variant::Int(7)]);
    assert_eq!(result.unwrap(), Variant::Int(10));
}

#[test]
fn ra42_native_script_undefined_method_returns_error() {
    let mut script = NativeScript::builder("EmptyScript").build();

    let result = script.call_method("nonexistent", &[]);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ScriptError::MethodNotFound(_)));
}

// ===========================================================================
// 3. ScriptInstance::has_method
// ===========================================================================

#[test]
fn ra42_has_method_returns_true_for_defined() {
    let script = NativeScript::builder("TestScript")
        .method("_ready", |_| Ok(Variant::Nil))
        .method("_process", |_| Ok(Variant::Nil))
        .build();

    assert!(script.has_method("_ready"));
    assert!(script.has_method("_process"));
}

#[test]
fn ra42_has_method_returns_false_for_undefined() {
    let script = NativeScript::builder("TestScript")
        .method("_ready", |_| Ok(Variant::Nil))
        .build();

    assert!(!script.has_method("nonexistent"));
    assert!(!script.has_method("_process"));
}

// ===========================================================================
// 4. ScriptBridge routes calls to attached scripts
// ===========================================================================

#[test]
fn ra42_script_bridge_attach_and_call() {
    let mut bridge = ScriptBridge::new();

    let script = NativeScript::builder("PlayerScript")
        .method("get_health", |_| Ok(Variant::Int(100)))
        .build();

    let oid = ObjectId::next();
    bridge.attach_script(oid, Box::new(script));
    assert!(bridge.has_script(oid));

    let result = bridge.call(oid, "get_health", &[]);
    assert_eq!(result.unwrap(), Variant::Int(100));
}

#[test]
fn ra42_script_bridge_no_script_returns_error() {
    let mut bridge = ScriptBridge::new();
    let result = bridge.call(ObjectId::next(), "anything", &[]);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ScriptError::NoScript));
}

#[test]
fn ra42_script_bridge_detach() {
    let mut bridge = ScriptBridge::new();

    let script = NativeScript::builder("TempScript")
        .method("test", |_| Ok(Variant::Bool(true)))
        .build();

    let oid = ObjectId::next();
    bridge.attach_script(oid, Box::new(script));
    assert!(bridge.has_script(oid));

    bridge.detach_script(oid);
    assert!(!bridge.has_script(oid));
}

// ===========================================================================
// 5. Multiple methods on same script
// ===========================================================================

#[test]
fn ra42_multiple_methods_dispatch_correctly() {
    let mut script = NativeScript::builder("MultiMethod")
        .method("method_a", |_| Ok(Variant::String("A".into())))
        .method("method_b", |_| Ok(Variant::String("B".into())))
        .method("method_c", |_| Ok(Variant::String("C".into())))
        .build();

    assert_eq!(script.call_method("method_a", &[]).unwrap(), Variant::String("A".into()));
    assert_eq!(script.call_method("method_b", &[]).unwrap(), Variant::String("B".into()));
    assert_eq!(script.call_method("method_c", &[]).unwrap(), Variant::String("C".into()));
}

// ===========================================================================
// 6. list_methods returns correct metadata
// ===========================================================================

#[test]
fn ra42_list_methods_returns_all_registered() {
    let script = NativeScript::builder("TestScript")
        .method("_ready", |_| Ok(Variant::Nil))
        .method("_process", |_| Ok(Variant::Nil))
        .method("custom_func", |_| Ok(Variant::Nil))
        .build();

    let methods = script.list_methods();
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"_ready"));
    assert!(names.contains(&"_process"));
    assert!(names.contains(&"custom_func"));
    assert_eq!(methods.len(), 3);
}

// ===========================================================================
// 7. ScriptBridge property access through bridge
// ===========================================================================

#[test]
fn ra42_bridge_property_access() {
    let mut bridge = ScriptBridge::new();

    let script = NativeScript::builder("PropScript")
        .property("health", Variant::Int(100))
        .property("name", Variant::String("Player".into()))
        .build();

    let oid = ObjectId::next();
    bridge.attach_script(oid, Box::new(script));

    assert_eq!(bridge.get_property(oid, "health"), Some(Variant::Int(100)));
    assert!(bridge.set_property(oid, "health", Variant::Int(50)));
    assert_eq!(bridge.get_property(oid, "health"), Some(Variant::Int(50)));
}
