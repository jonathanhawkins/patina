//! pat-nzgdu.114 — the navigation *resources* are ClassDB-registered.
//!
//! Schemas are generated from Godot's `extension_api.json`, not recalled. The
//! tests below deliberately pin the two 2D/3D asymmetries that a symmetric
//! reading of the bead would get wrong.

use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list, is_parent_class,
    register_navigation_classes,
};
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

#[test]
fn navigation_resources_are_registered_as_resources() {
    register_navigation_classes();

    for class in [
        "NavigationPolygon",
        "NavigationMesh",
        "NavigationMeshSourceGeometryData2D",
        "NavigationMeshSourceGeometryData3D",
    ] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "Resource"),
            "{class} is a Resource in Godot — that is what makes it loadable from .tres/.tscn"
        );
        // `register_navigation_classes` bootstraps Object and RefCounted but not
        // Resource; if that bootstrap were missing the chain would sever here
        // and `class_exists` alone would not notice (pat-nzgdu.98).
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object through Resource"
        );
    }
}

/// The 3D mesh carries the full Recast bake surface.
#[test]
fn navigation_mesh_carries_the_full_bake_param_surface() {
    register_navigation_classes();
    assert_props(
        "NavigationMesh",
        &[
            "vertices",
            "polygons",
            "cell_size",
            "cell_height",
            "border_size",
            "agent_height",
            "agent_radius",
            "agent_max_climb",
            "agent_max_slope",
            "region_min_size",
            "region_merge_size",
            "edge_max_length",
            "edge_max_error",
            "vertices_per_polygon",
            "detail_sample_distance",
            "detail_sample_max_error",
            "filter_low_hanging_obstacles",
            "filter_ledge_spans",
            "filter_walkable_low_height_spans",
            "filter_baking_aabb",
            "filter_baking_aabb_offset",
        ],
    );
    // Easy to assume int; Godot types it float.
    assert_eq!(
        get_property_list("NavigationMesh")
            .into_iter()
            .find(|p| p.name == "vertices_per_polygon")
            .expect("vertices_per_polygon exists")
            .default_value,
        // doc/classes/NavigationMesh.xml:164 — `type="float" default="6.0"`.
        Variant::Float(6.0),
        "vertices_per_polygon is a float in Godot, not an int"
    );
}

/// ⚠️ The asymmetry: NavigationMesh's parse properties are `geometry_`-prefixed,
/// NavigationPolygon's are not. A symmetric reading of the bead gets this wrong.
#[test]
fn parse_properties_are_geometry_prefixed_on_3d_but_not_2d() {
    register_navigation_classes();

    assert_props(
        "NavigationMesh",
        &[
            "geometry_parsed_geometry_type",
            "geometry_collision_mask",
            "geometry_source_geometry_mode",
            "geometry_source_group_name",
        ],
    );
    assert_props(
        "NavigationPolygon",
        &[
            "parsed_geometry_type",
            "parsed_collision_mask",
            "source_geometry_mode",
            "source_geometry_group_name",
        ],
    );

    // ...and neither carries the other's spelling.
    let mesh = props("NavigationMesh");
    assert!(
        !mesh.iter().any(|p| p == "parsed_geometry_type"),
        "NavigationMesh spells it geometry_parsed_geometry_type"
    );
    let poly = props("NavigationPolygon");
    assert!(
        !poly.iter().any(|p| p == "geometry_parsed_geometry_type"),
        "NavigationPolygon spells it parsed_geometry_type"
    );
}

/// ⚠️ The second asymmetry: both expose `sample_partition_type`, but the enums
/// are entirely different value sets — 3D partitions a voxel field, 2D
/// partitions an outline. Conflating them would silently pick the wrong mode.
#[test]
fn sample_partition_enums_differ_between_2d_and_3d() {
    register_navigation_classes();

    assert!(class_has_enum("NavigationMesh", "SamplePartitionType"));
    for (name, value) in [
        ("SAMPLE_PARTITION_WATERSHED", 0),
        ("SAMPLE_PARTITION_MONOTONE", 1),
        ("SAMPLE_PARTITION_LAYERS", 2),
        ("SAMPLE_PARTITION_MAX", 3),
    ] {
        assert_eq!(
            class_get_integer_constant("NavigationMesh", name),
            Some(value)
        );
    }

    assert!(class_has_enum("NavigationPolygon", "SamplePartitionType"));
    for (name, value) in [
        ("SAMPLE_PARTITION_CONVEX_PARTITION", 0),
        ("SAMPLE_PARTITION_TRIANGULATE", 1),
        ("SAMPLE_PARTITION_MAX", 2),
    ] {
        assert_eq!(
            class_get_integer_constant("NavigationPolygon", name),
            Some(value)
        );
    }

    // The 3D-only modes must not leak into the 2D class...
    assert_eq!(
        class_get_integer_constant("NavigationPolygon", "SAMPLE_PARTITION_WATERSHED"),
        None,
        "WATERSHED is a voxel-field mode; a 2D outline has no such partition"
    );
    // ...and their shared MAX has a different value, which is the tell.
    assert_eq!(
        class_get_integer_constant("NavigationPolygon", "SAMPLE_PARTITION_MAX"),
        Some(2)
    );
    assert_eq!(
        class_get_integer_constant("NavigationMesh", "SAMPLE_PARTITION_MAX"),
        Some(3)
    );
}

#[test]
fn navigation_polygon_carries_its_2d_bake_surface() {
    register_navigation_classes();
    assert_props(
        "NavigationPolygon",
        &[
            "vertices",
            "polygons",
            "outlines",
            "cell_size",
            "border_size",
            "agent_radius",
            "baking_rect",
            "baking_rect_offset",
        ],
    );
}

/// The source-geometry containers a bake consumes: 2D holds outlines, 3D holds
/// an indexed triangle soup. They are not the same shape.
#[test]
fn source_geometry_data_classes_hold_their_own_shapes() {
    register_navigation_classes();
    assert_props(
        "NavigationMeshSourceGeometryData2D",
        &[
            "traversable_outlines",
            "obstruction_outlines",
            "projected_obstructions",
        ],
    );
    assert_props(
        "NavigationMeshSourceGeometryData3D",
        &["vertices", "indices", "projected_obstructions"],
    );
}

#[test]
fn shared_enums_match_godot() {
    register_navigation_classes();
    // ParsedGeometryType / SourceGeometryMode are the same on both classes.
    for class in ["NavigationMesh", "NavigationPolygon"] {
        assert!(class_has_enum(class, "ParsedGeometryType"));
        assert!(class_has_enum(class, "SourceGeometryMode"));
        assert_eq!(
            class_get_integer_constant(class, "PARSED_GEOMETRY_MESH_INSTANCES"),
            Some(0)
        );
        assert_eq!(
            class_get_integer_constant(class, "PARSED_GEOMETRY_STATIC_COLLIDERS"),
            Some(1)
        );
        assert_eq!(
            class_get_integer_constant(class, "SOURCE_GEOMETRY_ROOT_NODE_CHILDREN"),
            Some(0)
        );
        assert_eq!(
            class_get_integer_constant(class, "SOURCE_GEOMETRY_GROUPS_EXPLICIT"),
            Some(2)
        );
    }
}

#[test]
fn registration_is_idempotent() {
    register_navigation_classes();
    let before = props("NavigationMesh");
    register_navigation_classes();
    assert_eq!(before, props("NavigationMesh"));
}
