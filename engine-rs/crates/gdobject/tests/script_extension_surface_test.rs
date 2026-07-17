//! pat-nzgdu.204 — the `Script` / `ScriptLanguage` / `*Extension` class surface.
//!
//! **Surface only; the runtime is deliberately out of scope.** These are the hooks
//! a *native GDExtension plugin* implements to add a scripting language, and
//! loading native plugins is not feasible on wasm. The decision and its citation
//! live in `prd/UPSTREAM_GAP_MATRIX.md` § "2026-07-17 Scope Decisions".
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; the
//! registration was **generated** from that dump rather than transcribed, because
//! 97 virtuals is exactly where hand-copying invents arity errors.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_io_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

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

/// Methods `class` declares that `base` does not.
fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's clause: both Extensions exist and are correctly parented.
#[test]
fn script_extension_surface_test() {
    register_io_classes();

    assert!(class_exists("ScriptExtension"));
    assert!(class_exists("ScriptLanguageExtension"));
    assert!(is_parent_class("ScriptExtension", "Script"));
    assert!(is_parent_class("ScriptLanguageExtension", "ScriptLanguage"));

    // ⚠️ The bead names only the two Extensions, but BOTH parents were missing
    // too. A child over an absent parent has a silently severed chain — the
    // whole point of checking ancestry rather than just class_exists.
    assert!(
        class_exists("Script"),
        "Script was missing and had to be added"
    );
    assert!(
        class_exists("ScriptLanguage"),
        "ScriptLanguage was missing and had to be added"
    );
    assert!(
        is_parent_class("ScriptExtension", "Resource"),
        "ScriptExtension must reach Resource through Script"
    );
    assert!(is_parent_class("ScriptExtension", "Object"));
    assert!(is_parent_class("ScriptLanguageExtension", "Object"));
}

/// ⚠️ The Extensions are INSTANTIABLE; their parents are ABSTRACT. That asymmetry
/// is the design — you construct the Extension and override its virtuals from a
/// GDExtension.
#[test]
fn the_extensions_are_instantiable_but_their_parents_are_not() {
    register_io_classes();

    assert!(is_abstract("Script"), "Script is abstract in Godot");
    assert!(!can_instantiate("Script"));
    assert!(is_abstract("ScriptLanguage"));
    assert!(!can_instantiate("ScriptLanguage"));

    for class in ["ScriptExtension", "ScriptLanguageExtension"] {
        assert!(
            !is_abstract(class),
            "{class} IS instantiable in Godot — making it abstract would diverge"
        );
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ BOTH Extensions are PURE VIRTUAL SURFACES — zero non-virtual methods.
///
/// Nothing here is callable *on* the class; every entry is a hook a native plugin
/// fills in. A non-virtual appearing would be invented.
#[test]
fn both_extensions_are_pure_virtual_surfaces() {
    register_io_classes();

    for (class, base) in [
        ("ScriptExtension", "Script"),
        ("ScriptLanguageExtension", "ScriptLanguage"),
    ] {
        let own = own_methods(class, base);
        assert!(!own.is_empty(), "{class} declares its own hooks");
        let non_virtual: Vec<&String> = own.iter().filter(|m| !m.starts_with('_')).collect();
        assert!(
            non_virtual.is_empty(),
            "{class} declares ONLY virtuals in Godot; found non-virtual: {non_virtual:?}"
        );

        // ...and no properties of their own.
        let base_props = props(base);
        let own_props: Vec<String> = props(class)
            .into_iter()
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own_props.is_empty(),
            "{class} declares no properties; found {own_props:?}"
        );
    }
}

/// The counts are load-bearing: they are how a generated registration proves it
/// matches the dump rather than a hand-typed subset.
#[test]
fn the_virtual_counts_match_godot_exactly() {
    register_io_classes();

    assert_eq!(
        own_methods("ScriptExtension", "Script").len(),
        37,
        "Godot declares 37 virtuals on ScriptExtension"
    );
    assert_eq!(
        own_methods("ScriptLanguageExtension", "ScriptLanguage").len(),
        60,
        "Godot declares 60 virtuals on ScriptLanguageExtension"
    );

    // Spot-check arities that a transcription would plausibly get wrong.
    assert_eq!(arity("ScriptExtension", "_can_instantiate"), 0);
    assert_eq!(arity("ScriptExtension", "_placeholder_erased"), 1);
    assert_eq!(arity("ScriptLanguageExtension", "_get_name"), 0);
}

/// ⚠️ `Script` carries a REAL surface (18 methods + `source_code`), unlike
/// `ScriptLanguage`, which binds NOTHING. Assuming both bases are empty — or that
/// both are populated — is wrong either way.
#[test]
fn script_has_a_real_surface_but_scriptlanguage_binds_nothing() {
    register_io_classes();

    // Script's own surface.
    assert!(props("Script").iter().any(|p| p == "source_code"));
    for m in [
        "can_instantiate",
        "has_source_code",
        "get_source_code",
        "reload",
        "is_tool",
        "get_global_name",
    ] {
        assert!(class_has_method("Script", m), "Script must declare {m}");
    }
    assert_eq!(arity("Script", "instance_has"), 1);
    assert_eq!(arity("Script", "set_source_code"), 1);
    assert_eq!(arity("Script", "get_property_default_value"), 1);

    // ScriptLanguage binds NOTHING — the whole language surface lives on the
    // Extension's virtuals.
    let own = own_methods("ScriptLanguage", "Object");
    assert!(
        own.is_empty(),
        "ScriptLanguage is a pure marker base in Godot; found {own:?}"
    );
    let own_props: Vec<String> = props("ScriptLanguage")
        .into_iter()
        .filter(|p| !props("Object").contains(p))
        .collect();
    assert!(own_props.is_empty());
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "Script",
            "ScriptLanguage",
            "ScriptExtension",
            "ScriptLanguageExtension",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_io_classes();
    let first = ids();
    register_io_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
