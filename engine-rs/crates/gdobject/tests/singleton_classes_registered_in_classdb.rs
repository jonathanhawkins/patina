//! pat-s3rrv.108 — the engine singletons exist in ClassDB.
//!
//! # The gap
//!
//! `OS`, `Time`, `Performance` and `ProjectSettings` were reachable from
//! GDScript — the interpreter matches them as bare identifiers and dispatches
//! through its own tables — but ClassDB had never heard of them. So
//! `OS.get_name()` worked while `ClassDB.class_exists("OS")` was false, and any
//! tool script enumerating singleton classes through the reflection API saw
//! nothing. The dispatch table and ClassDB are separate sources of truth, and
//! they disagreed.
//!
//! ⚠️ These are registered BARE, matching the `DisplayServer` / `EngineDebugger`
//! entries beside them. `OS` carries three documented properties
//! (`low_processor_usage_mode`, `low_processor_usage_mode_sleep_usec`,
//! `delta_smoothing`) and **none is read anywhere in this workspace**, so
//! advertising them would put controls in the inspector that silently do
//! nothing — the same false-green this file already guards against for methods.

use gdobject::class_db::{class_exists, get_class_info, register_all_classes};

#[test]
fn engine_singletons_are_registered() {
    register_all_classes();

    for name in ["OS", "Time", "Performance", "ProjectSettings"] {
        assert!(
            class_exists(name),
            "ClassDB must know the `{name}` singleton; a tool script \
             enumerating classes through the reflection API sees nothing \
             without it, even though the interpreter dispatches `{name}` calls \
             fine"
        );
    }

    // The three that were already registered, asserted so a regression in
    // either group is attributable.
    for name in ["Engine", "DisplayServer", "EngineDebugger"] {
        assert!(class_exists(name), "`{name}` was already registered");
    }
}

/// ⚠️ The control. Without it, `class_exists` returning true for everything
/// would satisfy the test above and prove nothing.
#[test]
fn class_exists_still_says_no_to_an_unknown_class() {
    register_all_classes();
    assert!(
        !class_exists("NoSuchSingletonAnywhere"),
        "class_exists must still answer false for a class that does not exist"
    );
}

/// 🔑 Registered with NO fabricated surface.
///
/// ⚠️ The point of registering these was reflection completeness, not to
/// advertise capability. `OS`'s properties are unread in this workspace, so an
/// empty property list is the honest state — a populated one would show
/// settings in the inspector that change nothing.
#[test]
fn the_singletons_advertise_no_unimplemented_properties() {
    register_all_classes();
    let info = get_class_info("OS").expect("OS is registered");
    assert!(
        info.properties.is_empty(),
        "OS must not advertise properties this engine does not read; found {:?}",
        info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(
        info.parent_class, "Object",
        "the singletons derive from Object, as in Godot"
    );
}
