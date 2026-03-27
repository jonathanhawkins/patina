//! pat-gphi: Revalidate class-default filtering and explicit-property comparison under 4.6.1.
//!
//! Acceptance:
//!   - `class_defaults.rs` filtering is still correct for the repinned 4.6.1 target
//!   - No new false positives dominate parity reporting
//!   - `variant_eq` handles all Variant types used in property comparisons
//!   - Script property filtering correctly distinguishes default vs changed values

use gdcore::math::{Color, Rect2, Vector2, Vector3};
use gdcore::{NodePath, StringName};
use gdvariant::Variant;
use patina_runner::class_defaults;

// ===========================================================================
// Part 1: variant_eq completeness — all Variant types that may appear in
//         class or script property defaults under 4.6.1
// ===========================================================================

#[test]
fn variant_eq_stringname_equal() {
    let a = Variant::StringName(StringName::new("idle"));
    let b = Variant::StringName(StringName::new("idle"));
    assert!(
        class_defaults::variant_eq(&a, &b),
        "identical StringName values must compare equal"
    );
}

#[test]
fn variant_eq_stringname_not_equal() {
    let a = Variant::StringName(StringName::new("idle"));
    let b = Variant::StringName(StringName::new("run"));
    assert!(
        !class_defaults::variant_eq(&a, &b),
        "different StringName values must not compare equal"
    );
}

#[test]
fn variant_eq_nodepath_equal() {
    let a = Variant::NodePath(NodePath::new("/root/Player"));
    let b = Variant::NodePath(NodePath::new("/root/Player"));
    assert!(
        class_defaults::variant_eq(&a, &b),
        "identical NodePath values must compare equal"
    );
}

#[test]
fn variant_eq_nodepath_not_equal() {
    let a = Variant::NodePath(NodePath::new("/root/Player"));
    let b = Variant::NodePath(NodePath::new("/root/Enemy"));
    assert!(
        !class_defaults::variant_eq(&a, &b),
        "different NodePath values must not compare equal"
    );
}

#[test]
fn variant_eq_nodepath_empty() {
    let a = Variant::NodePath(NodePath::new(""));
    let b = Variant::NodePath(NodePath::new(""));
    assert!(
        class_defaults::variant_eq(&a, &b),
        "empty NodePaths must compare equal"
    );
}

#[test]
fn variant_eq_rect2_equal() {
    let a = Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 100.0)));
    let b = Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 100.0)));
    assert!(
        class_defaults::variant_eq(&a, &b),
        "identical Rect2 values must compare equal"
    );
}

#[test]
fn variant_eq_rect2_within_tolerance() {
    let a = Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 100.0)));
    let b = Variant::Rect2(Rect2::new(
        Vector2::new(0.0005, 0.0),
        Vector2::new(100.0, 99.9995),
    ));
    assert!(
        class_defaults::variant_eq(&a, &b),
        "Rect2 values within tolerance must compare equal"
    );
}

#[test]
fn variant_eq_rect2_not_equal() {
    let a = Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(100.0, 100.0)));
    let b = Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(200.0, 100.0)));
    assert!(
        !class_defaults::variant_eq(&a, &b),
        "different Rect2 values must not compare equal"
    );
}

#[test]
fn variant_eq_cross_type_always_false() {
    // Cross-type comparisons must never be equal (4.6.1 contract).
    let pairs: Vec<(Variant, Variant)> = vec![
        (Variant::Int(0), Variant::Float(0.0)),
        (Variant::String("0".into()), Variant::Int(0)),
        (Variant::Bool(false), Variant::Int(0)),
        (Variant::Nil, Variant::Int(0)),
        (Variant::Nil, Variant::Bool(false)),
        (Variant::Nil, Variant::String(String::new())),
        (
            Variant::String("idle".into()),
            Variant::StringName(StringName::new("idle")),
        ),
        (
            Variant::Vector2(Vector2::ZERO),
            Variant::Vector3(Vector3::ZERO),
        ),
    ];
    for (a, b) in &pairs {
        assert!(
            !class_defaults::variant_eq(a, b),
            "cross-type comparison must be false: {:?} vs {:?}",
            a.variant_type(),
            b.variant_type()
        );
    }
}

// ===========================================================================
// Part 2: Registry completeness — every registered class has the expected
//         property defaults matching Godot 4.6.1
// ===========================================================================

