//! pat-s3rrv.96 — the shape-query parameter resources' member tables.
//!
//! Godot ground truth: `doc/classes/PhysicsShapeQueryParameters2D.xml` and
//! `PhysicsShapeQueryParameters3D.xml` (9 members each), and
//! `PhysicsTestMotionResult3D.xml` (0 members, 14 methods).
//!
//! # 🔑 `shape` is the member the query exists to carry
//!
//! Both classes registered `shape_rid` but not `shape`. A script following the
//! ordinary Godot idiom — build a `RectangleShape2D`, assign it to
//! `query.shape`, call `intersect_shape` — had nowhere to put the shape, while
//! `class_get_property_list` reported a plausible seven-member table. `exclude`,
//! the RID exclusion list every real query uses to skip its own body, was
//! missing the same way.
//!
//! # ⚠️ The two `collide_with_*` bools have OPPOSITE defaults
//!
//! `collide_with_bodies` is **true** and `collide_with_areas` is **false**. They
//! are adjacent, identically shaped, and a single "default false for both" pass
//! would silently stop every shape query from finding bodies — the common case —
//! while still reporting both members present.

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes, ClassInfo};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

const SHAPE_QUERY: [&str; 2] = [
    "PhysicsShapeQueryParameters2D",
    "PhysicsShapeQueryParameters3D",
];

fn info(class: &str) -> ClassInfo {
    get_class_info(class).unwrap_or_else(|| panic!("{class} is registered"))
}

fn prop(class: &str, name: &str) -> gdobject::class_db::PropertyInfo {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class}.{name} is registered"))
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** Both classes resolve, under `RefCounted`.
#[test]
fn a_shape_query_parameters_are_registered_under_refcounted() {
    register_all_classes();

    for class in SHAPE_QUERY {
        assert!(class_exists(class), "{class} must resolve");
        // Direct parent — every one of these six is a RefCounted resource, not
        // a Node and not an Object, and a flattened parent still reaches Object.
        assert_eq!(info(class).parent_class, "RefCounted", "{class}");
        assert!(is_parent_class(class, "Object"));
    }
}

/// 🔑 `shape` and `exclude` — the two members that were missing.
#[test]
fn b_shape_and_exclude_are_carried() {
    register_all_classes();

    for class in SHAPE_QUERY {
        let shape = prop(class, "shape");
        assert_eq!(
            shape.default_value,
            Variant::Nil,
            "{class}.shape is an unset Resource"
        );
        assert_eq!(shape.getter, "get_shape");
        assert_eq!(shape.setter, "set_shape");

        let exclude = prop(class, "exclude");
        assert_eq!(
            exclude.default_value,
            Variant::Array(Vec::new()),
            "{class}.exclude is an empty RID array"
        );
        assert_eq!(exclude.getter, "get_exclude");

        // ⚠️ `shape` and `shape_rid` are DIFFERENT members — the resource and
        // the server handle. `shape_rid` was already present, which is why the
        // gap read as "already done" at a glance.
        assert_ne!(
            prop(class, "shape_rid").getter,
            shape.getter,
            "{class}: shape and shape_rid are distinct members"
        );
    }
}

/// The full nine-member table from the doc XML.
#[test]
fn c_all_nine_documented_members_are_present() {
    register_all_classes();

    for class in SHAPE_QUERY {
        let names: Vec<String> = info(class).properties.into_iter().map(|p| p.name).collect();
        for expected in [
            "collide_with_areas",
            "collide_with_bodies",
            "collision_mask",
            "exclude",
            "margin",
            "motion",
            "shape",
            "shape_rid",
            "transform",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "{class}.{expected} missing; got {names:?}"
            );
        }
        assert_eq!(names.len(), 9, "{class} declares exactly nine: {names:?}");
    }
}

/// 🔑 The opposite-default bools, and the `is_*_enabled` getters.
#[test]
fn d_collide_with_defaults_are_opposite() {
    register_all_classes();

    for class in SHAPE_QUERY {
        assert_eq!(
            prop(class, "collide_with_bodies").default_value,
            Variant::Bool(true),
            "{class}: bodies default TRUE"
        );
        assert_eq!(
            prop(class, "collide_with_areas").default_value,
            Variant::Bool(false),
            "{class}: …and areas default FALSE — the pair is asymmetric"
        );

        // ⚠️ Both getters are `is_*_enabled`, not `get_*`.
        assert_eq!(
            prop(class, "collide_with_bodies").getter,
            "is_collide_with_bodies_enabled"
        );
        assert_eq!(
            prop(class, "collide_with_areas").getter,
            "is_collide_with_areas_enabled"
        );

        // ⚠️ The mask defaults to ALL layers set, not 0. A 0 default would make
        // every query match nothing while looking like a sensible empty value.
        assert_eq!(
            prop(class, "collision_mask").default_value,
            Variant::Int(0xFFFF_FFFF),
            "{class}: collision_mask is 4294967295"
        );
    }
}

/// ⚠️ `PhysicsTestMotionResult3D` declares NO members — its `collision_index`
/// is a method ARGUMENT, not a property.
///
/// This is pinned because it reads exactly like a missing member: an audit that
/// greps `PropertyInfo::new("collision_index"` finds a hit inside
/// `.with_arguments(vec![...])` and concludes the class carries it. Registering
/// it as a member would advertise a property Godot does not have, on a class
/// whose entire surface is its fourteen accessor methods.
#[test]
fn e_test_motion_result_has_methods_not_members() {
    register_all_classes();

    for class in ["PhysicsTestMotionResult2D", "PhysicsTestMotionResult3D"] {
        assert!(class_exists(class));
        let own: Vec<String> = info(class).properties.into_iter().map(|p| p.name).collect();
        assert!(
            own.is_empty(),
            "{class} documents zero members; got {own:?}"
        );
        assert!(
            !own.contains(&"collision_index".to_string()),
            "collision_index is an argument to the getters, not a member"
        );

        // Positive control: the accessor surface really is there, so "no
        // members" is a statement about this class and not an empty lookup.
        let methods: Vec<String> = info(class).methods.into_iter().map(|m| m.name).collect();
        for m in ["get_travel", "get_remainder", "get_collision_normal"] {
            assert!(methods.contains(&m.to_string()), "{class}.{m}() missing");
        }
    }
}
