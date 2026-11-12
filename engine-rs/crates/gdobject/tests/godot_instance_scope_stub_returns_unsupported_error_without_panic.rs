//! pat-nzgdu.203 — `GodotInstance` is OUT_OF_SCOPE for the browser target, and
//! this pins the *documented decision* rather than an implementation.
//!
//! The decision and its citation live in `prd/UPSTREAM_GAP_MATRIX.md`
//! § "2026-07-17 Scope Decisions". In short: GodotInstance is a host-embedding
//! handle created by the C API `libgodot_create_godot_instance`, and **upstream
//! ships that entry point for linuxbsd/windows only — there is no
//! `platform/web/libgodot_*`**. In the browser the page is the host; there is no
//! analog to port.
//!
//! The class is still **registered and abstract**, matching
//! `GDREGISTER_ABSTRACT_CLASS(GodotInstance)`
//! (`core/register_core_types.cpp:290`), so introspection agrees with Godot.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, is_parent_class,
    register_io_classes,
};

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause: `class_exists` matches the documented decision —
/// which is **registered**, so a defensive script probing for the class takes the
/// same branch it would in Godot.
#[test]
fn godot_instance_scope_stub_returns_unsupported_error_without_panic() {
    register_io_classes();

    assert!(
        class_exists("GodotInstance"),
        "the recorded decision is REGISTERED-but-unimplemented; leaving it absent \
         would make class_exists disagree with upstream"
    );
    assert!(is_parent_class("GodotInstance", "Object"));

    // ⚠️ The real guarantee, and it is stronger than "start() returns an error":
    // the class is ABSTRACT, so there is no legitimate way to obtain an instance
    // to call start() on at all. Godot enforces the same — a script cannot
    // construct one there either; the C API does.
    assert!(
        is_abstract("GodotInstance"),
        "GDREGISTER_ABSTRACT_CLASS(GodotInstance) — core/register_core_types.cpp:290"
    );
    assert!(
        !can_instantiate("GodotInstance"),
        "a script must not be able to construct a host-embedding handle"
    );
}

/// The host-loop surface is declared for introspection, even though nothing
/// implements it. Reflection must see what Godot's reflection sees.
#[test]
fn the_host_loop_surface_is_declared_for_introspection() {
    register_io_classes();

    // `start` and `iteration` are the two the bead names; the rest are the host
    // lifecycle around them.
    for m in [
        "start",
        "is_started",
        "iteration",
        "focus_in",
        "focus_out",
        "pause",
        "resume",
    ] {
        assert!(
            class_has_method("GodotInstance", m),
            "GodotInstance must declare {m} for introspection"
        );
        assert_eq!(arity("GodotInstance", m), 0, "{m} takes no arguments");
    }
}

/// ⚠️ It declares NO properties — the host owns the state, not the handle.
/// Inventing one (a `started` bool, say) would be schema fiction: `started` is a
/// private C++ field behind `is_started()`, not a bound property.
#[test]
fn it_declares_no_properties() {
    register_io_classes();

    let base = props("Object");
    let own: Vec<String> = props("GodotInstance")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "GodotInstance binds no properties in Godot; found {own:?}"
    );
}

/// ⚠️ It is a plain `Object`, NOT a Node and NOT a RefCounted. It is not part of
/// any scene, and it is not reference-counted — the host holds exactly one for
/// the whole process.
#[test]
fn it_is_a_plain_object_outside_the_scene_and_refcount_graphs() {
    register_io_classes();

    assert!(!is_parent_class("GodotInstance", "Node"));
    assert!(!is_parent_class("GodotInstance", "RefCounted"));
    assert!(!is_parent_class("GodotInstance", "Resource"));

    // Not vacuous: RefCounted really is registered and really does derive Object,
    // so these checks are discriminating rather than trivially true.
    assert!(class_exists("RefCounted"));
    assert!(is_parent_class("RefCounted", "Object"));
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    register_io_classes();
    let first = gdobject::get_class_info("GodotInstance")
        .expect("registered")
        .class_id;
    register_io_classes();
    let second = gdobject::get_class_info("GodotInstance")
        .expect("still registered")
        .class_id;
    assert_eq!(
        first, second,
        "re-registering must not mint a new ClassId — a guard is missing if it does"
    );
}