/// All 2D node classes that should be in the registry.
const EXPECTED_CLASSES: &[&str] = &[
    "Node2D",
    "Sprite2D",
    "AnimatedSprite2D",
    "CharacterBody2D",
    "StaticBody2D",
    "RigidBody2D",
    "Area2D",
    "Camera2D",
    "CollisionShape2D",
    "CollisionPolygon2D",
    "RayCast2D",
    "Path2D",
    "PathFollow2D",
    "Line2D",
    "Polygon2D",
    "Light2D",
    "PointLight2D",
    "DirectionalLight2D",
    "AudioStreamPlayer2D",
    "NavigationAgent2D",
    "TileMap",
    "Marker2D",
    "RemoteTransform2D",
    "VisibleOnScreenNotifier2D",
    "GPUParticles2D",
    "CPUParticles2D",
    "Parallax2D",
    "Control",
    "Label",
    "Button",
];

#[test]
fn all_expected_classes_registered() {
    for class in EXPECTED_CLASSES {
        assert!(
            class_defaults::get_property_default(class, "visible").is_some()
                || class_defaults::get_property_default(class, "anchor_left").is_some(),
            "class '{}' must be registered in CLASS_DEFAULTS",
            class
        );
    }
}

#[test]
fn node_and_window_not_in_registry() {
    // Node and Window are intentionally excluded — they have no class-default
    // properties in the oracle property dump.
    assert!(class_defaults::get_property_default("Node", "name").is_none());
    assert!(class_defaults::get_property_default("Window", "title").is_none());
}

// ===========================================================================
// Part 3: Godot 4.6.1 default values validation — confirm defaults match
//         the repinned oracle target
// ===========================================================================

#[test]
fn canvasitem_defaults_match_461() {
    // CanvasItem defaults are inherited by all 2D nodes.
    let cases: Vec<(&str, &str, Variant)> = vec![
        ("Node2D", "visible", Variant::Bool(true)),
        ("Node2D", "modulate", Variant::Color(Color::WHITE)),
        ("Node2D", "self_modulate", Variant::Color(Color::WHITE)),
        ("Node2D", "z_index", Variant::Int(0)),
        ("Node2D", "z_as_relative", Variant::Bool(true)),
        ("Node2D", "show_behind_parent", Variant::Bool(false)),
        ("Node2D", "light_mask", Variant::Int(1)),
    ];
    for (class, prop, expected) in &cases {
        let actual = class_defaults::get_property_default(class, prop);
        assert!(
            actual.is_some(),
            "{class}.{prop} must be registered"
        );
        assert!(
            class_defaults::variant_eq(actual.unwrap(), expected),
            "{class}.{prop} default mismatch: got {:?}, expected {:?}",
            actual.unwrap(),
            expected
        );
    }
}

#[test]
fn node2d_transform_defaults_match_461() {
    let cases: Vec<(&str, Variant)> = vec![
        ("position", Variant::Vector2(Vector2::ZERO)),
        ("rotation", Variant::Float(0.0)),
        ("scale", Variant::Vector2(Vector2::ONE)),
        ("skew", Variant::Float(0.0)),
    ];
    for (prop, expected) in &cases {
        let actual = class_defaults::get_property_default("Node2D", prop).unwrap();
        assert!(
            class_defaults::variant_eq(actual, expected),
            "Node2D.{prop} default mismatch"
        );
    }
}

#[test]
fn collision_object_defaults_match_461() {
    for class in &["CharacterBody2D", "RigidBody2D", "StaticBody2D", "Area2D"] {
        assert_eq!(
            *class_defaults::get_property_default(class, "collision_layer").unwrap(),
            Variant::Int(1),
            "{class}.collision_layer default"
        );
        assert_eq!(
            *class_defaults::get_property_default(class, "collision_mask").unwrap(),
            Variant::Int(1),
            "{class}.collision_mask default"
        );
        assert_eq!(
            *class_defaults::get_property_default(class, "input_pickable").unwrap(),
            Variant::Bool(true),
            "{class}.input_pickable default"
        );
    }
}

