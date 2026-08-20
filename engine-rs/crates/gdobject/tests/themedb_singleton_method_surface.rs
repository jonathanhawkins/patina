//! pat-s3rrv.126 — `ThemeDB` carries Godot's full method surface, not just a
//! class name.
//!
//! Godot ground truth: `scene/theme/theme_db.cpp:415-427` binds exactly twelve
//! methods — `get_default_theme`, `get_project_theme`, and the getter/setter
//! pair for each of the five `fallback_*` members. `doc/classes/ThemeDB.xml`
//! lists only the first two as `<method>` entries; the other ten are the
//! members' `setter=` / `getter=` attributes, which is why counting the XML's
//! `<method>` tags gives 2 and the real answer is 12.
//!
//! ⚠️ **This class read as done because the wrong thing was being asserted.**
//! `ThemeDB` was registered with a parent, a signal and five properties, and
//! `control_theme_override_resolution.rs` pinned it with
//! `assert!(class_exists("ThemeDB"))` — which passed the whole time
//! `class_get_method_list("ThemeDB")` returned an EMPTY vec. The bead is
//! explicit that a test which only asserts `class_exists` must not be able to
//! pass, so the first thing asserted here is that the method list is not empty.
//!
//! ⚠️ **Scope.** This covers the ClassDB surface only. `ThemeDB` is still not
//! dispatchable from GDScript — it is absent from `BUILTIN_SINGLETONS` and has
//! no arm in `interpreter.rs` — so `ThemeDB.get_default_theme()` still raises
//! UndefinedVariable. Do not read a green run here as the singleton working.

use gdobject::class_db::{
    get_method_list, get_property_getter, get_property_setter, register_all_classes,
};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

/// Every method Godot binds on `ThemeDB`, with its argument count.
///
/// Transcribed from `theme_db.cpp:415-427` rather than derived from the
/// property names: deriving would invent `set_fallback_base_scale` correctly
/// but has no way to know that `get_default_theme` and `get_project_theme`
/// exist at all, since they back no member.
const THEMEDB_METHODS: &[(&str, usize)] = &[
    ("get_default_theme", 0),
    ("get_project_theme", 0),
    ("set_fallback_base_scale", 1),
    ("get_fallback_base_scale", 0),
    ("set_fallback_font", 1),
    ("get_fallback_font", 0),
    ("set_fallback_font_size", 1),
    ("get_fallback_font_size", 0),
    ("set_fallback_icon", 1),
    ("get_fallback_icon", 0),
    ("set_fallback_stylebox", 1),
    ("get_fallback_stylebox", 0),
];

fn arity_of(class_name: &str, method: &str) -> Option<usize> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

/// 🔑 The method list is non-empty — the assertion the old `class_exists` probe
/// could not make.
#[test]
fn themedb_has_a_method_list_at_all() {
    register_all_classes();
    assert!(class_exists("ThemeDB"), "ThemeDB must be registered");
    assert!(
        is_parent_class("ThemeDB", "Object"),
        "ThemeDB derives Object in Godot (`register_scene_types.cpp`)"
    );
    assert!(
        !get_method_list("ThemeDB").is_empty(),
        "🔑 ThemeDB's method list must not be empty. An empty list is what let \
         `class_exists(\"ThemeDB\")` pass for a registration that exposed \
         nothing a script could call"
    );
}

/// All twelve, each with the arity Godot binds.
///
/// ⚠️ Arity is asserted, not just presence: a setter registered with 0
/// arguments still answers `class_has_method` and then mis-dispatches, which is
/// the failure mode a presence-only check cannot see.
#[test]
fn themedb_exposes_godots_twelve_methods() {
    register_all_classes();
    for (method, argc) in THEMEDB_METHODS {
        assert_eq!(
            arity_of("ThemeDB", method),
            Some(*argc),
            "ThemeDB.{method}() must be registered taking {argc} argument(s) \
             (`theme_db.cpp:415-427`)"
        );
    }
}

/// The five members resolve to those same accessor names.
///
/// ⚠️ The half that keeps the two lists honest. Registering a `set_fallback_*`
/// METHOD and leaving the PROPERTY with no accessors is a silent split: the
/// method list looks complete while `property_get_setter` returns `""`, so a
/// write through the property never reaches the setter. Both sides are
/// asserted against one table so they cannot drift apart.
#[test]
fn themedb_fallback_properties_name_their_accessors() {
    register_all_classes();
    for member in [
        "fallback_base_scale",
        "fallback_font",
        "fallback_font_size",
        "fallback_icon",
        "fallback_stylebox",
    ] {
        assert_eq!(
            get_property_getter("ThemeDB", member),
            format!("get_{member}"),
            "ThemeDB.{member} must name its getter"
        );
        assert_eq!(
            get_property_setter("ThemeDB", member),
            format!("set_{member}"),
            "ThemeDB.{member} must name its setter"
        );
    }
}

/// Godot's documented defaults, which the fallback chain reads when nothing
/// else supplies an item.
///
/// ⚠️ `fallback_font`, `fallback_icon` and `fallback_stylebox` are object refs
/// and default to NULL — asserted as `Nil` rather than skipped, because a
/// non-null default here would mean every unstyled Control silently picked up
/// an engine resource.
#[test]
fn themedb_fallback_defaults_match_godot() {
    register_all_classes();
    let props = gdobject::class_db::get_property_list("ThemeDB");
    let default_of = |name: &str| {
        props
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.default_value.clone())
    };
    assert_eq!(
        default_of("fallback_base_scale"),
        Some(Variant::Float(1.0)),
        "ThemeDB.xml gives fallback_base_scale default 1.0"
    );
    assert_eq!(
        default_of("fallback_font_size"),
        Some(Variant::Int(16)),
        "ThemeDB.xml gives fallback_font_size default 16"
    );
    for null_member in ["fallback_font", "fallback_icon", "fallback_stylebox"] {
        assert_eq!(
            default_of(null_member),
            Some(Variant::Nil),
            "ThemeDB.{null_member} is an object ref and must default to null"
        );
    }
}
