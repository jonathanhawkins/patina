//! pat-s3rrv.197 (clause 1) — the five mesh-authoring classes resolve in
//! ClassDB with their doc-XML surfaces.
//!
//! Ground truth: `upstream/godot/doc/classes/{ImmediateMesh, SurfaceTool,
//! MeshDataTool, MeshTexture, MeshConvexDecompositionSettings}.xml`. Every
//! parent and default below is transcribed from those files, not derived.
//!
//! # What was wrong
//!
//! All five returned ZERO hits in `class_db.rs`. Four of them had no
//! implementation of any kind and appeared only in the editor's *generated*
//! property-schema table — which `inspector_schema.rs` consults as a fallback,
//! and which is NOT ClassDB. The fifth is worse: `gdserver3d::mesh::SurfaceTool`
//! is a complete procedural mesh builder, re-exported from its crate root, with
//! no ClassDB entry and no script path — so there was no way to build a mesh at
//! runtime at all.

use gdobject::class_db::{class_exists, get_property_list, register_all_classes};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// 🔑 Clause 1: all five register, under the parents Godot gives them.
///
/// ⚠️ The parents are NOT uniform, which is the part a guess gets wrong.
/// `ImmediateMesh` inherits **Mesh** (so it is a Resource and can be assigned to
/// `MeshInstance3D.mesh`), `MeshTexture` inherits **Texture2D** (it draws), and
/// only the three tools are bare `RefCounted`. Registering ImmediateMesh as
/// RefCounted would make it unassignable to a MeshInstance3D while still
/// existing and answering methods.
#[test]
fn the_mesh_authoring_classes_are_registered() {
    register_all_classes();

    for (class, parent) in [
        ("ImmediateMesh", "Mesh"),
        ("SurfaceTool", "RefCounted"),
        ("MeshDataTool", "RefCounted"),
        ("MeshTexture", "Texture2D"),
        ("MeshConvexDecompositionSettings", "RefCounted"),
    ] {
        assert!(class_exists(class), "{class} must be registered in ClassDB");
        assert!(
            is_parent_class(class, parent),
            "{class} inherits {parent} (doc XML `inherits=`)"
        );
    }
}

/// `ImmediateMesh` reaches `Resource` through `Mesh`, which is what lets it be
/// assigned to a `MeshInstance3D`.
///
/// ⚠️ Asserted through the CHAIN rather than just the immediate parent: a
/// registration whose parent string is right but whose grandparent is missing
/// from ClassDB silently truncates the chain, and `is_parent_class` then answers
/// false for everything above the gap.
#[test]
fn immediate_mesh_is_a_resource_through_mesh() {
    register_all_classes();

    assert!(is_parent_class("ImmediateMesh", "Mesh"));
    assert!(
        is_parent_class("ImmediateMesh", "Resource"),
        "ImmediateMesh must reach Resource through Mesh, or it cannot be \
         assigned to MeshInstance3D.mesh"
    );
    assert!(is_parent_class("ImmediateMesh", "Object"));
}

/// `MeshTexture`'s documented members.
#[test]
fn mesh_texture_exposes_its_documented_members() {
    register_all_classes();

    for want in ["base_texture", "image_size", "mesh"] {
        assert!(
            get_property_list("MeshTexture")
                .iter()
                .any(|p| p.name == want),
            "MeshTexture must expose `{want}`"
        );
    }
    assert!(
        matches!(
            default_of("MeshTexture", "image_size"),
            Some(Variant::Vector2(_))
        ),
        "image_size is a Vector2, defaulting to (0, 0)"
    );
}

/// `MeshConvexDecompositionSettings`' defaults, which are the interesting part.
///
/// ⚠️ Several of these are non-obvious and fail SILENTLY if guessed: a
/// `max_convex_hulls` of 0 decomposes into nothing, a `resolution` of 0 samples
/// nothing, and `min_volume_per_convex_hull` is **0.0001** — a small number that
/// rounds to zero if anyone assumes it is a plain 0.0.
#[test]
fn convex_decomposition_settings_carry_godots_defaults() {
    register_all_classes();

    assert_eq!(
        default_of("MeshConvexDecompositionSettings", "max_convex_hulls"),
        Some(Variant::Int(1))
    );
    assert_eq!(
        default_of("MeshConvexDecompositionSettings", "resolution"),
        Some(Variant::Int(10000))
    );
    assert_eq!(
        default_of(
            "MeshConvexDecompositionSettings",
            "max_num_vertices_per_convex_hull"
        ),
        Some(Variant::Int(32))
    );
    assert_eq!(
        default_of(
            "MeshConvexDecompositionSettings",
            "convex_hull_downsampling"
        ),
        Some(Variant::Int(4))
    );
    assert_eq!(
        default_of("MeshConvexDecompositionSettings", "max_concavity"),
        Some(Variant::Float(1.0))
    );
    match default_of(
        "MeshConvexDecompositionSettings",
        "min_volume_per_convex_hull",
    ) {
        Some(Variant::Float(v)) => assert!(
            (v - 0.0001).abs() < 1e-9,
            "min_volume_per_convex_hull is 0.0001, got {v}"
        ),
        other => panic!("min_volume_per_convex_hull must be a Float, got {other:?}"),
    }
    // ⚠️ `convex_hull_approximation` and `project_hull_vertices` default TRUE
    // while `normalize_mesh` defaults false — the three bools do not agree, so
    // one shared assumption is wrong for two of them.
    assert_eq!(
        default_of(
            "MeshConvexDecompositionSettings",
            "convex_hull_approximation"
        ),
        Some(Variant::Bool(true))
    );
    assert_eq!(
        default_of("MeshConvexDecompositionSettings", "project_hull_vertices"),
        Some(Variant::Bool(true))
    );
    assert_eq!(
        default_of("MeshConvexDecompositionSettings", "normalize_mesh"),
        Some(Variant::Bool(false))
    );
}
