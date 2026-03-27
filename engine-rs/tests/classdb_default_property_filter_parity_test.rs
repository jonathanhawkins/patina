//! pat-e6mg: Match ClassDB default-property filtering for script-exported fields.
//!
//! Godot's scene serialization contract:
//!   - Class (engine) properties are saved only when they differ from ClassDB defaults.
//!   - Script-exported (@export) properties are saved only when they differ from
//!     the default declared in the GDScript source.
//!   - Internal properties (prefix `_`) and `script` are never saved.
//!   - `metadata/*` properties are always saved.
//!
//! Acceptance: API probe and runtime tests agree on default-property filtering
//! for script-exported fields.

use gdvariant::Variant;
use patina_runner::class_defaults;

// ===========================================================================
// Part 1: should_output_script_property — basic filtering
// ===========================================================================

/// Script property matching its default should NOT be output.
#[test]
fn script_property_at_default_not_output() {
    let default = Variant::Float(100.0);
    let current = Variant::Float(100.0);
    assert!(
        !class_defaults::should_output_script_property("speed", &current, &default),
        "script property matching default should not be output"
    );
}

/// Script property differing from default SHOULD be output.
#[test]
fn script_property_changed_from_default_is_output() {
    let default = Variant::Float(100.0);
    let current = Variant::Float(250.0);
    assert!(
        class_defaults::should_output_script_property("speed", &current, &default),
        "script property differing from default should be output"
    );
}

/// Script bool property at default should NOT be output.
#[test]
fn script_bool_at_default_not_output() {
    let default = Variant::Bool(true);
    let current = Variant::Bool(true);
    assert!(
        !class_defaults::should_output_script_property("can_shoot", &current, &default),
    );
}

/// Script bool property changed should be output.
#[test]
fn script_bool_changed_is_output() {
    let default = Variant::Bool(true);
    let current = Variant::Bool(false);
    assert!(
        class_defaults::should_output_script_property("can_shoot", &current, &default),
    );
}

/// Script int property at default should NOT be output.
#[test]
fn script_int_at_default_not_output() {
    let default = Variant::Int(100);
    let current = Variant::Int(100);
    assert!(
        !class_defaults::should_output_script_property("health", &current, &default),
    );
}

/// Script int property changed should be output.
#[test]
fn script_int_changed_is_output() {
    let default = Variant::Int(100);
    let current = Variant::Int(50);
    assert!(
        class_defaults::should_output_script_property("health", &current, &default),
    );
}

/// Script string property at default should NOT be output.
#[test]
fn script_string_at_default_not_output() {
    let default = Variant::String("Player".into());
    let current = Variant::String("Player".into());
    assert!(
        !class_defaults::should_output_script_property("name_str", &current, &default),
    );
}

/// Script string property changed should be output.
#[test]
fn script_string_changed_is_output() {
    let default = Variant::String("Player".into());
    let current = Variant::String("Enemy".into());
    assert!(
        class_defaults::should_output_script_property("name_str", &current, &default),
    );
}

/// Script Vector2 property at default should NOT be output.
#[test]
fn script_vector2_at_default_not_output() {
    let default = Variant::Vector2(gdcore::math::Vector2::ZERO);
    let current = Variant::Vector2(gdcore::math::Vector2::ZERO);
    assert!(
        !class_defaults::should_output_script_property("velocity", &current, &default),
    );
}

/// Script Vector2 property changed should be output.
#[test]
fn script_vector2_changed_is_output() {
    let default = Variant::Vector2(gdcore::math::Vector2::ZERO);
    let current = Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0));
    assert!(
        class_defaults::should_output_script_property("velocity", &current, &default),
    );
}

// ===========================================================================
// Part 2: should_output_script_property — internal/special names
// ===========================================================================

/// Internal script properties (prefix `_`) should never be output.
#[test]
fn script_internal_property_never_output() {
    assert!(
        !class_defaults::should_output_script_property(
            "_internal_state",
            &Variant::Int(42),
            &Variant::Int(0)
        ),
        "internal properties (prefix _) must never be output even if changed"
    );
}

/// `script` property should never be output via script filter.
#[test]
fn script_ref_never_output() {
    assert!(
        !class_defaults::should_output_script_property(
            "script",
            &Variant::String("res://test.gd".into()),
            &Variant::Nil
        ),
    );
}

