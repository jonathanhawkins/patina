//! pat-s3rrv.145 — `PCKPacker` and `OS` are visible to ClassDB, and only their
//! runnable surface is advertised.
//!
//! Godot ground truth: `doc/classes/PCKPacker.xml` (RefCounted, instantiable,
//! 4 methods) and `doc/classes/OS.xml`.
//!
//! ⚠️ **This file's rule, inherited from `class_db.rs`: ClassDB may not
//! advertise what the engine cannot run.** A registered method that has no
//! dispatch arm makes `has_method()` lie, and a registered property with no
//! reader puts a setting in the inspector that silently does nothing. Both
//! halves are asserted here — what IS registered, and what deliberately is not.

use gdobject::class_db::{
    can_instantiate, class_has_method, get_method_list, get_property_list, register_all_classes,
};
use gdobject::{class_exists, is_parent_class};

fn arity(class_name: &str, method: &str) -> Option<usize> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

#[test]
fn pck_packer_is_registered_and_instantiable() {
    register_all_classes();
    assert!(
        class_exists("PCKPacker"),
        "PCKPacker is fully dispatched from GDScript and was invisible to \
         ClassDB, so ClassDB.instantiate(\"PCKPacker\") failed"
    );
    assert!(is_parent_class("PCKPacker", "RefCounted"));
    assert!(
        can_instantiate("PCKPacker"),
        "extension_api marks it is_instantiable — a script builds one directly"
    );
    // Arities count Godot's OPTIONAL arguments too, which is what
    // class_get_method_list reports upstream.
    assert_eq!(arity("PCKPacker", "pck_start"), Some(4));
    assert_eq!(arity("PCKPacker", "add_file"), Some(3));
    assert_eq!(arity("PCKPacker", "flush"), Some(1));
}

/// 🔑 The two PCKPacker names that must NOT be registered, for opposite reasons.
///
/// ⚠️ `add_file_removal` is a real Godot method with **no dispatch arm** —
/// registering it would make `has_method()` promise something that raises.
///
/// ⚠️ `get_file_count` is dispatched but is **not a Godot method** — a Patina
/// extension. Registering it would answer `true` where Godot answers `false`:
/// a parity break in the opposite direction. It stays callable from script and
/// absent from the table.
#[test]
fn pck_packer_advertises_neither_the_unimplemented_nor_the_nonstandard() {
    register_all_classes();
    assert!(
        !class_has_method("PCKPacker", "add_file_removal"),
        "🔑 documented in Godot but unimplemented here — advertising it would \
         make has_method() lie"
    );
    assert!(
        !class_has_method("PCKPacker", "get_file_count"),
        "🔑 implemented here but absent from Godot — advertising it would make \
         ClassDB disagree with Godot's own surface"
    );
}

/// `OS`'s method list is no longer empty.
///
/// ⚠️ `OS` was already registered (pat-s3rrv.108) — bare, so
/// `ClassDB.get_method_list("OS")` returned nothing while `OS.get_name()`
/// worked fine, because the dispatch table and ClassDB are separate sources of
/// truth. That is the gap this closes.
#[test]
fn os_exposes_the_methods_it_actually_dispatches() {
    register_all_classes();
    assert!(class_exists("OS"));
    assert!(
        !get_method_list("OS").is_empty(),
        "OS's method list must not be empty — the whole point of registering a \
         singleton the interpreter already serves"
    );
    for (method, argc) in [
        ("get_name", 0usize),
        ("has_feature", 1),
        ("get_environment", 1),
        ("get_processor_count", 0),
        ("shell_open", 1),
        ("alert", 2),
        ("create_process", 3),
    ] {
        assert_eq!(
            arity("OS", method),
            Some(argc),
            "OS.{method}() is dispatched by call_os_static and must be \
             registered taking {argc}"
        );
    }
}

/// 🔑 `get_ticks_msec` / `get_ticks_usec` are **not** OS methods in Godot 4.
///
/// ⚠️ They moved to `Time` (`doc/classes/Time.xml`); `OS.xml` does not mention
/// them. Patina's interpreter still dispatches them under `OS.` — Godot 3 API
/// that outlived the move — so they are dispatched-but-not-documented. Putting
/// them on `OS` in ClassDB would enshrine the wrong class in exactly the table
/// the editor's class docs and Quick Open read.
#[test]
fn os_does_not_claim_the_tick_methods_that_moved_to_time() {
    register_all_classes();
    for moved in ["get_ticks_msec", "get_ticks_usec"] {
        assert!(
            !class_has_method("OS", moved),
            "🔑 `{moved}` belongs to Time in Godot 4. It still works as \
             `OS.{moved}()` here, but ClassDB must describe Godot's surface, \
             not our dispatch table's history"
        );
    }
}

/// 🔑 OS's three properties stay unregistered, deliberately.
///
/// ⚠️ `low_processor_usage_mode`, `low_processor_usage_mode_sleep_usec` and
/// `delta_smoothing` are real Godot members whose values nothing in this
/// workspace acts on. pat-s3rrv.108 left them off on purpose and wrote down
/// why: advertising them would put settings in the inspector that silently do
/// nothing. This pins that decision so adding the methods did not quietly drag
/// the properties along with them.
#[test]
fn os_properties_remain_unadvertised_until_something_reads_them() {
    register_all_classes();
    let names: Vec<String> = get_property_list("OS")
        .into_iter()
        .map(|p| p.name)
        .collect();
    for member in [
        "low_processor_usage_mode",
        "low_processor_usage_mode_sleep_usec",
        "delta_smoothing",
    ] {
        assert!(
            !names.iter().any(|n| n == member),
            "🔑 `{member}` must stay unregistered while nothing reads it — an \
             inspector row that does nothing is worse than an absent one \
             (pat-s3rrv.108)"
        );
    }
}
