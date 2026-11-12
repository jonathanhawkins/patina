//! pat-j0r2: func dispatch via object method table.

use gdobject::class_db::*;
use gdobject::GodotObject;
use gdvariant::Variant;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .method(MethodInfo::new("get_name", 0))
            .method(MethodInfo::new("set_name", 1)),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .method(MethodInfo::new("get_position", 0))
            .method(MethodInfo::new("set_position", 1)),
    );
    guard
}

#[test]
fn class_has_own_method() {
    let _g = setup();
    assert!(class_has_method("Node", "get_name"));
    assert!(class_has_method("Node", "set_name"));
}

#[test]
fn class_has_inherited_method() {
    let _g = setup();
    assert!(
        class_has_method("Node2D", "get_name"),
        "inherited from Node"
    );
    assert!(
        class_has_method("Node2D", "set_name"),
        "inherited from Node"
    );
}

#[test]
fn class_has_own_and_inherited() {
    let _g = setup();
    assert!(class_has_method("Node2D", "get_position"), "own");
    assert!(class_has_method("Node2D", "get_name"), "inherited");
}

#[test]
fn class_does_not_have_child_method() {
    let _g = setup();
    assert!(
        !class_has_method("Node", "get_position"),
        "Node2D method not on Node"
    );
}

#[test]
fn instantiate_has_methods_via_class_info() {
    let _g = setup();
    let info = get_class_info("Node2D").unwrap();
    assert!(info.methods.iter().any(|m| m.name == "get_position"));
    assert!(info.methods.iter().any(|m| m.name == "set_position"));
}

#[test]
fn method_argument_count() {
    let _g = setup();
    let info = get_class_info("Node").unwrap();
    let get_name = info.methods.iter().find(|m| m.name == "get_name").unwrap();
    assert_eq!(get_name.argument_count, 0);
    let set_name = info.methods.iter().find(|m| m.name == "set_name").unwrap();
    assert_eq!(set_name.argument_count, 1);
}
