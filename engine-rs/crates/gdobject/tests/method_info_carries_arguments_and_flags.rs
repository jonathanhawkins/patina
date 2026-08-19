//! pat-s3rrv.57 (merged finding 6): `MethodInfo` must carry Godot's argument
//! list and method flags, not just a name and an arity.
//!
//! ClassDB could not tell a virtual method from a concrete one — 278 of 1413
//! declared pairs, with `Node` and `Control` declaring none — and that is the
//! same gap that made `ClassDB.class_get_method_list` emit `args = []` and
//! `flags = 0` for every method in the engine.
//!
//! The fields are additive: all 1859 `MethodInfo::new(name, argc)` sites keep
//! working unchanged and describe a plain concrete method, which is exactly what
//! they meant.

use gdobject::class_db::{
    clear_for_testing, get_method_list, register_class, ClassRegistration, MethodInfo, PropertyInfo,
};
use gdvariant::Variant;

/// Godot's `MethodFlags` (`core/object/object.h:224-233`).
const METHOD_FLAG_NORMAL: i64 = 1;
const METHOD_FLAG_CONST: i64 = 4;
const METHOD_FLAG_VIRTUAL: i64 = 8;
const METHOD_FLAG_VARARG: i64 = 16;
const METHOD_FLAG_STATIC: i64 = 32;

#[test]
fn a_plain_method_keeps_the_old_shape() {
    let m = MethodInfo::new("accelerate", 2);

    assert_eq!(
        m.argument_count, 2,
        "the arity every existing site supplies"
    );
    assert!(
        m.arguments.is_empty(),
        "no per-argument detail was registered — which is NOT the same as \
         taking no arguments; `argument_count` is the arity"
    );
    assert!(!m.is_virtual && !m.is_static && !m.is_const && !m.is_vararg);

    // ⚠️ NORMAL is Godot's METHOD_FLAGS_DEFAULT — always set, not an
    // alternative to the others. A plain method reports 1, never 0.
    assert_eq!(m.godot_flags(), METHOD_FLAG_NORMAL);
}

#[test]
fn flags_compose_the_way_godot_ors_them() {
    assert_eq!(
        MethodInfo::new("_ready", 0).virtual_method().godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_VIRTUAL
    );
    assert_eq!(
        MethodInfo::new("get_size", 0)
            .const_method()
            .static_method()
            .godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_CONST | METHOD_FLAG_STATIC
    );
    assert_eq!(
        MethodInfo::new("printerr", 0).vararg_method().godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_VARARG
    );
}

#[test]
fn with_arguments_keeps_the_arity_in_step() {
    let m = MethodInfo::new("blend", 99).with_arguments(vec![
        PropertyInfo::new("weight", Variant::Float(0.0)),
        PropertyInfo::new("clip", Variant::String(String::new())),
    ]);

    assert_eq!(
        m.argument_count, 2,
        "attaching the real arguments must correct the arity, so the two can \
         never drift apart"
    );
    assert_eq!(m.arguments[0].name, "weight");
    assert_eq!(m.arguments[1].name, "clip");
}

/// The registry round-trip: what a registration declares is what
/// `get_method_list` reports, which is what `ClassDB.class_get_method_list`
/// serialises for a script.
#[test]
fn classdb_reports_arguments_and_virtuality() {
    clear_for_testing();
    register_class(
        ClassRegistration::new("FlagProbe")
            .method(
                MethodInfo::new("_process", 1)
                    .virtual_method()
                    .with_arguments(vec![PropertyInfo::new("delta", Variant::Float(0.0))]),
            )
            .method(MethodInfo::new("start", 0)),
    );

    let methods = get_method_list("FlagProbe");
    let virt = methods
        .iter()
        .find(|m| m.name == "_process")
        .expect("_process registered");
    let concrete = methods
        .iter()
        .find(|m| m.name == "start")
        .expect("start registered");

    assert!(virt.is_virtual, "_process is a virtual a script overrides");
    assert!(
        !concrete.is_virtual,
        "start is concrete — the distinction ClassDB could not make before"
    );
    assert_eq!(virt.arguments.len(), 1);
    assert_eq!(virt.arguments[0].name, "delta");
    assert_eq!(virt.godot_flags(), METHOD_FLAG_NORMAL | METHOD_FLAG_VIRTUAL);
    assert_eq!(concrete.godot_flags(), METHOD_FLAG_NORMAL);
}
