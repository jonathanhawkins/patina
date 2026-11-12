//! pat-s3rrv.89 — the `MultiMesh` / `MultiMeshInstance3D` ClassDB surface.
//!
//! Godot ground truth: `doc/classes/MultiMesh.xml` and
//! `doc/classes/MultiMeshInstance3D.xml`.
//!
//! # What was missing
//!
//! `MultiMeshInstance3D` was registered as a **bare name** — parent only, no
//! members — so the single property it documents was invisible to
//! `class_get_property_list`, to `.tscn` default seeding and to script
//! introspection. `MultiMesh` carried seven of its thirteen members and neither
//! of its two enums.
//!
//! # 🔑 `MultiMesh.mesh` is the one that matters
//!
//! It is the geometry every instance draws. Without it the class reported a
//! plausible-looking property list while the thing it exists to replicate was
//! absent — the failure is invisible to any test that just counts members.
//!
//! # ⚠️ Two defaults that are not what they look like
//!
//! * `visible_instance_count` is **-1**, not 0. -1 means "draw them all"; 0 is a
//!   real instruction to draw nothing, so the wrong default silently empties
//!   every MultiMesh.
//! * `transform_format` is **0 = TRANSFORM_2D**, even on a
//!   `MultiMeshInstance3D`. Reading it as 3D because the node is 3D gets the
//!   buffer stride wrong — 8 floats per instance against 12.

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes, ClassInfo};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn info(class: &str) -> ClassInfo {
    get_class_info(class).unwrap_or_else(|| panic!("{class} is registered"))
}

fn prop(class: &str, name: &str) -> gdobject::class_db::PropertyInfo {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class}.{name} is registered"))
}

fn enum_constants(class: &str, enum_name: &str) -> Vec<(String, i64)> {
    info(class)
        .enums
        .into_iter()
        .find(|e| e.name == enum_name)
        .unwrap_or_else(|| panic!("{class} declares enum {enum_name}"))
        .constants
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** `MultiMeshInstance3D.multimesh` exists.
#[test]
fn a_multimesh_instance3d_carries_its_multimesh() {
    register_all_classes();

    assert!(class_exists("MultiMeshInstance3D"));
    // Direct parent, not a transitive walk — `GeometryInstance3D` is where the
    // visibility/LOD block lives, and reaching `Node3D` some other way would
    // still satisfy `is_parent_class`.
    assert_eq!(
        info("MultiMeshInstance3D").parent_class,
        "GeometryInstance3D"
    );
    assert!(is_parent_class("MultiMeshInstance3D", "Node3D"));

    let mm = prop("MultiMeshInstance3D", "multimesh");
    assert_eq!(mm.default_value, Variant::Nil, "an unset resource is null");
    assert_eq!(mm.getter, "get_multimesh");
    assert_eq!(mm.setter, "set_multimesh");

    // ⚠️ It must be the node's OWN member, not something inherited that merely
    // happens to share the name.
    let own: Vec<String> = info("MultiMeshInstance3D")
        .properties
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(own, vec!["multimesh".to_string()]);
}

/// 🔑 `MultiMesh.mesh` — the geometry the instances replicate.
#[test]
fn b_multimesh_carries_the_mesh_it_replicates() {
    register_all_classes();

    assert!(class_exists("MultiMesh"));
    assert_eq!(info("MultiMesh").parent_class, "Resource");

    let mesh = prop("MultiMesh", "mesh");
    assert_eq!(mesh.default_value, Variant::Nil);
    assert_eq!(mesh.getter, "get_mesh");
}

/// ⚠️ The two defaults that are not what they look like.
#[test]
fn c_sentinel_defaults_are_preserved() {
    register_all_classes();

    assert_eq!(
        prop("MultiMesh", "visible_instance_count").default_value,
        Variant::Int(-1),
        "-1 means draw ALL; 0 would be a real instruction to draw nothing"
    );
    // Positive control: the neighbouring count really is 0, so the -1 above is
    // a deliberate sentinel rather than a blanket default.
    assert_eq!(
        prop("MultiMesh", "instance_count").default_value,
        Variant::Int(0)
    );
    assert_eq!(
        prop("MultiMesh", "transform_format").default_value,
        Variant::Int(0),
        "0 is TRANSFORM_2D even for a MultiMeshInstance3D"
    );
}

/// Every documented member, with the accessors Godot spells.
///
/// ⚠️ Four of them carry **underscore-prefixed** accessors
/// (`_get_transform_array`). The leading underscore is part of the documented
/// name — spelling them without it advertises getters that do not exist.
#[test]
fn d_members_and_accessors_match_the_doc_xml() {
    register_all_classes();

    for name in [
        "buffer",
        "color_array",
        "custom_aabb",
        "custom_data_array",
        "instance_count",
        "mesh",
        "physics_interpolation_quality",
        "transform_2d_array",
        "transform_array",
        "transform_format",
        "use_colors",
        "use_custom_data",
        "visible_instance_count",
    ] {
        assert!(
            get_property_list("MultiMesh")
                .iter()
                .any(|p| p.name == name),
            "MultiMesh.{name} must be registered"
        );
    }

    for (name, getter) in [
        ("transform_array", "_get_transform_array"),
        ("transform_2d_array", "_get_transform_2d_array"),
        ("color_array", "_get_color_array"),
        ("custom_data_array", "_get_custom_data_array"),
    ] {
        assert_eq!(prop("MultiMesh", name).getter, getter);
    }

    // ⚠️ The two bools use `is_using_*`, not `get_*`.
    assert_eq!(prop("MultiMesh", "use_colors").getter, "is_using_colors");
    assert_eq!(
        prop("MultiMesh", "use_custom_data").getter,
        "is_using_custom_data"
    );

    assert_eq!(
        prop("MultiMesh", "custom_aabb").default_value,
        Variant::Aabb(gdcore::math3d::Aabb::new(
            gdcore::math::Vector3::ZERO,
            gdcore::math::Vector3::ZERO,
        ))
    );
}

/// Both enums, at Godot's exact values.
#[test]
fn e_enums_match_godot() {
    register_all_classes();

    assert_eq!(
        enum_constants("MultiMesh", "TransformFormat"),
        vec![
            ("TRANSFORM_2D".to_string(), 0),
            ("TRANSFORM_3D".to_string(), 1),
        ]
    );
    assert_eq!(
        enum_constants("MultiMesh", "PhysicsInterpolationQuality"),
        vec![
            ("INTERP_QUALITY_FAST".to_string(), 0),
            ("INTERP_QUALITY_HIGH".to_string(), 1),
        ]
    );
}

/// No member is registered twice on either class.
///
/// ⚠️ A duplicate would not error — `get_property_list` merges by name, taking
/// the derived value, so a second registration silently replaces the default
/// with nothing visible in the list.
#[test]
fn f_no_member_is_registered_twice() {
    register_all_classes();

    for class in ["MultiMesh", "MultiMeshInstance3D"] {
        let names: Vec<String> = info(class).properties.into_iter().map(|p| p.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            before,
            sorted.len(),
            "{class} duplicates a member: {names:?}"
        );
    }
}