#[test]
fn rigidbody2d_defaults_match_461() {
    let cases: Vec<(&str, Variant)> = vec![
        ("mass", Variant::Float(1.0)),
        ("gravity_scale", Variant::Float(1.0)),
        ("continuous_cd", Variant::Int(0)),
        ("linear_velocity", Variant::Vector2(Vector2::ZERO)),
        ("angular_velocity", Variant::Float(0.0)),
        ("can_sleep", Variant::Bool(true)),
        ("lock_rotation", Variant::Bool(false)),
    ];
    for (prop, expected) in &cases {
        let actual = class_defaults::get_property_default("RigidBody2D", prop).unwrap();
        assert!(
            class_defaults::variant_eq(actual, &expected),
            "RigidBody2D.{prop} default mismatch"
        );
    }
}

#[test]
fn characterbody2d_defaults_match_461() {
    assert_eq!(
        *class_defaults::get_property_default("CharacterBody2D", "motion_mode").unwrap(),
        Variant::Int(0)
    );
    let floor_angle =
        class_defaults::get_property_default("CharacterBody2D", "floor_max_angle").unwrap();
    assert!(class_defaults::variant_eq(
        floor_angle,
        &Variant::Float(std::f64::consts::FRAC_PI_4)
    ));
    assert!(class_defaults::variant_eq(
        class_defaults::get_property_default("CharacterBody2D", "velocity").unwrap(),
        &Variant::Vector2(Vector2::ZERO)
    ));
}

#[test]
fn control_layout_defaults_match_461() {
    for prop in &[
        "anchor_left",
        "anchor_top",
        "anchor_right",
        "anchor_bottom",
        "offset_left",
        "offset_top",
        "offset_right",
        "offset_bottom",
    ] {
        let actual = class_defaults::get_property_default("Control", prop).unwrap();
        assert!(
            class_defaults::variant_eq(actual, &Variant::Float(0.0)),
            "Control.{prop} default should be 0.0"
        );
    }
}

#[test]
fn label_defaults_match_461() {
    assert_eq!(
        *class_defaults::get_property_default("Label", "text").unwrap(),
        Variant::String(String::new())
    );
    assert_eq!(
        *class_defaults::get_property_default("Label", "horizontal_alignment").unwrap(),
        Variant::Int(0)
    );
}

#[test]
fn sprite2d_defaults_match_461() {
    let cases: Vec<(&str, Variant)> = vec![
        ("offset", Variant::Vector2(Vector2::ZERO)),
        ("flip_h", Variant::Bool(false)),
        ("flip_v", Variant::Bool(false)),
        ("centered", Variant::Bool(true)),
        ("frame", Variant::Int(0)),
        ("hframes", Variant::Int(1)),
        ("vframes", Variant::Int(1)),
    ];
    for (prop, expected) in &cases {
        let actual = class_defaults::get_property_default("Sprite2D", prop).unwrap();
        assert!(
            class_defaults::variant_eq(actual, &expected),
            "Sprite2D.{prop} default mismatch"
        );
    }
}

// ===========================================================================
// Part 4: False positive guards — properties at default must NOT be output,
//         properties changed from default MUST be output
// ===========================================================================

#[test]
fn no_false_positives_all_classes_at_defaults() {
    // For every registered class, confirm that default-valued properties
    // are correctly suppressed (not output).
    let test_props: Vec<(&str, &str, Variant)> = vec![
        ("Node2D", "position", Variant::Vector2(Vector2::ZERO)),
        ("Node2D", "rotation", Variant::Float(0.0)),
        ("Node2D", "scale", Variant::Vector2(Vector2::ONE)),
        ("Node2D", "visible", Variant::Bool(true)),
        ("Sprite2D", "flip_h", Variant::Bool(false)),
        ("Sprite2D", "centered", Variant::Bool(true)),
        ("CharacterBody2D", "motion_mode", Variant::Int(0)),
        ("RigidBody2D", "mass", Variant::Float(1.0)),
        ("RigidBody2D", "can_sleep", Variant::Bool(true)),
        ("Area2D", "monitoring", Variant::Bool(true)),
        ("Area2D", "monitorable", Variant::Bool(true)),
        ("Camera2D", "zoom", Variant::Vector2(Vector2::ONE)),
        ("CollisionShape2D", "disabled", Variant::Bool(false)),
        ("Control", "anchor_left", Variant::Float(0.0)),
        ("Label", "text", Variant::String(String::new())),
        ("Button", "flat", Variant::Bool(false)),
        (
            "AnimatedSprite2D",
            "animation",
            Variant::String("default".into()),
        ),
        ("StaticBody2D", "constant_angular_velocity", Variant::Float(0.0)),
    ];
    for (class, prop, default_val) in &test_props {
        assert!(
            !class_defaults::should_output_property(class, prop, default_val),
            "false positive: {class}.{prop} at default should NOT be output"
        );
    }
}

