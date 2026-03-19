//! Godot class property registry for output filtering.
//!
//! The oracle property dumps from Godot only report properties that:
//! 1. Are part of the class's registered property list (inheritance-aware)
//! 2. Have a value that differs from the class default
//!
//! This module provides a registry of known Godot class properties and their
//! defaults so the runner can filter its output to match oracle format.

use gdvariant::Variant;
use std::collections::HashMap;
use std::sync::LazyLock;

/// A property default: (property_name, default_value).
type PropDefault = (&'static str, Variant);

/// Map from class name → list of property defaults (including inherited).
static CLASS_DEFAULTS: LazyLock<HashMap<&'static str, Vec<PropDefault>>> = LazyLock::new(|| {
    let mut m: HashMap<&str, Vec<PropDefault>> = HashMap::new();

    // -- Node (base) --
    // Node has no commonly-reported properties in the oracle.

    // -- CanvasItem properties (inherited by Node2D and Control) --
    let canvas_item: Vec<PropDefault> = vec![
        ("visible", Variant::Bool(true)),
        ("modulate", Variant::Color(gdcore::math::Color::WHITE)),
        ("self_modulate", Variant::Color(gdcore::math::Color::WHITE)),
        ("z_index", Variant::Int(0)),
        ("z_as_relative", Variant::Bool(true)),
        ("show_behind_parent", Variant::Bool(false)),
        ("light_mask", Variant::Int(1)),
    ];

    // -- Node2D = CanvasItem + transform properties --
    let node2d: Vec<PropDefault> = {
        let mut v = canvas_item.clone();
        v.extend([
            ("position", Variant::Vector2(gdcore::math::Vector2::ZERO)),
            ("rotation", Variant::Float(0.0)),
            ("scale", Variant::Vector2(gdcore::math::Vector2::ONE)),
            ("skew", Variant::Float(0.0)),
        ]);
        v
    };

    // -- CollisionObject2D = Node2D + collision properties --
    let collision_object_2d: Vec<PropDefault> = {
        let mut v = node2d.clone();
        v.extend([
            ("collision_layer", Variant::Int(1)),
            ("collision_mask", Variant::Int(1)),
            ("input_pickable", Variant::Bool(true)),
        ]);
        v
    };

    // -- PhysicsBody2D = CollisionObject2D --
    let physics_body_2d = collision_object_2d.clone();

    // -- StaticBody2D --
    let static_body_2d: Vec<PropDefault> = {
        let mut v = physics_body_2d.clone();
        v.extend([
            (
                "constant_linear_velocity",
                Variant::Vector2(gdcore::math::Vector2::ZERO),
            ),
            ("constant_angular_velocity", Variant::Float(0.0)),
        ]);
        v
    };

    // -- RigidBody2D --
    let rigid_body_2d: Vec<PropDefault> = {
        let mut v = physics_body_2d.clone();
        v.extend([
            ("mass", Variant::Float(1.0)),
            ("gravity_scale", Variant::Float(1.0)),
            ("continuous_cd", Variant::Int(0)),
            (
                "linear_velocity",
                Variant::Vector2(gdcore::math::Vector2::ZERO),
            ),
            ("angular_velocity", Variant::Float(0.0)),
            ("can_sleep", Variant::Bool(true)),
            ("lock_rotation", Variant::Bool(false)),
        ]);
        v
    };

    // -- CharacterBody2D --
    let character_body_2d: Vec<PropDefault> = {
        let mut v = physics_body_2d.clone();
        v.extend([
            ("motion_mode", Variant::Int(0)),
            (
                "floor_max_angle",
                Variant::Float(std::f64::consts::FRAC_PI_4),
            ),
            ("velocity", Variant::Vector2(gdcore::math::Vector2::ZERO)),
        ]);
        v
    };

    // -- Area2D --
    let area_2d: Vec<PropDefault> = {
        let mut v = collision_object_2d.clone();
        v.extend([
            ("monitoring", Variant::Bool(true)),
            ("monitorable", Variant::Bool(true)),
        ]);
        v
    };

    // -- CollisionShape2D --
    let collision_shape_2d: Vec<PropDefault> = {
        let mut v = node2d.clone();
        v.extend([("disabled", Variant::Bool(false))]);
        v
    };

    // -- Sprite2D --
    let sprite_2d: Vec<PropDefault> = {
        let mut v = node2d.clone();
        v.extend([
            ("offset", Variant::Vector2(gdcore::math::Vector2::ZERO)),
            ("flip_h", Variant::Bool(false)),
            ("flip_v", Variant::Bool(false)),
            ("centered", Variant::Bool(true)),
            ("frame", Variant::Int(0)),
            ("hframes", Variant::Int(1)),
            ("vframes", Variant::Int(1)),
        ]);
        v
    };

    // -- AnimatedSprite2D --
    let animated_sprite_2d: Vec<PropDefault> = {
        let mut v = node2d.clone();
        v.extend([
            ("animation", Variant::String("default".into())),
            ("playing", Variant::Bool(false)),
            ("speed_scale", Variant::Float(1.0)),
            ("frame", Variant::Int(0)),
        ]);
        v
    };

    // -- Camera2D --
    let camera_2d: Vec<PropDefault> = {
        let mut v = node2d.clone();
        v.extend([
            ("zoom", Variant::Vector2(gdcore::math::Vector2::ONE)),
            ("offset", Variant::Vector2(gdcore::math::Vector2::ZERO)),
            ("anchor_mode", Variant::Int(1)),
        ]);
        v
    };

    // -- Control = CanvasItem + layout properties --
    let control: Vec<PropDefault> = {
        let mut v = canvas_item.clone();
        v.extend([
            ("anchor_left", Variant::Float(0.0)),
            ("anchor_top", Variant::Float(0.0)),
            ("anchor_right", Variant::Float(0.0)),
            ("anchor_bottom", Variant::Float(0.0)),
            ("offset_left", Variant::Float(0.0)),
            ("offset_top", Variant::Float(0.0)),
            ("offset_right", Variant::Float(0.0)),
            ("offset_bottom", Variant::Float(0.0)),
        ]);
        v
    };

    // -- Label --
    let label: Vec<PropDefault> = {
        let mut v = control.clone();
        v.extend([
            ("text", Variant::String(String::new())),
            ("horizontal_alignment", Variant::Int(0)),
            ("vertical_alignment", Variant::Int(0)),
        ]);
        v
    };

    // -- Button --
    let button: Vec<PropDefault> = {
        let mut v = control.clone();
        v.extend([
            ("text", Variant::String(String::new())),
            ("flat", Variant::Bool(false)),
            ("disabled", Variant::Bool(false)),
        ]);
        v
    };

    // -- Other Node2D-derived classes with no extra known properties --
    let node2d_basic = node2d.clone();

    // Register all classes
    m.insert("Node2D", node2d.clone());
    m.insert("Sprite2D", sprite_2d);
    m.insert("AnimatedSprite2D", animated_sprite_2d);
    m.insert("CharacterBody2D", character_body_2d);
    m.insert("StaticBody2D", static_body_2d);
    m.insert("RigidBody2D", rigid_body_2d);
    m.insert("Area2D", area_2d);
    m.insert("Camera2D", camera_2d);
    m.insert("CollisionShape2D", collision_shape_2d);
    m.insert("CollisionPolygon2D", node2d_basic.clone());
    m.insert("RayCast2D", node2d_basic.clone());
    m.insert("Path2D", node2d_basic.clone());
    m.insert("PathFollow2D", node2d_basic.clone());
    m.insert("Line2D", node2d_basic.clone());
    m.insert("Polygon2D", node2d_basic.clone());
    m.insert("Light2D", node2d_basic.clone());
    m.insert("PointLight2D", node2d_basic.clone());
    m.insert("DirectionalLight2D", node2d_basic.clone());
    m.insert("AudioStreamPlayer2D", node2d_basic.clone());
    m.insert("NavigationAgent2D", node2d_basic.clone());
    m.insert("TileMap", node2d_basic.clone());
    m.insert("Marker2D", node2d_basic.clone());
    m.insert("RemoteTransform2D", node2d_basic.clone());
    m.insert("VisibleOnScreenNotifier2D", node2d_basic.clone());
    m.insert("GPUParticles2D", node2d_basic.clone());
    m.insert("CPUParticles2D", node2d_basic.clone());
    m.insert("Parallax2D", node2d_basic);
    m.insert("Control", control);
    m.insert("Label", label);
    m.insert("Button", button);

    m
});

