//! pat-s3rrv.57: ClassDB and WeakRef must exist in the registry as classes.
//!
//! `ClassRegistration::new("ClassDB")` had **0 hits workspace-wide**. Godot
//! lists ClassDB under both `.singletons[]` and `.classes[]`, so
//! `ClassDB.class_exists("ClassDB")` answered false where Godot answers true and
//! `class_get_method_list("ClassDB")` — the reflection surface of the reflection
//! API itself — came back empty. `WeakRef` was not registered at all.
//!
//! The 30 signatures and their qualifiers are generated from
//! `upstream/godot/doc/classes/ClassDB.xml`, not typed by hand.

use gdobject::class_db::{
    class_exists, get_class_info, is_parent_class, register_all_classes, MethodInfo,
};

/// Godot's `MethodFlags` (`core/object/object.h:224-233`).
const METHOD_FLAG_NORMAL: i64 = 1;
const METHOD_FLAG_CONST: i64 = 4;
const METHOD_FLAG_VARARG: i64 = 16;

fn method(class: &str, name: &str) -> MethodInfo {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
        .into_iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
}

#[test]
fn classdb_is_registered_with_all_thirty_methods() {
    register_all_classes();

    assert!(
        class_exists("ClassDB"),
        "ClassDB.class_exists(\"ClassDB\") must be true, as it is in Godot"
    );
    assert_eq!(
        get_class_info("ClassDB").expect("registered").parent_class,
        "Object"
    );

    let methods = get_class_info("ClassDB").expect("registered").methods;
    assert_eq!(
        methods.len(),
        30,
        "ClassDB.xml declares exactly 30 methods; got {:?}",
        methods.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    for name in [
        "instantiate",
        "class_get_method_list",
        "class_get_property_list",
        "is_parent_class",
        "get_inheriters_from_class",
    ] {
        assert!(
            methods.iter().any(|m| m.name == name),
            "`{name}` must be reported"
        );
    }
}

/// **The discriminating pair**, and the first real registration in the engine to
/// carry either flag: 27 of ClassDB's methods are `const`, and
/// `class_call_static` is the lone `vararg`. Asserting one alone would pass
/// against a table that sets the same flags everywhere.
#[test]
fn const_and_vararg_qualifiers_survive_registration() {
    assert_eq!(
        method("ClassDB", "class_exists").godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_CONST,
        "a const method reports 5"
    );

    let vararg = method("ClassDB", "class_call_static");
    assert_eq!(
        vararg.godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_VARARG,
        "class_call_static is vararg and NOT const — 17, not 21"
    );
    assert!(vararg.is_vararg && !vararg.is_const);

    // A method with no qualifiers at all sits between the two.
    assert_eq!(
        method("ClassDB", "class_get_property_getter").godot_flags(),
        METHOD_FLAG_NORMAL
    );
}

#[test]
fn weakref_is_registered_under_refcounted() {
    register_all_classes();

    assert!(class_exists("WeakRef"), "WeakRef was not registered at all");
    assert!(
        is_parent_class("WeakRef", "RefCounted"),
        "WeakRef derives from RefCounted, which is what makes it a counted \
         handle rather than a bare Object"
    );
    assert_eq!(method("WeakRef", "get_ref").argument_count, 0);
}