#[test]
fn no_false_negatives_changed_properties_output() {
    // Properties that differ from default MUST be output.
    let test_props: Vec<(&str, &str, Variant)> = vec![
        (
            "Node2D",
            "position",
            Variant::Vector2(Vector2::new(100.0, 200.0)),
        ),
        ("Node2D", "rotation", Variant::Float(1.57)),
        (
            "Node2D",
            "scale",
            Variant::Vector2(Vector2::new(2.0, 2.0)),
        ),
        ("Node2D", "visible", Variant::Bool(false)),
        ("Sprite2D", "flip_h", Variant::Bool(true)),
        ("CharacterBody2D", "collision_mask", Variant::Int(3)),
        ("RigidBody2D", "mass", Variant::Float(5.0)),
        ("Area2D", "monitoring", Variant::Bool(false)),
        (
            "Camera2D",
            "zoom",
            Variant::Vector2(Vector2::new(2.0, 2.0)),
        ),
        ("CollisionShape2D", "disabled", Variant::Bool(true)),
        ("Control", "anchor_right", Variant::Float(1.0)),
        ("Label", "text", Variant::String("Hello".into())),
        ("Button", "disabled", Variant::Bool(true)),
        (
            "Node2D",
            "modulate",
            Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)),
        ),
    ];
    for (class, prop, changed_val) in &test_props {
        assert!(
            class_defaults::should_output_property(class, prop, changed_val),
            "false negative: {class}.{prop} changed from default MUST be output"
        );
    }
}

// ===========================================================================
// Part 5: Script property filtering under 4.6.1 — script-exported variables
//         use should_output_script_property with the script-declared default
// ===========================================================================

#[test]
fn script_property_stringname_at_default_not_output() {
    let default = Variant::StringName(StringName::new("idle"));
    let current = Variant::StringName(StringName::new("idle"));
    assert!(
        !class_defaults::should_output_script_property("state", &current, &default),
        "StringName script property at default should not be output"
    );
}

#[test]
fn script_property_stringname_changed_is_output() {
    let default = Variant::StringName(StringName::new("idle"));
    let current = Variant::StringName(StringName::new("attack"));
    assert!(
        class_defaults::should_output_script_property("state", &current, &default),
        "StringName script property changed should be output"
    );
}

#[test]
fn script_property_nodepath_at_default_not_output() {
    let default = Variant::NodePath(NodePath::new(""));
    let current = Variant::NodePath(NodePath::new(""));
    assert!(
        !class_defaults::should_output_script_property("target_path", &current, &default),
        "NodePath script property at default should not be output"
    );
}

#[test]
fn script_property_nodepath_changed_is_output() {
    let default = Variant::NodePath(NodePath::new(""));
    let current = Variant::NodePath(NodePath::new("/root/Enemy"));
    assert!(
        class_defaults::should_output_script_property("target_path", &current, &default),
        "NodePath script property changed should be output"
    );
}

#[test]
fn script_property_vector3_at_default_not_output() {
    let default = Variant::Vector3(Vector3::ZERO);
    let current = Variant::Vector3(Vector3::ZERO);
    assert!(
        !class_defaults::should_output_script_property("offset_3d", &current, &default),
    );
}

#[test]
fn script_property_vector3_changed_is_output() {
    let default = Variant::Vector3(Vector3::ZERO);
    let current = Variant::Vector3(Vector3::new(1.0, 2.0, 3.0));
    assert!(
        class_defaults::should_output_script_property("offset_3d", &current, &default),
    );
}

#[test]
fn script_property_color_at_default_not_output() {
    let default = Variant::Color(Color::WHITE);
    let current = Variant::Color(Color::WHITE);
    assert!(
        !class_defaults::should_output_script_property("tint", &current, &default),
    );
}

#[test]
fn script_property_color_changed_is_output() {
    let default = Variant::Color(Color::WHITE);
    let current = Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0));
    assert!(
        class_defaults::should_output_script_property("tint", &current, &default),
    );
}

