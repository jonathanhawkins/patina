//! pat-0q2kx.64 (ClassDB half): `PropertyInfo` carries the fields Godot's
//! `get_property_list()` dictionary needs, and the class defaults are reachable
//! for revert.
//!
//! Godot's dict is `{name, class_name, type, hint, hint_string, usage}`
//! (`core/object/object.cpp`), and `property_can_revert`/`property_get_revert`
//! answer from the class default. Patina's `PropertyInfo` had only
//! `name`/`default_value`/`hint`, so the dict shape was unrepresentable and
//! nothing could be reverted.
//!
//! The `type` field is deliberately NOT stored: it is derived from
//! `default_value`, so a property's declared type and its default can never
//! disagree. These tests pin that derivation along with the new fields.

use gdcore::math::Vector2;
use gdobject::class_db::{
    clear_for_testing, get_property_list, property_can_revert, property_get_revert, register_class,
    ClassRegistration, PropertyInfo, PROPERTY_USAGE_DEFAULT, PROPERTY_USAGE_EDITOR,
    PROPERTY_USAGE_NONE, PROPERTY_USAGE_STORAGE,
};
use gdvariant::Variant;

/// A two-level hierarchy where the derived class re-declares a base property, so
/// the override order is observable.
fn register_fixture() {
    clear_for_testing();
    register_class(
        ClassRegistration::new("FixtureBase")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector2(Vector2::ZERO),
            ))
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .property(
                PropertyInfo::new("mode", Variant::Int(0))
                    .with_hint(2)
                    .with_hint_string("Idle,Walk,Run"),
            )
            .property(
                PropertyInfo::new("hidden_thing", Variant::Int(7)).with_usage(PROPERTY_USAGE_NONE),
            ),
    );
    register_class(
        ClassRegistration::new("FixtureDerived")
            .parent("FixtureBase")
            // Re-declares `visible` with a different default: reverting must use
            // THIS one, not the base's.
            .property(PropertyInfo::new("visible", Variant::Bool(false))),
    );
}

#[test]
fn property_info_carries_the_dict_fields() {
    register_fixture();

    let props = get_property_list("FixtureBase");
    let position = props
        .iter()
        .find(|p| p.name == "position")
        .expect("position is registered");

    // `type` comes from the default value — never stored separately.
    assert_eq!(
        position.default_value.variant_type(),
        Variant::Vector2(Vector2::ZERO).variant_type(),
        "a Vector2 property reports the Vector2 type through its default"
    );
    assert_eq!(
        position.usage, PROPERTY_USAGE_DEFAULT,
        "an ordinary property gets Godot's default usage (STORAGE | EDITOR)"
    );
    assert_eq!(
        PROPERTY_USAGE_DEFAULT,
        PROPERTY_USAGE_STORAGE | PROPERTY_USAGE_EDITOR,
        "DEFAULT is exactly STORAGE|EDITOR, matching core/object/object.h"
    );
    assert!(
        position.hint_string.is_empty(),
        "an unhinted property has an empty hint string, not a placeholder"
    );

    // An enum-style property carries both hint and hint detail.
    let mode = props.iter().find(|p| p.name == "mode").expect("mode");
    assert_eq!(mode.hint, 2);
    assert_eq!(mode.hint_string, "Idle,Walk,Run");

    // Usage is settable, and a zero-usage property is still LISTED — Godot
    // reports it and lets the caller filter, rather than hiding it.
    let hidden = props
        .iter()
        .find(|p| p.name == "hidden_thing")
        .expect("hidden_thing is still listed");
    assert_eq!(hidden.usage, PROPERTY_USAGE_NONE);
}

#[test]
fn property_revert_reads_the_class_default() {
    register_fixture();

    assert!(
        property_can_revert("FixtureBase", "position"),
        "a registered property can be reverted"
    );
    assert!(
        !property_can_revert("FixtureBase", "not_a_property"),
        "an unknown property has no default to revert to"
    );
    assert_eq!(
        property_get_revert("FixtureBase", "position"),
        Some(Variant::Vector2(Vector2::ZERO))
    );
    assert_eq!(property_get_revert("FixtureBase", "not_a_property"), None);

    // ⚠️ Override order: the DERIVED default wins for a re-declared property,
    // matching the base-first/derived-last merge `get_property_list` builds.
    assert_eq!(
        property_get_revert("FixtureBase", "visible"),
        Some(Variant::Bool(true)),
        "the base's own default"
    );
    assert_eq!(
        property_get_revert("FixtureDerived", "visible"),
        Some(Variant::Bool(false)),
        "the derived class re-declared `visible`, so revert uses ITS default"
    );

    // Inherited properties are revertable through the chain.
    assert!(
        property_can_revert("FixtureDerived", "position"),
        "an inherited property is revertable from the derived class"
    );
}
