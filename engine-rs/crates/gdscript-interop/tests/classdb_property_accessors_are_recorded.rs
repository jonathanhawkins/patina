//! pat-s3rrv.57: `ClassDB.class_get_property_getter` / `class_get_property_setter`.
//!
//! Both raised `UndefinedFunction`, while `class_get_method_list` — once ClassDB
//! was registered as a class — happily *declared* them. Reflection and behaviour
//! disagreed: a tool could see the method and not call it.
//!
//! # Why the names are stored rather than derived
//!
//! The tempting shortcut is to synthesise `get_<prop>` / `set_<prop>` and check
//! the method exists. Measured against Godot's own documentation, that is wrong
//! for nearly half the API:
//!
//! ```text
//! getter == get_<x> | is_<x> | has_<x>   2777/5371  (51.7%)
//! setter == set_<x>                      4483/5371  (83.5%)
//! ```
//!
//! The misses are ordinary, not exotic — several members routinely share one
//! accessor pair. So `PropertyInfo` now carries the real names, populated from
//! the `getter=` / `setter=` attributes that `upstream/godot/doc/classes/*.xml`
//! puts on every `<member>`.
//!
//! ⚠️ An empty answer means "not recorded", not "no accessor". Godot also
//! answers empty for an unknown class or property, so the two are consistent at
//! the boundary.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func conventional_getter():\n\
                   \x20   return ClassDB.class_get_property_getter(\"Node2D\", \"position\")\n\
                   \n\
                   func conventional_setter():\n\
                   \x20   return ClassDB.class_get_property_setter(\"Node2D\", \"position\")\n\
                   \n\
                   func shared_accessor():\n\
                   \x20   return ClassDB.class_get_property_getter(\"PinJoint3D\", \"params/bias\")\n\
                   \n\
                   func inherited_property():\n\
                   \x20   return ClassDB.class_get_property_getter(\"Sprite2D\", \"position\")\n\
                   \n\
                   func unknown_class():\n\
                   \x20   return ClassDB.class_get_property_getter(\"NoSuchClassAnywhere\", \"x\")\n\
                   \n\
                   func unknown_property():\n\
                   \x20   return ClassDB.class_get_property_getter(\"Node2D\", \"no_such_property\")\n";

/// Runs one method of the probe script.
///
/// Mirrors the call sequence of the green
/// `classdb_instantiate_builds_non_node_classes` test — `run_class` →
/// `instantiate_class` → `call_instance_method` — rather than inventing one.
///
/// ⚠️ ClassDB answers out of the registry, so an unpopulated one makes every
/// case here return `""` and the test would "pass" against a lookup that knows
/// nothing.
fn call(method: &str) -> Variant {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("`{method}` failed: {e:?}"))
}

fn as_str(v: Variant) -> String {
    match v {
        Variant::String(s) => s,
        Variant::StringName(sn) => sn.as_str().to_string(),
        other => panic!("expected a string accessor name, got {other:?}"),
    }
}

/// **The acceptance case**: the methods answer with real accessor names.
#[test]
fn property_accessors_are_reported() {
    assert_eq!(
        as_str(call("conventional_getter")),
        "get_position",
        "Node2D.position reads through get_position",
    );
    assert_eq!(
        as_str(call("conventional_setter")),
        "set_position",
        "…and writes through set_position",
    );
}

/// 🔑 The case that proves the names are STORED, not derived.
///
/// `PinJoint3D.params/bias` reads through **`get_param`** — the property name
/// appears nowhere in its accessor. No `get_<prop>` rule can produce this, so a
/// derived implementation fails here while passing the conventional case above.
#[test]
fn an_accessor_that_no_naming_rule_could_produce() {
    assert_eq!(
        as_str(call("shared_accessor")),
        "get_param",
        "PinJoint3D's params/* members all read through one get_param; a \
         name-derived getter would answer `get_params/bias`, which does not \
         exist",
    );
}

/// A property declared on a base class keeps the base's accessors, so the
/// lookup has to walk inheritance rather than only the class's own list.
#[test]
fn an_inherited_property_reports_its_base_accessor() {
    assert_eq!(
        as_str(call("inherited_property")),
        "get_position",
        "Sprite2D inherits position from Node2D and must report its accessor",
    );
}

/// Unknown class and unknown property both answer empty rather than erroring —
/// Godot's behaviour, and what makes this safe to call while walking a
/// property list that may contain script-defined names.
#[test]
fn unknown_lookups_answer_empty_rather_than_failing() {
    assert_eq!(as_str(call("unknown_class")), "");
    assert_eq!(as_str(call("unknown_property")), "");
}
