//! pat-s3rrv.146 — the bead's acceptance, all seven clauses in one place.
//!
//! # Why this file exists next to `weakref_object_identity_and_lifecycle`
//!
//! That suite is the deep-dive: nine tests covering the identity and lifetime
//! contract clause by clause, and it is green. This one is the **acceptance**,
//! and it exists for two reasons:
//!
//! 1. **The bead names `weakref_is_its_own_object_class`.** A verifier running
//!    `--test weakref_is_its_own_object_class` against a file that does not
//!    exist gets no test target — the bead could never pass its own stated
//!    command, whatever the code did.
//! 2. **One clause was never asserted anywhere:**
//!    `ClassDB.get_parent_class("WeakRef") == "RefCounted"`. `get_parent_class`
//!    appears zero times in the sibling suite. The registration does set
//!    `.parent("RefCounted")`, so this is coverage of a real contract rather
//!    than a fix — but an unasserted contract is one refactor away from being
//!    silently wrong.
//!
//! ⚠️ **ClassDB starts EMPTY.** Nothing populates it until a registrar runs, so
//! `class_exists` answers false for *every* class until then — and it must be
//! `register_all_classes`, because `WeakRef` is registered inside
//! `register_root_window_chain`, which the narrower 2D/3D registrars never call.
//! A test that skipped this would fail with "ClassDB must know WeakRef" and look
//! like a missing registration. I watched exactly that failure come out of a
//! stale build of an earlier version of this file.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `src` and returns its value. Same harness as the sibling suites.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Runs `src` with ClassDB populated first.
fn run_val_with_classdb(src: &str) -> Variant {
    gdobject::class_db::register_all_classes();
    run_val(src)
}

// ===========================================================================
// The acceptance
// ===========================================================================

/// **The acceptance.** `weakref(obj)` is its own object, of class `WeakRef`,
/// resolving to the referent while it lives.
///
/// Each clause is a separate script and a separate assertion so a failure names
/// the clause instead of collapsing into one comparison.
#[test]
fn weakref_is_its_own_object_class() {
    // (a) the WeakRef is NOT the referent. This is the clause the bead was
    // opened for: `weakref` used to hand back the referent's own id, which made
    // every identity question below answer about the wrong object.
    assert_eq!(
        run_val(
            "var o = RefCounted.new()\n\
             var w = weakref(o)\n\
             return w != o\n"
        ),
        Variant::Bool(true),
        "weakref(o) must be a different object from o"
    );

    // (b) it reports its own class, not the referent's.
    assert_eq!(
        run_val(
            "var o = RefCounted.new()\n\
             var w = weakref(o)\n\
             return w.get_class()\n"
        ),
        Variant::String("WeakRef".to_string()),
        "get_class() must be WeakRef, not the referent's class"
    );

    // (c) and satisfies `is`.
    assert_eq!(
        run_val(
            "var o = RefCounted.new()\n\
             var w = weakref(o)\n\
             return w is WeakRef\n"
        ),
        Variant::Bool(true),
        "`w is WeakRef` must hold"
    );

    // (d) it resolves to the referent while that referent lives.
    assert_eq!(
        run_val(
            "var o = RefCounted.new()\n\
             var w = weakref(o)\n\
             return w.get_ref() == o\n"
        ),
        Variant::Bool(true),
        "get_ref() must return the referent while it is alive"
    );
}

/// The lifetime half: `get_ref()` goes null once the referent is released.
///
/// ⚠️ Split from the acceptance above deliberately. It is the one clause that
/// depends on the free actually happening, so a failure here means something
/// different from a failure up there — the identity is fine and the liveness is
/// not.
#[test]
fn weakref_is_its_own_object_class_get_ref_goes_null_after_free() {
    // ⚠️ `unreference()`, not `free()`. The referent is a `RefCounted`, whose
    // lifetime is the reference count — that is the spelling the sibling suite
    // proves works, and copying the proven call keeps this test about weakrefs
    // rather than about which teardown verb dispatches.
    assert_eq!(
        run_val(
            "var r = RefCounted.new()\n\
             var w = weakref(r)\n\
             r.unreference()\n\
             return w.get_ref() == null\n"
        ),
        Variant::Bool(true),
        "after the referent is unreferenced get_ref() must be null"
    );

    // The paired positive control, as its own script: a `get_ref` broken so as
    // to answer null ALWAYS would satisfy the assertion above for the wrong
    // reason. Separate scripts rather than one conjunction, so a failure names
    // which half broke.
    assert_eq!(
        run_val(
            "var r = RefCounted.new()\n\
             var w = weakref(r)\n\
             return w.get_ref() == null\n"
        ),
        Variant::Bool(false),
        "get_ref() must NOT read null while the referent is alive, or the \
         post-free assertion passes vacuously"
    );
}

/// The ClassDB half: `WeakRef` is registered, under `RefCounted`.
///
/// ⚠️ `get_parent_class` is the clause the sibling suite never asserted — it
/// appears zero times there. The registration does set `.parent("RefCounted")`,
/// so this pins an existing contract rather than reporting a bug.
#[test]
fn weakref_is_its_own_object_class_is_registered_under_refcounted() {
    assert_eq!(
        run_val_with_classdb("return ClassDB.class_exists(\"WeakRef\")\n"),
        Variant::Bool(true),
        "ClassDB must know WeakRef — Godot registers it in register_core_types"
    );
    assert_eq!(
        run_val_with_classdb("return ClassDB.get_parent_class(\"WeakRef\")\n"),
        Variant::String("RefCounted".to_string()),
        "WeakRef inherits RefCounted per Godot's class hierarchy"
    );

    // The control: the lookup reports what was registered rather than echoing
    // whatever it is asked about. Without this, a ClassDB that answered
    // "RefCounted" for everything would satisfy the assertion above.
    assert_eq!(
        run_val_with_classdb("return ClassDB.class_exists(\"WeakRefNotARealClass\")\n"),
        Variant::Bool(false),
        "a ClassDB that says yes to anything proves nothing about WeakRef"
    );
}
