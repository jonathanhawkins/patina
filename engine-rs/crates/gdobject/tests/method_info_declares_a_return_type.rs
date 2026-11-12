//! pat-s3rrv.57: `MethodInfo` carries a return type.
//!
//! `class_get_method_list` emitted `name`/`args`/`default_args`/`flags` but
//! never Godot's `return` key, because `MethodInfo` had nowhere to put one — so
//! a documentation generator or an autocomplete provider could see that
//! `class_exists` takes one argument and not that it answers a bool.
//!
//! ClassDB's own 30 return types are generated from
//! `upstream/godot/doc/classes/ClassDB.xml`.

use gdobject::class_db::{get_class_info, register_all_classes, MethodInfo};
use gdvariant::Variant;

fn classdb_method(name: &str) -> MethodInfo {
    register_all_classes();
    get_class_info("ClassDB")
        .expect("ClassDB is registered")
        .methods
        .into_iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("ClassDB must declare `{name}`"))
}

/// ⚠️ `None` means "not declared", not "returns void". The two are
/// indistinguishable downstream — both serialise as a NIL type — exactly as an
/// empty `arguments` is indistinguishable from "takes no arguments".
#[test]
fn a_plain_method_declares_no_return() {
    let m = MethodInfo::new("accelerate", 2);
    assert!(m.return_value.is_none());
}

#[test]
fn classdb_declares_all_thirty_of_its_returns() {
    register_all_classes();
    let methods = get_class_info("ClassDB").expect("registered").methods;

    let declared = methods.iter().filter(|m| m.return_value.is_some()).count();
    assert_eq!(
        declared, 30,
        "every ClassDB method returns something in Godot; none of the 30 is void"
    );
}

#[test]
fn the_declared_type_comes_from_the_value() {
    assert_eq!(
        classdb_method("class_exists")
            .return_value
            .expect("declared")
            .default_value,
        Variant::Bool(false),
        "class_exists answers a bool"
    );
    assert_eq!(
        classdb_method("class_get_signal")
            .return_value
            .expect("declared")
            .default_value,
        Variant::Dictionary(Default::default())
    );

    // ⚠️ The value is a TYPE MARKER, not a default: Godot's return dict has no
    // default. `Variant::Bool(false)` above does not mean class_exists returns
    // false.
    let name = classdb_method("class_exists")
        .return_value
        .expect("declared")
        .name;
    assert_eq!(name, "", "a return has no name, unlike an argument");
}

/// A typed array carries its ELEMENT type, and an object return would carry its
/// class, in the same `hint_string` — which is where `class_get_method_list`
/// reads `class_name` from.
#[test]
fn a_typed_array_return_names_its_element_type() {
    let ret = classdb_method("class_get_method_list")
        .return_value
        .expect("declared");

    assert_eq!(
        ret.default_value,
        Variant::Array(Default::default()),
        "Godot reports `Dictionary[]` as an ARRAY"
    );
    assert_eq!(
        ret.hint_string, "Dictionary",
        "with the element type alongside it, or a caller cannot tell it from an \
         untyped Array"
    );
}