// ===========================================================================
// Part 3: should_output_script_property — float tolerance
// ===========================================================================

/// Script float within tolerance should NOT be output (matches Godot serialization).
#[test]
fn script_float_within_tolerance_not_output() {
    let default = Variant::Float(100.0);
    let current = Variant::Float(100.0005); // within 0.001 tolerance
    assert!(
        !class_defaults::should_output_script_property("speed", &current, &default),
        "float within tolerance should be considered equal to default"
    );
}

/// Script float outside tolerance should be output.
#[test]
fn script_float_outside_tolerance_is_output() {
    let default = Variant::Float(100.0);
    let current = Variant::Float(100.5);
    assert!(
        class_defaults::should_output_script_property("speed", &current, &default),
        "float outside tolerance should be output"
    );
}

// ===========================================================================
// Part 4: get_property_default — class property lookup
// ===========================================================================

/// Known class property returns the correct default.
#[test]
fn get_property_default_known_class_property() {
    let default = class_defaults::get_property_default("Node2D", "position");
    assert!(default.is_some(), "Node2D position should be a known default");
    assert_eq!(
        *default.unwrap(),
        Variant::Vector2(gdcore::math::Vector2::ZERO)
    );
}

/// Known inherited property returns the correct default.
#[test]
fn get_property_default_inherited_property() {
    // Sprite2D inherits from Node2D which inherits from CanvasItem
    let default = class_defaults::get_property_default("Sprite2D", "visible");
    assert!(default.is_some(), "Sprite2D should inherit 'visible' default");
    assert_eq!(*default.unwrap(), Variant::Bool(true));
}

/// Unknown property returns None.
#[test]
fn get_property_default_unknown_property() {
    let default = class_defaults::get_property_default("Node2D", "nonexistent_prop");
    assert!(default.is_none(), "unknown property should return None");
}

/// Unknown class returns None.
#[test]
fn get_property_default_unknown_class() {
    let default = class_defaults::get_property_default("UnknownClass", "position");
    assert!(default.is_none(), "unknown class should return None");
}

/// RigidBody2D mass default is 1.0.
#[test]
fn get_property_default_rigidbody2d_mass() {
    let default = class_defaults::get_property_default("RigidBody2D", "mass");
    assert!(default.is_some());
    assert_eq!(*default.unwrap(), Variant::Float(1.0));
}

// ===========================================================================
// Part 5: Integration — class vs script property filtering agreement
// ===========================================================================

/// Class property at default: both should_output_property and get_property_default agree.
#[test]
fn class_and_script_filter_agreement_at_default() {
    let default = class_defaults::get_property_default("Node2D", "rotation").unwrap();
    // Class filter says: don't output (matches default).
    assert!(!class_defaults::should_output_property(
        "Node2D",
        "rotation",
        default
    ));
    // If this were also a script property with same default, script filter agrees.
    assert!(!class_defaults::should_output_script_property(
        "rotation",
        default,
        default
    ));
}

/// Class property changed from default: both filters agree it should be output.
#[test]
fn class_and_script_filter_agreement_changed() {
    let default = class_defaults::get_property_default("Node2D", "rotation").unwrap();
    let changed = Variant::Float(1.5);
    assert!(class_defaults::should_output_property(
        "Node2D",
        "rotation",
        &changed
    ));
    assert!(class_defaults::should_output_script_property(
        "rotation",
        &changed,
        default
    ));
}

/// Script-only property (not in ClassDB) is correctly handled.
///
/// Godot contract: a property like "speed" on a Node2D is not a class property,
/// so should_output_property returns false. But should_output_script_property
/// handles it correctly based on the script default.
#[test]
fn script_only_property_class_filter_rejects_script_filter_accepts() {
    // "speed" is not a Node2D class property.
    assert!(
        !class_defaults::should_output_property("Node2D", "speed", &Variant::Float(200.0)),
        "class filter should reject unknown property"
    );

    // But script filter accepts it when it differs from the script default.
    assert!(
        class_defaults::should_output_script_property(
            "speed",
            &Variant::Float(200.0),
            &Variant::Float(100.0)
        ),
        "script filter should accept when value differs from script default"
    );

    // Script filter rejects when it matches the script default.
    assert!(
        !class_defaults::should_output_script_property(
            "speed",
            &Variant::Float(100.0),
            &Variant::Float(100.0)
        ),
        "script filter should reject when value matches script default"
    );
}

