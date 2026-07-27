//! pat-0q2kx.253 — `ScriptBacktrace` is registered in ClassDB.
//!
//! The bead's acceptance test lives in `gdscript-interop`, because it also has
//! to capture a real GDScript call stack. This covers the half that does not
//! need the interpreter: that the class exists with Godot's parent and the
//! method surface a script will actually call.
//!
//! Worth testing separately because a *wrong* registration is worse than a
//! missing one — `class_exists` still answers true, so a mis-arity'd method
//! passes the acceptance's existence check and only fails later at the call.
//!
//! Binary named `backtrace_class` to keep the verify command inside the
//! 58-char truncation limit.

use gdobject::class_db::{
    class_exists, get_property_list, inheritance_chain, register_3d_classes, MethodInfo,
};

/// ClassDB is not populated for free in a test process.
///
/// `ScriptBacktrace` sits with the other `RefCounted` handle classes (Thread,
/// Mutex, Semaphore, Expression, RegEx) inside `register_3d_classes`, despite
/// the name — that is where this family lives.
fn seed() {
    register_3d_classes();
}

#[test]
fn script_backtrace_is_registered_as_refcounted() {
    seed();
    assert!(
        class_exists("ScriptBacktrace"),
        "ScriptBacktrace must be registered",
    );
    let chain = inheritance_chain("ScriptBacktrace");
    assert_eq!(
        chain.get(1).map(String::as_str),
        Some("RefCounted"),
        "Godot declares ScriptBacktrace as inheriting RefCounted; chain was {chain:?}",
    );
}

/// Every method Godot documents is present, with the documented arity.
///
/// The arities are the interesting part and the easiest thing to get subtly
/// wrong: the **local** and **member** accessors are indexed by
/// `(frame, index)` while the **global** ones take a bare `index`, and
/// `format` takes `(indent_all, indent_frames)`.
#[test]
fn the_documented_method_surface_is_present_with_correct_arities() {
    seed();
    let expected: &[(&str, usize)] = &[
        ("format", 2),
        ("get_frame_count", 0),
        ("get_frame_file", 1),
        ("get_frame_function", 1),
        ("get_frame_line", 1),
        ("get_global_variable_count", 0),
        ("get_global_variable_name", 1),
        ("get_global_variable_value", 1),
        ("get_language_name", 0),
        ("get_local_variable_count", 1),
        ("get_local_variable_name", 2),
        ("get_local_variable_value", 2),
        ("get_member_variable_count", 1),
        ("get_member_variable_name", 2),
        ("get_member_variable_value", 2),
        ("is_empty", 0),
    ];

    for (name, arity) in expected {
        let found = own_method(name);
        let found = found
            .unwrap_or_else(|| panic!("ScriptBacktrace must declare {name}; it is in the doc XML"));
        assert_eq!(
            found.argument_count, *arity,
            "{name} must take {arity} arguments, as documented",
        );
    }
}

/// Looks up a method declared *on* `ScriptBacktrace`.
///
/// `get_method_list` returns inherited methods too, base-first and without
/// deduping by name, so a forward search would return an ancestor's declaration
/// whenever a name is shadowed. Searching from the end takes the most-derived
/// one, which is what `ScriptBacktrace` itself declares.
fn own_method(name: &str) -> Option<MethodInfo> {
    gdobject::class_db::get_method_list("ScriptBacktrace")
        .into_iter()
        .rev()
        .find(|m| m.name == name)
}

/// A captured backtrace carries no properties — everything is behind getters.
/// Guards against someone "helpfully" adding fields that Godot does not expose.
#[test]
fn script_backtrace_declares_no_properties() {
    seed();
    let own: Vec<String> = get_property_list("ScriptBacktrace")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let inherited: Vec<String> = get_property_list("RefCounted")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let extra: Vec<&String> = own.iter().filter(|n| !inherited.contains(n)).collect();
    assert!(
        extra.is_empty(),
        "ScriptBacktrace exposes only methods in Godot, but declares {extra:?}",
    );
}
