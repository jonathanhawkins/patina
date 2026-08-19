//! pat-s3rrv.96 — the six physics query/test-motion classes are in ClassDB as
//! RefCounted, so `class_exists` and `is_parent_class` answer for them.
//!
//! Godot's real `body_test_motion` threads `PhysicsTestMotionParameters2D` and
//! writes a `PhysicsTestMotionResult2D`. Patina's staged form returns a
//! Dictionary instead, and `interpreter.rs:7821` says why: "neither resource
//! exists in Patina yet". These registrations are the first half of closing
//! that — real Godot code naming these classes previously found nothing.

use gdobject::class_db::{class_exists, get_property_list, is_parent_class, register_all_classes};

/// The six the bead enumerates.
const QUERY_CLASSES: [&str; 6] = [
    "PhysicsShapeQueryParameters2D",
    "PhysicsShapeQueryParameters3D",
    "PhysicsTestMotionParameters2D",
    "PhysicsTestMotionParameters3D",
    "PhysicsTestMotionResult2D",
    "PhysicsTestMotionResult3D",
];

fn setup() {
    // ⚠️ ClassDB is empty in a fresh test process — without this every query
    // below answers "absent" for reasons unrelated to registration.
    register_all_classes();
}

fn default_of(class_name: &str, prop: &str) -> gdvariant::Variant {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == prop)
        .unwrap_or_else(|| panic!("{class_name} should declare `{prop}`"))
        .default_value
}

/// **The acceptance clause:** all six exist and descend from `RefCounted`.
#[test]
fn physics_query_classes_are_registered_as_refcounted() {
    setup();
    for cls in QUERY_CLASSES {
        assert!(class_exists(cls), "{cls} must be registered in ClassDB");
        assert!(
            is_parent_class(cls, "RefCounted"),
            "{cls} is a RefCounted resource in Godot — a script does \
             `{cls}.new()` and lets the refcount free it",
        );
    }
}

/// 🔑 The 2D and 3D `margin` defaults are NOT the same value.
///
/// ⚠️ This is the assertion that catches a 3D registration copied from its 2D
/// sibling. Godot's defaults are 0.08 (2D, pixels) and 0.001 (3D, metres) — an
/// eightyfold difference, because the two count different units. Every other
/// property on the two classes matches, so nothing else here would notice.
#[test]
fn test_motion_margin_defaults_differ_between_2d_and_3d() {
    setup();
    assert_eq!(
        default_of("PhysicsTestMotionParameters2D", "margin"),
        gdvariant::Variant::Float(0.08),
        "the 2D margin default is 0.08 pixels",
    );
    assert_eq!(
        default_of("PhysicsTestMotionParameters3D", "margin"),
        gdvariant::Variant::Float(0.001),
        "the 3D margin default is 0.001 metres — 0.08 here means the 3D entry \
         was copied from the 2D one and every 3D body gets an 80x margin",
    );
}

/// 🔑 Two setters do not follow from their property names.
///
/// ⚠️ `set_collide_separation_ray_enabled` and `set_recovery_as_collision_enabled`
/// both carry an `_enabled` suffix the property does not. A name derived by rule
/// would be wrong, and nothing else in the table would catch it.
#[test]
fn boolean_setters_carry_godots_enabled_suffix() {
    setup();
    for cls in [
        "PhysicsTestMotionParameters2D",
        "PhysicsTestMotionParameters3D",
    ] {
        let props = get_property_list(cls);
        let setter = |name: &str| {
            props
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{cls} should declare `{name}`"))
                .setter
                .clone()
        };
        assert_eq!(
            setter("collide_separation_ray"),
            "set_collide_separation_ray_enabled",
            "{cls}: Godot's setter carries an `_enabled` suffix the property \
             name does not",
        );
        assert_eq!(
            setter("recovery_as_collision"),
            "set_recovery_as_collision_enabled",
            "{cls}: same irregular `_enabled` suffix",
        );
    }
}

/// 🔑 `max_collisions` is 3D-only — the Result classes differ in SHAPE.
///
/// ⚠️ A 3D motion test can report up to `max_collisions` hits, so every 3D
/// getter takes a `collision_index` and 3D adds `get_collision_count`; 2D
/// reports exactly one and its getters are nullary. A 3D registration copied
/// from 2D would declare getters no real caller can use.
#[test]
fn three_d_test_motion_reports_multiple_collisions_and_two_d_does_not() {
    setup();

    let two: Vec<String> = get_property_list("PhysicsTestMotionParameters2D")
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        !two.contains(&"max_collisions".to_string()),
        "a 2D motion test reports one collision, so it has no max_collisions; \
         got {two:?}",
    );
    assert_eq!(
        default_of("PhysicsTestMotionParameters3D", "max_collisions"),
        gdvariant::Variant::Int(1),
        "3D declares max_collisions, defaulting to 1",
    );

    assert!(
        gdobject::class_db::class_has_method("PhysicsTestMotionResult3D", "get_collision_count"),
        "only the 3D result reports how many collisions it holds",
    );
    assert!(
        !gdobject::class_db::class_has_method("PhysicsTestMotionResult2D", "get_collision_count"),
        "…and the 2D result must NOT — it always holds exactly one",
    );
}

/// The accessor set a caller of `body_test_motion` actually reads.
#[test]
fn test_motion_result_exposes_godots_accessor_set() {
    setup();
    for cls in ["PhysicsTestMotionResult2D", "PhysicsTestMotionResult3D"] {
        for m in [
            "get_travel",
            "get_remainder",
            "get_collision_point",
            "get_collision_normal",
            "get_collision_depth",
            "get_collider",
            "get_collider_id",
            "get_collider_rid",
            "get_collider_shape",
            "get_collider_velocity",
            "get_collision_safe_fraction",
            "get_collision_unsafe_fraction",
        ] {
            assert!(
                gdobject::class_db::class_has_method(cls, m),
                "{cls} must declare `{m}`",
            );
        }
    }
}
