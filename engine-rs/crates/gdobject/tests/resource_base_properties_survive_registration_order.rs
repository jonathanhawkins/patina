//! pat-s3rrv.57 (finding (a), the `Resource` exclusion): the base of every
//! `.tres` and every sub-resource carried **zero** properties.
//!
//! `Resource` was skipped by the earlier metadata passes because it is
//! registered from ten different places and the note on the bead read "run
//! order decides which of several guarded registrations wins". Measuring that
//! claim showed something more specific: all ten registrations are *bare*
//! (`name + parent`, no members), so nothing was being lost — but nothing was
//! being carried either, and `get_property_list` on EVERY Resource subclass was
//! short the three members Godot always shows at the top of the inspector.
//!
//! # Why the registration that carries them is UNGUARDED
//!
//! Adding the properties to one of the ten `if !class_exists("Resource")`
//! blocks would have made the outcome depend on which registrar ran first: any
//! of the other nine winning the race skips the guarded block entirely and the
//! properties silently vanish. `ClassDB::register` merges by member name, so
//! the enriched pass runs unguarded instead — additive in any order, and
//! idempotent when a registrar is invoked twice (`register_all_classes` calls
//! six of them twice, once directly and once through
//! `register_editor_classes`).
//!
//! `resource_scene_unique_id` is deliberately NOT registered:
//! `doc/classes/Resource.xml` states no `default=` for it, and this translation
//! registers only values the documentation states.

use gdobject::class_db::{
    get_property_getter, get_property_list, get_property_setter, property_get_revert,
    register_all_classes, register_io_classes,
};
use gdvariant::Variant;

/// **The acceptance case**: the three documented members are registered with
/// the defaults `doc/classes/Resource.xml` states.
#[test]
fn resource_carries_its_three_documented_members() {
    register_all_classes();

    assert_eq!(
        property_get_revert("Resource", "resource_local_to_scene"),
        Some(Variant::Bool(false)),
        "doc/classes/Resource.xml: default=\"false\"",
    );
    assert_eq!(
        property_get_revert("Resource", "resource_name"),
        Some(Variant::String(String::new())),
        "doc/classes/Resource.xml: default=\"\"",
    );
    assert_eq!(
        property_get_revert("Resource", "resource_path"),
        Some(Variant::String(String::new())),
        "doc/classes/Resource.xml: default=\"\"",
    );

    // The one member with no documented default stays out rather than being
    // invented — an empty String here would be a guess, not a fact.
    assert_eq!(
        property_get_revert("Resource", "resource_scene_unique_id"),
        None,
        "the XML states no default for resource_scene_unique_id, so ClassDB \
         must stay silent about it rather than guess one",
    );
}

/// 🔑 The case that proves the accessors are STORED, not derived from the
/// property name.
///
/// `resource_name` reads through **`get_name`** and writes through
/// **`set_name`** — the `resource_` prefix appears in neither. A `get_<prop>` /
/// `set_<prop>` rule would answer `get_resource_name`, which does not exist.
#[test]
fn an_accessor_pair_no_naming_rule_could_produce() {
    register_all_classes();

    assert_eq!(get_property_getter("Resource", "resource_name"), "get_name");
    assert_eq!(get_property_setter("Resource", "resource_name"), "set_name");

    // `resource_local_to_scene` is the other shape a derivation gets wrong: the
    // getter is an `is_`, and it drops the prefix too.
    assert_eq!(
        get_property_getter("Resource", "resource_local_to_scene"),
        "is_local_to_scene",
    );
    assert_eq!(
        get_property_setter("Resource", "resource_local_to_scene"),
        "set_local_to_scene",
    );
}

/// Every Resource subclass inherits them, which is the reason this matters:
/// the inspector builds its rows from `get_property_list`, and that walks the
/// chain.
///
/// `StyleBoxFlat -> StyleBox -> Resource` is deliberately two levels deep, so a
/// lookup that only checked the immediate parent would fail here.
#[test]
fn a_resource_subclass_inherits_them() {
    register_all_classes();

    let names: Vec<String> = get_property_list("StyleBoxFlat")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for member in ["resource_local_to_scene", "resource_name", "resource_path"] {
        assert_eq!(
            names.iter().filter(|n| *n == member).count(),
            1,
            "StyleBoxFlat must inherit `{member}` from Resource exactly once — \
             get_property_list concatenates the chain without deduping, so a \
             second declaration anywhere would surface as a doubled inspector \
             row; got {names:?}",
        );
    }
}

/// 🔑 The regression case for the UNGUARDED registration.
///
/// `register_io_classes` registers `Resource` bare. Running it first is exactly
/// the order that a guarded enrichment loses to: the class already exists, so
/// `if !class_exists("Resource")` is false and the pass carrying the members is
/// skipped. Unguarded, it merges instead.
///
/// ⚠️ This test depends on `register_io_classes` being the FIRST registrar to
/// run in its process, so it must stay the only order-sensitive test here. The
/// project's runner gives every test its own process, which is what makes the
/// ordering real; under a shared-process runner another test may have already
/// populated the registry and this degrades to a duplicate of the acceptance
/// case rather than failing.
#[test]
fn the_members_survive_a_hostile_registration_order() {
    // The bare registration wins the race...
    register_io_classes();
    // ...and the enriched pass still lands.
    register_all_classes();

    assert_eq!(
        property_get_revert("Resource", "resource_path"),
        Some(Variant::String(String::new())),
        "a bare `Resource` registration running first must not be able to \
         suppress the pass that carries its members",
    );
    assert_eq!(
        get_property_list("Resource")
            .into_iter()
            .filter(|p| p.name == "resource_path")
            .count(),
        1,
        "…and merging must not append it twice",
    );
}