/// Nil default: script property with Nil default is always output if non-Nil.
#[test]
fn script_property_nil_default_non_nil_value_is_output() {
    assert!(class_defaults::should_output_script_property(
        "target",
        &Variant::String("enemy".into()),
        &Variant::Nil
    ));
}

/// Nil default and Nil value: should not be output.
#[test]
fn script_property_nil_default_nil_value_not_output() {
    assert!(!class_defaults::should_output_script_property(
        "target",
        &Variant::Nil,
        &Variant::Nil
    ));
}

// ===========================================================================
// Part 6: pat-e6mg — Array-typed script-exported fields
// ===========================================================================

/// Script array property at default should NOT be output.
#[test]
fn script_array_at_default_not_output() {
    let default = Variant::Array(vec![
        Variant::String("enemy".into()),
        Variant::String("flying".into()),
    ]);
    let current = Variant::Array(vec![
        Variant::String("enemy".into()),
        Variant::String("flying".into()),
    ]);
    assert!(
        !class_defaults::should_output_script_property("tags", &current, &default),
        "array matching default should not be output"
    );
}

/// Script array property changed (different element) should be output.
#[test]
fn script_array_changed_element_is_output() {
    let default = Variant::Array(vec![
        Variant::String("enemy".into()),
        Variant::String("flying".into()),
    ]);
    let current = Variant::Array(vec![
        Variant::String("enemy".into()),
        Variant::String("ground".into()),
    ]);
    assert!(
        class_defaults::should_output_script_property("tags", &current, &default),
        "array with changed element should be output"
    );
}

/// Script array property changed (different length) should be output.
#[test]
fn script_array_changed_length_is_output() {
    let default = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
    let current = Variant::Array(vec![Variant::Int(1)]);
    assert!(
        class_defaults::should_output_script_property("ids", &current, &default),
        "array with different length should be output"
    );
}

/// Empty array matching empty array default should NOT be output.
#[test]
fn script_empty_array_at_default_not_output() {
    let default = Variant::Array(vec![]);
    let current = Variant::Array(vec![]);
    assert!(
        !class_defaults::should_output_script_property("items", &current, &default),
        "empty arrays matching should not be output"
    );
}

/// Non-empty array vs empty array default should be output.
#[test]
fn script_nonempty_array_vs_empty_default_is_output() {
    let default = Variant::Array(vec![]);
    let current = Variant::Array(vec![Variant::Int(42)]);
    assert!(
        class_defaults::should_output_script_property("items", &current, &default),
        "non-empty array vs empty default should be output"
    );
}

// ===========================================================================
// Part 7: pat-e6mg — Cross-type numeric (Int vs Float) are distinct in Godot
// ===========================================================================

/// Godot's property serialization treats Int and Float as distinct types.
/// Int(0) != Float(0.0) for default comparison purposes.
#[test]
fn script_int_float_cross_type_are_distinct() {
    // Int(0) vs Float(0.0) → different types → always output.
    assert!(
        class_defaults::should_output_script_property(
            "level",
            &Variant::Float(0.0),
            &Variant::Int(0)
        ),
        "Float(0.0) should differ from Int(0) default (distinct Variant types)"
    );
    assert!(
        class_defaults::should_output_script_property(
            "level",
            &Variant::Int(0),
            &Variant::Float(0.0)
        ),
        "Int(0) should differ from Float(0.0) default (distinct Variant types)"
    );
}

// ===========================================================================
// Part 8: pat-e6mg — Nested array comparison
// ===========================================================================

/// Nested arrays at default should NOT be output.
#[test]
fn script_nested_array_at_default_not_output() {
    let default = Variant::Array(vec![
        Variant::Array(vec![Variant::Int(1), Variant::Int(2)]),
        Variant::Array(vec![Variant::Int(3)]),
    ]);
    let current = Variant::Array(vec![
        Variant::Array(vec![Variant::Int(1), Variant::Int(2)]),
        Variant::Array(vec![Variant::Int(3)]),
    ]);
    assert!(
        !class_defaults::should_output_script_property("grid", &current, &default),
        "nested arrays matching should not be output"
    );
}

