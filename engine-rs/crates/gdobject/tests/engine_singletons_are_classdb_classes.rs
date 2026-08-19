//! pat-s3rrv.124 — `Engine`, `DisplayServer` and `EngineDebugger` as ClassDB
//! classes.
//!
//! All three dispatch from GDScript and all three are entries in
//! `extension_api.json`'s `.singletons`, but none was a ClassDB class. So
//! `ClassDB.class_exists("Engine")` was false, `get_property_list("Engine")`
//! came back empty, and the editor's ClassDB-driven surfaces were blind to them.
//!
//! ⚠️ Being a singleton and being a class are **separate** registrations in
//! Godot, and Patina had them on opposite sides: the interpreter's
//! `BUILTIN_SINGLETONS` knew these names while ClassDB did not.

use gdobject::class_db::{get_class_info, register_all_classes};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
        .clone()
}

#[test]
fn engine_singletons_are_classdb_classes() {
    // ⚠️ ClassDB is empty until a register_* entry point runs; these three live
    // in `register_root_window_chain`, so the narrower 2D/3D entry points would
    // not reach them.
    register_all_classes();

    for class in ["Engine", "DisplayServer", "EngineDebugger"] {
        assert!(
            class_exists(class),
            "ClassDB must know {class} — it dispatches from script and is a \
             Godot singleton entry"
        );
        assert!(
            is_parent_class(class, "Object"),
            "{class} derives Object in Godot"
        );
    }
}

/// 🔑 ENGINE'S SEVEN DOCUMENTED PROPERTIES, WITH GODOT'S DEFAULTS.
///
/// ⚠️ Both print flags default to **true**. A `false` default would read as
/// "printing is off by default", the opposite of Godot, and would be invisible
/// to any test that only checked the properties exist.
#[test]
fn engine_carries_its_documented_properties() {
    register_all_classes();

    assert_eq!(default_of("Engine", "max_fps"), Variant::Int(0));
    assert_eq!(
        default_of("Engine", "max_physics_steps_per_frame"),
        Variant::Int(8)
    );
    assert_eq!(
        default_of("Engine", "physics_jitter_fix"),
        Variant::Float(0.5)
    );
    assert_eq!(
        default_of("Engine", "physics_ticks_per_second"),
        Variant::Int(60)
    );
    assert_eq!(
        default_of("Engine", "print_error_messages"),
        Variant::Bool(true),
        "Godot prints error messages by default"
    );
    assert_eq!(default_of("Engine", "print_to_stdout"), Variant::Bool(true));
    assert_eq!(default_of("Engine", "time_scale"), Variant::Float(1.0));
}

/// `DisplayServer` and `EngineDebugger` are pure method surfaces — Godot's docs
/// give them no `<member>` entries at all.
///
/// ⚠️ Asserted so that a future pass adding properties has to think about
/// whether Godot really has them, rather than inventing a plausible-looking set.
#[test]
fn the_property_free_singletons_carry_no_invented_properties() {
    register_all_classes();

    for class in ["DisplayServer", "EngineDebugger"] {
        let info = get_class_info(class).unwrap_or_else(|| panic!("{class} registered"));
        assert!(
            info.properties.is_empty(),
            "{class} has no documented properties; got {:?}",
            info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}