// ===========================================================================
// Part 6: Inheritance chain consistency — derived classes inherit all base
//         class defaults correctly
// ===========================================================================

#[test]
fn all_node2d_derived_inherit_canvasitem_props() {
    let node2d_derived = &[
        "Node2D",
        "Sprite2D",
        "AnimatedSprite2D",
        "CharacterBody2D",
        "StaticBody2D",
        "RigidBody2D",
        "Area2D",
        "Camera2D",
        "CollisionShape2D",
        "Marker2D",
        "TileMap",
    ];
    let canvas_item_props = &["visible", "modulate", "z_index", "light_mask"];

    for class in node2d_derived {
        for prop in canvas_item_props {
            assert!(
                class_defaults::get_property_default(class, prop).is_some(),
                "{class} must inherit CanvasItem property '{prop}'"
            );
        }
    }
}

#[test]
fn all_node2d_derived_inherit_transform_props() {
    let node2d_derived = &[
        "Sprite2D",
        "CharacterBody2D",
        "RigidBody2D",
        "Area2D",
        "Camera2D",
        "CollisionShape2D",
    ];
    let transform_props = &["position", "rotation", "scale", "skew"];

    for class in node2d_derived {
        for prop in transform_props {
            assert!(
                class_defaults::get_property_default(class, prop).is_some(),
                "{class} must inherit Node2D transform property '{prop}'"
            );
        }
    }
}

#[test]
fn control_does_not_inherit_node2d_transform() {
    // Control inherits from CanvasItem, NOT Node2D.
    assert!(
        class_defaults::get_property_default("Control", "position").is_none(),
        "Control must NOT have Node2D position"
    );
    assert!(
        class_defaults::get_property_default("Control", "rotation").is_none(),
        "Control must NOT have Node2D rotation"
    );
    // But it does inherit CanvasItem properties.
    assert!(
        class_defaults::get_property_default("Control", "visible").is_some(),
        "Control must inherit CanvasItem visible"
    );
}

// ===========================================================================
// Part 7: Edge cases under 4.6.1
// ===========================================================================

#[test]
fn metadata_properties_always_output_regardless_of_class() {
    // metadata/ prefix properties are user-defined and always output.
    for class in EXPECTED_CLASSES {
        assert!(
            class_defaults::should_output_property(
                class,
                "metadata/custom",
                &Variant::String("value".into())
            ),
            "metadata/ must always be output for {class}"
        );
    }
    // Even for unknown classes.
    assert!(class_defaults::should_output_property(
        "UnknownNode",
        "metadata/tag",
        &Variant::Int(1)
    ));
}

#[test]
fn internal_prefix_never_output_for_any_class() {
    for class in EXPECTED_CLASSES {
        assert!(
            !class_defaults::should_output_property(
                class,
                "_hidden",
                &Variant::String("secret".into())
            ),
            "_ prefix must never be output for {class}"
        );
    }
}

#[test]
fn float_tolerance_boundary_at_0_001() {
    // Values within 0.001 tolerance are considered equal.
    assert!(
        class_defaults::variant_eq(&Variant::Float(0.0), &Variant::Float(0.0009)),
        "0.0009 is within 0.001 tolerance — should be equal"
    );
    assert!(
        class_defaults::variant_eq(&Variant::Float(0.0), &Variant::Float(0.0005)),
        "0.0005 is within 0.001 tolerance — should be equal"
    );
    // Exactly at 0.001 is NOT within tolerance (strict <).
    assert!(
        !class_defaults::variant_eq(&Variant::Float(0.0), &Variant::Float(0.001)),
        "0.001 is exactly at boundary — should NOT be equal (strict <)"
    );
    // Beyond tolerance.
    assert!(
        !class_defaults::variant_eq(&Variant::Float(0.0), &Variant::Float(0.002)),
        "0.002 is beyond tolerance — should NOT be equal"
    );
}

#[test]
fn negative_float_tolerance() {
    assert!(class_defaults::variant_eq(
        &Variant::Float(-0.0005),
        &Variant::Float(0.0)
    ));
    assert!(!class_defaults::variant_eq(
        &Variant::Float(-0.5),
        &Variant::Float(0.0)
    ));
}