/// Nested arrays with inner change should be output.
#[test]
fn script_nested_array_inner_change_is_output() {
    let default = Variant::Array(vec![
        Variant::Array(vec![Variant::Int(1), Variant::Int(2)]),
    ]);
    let current = Variant::Array(vec![
        Variant::Array(vec![Variant::Int(1), Variant::Int(99)]),
    ]);
    assert!(
        class_defaults::should_output_script_property("grid", &current, &default),
        "nested array with inner change should be output"
    );
}

// ===========================================================================
// Part 9: pat-e6mg — Class filter + script filter integration for exports
// ===========================================================================

/// A script-exported field that shadows a class property name: both filters
/// must agree when value matches the class default AND the script default.
#[test]
fn script_export_shadowing_class_property_both_agree_at_default() {
    // "position" is a Node2D class property with default Vector2(0,0).
    // A script also exports "position" with the same default.
    let class_default = class_defaults::get_property_default("Node2D", "position").unwrap();
    let script_default = Variant::Vector2(gdcore::math::Vector2::ZERO);

    // Class filter: at default → don't output.
    assert!(!class_defaults::should_output_property(
        "Node2D", "position", class_default
    ));
    // Script filter: at default → don't output.
    assert!(!class_defaults::should_output_script_property(
        "position", class_default, &script_default
    ));
}

/// A script-exported field that shadows a class property: both detect change.
#[test]
fn script_export_shadowing_class_property_both_detect_change() {
    let changed = Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0));
    let script_default = Variant::Vector2(gdcore::math::Vector2::ZERO);

    assert!(class_defaults::should_output_property("Node2D", "position", &changed));
    assert!(class_defaults::should_output_script_property("position", &changed, &script_default));
}

/// A script-exported field with a DIFFERENT default than the class property:
/// class filter says "don't output" (matches class default), but script filter
/// says "output" (differs from script default). This tests the Godot 4.6.1 contract
/// where script defaults take precedence for export filtering.
#[test]
fn script_export_different_default_than_class() {
    // Class default for Node2D "rotation" is 0.0.
    let class_default = class_defaults::get_property_default("Node2D", "rotation").unwrap();
    assert_eq!(*class_default, Variant::Float(0.0));

    // Script exports "rotation" with default 1.5 (non-zero).
    let script_default = Variant::Float(1.5);
    let current = Variant::Float(0.0); // value is at the CLASS default, not script default

    // Class filter: matches class default → don't output.
    assert!(!class_defaults::should_output_property("Node2D", "rotation", &current));
    // Script filter: differs from script default (1.5 != 0.0) → output!
    assert!(
        class_defaults::should_output_script_property("rotation", &current, &script_default),
        "script filter must output when value differs from script default, even if it matches class default (4.6.1 contract)"
    );
}

/// metadata/ properties are always output regardless of value.
#[test]
fn metadata_properties_always_output() {
    assert!(class_defaults::should_output_property(
        "Node2D", "metadata/custom_key", &Variant::Int(0)
    ));
    assert!(class_defaults::should_output_property(
        "Node2D", "metadata/custom_key", &Variant::Nil
    ));
    assert!(class_defaults::should_output_property(
        "Sprite2D", "metadata/editor_hint", &Variant::Bool(false)
    ));
}

// ===========================================================================
// Part 10: pat-e6mg — Script export filtering completeness for all Variant types
// ===========================================================================

/// Script-exported Color property at default should NOT be output.
#[test]
fn script_color_at_default_not_output() {
    let default = Variant::Color(gdcore::math::Color::WHITE);
    let current = Variant::Color(gdcore::math::Color::WHITE);
    assert!(
        !class_defaults::should_output_script_property("tint", &current, &default),
        "Color matching default should not be output"
    );
}

/// Script-exported Color property changed should be output.
#[test]
fn script_color_changed_is_output() {
    let default = Variant::Color(gdcore::math::Color::WHITE);
    let current = Variant::Color(gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0));
    assert!(
        class_defaults::should_output_script_property("tint", &current, &default),
        "Color differing from default should be output"
    );
}

/// Script-exported NodePath at default should NOT be output.
#[test]
fn script_nodepath_at_default_not_output() {
    let default = Variant::NodePath(gdcore::NodePath::new(""));
    let current = Variant::NodePath(gdcore::NodePath::new(""));
    assert!(
        !class_defaults::should_output_script_property("target", &current, &default),
    );
}

