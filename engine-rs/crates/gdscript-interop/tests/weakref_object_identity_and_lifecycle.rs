//! pat-s3rrv.146 acceptance: `weakref(obj)` returns a real **WeakRef object**,
//! not the referent.
//!
//! The liveness half of the contract already worked, because `weakref` handed
//! back the referent's own id and `get_ref` re-queried the alive registry. Every
//! identity question that hangs off it did not: `weakref(o) == o` was true,
//! `get_class()` reported the referent's class, and `w is WeakRef` could never
//! hold. That is the shape this test pins.
//!
//! Each clause runs as its own script so a failure names the clause rather than
//! collapsing into one array comparison, and so no clause can be masked by an
//! earlier one erroring out.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Same harness as `refcounted_weakref_lifecycle`.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// The WeakRef is a DIFFERENT object from the thing it watches.
///
/// This is the clause that was false before this bead, and the one every other
/// identity clause depends on.
#[test]
fn a_weakref_is_not_its_referent() {
    let v = run_val("var r = RefCounted.new()\nvar w = weakref(r)\nreturn w != r\n");
    assert_eq!(
        v,
        Variant::Bool(true),
        "weakref(r) must be a distinct object; returning the referent unchanged \
         makes `w != r` false and every question below meaningless"
    );
}

#[test]
fn a_weakref_reports_its_own_class() {
    let v = run_val("var r = RefCounted.new()\nvar w = weakref(r)\nreturn w.get_class()\n");
    assert_eq!(
        v,
        Variant::String("WeakRef".into()),
        "get_class() must say WeakRef, not the referent's class"
    );
}

/// `is` goes through `value_is_type`, which is a separate path from
/// `get_class()` — a handle can answer one correctly and the other not.
#[test]
fn a_weakref_satisfies_the_is_operator() {
    let v = run_val("var r = RefCounted.new()\nvar w = weakref(r)\nreturn w is WeakRef\n");
    assert_eq!(v, Variant::Bool(true), "`w is WeakRef` must hold");
}

/// The inheritance walk, not just the leaf name: WeakRef extends RefCounted
/// extends Object, and `is_class` answers for the whole chain.
#[test]
fn a_weakref_is_class_walks_its_ancestors() {
    for (name, expected) in [
        ("WeakRef", true),
        ("RefCounted", true),
        ("Object", true),
        ("Node", false),
    ] {
        let src = format!(
            "var r = RefCounted.new()\nvar w = weakref(r)\nreturn w.is_class(\"{name}\")\n"
        );
        assert_eq!(
            run_val(&src),
            Variant::Bool(expected),
            "is_class({name:?}) should be {expected}"
        );
    }
}

/// While the referent lives, `get_ref()` hands it back — so the indirection is
/// transparent to a script that just wants the object.
#[test]
fn get_ref_resolves_to_the_referent_while_it_lives() {
    let v = run_val("var r = RefCounted.new()\nvar w = weakref(r)\nreturn w.get_ref() == r\n");
    assert_eq!(
        v,
        Variant::Bool(true),
        "get_ref() must resolve back to the watched object"
    );
}

/// The point of a weak reference. `unreference()` takes the count 1 -> 0, which
/// releases the handle and unregisters it from the alive registry; `get_ref()`
/// re-queries that registry rather than answering from anything it cached.
#[test]
fn get_ref_is_null_once_the_referent_is_freed() {
    let v = run_val(
        "var r = RefCounted.new()\nvar w = weakref(r)\nr.unreference()\nreturn w.get_ref() == null\n",
    );
    assert_eq!(
        v,
        Variant::Bool(true),
        "after the referent is freed get_ref() must be null"
    );
}

/// ⚠️ Paired with the clause above as a positive control: if `get_ref()` were
/// broken so as to answer null *always*, the post-free assertion would pass for
/// the wrong reason. This pins that it is null only AFTER the free.
#[test]
fn get_ref_is_not_null_before_the_free() {
    let v = run_val("var r = RefCounted.new()\nvar w = weakref(r)\nreturn w.get_ref() == null\n");
    assert_eq!(
        v,
        Variant::Bool(false),
        "get_ref() must not read null while the referent is alive — otherwise \
         the post-free assertion passes vacuously"
    );
}

/// The WeakRef outlives what it watches: freeing the referent must not make the
/// watcher itself invalid.
#[test]
fn the_weakref_stays_valid_after_its_referent_dies() {
    let v = run_val(
        "var r = RefCounted.new()\nvar w = weakref(r)\nr.unreference()\nreturn is_instance_valid(w)\n",
    );
    assert_eq!(
        v,
        Variant::Bool(true),
        "the WeakRef is its own object and survives the referent"
    );
}

/// Reflection has to know the class as well — `ClassDB.class_exists` is the
/// clause the bead names explicitly.
#[test]
fn weakref_is_registered_in_classdb() {
    // ⚠️ The registration exists in the source but the global ClassDB starts
    // EMPTY: nothing populates it until a registrar runs, so `class_exists`
    // answers false for every class, not just this one. It must be
    // `register_all_classes` and not the 2D/3D pair — WeakRef is registered in
    // `register_root_window_chain`, which only the full registrar runs. Same
    // setup the sibling ClassDB tests use.
    gdobject::class_db::register_all_classes();
    let v = run_val("return ClassDB.class_exists(\"WeakRef\")\n");
    assert_eq!(
        v,
        Variant::Bool(true),
        "ClassDB must know WeakRef; Godot registers it in register_core_types"
    );
}
