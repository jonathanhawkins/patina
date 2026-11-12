//! pat-s3rrv.57: `ClassDB.class_call_static()`.
//!
//! It raised `UndefinedFunction`, while `class_get_method_list` — once ClassDB
//! was registered as a class — declared it. Reflection and behaviour disagreed:
//! a tool could see the method and not call it.
//!
//! # Why this delegates instead of growing a table
//!
//! Godot dispatches `class_call_static` through the same static-method table
//! that backs `ClassName.method(...)` written directly, so the two spellings
//! cannot disagree. Patina now does the same — the implementation calls the
//! very handlers the direct spelling uses (`variant_math::vector_static_call`,
//! then the host's `physics_query_static_call`). A second table would drift the
//! first time anyone added a static to only one of them, and the drift would be
//! invisible: both spellings look implemented.
//!
//! The agreement test below is the one that actually pins that. Asserting only
//! that `class_call_static` returns *something* would pass against a private
//! second implementation that had already diverged.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = "extends Node\n\
                   \n\
                   func via_classdb():\n\
                   \x20   return ClassDB.class_call_static(\"Color\", \"html\", \"#ff8800\")\n\
                   \n\
                   func via_direct():\n\
                   \x20   return Color.html(\"#ff8800\")\n\
                   \n\
                   func predicate():\n\
                   \x20   return ClassDB.class_call_static(\"Color\", \"html_is_valid\", \"#zzz\")\n\
                   \n\
                   func no_such_static():\n\
                   \x20   return ClassDB.class_call_static(\"Color\", \"not_a_real_static\")\n\
                   \n\
                   func no_such_class():\n\
                   \x20   return ClassDB.class_call_static(\"NoSuchClassAnywhere\", \"whatever\")\n";

/// Runs one method of the probe script.
///
/// Mirrors the call sequence of the green
/// `classdb_instantiate_builds_non_node_classes` test — `run_class` →
/// `instantiate_class` → `call_instance_method` — rather than inventing one.
fn call(method: &str) -> Result<Variant, String> {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .map_err(|e| format!("{e:?}"))
}

/// **The acceptance case**: both spellings answer identically.
///
/// This is what makes the delegation checkable. If `class_call_static` ever
/// grows its own dispatch table, this fails the moment the two disagree —
/// which is the whole reason Godot keeps them on one table.
#[test]
fn class_call_static_agrees_with_the_direct_spelling() {
    let via_classdb = call("via_classdb").expect("ClassDB.class_call_static(Color, html)");
    let via_direct = call("via_direct").expect("Color.html()");

    assert!(
        matches!(via_classdb, Variant::Color(_)),
        "class_call_static must return the constructed Color, got {via_classdb:?}"
    );
    assert_eq!(
        via_classdb, via_direct,
        "`ClassDB.class_call_static(\"Color\", \"html\", ...)` and \
         `Color.html(...)` must be the same call — Godot dispatches both through \
         one static table, and two tables would drift invisibly"
    );
}

/// Arguments past the class and method are forwarded — the signature is vararg,
/// and a predicate returning `false` proves the argument actually arrived
/// (a dropped argument would make `html_is_valid` answer about an empty string).
#[test]
fn call_arguments_are_forwarded() {
    assert_eq!(
        call("predicate").expect("html_is_valid dispatches"),
        Variant::Bool(false),
        "\"#zzz\" is not a valid html colour; `true` here would mean the \
         argument never reached the static",
    );
}

/// An unknown static raises rather than answering null.
///
/// ⚠️ Deliberate divergence from Godot, which prints an error and returns null.
/// A silent null is the exact failure mode this bead exists to remove: the
/// caller cannot tell "no such method" from "the method returned null".
#[test]
fn an_unknown_static_is_an_error_not_a_silent_null() {
    let err = call("no_such_static").expect_err("a missing static must not answer Nil");
    assert!(
        err.contains("not_a_real_static") && err.contains("Color"),
        "the error must name the class and method that failed to resolve, got: {err}"
    );

    let err = call("no_such_class").expect_err("an unknown class must not answer Nil");
    assert!(
        err.contains("NoSuchClassAnywhere"),
        "…likewise for an unknown class, got: {err}"
    );
}