/// Script-exported NodePath changed should be output.
#[test]
fn script_nodepath_changed_is_output() {
    let default = Variant::NodePath(gdcore::NodePath::new(""));
    let current = Variant::NodePath(gdcore::NodePath::new("/root/Enemy"));
    assert!(
        class_defaults::should_output_script_property("target", &current, &default),
    );
}

/// Script-exported Rect2 at default should NOT be output.
#[test]
fn script_rect2_at_default_not_output() {
    let default = Variant::Rect2(gdcore::math::Rect2::new(
        gdcore::math::Vector2::ZERO,
        gdcore::math::Vector2::new(100.0, 100.0),
    ));
    let current = Variant::Rect2(gdcore::math::Rect2::new(
        gdcore::math::Vector2::ZERO,
        gdcore::math::Vector2::new(100.0, 100.0),
    ));
    assert!(
        !class_defaults::should_output_script_property("bounds", &current, &default),
    );
}

/// Script-exported Rect2 changed should be output.
#[test]
fn script_rect2_changed_is_output() {
    let default = Variant::Rect2(gdcore::math::Rect2::new(
        gdcore::math::Vector2::ZERO,
        gdcore::math::Vector2::new(100.0, 100.0),
    ));
    let current = Variant::Rect2(gdcore::math::Rect2::new(
        gdcore::math::Vector2::new(10.0, 10.0),
        gdcore::math::Vector2::new(200.0, 200.0),
    ));
    assert!(
        class_defaults::should_output_script_property("bounds", &current, &default),
    );
}

// ===========================================================================
// Part 11: pat-e6mg — Multiple script exports on a single class: filtering
//          must be independent per-property
// ===========================================================================

/// Each script-exported field is independently filtered against its own default.
#[test]
fn multiple_script_exports_independent_filtering() {
    // Simulate a script with three exports: speed=100.0, health=50, label="Player"
    let speed_default = Variant::Float(100.0);
    let health_default = Variant::Int(50);
    let label_default = Variant::String("Player".into());

    // Instance has speed changed, health at default, label changed.
    let speed_val = Variant::Float(200.0);
    let health_val = Variant::Int(50);
    let label_val = Variant::String("Enemy".into());

    assert!(
        class_defaults::should_output_script_property("speed", &speed_val, &speed_default),
        "speed changed → output"
    );
    assert!(
        !class_defaults::should_output_script_property("health", &health_val, &health_default),
        "health at default → skip"
    );
    assert!(
        class_defaults::should_output_script_property("label", &label_val, &label_default),
        "label changed → output"
    );
}

/// Script export with Vector2 default that uses non-zero values.
#[test]
fn script_vector2_nonzero_default_at_default_not_output() {
    let default = Variant::Vector2(gdcore::math::Vector2::new(50.0, 75.0));
    let current = Variant::Vector2(gdcore::math::Vector2::new(50.0, 75.0));
    assert!(
        !class_defaults::should_output_script_property("spawn_point", &current, &default),
        "Vector2 at non-zero default should not be output"
    );
}

/// Script export: value reverted back to default after change should NOT be output.
#[test]
fn script_export_reverted_to_default_not_output() {
    // A property was changed at runtime but then set back to the default value.
    // Godot's serialization should NOT save it (it matches the default again).
    let default = Variant::Float(100.0);
    let reverted = Variant::Float(100.0);
    assert!(
        !class_defaults::should_output_script_property("speed", &reverted, &default),
        "value reverted to default should not be output"
    );
}

/// Script export: class property at class-default but script default differs.
/// This is the critical 4.6.1 contract test: script default takes precedence.
#[test]
fn script_export_precedence_over_class_default_for_visible() {
    // Class default for "visible" on Node2D is true.
    let class_default = class_defaults::get_property_default("Node2D", "visible").unwrap();
    assert_eq!(*class_default, Variant::Bool(true));

    // Script re-exports "visible" with a default of false.
    let script_default = Variant::Bool(false);
    let current = Variant::Bool(true); // at CLASS default, not script default

    // Class filter: matches class default → don't output.
    assert!(!class_defaults::should_output_property("Node2D", "visible", &current));
    // Script filter: differs from script default → output!
    assert!(
        class_defaults::should_output_script_property("visible", &current, &script_default),
        "script filter must output when value matches class default but differs from script default"
    );
}
