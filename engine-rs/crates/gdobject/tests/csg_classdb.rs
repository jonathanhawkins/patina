//! pat-0q2kx.194 clause 3 — the missing CSG primitive schemas.
//!
//! `CSGTorus3D`, `CSGPolygon3D` and `CSGMesh3D` were absent from ClassDB
//! entirely, so a `.tscn` containing one lost every property on load and the
//! inspector had nothing to show.
//!
//! Defaults come from `modules/csg/doc_classes/*.xml` — CSG is a Godot
//! **module**, so its docs are not under `doc/classes/`, and a lookup there
//! silently finds nothing. Where the doc omits a default, the value is the
//! constructor's in `modules/csg/csg_shape.cpp`.
//!
//! ⚠️ **Schema only.** `csg_primitive_mesh` still returns `None` for all three,
//! so they contribute no geometry — and CSG evaluation itself still runs only in
//! the test-only adapter, not the editor or Play render path. These tests assert
//! the schema and nothing about rendering.

use gdobject::class_db::{get_class_info, register_3d_classes};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
        .clone()
}

#[test]
fn missing_csg_primitives_are_registered_under_csg_primitive_3d() {
    register_3d_classes();

    for class in ["CSGTorus3D", "CSGPolygon3D", "CSGMesh3D"] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "CSGPrimitive3D"),
            "{class} derives CSGPrimitive3D"
        );
        // The chain must reach Node3D, or the render walk would not treat it as
        // a spatial node even once geometry exists.
        assert!(
            is_parent_class(class, "Node3D"),
            "{class} must reach Node3D through CSGShape3D"
        );
    }
}

#[test]
fn csg_torus_carries_godots_radii_and_segment_counts() {
    register_3d_classes();

    assert_eq!(
        default_of("CSGTorus3D", "inner_radius"),
        Variant::Float(0.5)
    );
    assert_eq!(
        default_of("CSGTorus3D", "outer_radius"),
        Variant::Float(1.0)
    );
    assert_eq!(default_of("CSGTorus3D", "sides"), Variant::Int(8));
    assert_eq!(default_of("CSGTorus3D", "ring_sides"), Variant::Int(6));
    // A torus smooths by default; the polygon extruder does not.
    assert_eq!(
        default_of("CSGTorus3D", "smooth_faces"),
        Variant::Bool(true)
    );
    assert_eq!(
        default_of("CSGPolygon3D", "smooth_faces"),
        Variant::Bool(false),
        "CSGPolygon3D's smooth_faces default differs from the torus's"
    );
}

/// `CSGPolygon3D`'s path settings, several of which the doc XML does not carry —
/// those come from the constructor.
#[test]
fn csg_polygon_carries_its_path_defaults() {
    register_3d_classes();

    assert_eq!(default_of("CSGPolygon3D", "depth"), Variant::Float(1.0));
    assert_eq!(
        default_of("CSGPolygon3D", "spin_degrees"),
        Variant::Float(360.0)
    );
    assert_eq!(default_of("CSGPolygon3D", "spin_sides"), Variant::Int(8));
    assert_eq!(
        default_of("CSGPolygon3D", "path_interval"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("CSGPolygon3D", "path_continuous_u"),
        Variant::Bool(true),
        "continuous U is on by default"
    );

    // ⚠️ The one that is NOT enum-zero: PathRotation runs POLYGON=0, PATH=1,
    // PATH_FOLLOW=2, and Godot's constructor picks PATH_FOLLOW. Registering 0
    // would silently change how the polygon orients along its path.
    assert_eq!(
        default_of("CSGPolygon3D", "path_rotation"),
        Variant::Int(2),
        "path_rotation defaults to PATH_ROTATION_PATH_FOLLOW (2), not enum-zero"
    );
    // …while path_interval_type genuinely IS zero (PATH_INTERVAL_DISTANCE).
    assert_eq!(
        default_of("CSGPolygon3D", "path_interval_type"),
        Variant::Int(0)
    );

    // The default outline is Godot's unit square, not an empty polygon — an
    // empty one would extrude nothing.
    match default_of("CSGPolygon3D", "polygon") {
        Variant::PackedVector2Array(pts) => assert_eq!(
            pts.len(),
            4,
            "the default polygon is the 4-point unit square, got {pts:?}"
        ),
        v => panic!("polygon must be a PackedVector2Array, got {v:?}"),
    }
}

/// `CSGMesh3D` is a reference holder: a mesh and a material, both null until
/// assigned. Declaring anything else would invent a schema.
#[test]
fn csg_mesh_declares_only_its_two_references() {
    register_3d_classes();

    let info = get_class_info("CSGMesh3D").expect("registered");
    assert_eq!(
        info.properties.len(),
        2,
        "CSGMesh3D has exactly mesh + material, got {:?}",
        info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(default_of("CSGMesh3D", "mesh"), Variant::Nil);
    assert_eq!(default_of("CSGMesh3D", "material"), Variant::Nil);
}
