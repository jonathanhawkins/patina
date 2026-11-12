//! pat-s3rrv.57 (merged finding 3): the primitive-mesh family — 27 members
//! across PrimitiveMesh and its four shape subclasses.
//!
//! # Why these were safe after all
//!
//! An earlier pass listed the mesh primitives as "known risky, whose
//! radius/height the 3D renderer reads". The renderer does read those names —
//! but off a **node** (`main_loop.rs:1411`), and all five of these are
//! Resources (PrimitiveMesh → Mesh → Resource). `create_node` seeds node
//! property bags only, so registering a Resource cannot change what the runtime
//! reads. Classifying by parent chain settles it without comparing a single
//! default.
//!
//! ⚠️ Nearly every default here is destructive at zero, which is why each is
//! asserted by value rather than trusted.

use gdobject::class_db::{get_class_info, register_all_classes};
use gdvariant::Variant;

fn own(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

#[test]
fn the_family_carries_its_member_counts() {
    assert_eq!(own_count("PrimitiveMesh"), 5);
    assert_eq!(own_count("SphereMesh"), 5);
    assert_eq!(own_count("CylinderMesh"), 7);
    assert_eq!(own_count("PlaneMesh"), 5);
    assert_eq!(own_count("PrismMesh"), 5);
}

/// ⚠️ The tessellation counts. A zero on any of these produces a degenerate
/// mesh, which reads as "the shape does not render" rather than as a wrong
/// default — the hardest kind of bad default to trace back.
#[test]
fn tessellation_defaults_are_not_zero() {
    assert_eq!(own("SphereMesh", "radial_segments"), Variant::Int(64));
    assert_eq!(own("SphereMesh", "rings"), Variant::Int(32));
    assert_eq!(own("CylinderMesh", "radial_segments"), Variant::Int(64));
    assert_eq!(own("CylinderMesh", "rings"), Variant::Int(4));
}

/// ⚠️ PlaneMesh's `orientation` is **1** (Z-facing), not 0, and its `size` is
/// (2, 2), not zero. A zero size renders nothing; a zero orientation faces the
/// wrong axis — both look like a broken renderer rather than a wrong constant.
#[test]
fn plane_mesh_size_and_orientation_are_godots() {
    assert_eq!(
        own("PlaneMesh", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(2.0, 2.0))
    );
    assert_eq!(own("PlaneMesh", "orientation"), Variant::Int(1));
}

#[test]
fn the_shape_dimensions_match_godot() {
    assert_eq!(own("SphereMesh", "radius"), Variant::Float(0.5));
    assert_eq!(own("SphereMesh", "height"), Variant::Float(1.0));
    // A cylinder is TALLER than a sphere by default: height 2.0 vs 1.0.
    assert_eq!(own("CylinderMesh", "height"), Variant::Float(2.0));
    assert_eq!(own("CylinderMesh", "cap_top"), Variant::Bool(true));
    assert_eq!(
        own("PrismMesh", "size"),
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 1.0, 1.0))
    );
    assert_eq!(own("PrismMesh", "left_to_right"), Variant::Float(0.5));
}

/// The shared base's members reach every subclass exactly once.
#[test]
fn subclasses_inherit_the_base_members_once() {
    register_all_classes();
    assert_eq!(own("PrimitiveMesh", "uv2_padding"), Variant::Float(2.0));

    for sub in ["SphereMesh", "CylinderMesh", "PlaneMesh", "PrismMesh"] {
        let names: Vec<String> = gdobject::class_db::get_property_list(sub)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for base in [
            "material",
            "flip_faces",
            "add_uv2",
            "uv2_padding",
            "custom_aabb",
        ] {
            assert_eq!(
                names.iter().filter(|n| *n == base).count(),
                1,
                "{sub} must inherit `{base}` from PrimitiveMesh exactly once"
            );
        }
    }
}
