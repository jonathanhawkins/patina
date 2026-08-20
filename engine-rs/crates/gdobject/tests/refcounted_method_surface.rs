//! pat-s3rrv.147 — `RefCounted` carries its four Godot methods in ClassDB.
//!
//! Godot ground truth: `doc/classes/RefCounted.xml` — `get_reference_count`
//! (const), `init_ref`, `reference`, `unreference`, all zero-argument, the last
//! three returning **bool**.
//!
//! ⚠️ **Scope: the ClassDB half only.** The interpreter still does not release
//! a reference when a binding goes out of scope — `refcounted_handle.rs` says
//! so in its own doc comment, and `var r = RefCounted.new()` inside a function
//! still leaks the handle for the life of the process. A green run here means
//! introspection is correct, not that the lifecycle is.

use gdobject::class_db::{class_has_method, get_method_list, register_all_classes};
use gdobject::{class_exists, is_parent_class};

fn method(class_name: &str, name: &str) -> Option<gdobject::class_db::MethodInfo> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == name)
}

/// The bead's named acceptance clause.
#[test]
fn refcounted_advertises_its_four_methods() {
    register_all_classes();
    assert!(
        class_has_method("RefCounted", "get_reference_count"),
        "ClassDB.class_has_method(\"RefCounted\", \"get_reference_count\") was \
         false while the interpreter dispatched it perfectly well — the \
         dispatch table and ClassDB are separate sources of truth"
    );
    for name in [
        "get_reference_count",
        "init_ref",
        "reference",
        "unreference",
    ] {
        let m = method("RefCounted", name)
            .unwrap_or_else(|| panic!("RefCounted.{name}() must be registered"));
        assert_eq!(m.argument_count, 0, "{name} takes no arguments");
    }
}

/// 🔑 Only `get_reference_count` is const.
///
/// ⚠️ The other three MUTATE the count — that is their entire purpose. Marking
/// them const would tell a caller that `unreference()` is side-effect free,
/// which is the opposite of true.
#[test]
fn only_the_query_is_const() {
    register_all_classes();
    assert!(
        method("RefCounted", "get_reference_count")
            .expect("registered")
            .is_const,
        "asking for the count must not be able to change it"
    );
    for mutating in ["init_ref", "reference", "unreference"] {
        assert!(
            !method("RefCounted", mutating).expect("registered").is_const,
            "🔑 `{mutating}` changes the reference count — a const marking would \
             advertise it as side-effect free"
        );
    }
}

/// 🔑 The methods are attached to the registration site that actually RUNS.
///
/// ⚠️ This is the assertion that caught a real bug while writing this bead.
/// `RefCounted` is registered **thirteen** times across `class_db.rs` — once
/// per `register_*_classes` function, each `class_exists`-guarded because each
/// bootstraps the inheritance chain it needs. The guard means only the FIRST
/// one to run ever registers anything; the other twelve are skipped entirely.
///
/// So attaching the methods to the wrong copy leaves them silently
/// unregistered — a bare copy wins the guard, and the richer copy never
/// executes. My first attempt put them in `register_3d_classes`, which runs
/// fourth: everything compiled, the diff looked right, and
/// `class_has_method("RefCounted", "get_reference_count")` was still false.
///
/// `register_all_classes()` runs every pass in order, so this test fails if the
/// methods are ever moved to a site that loses the guard race.
#[test]
fn the_methods_are_registered_by_the_pass_that_actually_runs() {
    register_all_classes();
    // Idempotent: a second full pass must neither erase nor duplicate.
    register_all_classes();
    assert_eq!(
        get_method_list("RefCounted")
            .iter()
            .filter(|m| m.name == "get_reference_count")
            .count(),
        1,
        "🔑 exactly one copy. 0 means the methods were attached to a \
         registration site that a bare copy beat to the guard — the diff looks \
         correct and registers nothing"
    );
    for name in [
        "get_reference_count",
        "init_ref",
        "reference",
        "unreference",
    ] {
        assert!(
            class_has_method("RefCounted", name),
            "{name} must be registered by whichever pass wins the guard"
        );
    }
}

/// `WeakRef` — already registered before this bead, asserted because the
/// acceptance names it.
///
/// ⚠️ The bead claims `grep 'ClassRegistration::new("WeakRef")'` returns
/// nothing. It does not: `WeakRef : RefCounted` with `get_ref` was already
/// there. Pinned here so the acceptance clause has a home and a future
/// refactor cannot quietly drop it.
#[test]
fn weakref_is_registered_under_refcounted() {
    register_all_classes();
    assert!(class_exists("WeakRef"));
    assert!(
        is_parent_class("WeakRef", "RefCounted"),
        "WeakRef is RefCounted in Godot, so it inherits the four methods above"
    );
    assert!(class_has_method("WeakRef", "get_ref"));
    // Inherited, not redeclared — `get_method_list` walks the chain.
    assert!(
        class_has_method("WeakRef", "get_reference_count"),
        "a WeakRef is itself reference-counted"
    );
}
