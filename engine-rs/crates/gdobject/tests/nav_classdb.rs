//! pat-0q2kx.187 — navigation ClassDB defaults must match Godot's doc XML.
//!
//! Every numeric default on `NavigationMesh` and `NavigationPolygon` was
//! registered as `0.0`, and both collision masks as `0`. That is not "not yet
//! filled in" — it is **actively wrong**, and wrong in the direction that fails
//! silently: a `.tscn` that omits a property (because it holds Godot's default)
//! round-trips into Patina as a zero. A `cell_size` of 0 or an `agent_radius` of
//! 0 is a degenerate bake, not a default one.
//!
//! Ground truth is `upstream/godot/doc/classes/NavigationMesh.xml` and
//! `NavigationPolygon.xml`.
//!
//! ⚠️ **Defaults only.** The bead also asks for the missing navigation classes
//! (obstacles, path-query parameters/results, `PolygonPathFinder`, the server
//! singletons) and full property schemas for regions and links. None of that is
//! done here; these tests assert nothing about it.

use gdobject::class_db::{get_class_info, register_navigation_classes};
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

/// **The acceptance clause.** `NavigationMesh`'s bake parameters carry Godot's
/// documented values, not zeros.
#[test]
fn navigation_classdb_schema_parity() {
    register_navigation_classes();

    for (name, want) in [
        ("cell_size", 0.25),
        ("cell_height", 0.25),
        ("agent_height", 1.5),
        ("agent_radius", 0.5),
        ("agent_max_climb", 0.25),
        ("agent_max_slope", 45.0),
        ("region_min_size", 2.0),
        ("region_merge_size", 20.0),
        ("edge_max_error", 1.3),
        ("vertices_per_polygon", 6.0),
        ("detail_sample_distance", 6.0),
        ("detail_sample_max_error", 1.0),
    ] {
        assert_eq!(
            default_of("NavigationMesh", name),
            Variant::Float(want),
            "NavigationMesh.{name} must be Godot's {want}, not a zero"
        );
    }

    // 4294967295 = u32::MAX, "collide with every layer". Registered as 0 it
    // meant the opposite — collide with nothing — so a default-configured bake
    // would silently find no geometry at all.
    assert_eq!(
        default_of("NavigationMesh", "geometry_collision_mask"),
        Variant::Int(4_294_967_295),
        "an all-layers mask is the default; 0 means 'no geometry'"
    );
}

/// `NavigationPolygon` has its **own** values — notably `cell_size` 1.0 where
/// the 3D mesh uses 0.25, and `agent_radius` 10.0 where the mesh uses 0.5. The
/// two are easy to conflate, and a shared "fix the zeros" pass that used one set
/// of numbers for both would pass a laxer test than this one.
#[test]
fn navigation_polygon_has_its_own_2d_defaults() {
    register_navigation_classes();

    assert_eq!(
        default_of("NavigationPolygon", "cell_size"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("NavigationPolygon", "agent_radius"),
        Variant::Float(10.0)
    );
    assert_eq!(
        default_of("NavigationPolygon", "parsed_collision_mask"),
        Variant::Int(4_294_967_295)
    );

    // ⚠️ Enum-zero is NOT the default here: ParsedGeometryType defaults to 2
    // (BOTH), so registering 0 would silently change which geometry is parsed.
    assert_eq!(
        default_of("NavigationPolygon", "parsed_geometry_type"),
        Variant::Int(2),
        "ParsedGeometryType defaults to 2, not enum-zero"
    );

    // And the two classes must not have been collapsed onto one table.
    assert_ne!(
        default_of("NavigationPolygon", "cell_size"),
        default_of("NavigationMesh", "cell_size"),
        "2D and 3D navigation have different cell sizes"
    );
}

/// Some navigation defaults **are** legitimately zero. Asserting them guards
/// against a blanket "replace every 0.0" sweep, which would be just as wrong as
/// the original zeros — in the opposite direction.
#[test]
fn genuinely_zero_defaults_are_left_alone() {
    register_navigation_classes();

    for class in ["NavigationMesh", "NavigationPolygon"] {
        assert_eq!(
            default_of(class, "border_size"),
            Variant::Float(0.0),
            "{class}.border_size is 0.0 in Godot and must stay 0.0"
        );
    }
    assert_eq!(
        default_of("NavigationMesh", "edge_max_length"),
        Variant::Float(0.0),
        "edge_max_length is 0.0 in Godot (meaning 'no limit')"
    );
    // Both partition/source-geometry enums genuinely start at zero.
    assert_eq!(
        default_of("NavigationMesh", "sample_partition_type"),
        Variant::Int(0)
    );
    assert_eq!(
        default_of("NavigationPolygon", "source_geometry_mode"),
        Variant::Int(0)
    );
}