/// Returns `true` if the property is a known Godot class property for the given class
/// AND its value differs from the Godot default.
///
/// Properties starting with `metadata/` are always included (they're always non-default
/// by definition since Godot only stores them when explicitly set).
pub fn should_output_property(class_name: &str, prop_name: &str, value: &Variant) -> bool {
    // Internal properties (prefixed with _) are never output.
    if prop_name.starts_with('_') {
        return false;
    }

    // Script reference is never output.
    if prop_name == "script" {
        return false;
    }

    // metadata/ properties are always output (they're user-defined, always non-default).
    if prop_name.starts_with("metadata/") {
        return true;
    }

    // Look up the class's known defaults.
    let defaults = match CLASS_DEFAULTS.get(class_name) {
        Some(d) => d,
        None => return false, // Unknown class → don't output (Node, Window, etc.)
    };

    // Find the property in the class defaults.
    for (name, default_val) in defaults {
        if *name == prop_name {
            // Only output if value differs from default.
            return !variant_eq(value, default_val);
        }
    }

    // Property not in known defaults for this class → don't output.
    false
}

/// Compares two Variant values for equality (with float tolerance for matching oracle behavior).
fn variant_eq(a: &Variant, b: &Variant) -> bool {
    const FLOAT_TOL: f64 = 0.001;
    match (a, b) {
        (Variant::Nil, Variant::Nil) => true,
        (Variant::Bool(a), Variant::Bool(b)) => a == b,
        (Variant::Int(a), Variant::Int(b)) => a == b,
        (Variant::Float(a), Variant::Float(b)) => (*a - *b).abs() < FLOAT_TOL,
        (Variant::String(a), Variant::String(b)) => a == b,
        (Variant::Vector2(a), Variant::Vector2(b)) => {
            (f64::from(a.x) - f64::from(b.x)).abs() < FLOAT_TOL
                && (f64::from(a.y) - f64::from(b.y)).abs() < FLOAT_TOL
        }
        (Variant::Vector3(a), Variant::Vector3(b)) => {
            (f64::from(a.x) - f64::from(b.x)).abs() < FLOAT_TOL
                && (f64::from(a.y) - f64::from(b.y)).abs() < FLOAT_TOL
                && (f64::from(a.z) - f64::from(b.z)).abs() < FLOAT_TOL
        }
        (Variant::Color(a), Variant::Color(b)) => {
            (f64::from(a.r) - f64::from(b.r)).abs() < FLOAT_TOL
                && (f64::from(a.g) - f64::from(b.g)).abs() < FLOAT_TOL
                && (f64::from(a.b) - f64::from(b.b)).abs() < FLOAT_TOL
                && (f64::from(a.a) - f64::from(b.a)).abs() < FLOAT_TOL
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2};

    // -----------------------------------------------------------------------
    // should_output_property: basic filtering
    // -----------------------------------------------------------------------

    #[test]
    fn internal_properties_never_output() {
        assert!(!should_output_property(
            "Node2D",
            "_script_path",
            &Variant::String("res://test.gd".into())
        ));
        assert!(!should_output_property(
            "Node2D",
            "_instance",
            &Variant::String("foo".into())
        ));
    }

    #[test]
    fn script_property_never_output() {
        assert!(!should_output_property(
            "Node2D",
            "script",
            &Variant::String("ExtResource(\"1\")".into())
        ));
    }

    #[test]
    fn metadata_always_output() {
        assert!(should_output_property(
            "Node2D",
            "metadata/custom_tag",
            &Variant::String("hero".into())
        ));
        assert!(should_output_property(
            "Node",
            "metadata/anything",
            &Variant::Int(42)
        ));
    }

    #[test]
    fn unknown_class_properties_not_output() {
        // Node class has no known properties
        assert!(!should_output_property(
            "Node",
            "name_tag",
            &Variant::String("test".into())
        ));
    }

    // -----------------------------------------------------------------------
    // should_output_property: Node2D defaults filtering
    // -----------------------------------------------------------------------

    #[test]
    fn node2d_default_position_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "position",
            &Variant::Vector2(Vector2::ZERO)
        ));
    }

    #[test]
    fn node2d_nondefault_position_output() {
        assert!(should_output_property(
            "Node2D",
            "position",
            &Variant::Vector2(Vector2::new(100.0, 200.0))
        ));
    }

    #[test]
    fn node2d_default_rotation_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "rotation",
            &Variant::Float(0.0)
        ));
    }

    #[test]
    fn node2d_nondefault_rotation_output() {
        assert!(should_output_property(
            "Node2D",
            "rotation",
            &Variant::Float(1.5)
        ));
    }

    #[test]
    fn node2d_default_scale_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "scale",
            &Variant::Vector2(Vector2::ONE)
        ));
    }

    #[test]
    fn node2d_nondefault_scale_output() {
        assert!(should_output_property(
            "Node2D",
            "scale",
            &Variant::Vector2(Vector2::new(2.0, 2.0))
        ));
    }

    #[test]
    fn node2d_default_visible_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "visible",
            &Variant::Bool(true)
        ));
    }

    #[test]
    fn node2d_invisible_output() {
        assert!(should_output_property(
            "Node2D",
            "visible",
            &Variant::Bool(false)
        ));
    }

    #[test]
    fn node2d_custom_property_not_output() {
        // speed is not a standard Node2D property
        assert!(!should_output_property(
            "Node2D",
            "speed",
            &Variant::Int(200)
        ));
    }

    #[test]
    fn node2d_custom_string_property_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "label",
            &Variant::String("Hero".into())
        ));
    }

    // -----------------------------------------------------------------------
    // should_output_property: inherited classes
    // -----------------------------------------------------------------------

    #[test]
    fn sprite2d_inherits_node2d_position() {
        assert!(should_output_property(
            "Sprite2D",
            "position",
            &Variant::Vector2(Vector2::new(50.0, 50.0))
        ));
    }

    #[test]
    fn sprite2d_offset_default_not_output() {
        assert!(!should_output_property(
            "Sprite2D",
            "offset",
            &Variant::Vector2(Vector2::ZERO)
        ));
    }

    #[test]
    fn sprite2d_offset_nondefault_output() {
        assert!(should_output_property(
            "Sprite2D",
            "offset",
            &Variant::Vector2(Vector2::new(0.0, -16.0))
        ));
    }

    #[test]
    fn characterbody2d_collision_mask_nondefault_output() {
        assert!(should_output_property(
            "CharacterBody2D",
            "collision_mask",
            &Variant::Int(3)
        ));
    }

    #[test]
    fn characterbody2d_collision_mask_default_not_output() {
        assert!(!should_output_property(
            "CharacterBody2D",
            "collision_mask",
            &Variant::Int(1)
        ));
    }

    #[test]
    fn rigidbody2d_mass_nondefault_output() {
        assert!(should_output_property(
            "RigidBody2D",
            "mass",
            &Variant::Float(5.0)
        ));
    }

    #[test]
    fn rigidbody2d_mass_default_not_output() {
        assert!(!should_output_property(
            "RigidBody2D",
            "mass",
            &Variant::Float(1.0)
        ));
    }

    #[test]
    fn staticbody2d_collision_layer_nondefault_output() {
        assert!(should_output_property(
            "StaticBody2D",
            "collision_layer",
            &Variant::Int(2)
        ));
    }

    #[test]
    fn area2d_monitoring_default_not_output() {
        assert!(!should_output_property(
            "Area2D",
            "monitoring",
            &Variant::Bool(true)
        ));
    }

    #[test]
    fn camera2d_zoom_nondefault_output() {
        assert!(should_output_property(
            "Camera2D",
            "zoom",
            &Variant::Vector2(Vector2::new(2.0, 2.0))
        ));
    }

    #[test]
    fn camera2d_zoom_default_not_output() {
        assert!(!should_output_property(
            "Camera2D",
            "zoom",
            &Variant::Vector2(Vector2::ONE)
        ));
    }

    // -----------------------------------------------------------------------
    // should_output_property: CanvasItem color properties
    // -----------------------------------------------------------------------

    #[test]
    fn node2d_modulate_default_not_output() {
        assert!(!should_output_property(
            "Node2D",
            "modulate",
            &Variant::Color(Color::WHITE)
        ));
    }

    #[test]
    fn node2d_modulate_nondefault_output() {
        assert!(should_output_property(
            "Node2D",
            "modulate",
            &Variant::Color(Color::new(0.2, 0.4, 0.6, 1.0))
        ));
    }

    // -----------------------------------------------------------------------
    // should_output_property: Control class
    // -----------------------------------------------------------------------

    #[test]
    fn control_anchor_default_not_output() {
        assert!(!should_output_property(
            "Control",
            "anchor_left",
            &Variant::Float(0.0)
        ));
    }

    #[test]
    fn control_anchor_nondefault_output() {
        assert!(should_output_property(
            "Control",
            "anchor_right",
            &Variant::Float(1.0)
        ));
    }

    // -----------------------------------------------------------------------
    // should_output_property: CollisionShape2D
    // -----------------------------------------------------------------------

    #[test]
    fn collisionshape2d_disabled_default_not_output() {
        assert!(!should_output_property(
            "CollisionShape2D",
            "disabled",
            &Variant::Bool(false)
        ));
    }

    #[test]
    fn collisionshape2d_disabled_nondefault_output() {
        assert!(should_output_property(
            "CollisionShape2D",
            "disabled",
            &Variant::Bool(true)
        ));
    }

    // -----------------------------------------------------------------------
    // variant_eq
    // -----------------------------------------------------------------------

    #[test]
    fn variant_eq_floats_within_tolerance() {
        assert!(variant_eq(&Variant::Float(0.0), &Variant::Float(0.0005)));
    }

    #[test]
    fn variant_eq_floats_outside_tolerance() {
        assert!(!variant_eq(&Variant::Float(0.0), &Variant::Float(0.5)));
    }

    #[test]
    fn variant_eq_vectors_within_tolerance() {
        assert!(variant_eq(
            &Variant::Vector2(Vector2::new(100.0, 200.0)),
            &Variant::Vector2(Vector2::new(100.0005, 199.9995))
        ));
    }

    #[test]
    fn variant_eq_colors() {
        assert!(variant_eq(
            &Variant::Color(Color::WHITE),
            &Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0))
        ));
        assert!(!variant_eq(
            &Variant::Color(Color::WHITE),
            &Variant::Color(Color::new(0.5, 1.0, 1.0, 1.0))
        ));
    }

    #[test]
    fn variant_eq_different_types() {
        assert!(!variant_eq(&Variant::Int(0), &Variant::Float(0.0)));
    }
}
