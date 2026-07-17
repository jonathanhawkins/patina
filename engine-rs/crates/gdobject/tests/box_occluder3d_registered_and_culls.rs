//! pat-nzgdu.207 — the Occluder3D family's ClassDB surface.
//!
//! Covers the acceptance's first clause (`BoxOccluder3D ← Occluder3D ← Resource`).
//!
//! **The second clause — "a MeshInstance3D fully behind a BoxOccluder3D is culled
//! from the draw list" — is NOT done, and the reason is worth reading:** the cull
//! math already exists and is tested (`gdserver3d/src/occluder.rs`,
//! `OccluderInstance3D::occludes()`, 21 tests), but **nothing in the engine calls
//! it**. Wiring it into `gdrender3d`'s draw loop needs a per-instance world AABB
//! that `Mesh3D` does not currently expose — a design decision, not a wire-up.
//! See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; defaults
//! against its `--doctool` XML.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

/// The four shape resources the bead names, plus ArrayOccluder3D.
const SHAPES: &[&str] = &[
    "BoxOccluder3D",
    "SphereOccluder3D",
    "QuadOccluder3D",
    "PolygonOccluder3D",
    "ArrayOccluder3D",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// The acceptance's clause: `BoxOccluder3D ← Occluder3D ← Resource`.
#[test]
fn box_occluder3d_registered_and_culls() {
    register_3d_classes();

    assert!(class_exists("BoxOccluder3D"));
    assert!(
        is_parent_class("BoxOccluder3D", "Occluder3D"),
        "the acceptance's chain: BoxOccluder3D <- Occluder3D"
    );
    assert!(
        is_parent_class("BoxOccluder3D", "Resource"),
        "...and Occluder3D <- Resource"
    );
    assert!(is_parent_class("BoxOccluder3D", "Object"));

    // Every shape shares the chain — an unregistered intermediate severs it
    // silently (pat-nzgdu.98).
    for shape in SHAPES {
        assert!(class_exists(shape), "ClassDB must know {shape}");
        assert!(
            is_parent_class(shape, "Occluder3D"),
            "{shape} <- Occluder3D"
        );
        assert!(is_parent_class(shape, "Resource"));
        assert!(!is_abstract(shape), "{shape} is instantiable in Godot");
        assert!(can_instantiate(shape));
        let obj = instantiate(shape).unwrap_or_else(|| panic!("{shape} instantiates"));
        assert_eq!(obj.get_class(), *shape);
    }
}

/// ⚠️ The shape is a RESOURCE; the NODE is `OccluderInstance3D`.
///
/// An occluder shape is a saved asset shared between instances, not something in
/// the tree — the same shape/instance split as Mesh vs MeshInstance3D.
/// Conflating them is the trap this family invites.
#[test]
fn the_shape_is_a_resource_and_the_node_is_a_visual_instance() {
    register_3d_classes();

    // The base is abstract: you use a concrete shape, never a bare Occluder3D.
    assert!(is_abstract("Occluder3D"), "Occluder3D is abstract in Godot");
    assert!(!can_instantiate("Occluder3D"));
    assert!(is_parent_class("Occluder3D", "Resource"));
    assert!(
        !is_parent_class("Occluder3D", "Node"),
        "an occluder SHAPE is not a node"
    );

    // The node holds one and lives in the tree.
    assert!(class_exists("OccluderInstance3D"));
    assert!(
        is_parent_class("OccluderInstance3D", "VisualInstance3D"),
        "the node is a VisualInstance3D — it participates in rendering"
    );
    assert!(is_parent_class("OccluderInstance3D", "Node3D"));
    assert!(
        !is_parent_class("OccluderInstance3D", "Resource"),
        "the node is not a resource"
    );
    assert!(!is_abstract("OccluderInstance3D"));

    // ...and it references the shape rather than being one.
    assert!(props("OccluderInstance3D").iter().any(|p| p == "occluder"));
    assert_eq!(
        default_of("OccluderInstance3D", "occluder"),
        Variant::Nil,
        "a fresh instance holds no shape until assigned"
    );
}

/// Each shape's own property. ⚠️ Box is a **Vector3** size but Quad is a
/// **Vector2** — a quad is flat. Copying Box's schema onto Quad is the obvious
/// mistake.
#[test]
fn each_shape_carries_godots_own_property_and_type() {
    register_3d_classes();

    match default_of("BoxOccluder3D", "size") {
        Variant::Vector3(v) => assert!(
            (v.x - 1.0).abs() < 1e-6 && (v.y - 1.0).abs() < 1e-6 && (v.z - 1.0).abs() < 1e-6,
            "BoxOccluder3D.size defaults to Vector3(1,1,1), got {v:?}"
        ),
        other => panic!("BoxOccluder3D.size must be a Vector3, got {other:?}"),
    }
    match default_of("QuadOccluder3D", "size") {
        Variant::Vector2(v) => assert!(
            (v.x - 1.0).abs() < 1e-6 && (v.y - 1.0).abs() < 1e-6,
            "QuadOccluder3D.size defaults to Vector2(1,1), got {v:?}"
        ),
        other => panic!("QuadOccluder3D.size must be a VECTOR2 (it is flat), got {other:?}"),
    }
    assert_eq!(
        default_of("SphereOccluder3D", "radius"),
        Variant::Float(1.0)
    );
    assert_eq!(
        default_of("PolygonOccluder3D", "polygon"),
        Variant::PackedVector2Array(Vec::new()),
        "a polygon occluder's vertices are 2D (local XY plane)"
    );

    // ⚠️ ArrayOccluder3D is the only one with TWO properties, and the only one
    // with a plural `set_arrays(vertices, indices)` — an arbitrary triangle mesh
    // needs both halves set together.
    assert_eq!(
        default_of("ArrayOccluder3D", "vertices"),
        Variant::PackedVector3Array(Vec::new()),
        "array vertices are 3D, unlike the polygon's 2D"
    );
    assert_eq!(
        default_of("ArrayOccluder3D", "indices"),
        Variant::PackedInt32Array(Vec::new())
    );
    assert_eq!(arity("ArrayOccluder3D", "set_arrays"), 2);
    assert!(class_has_method("ArrayOccluder3D", "set_vertices"));
    assert!(class_has_method("ArrayOccluder3D", "set_indices"));
}

/// ⚠️ `bake_mask` defaults to **4294967295** — all 32 layers. A default of 0 or 1
/// would make baking silently miss geometry, which reads as "occlusion is broken"
/// rather than as a wrong constant.
#[test]
fn occluder_instance_defaults_match_godot() {
    register_3d_classes();

    assert_eq!(
        default_of("OccluderInstance3D", "bake_mask"),
        Variant::Int(4_294_967_295),
        "all 32 layers are baked by default"
    );
    assert_eq!(
        default_of("OccluderInstance3D", "bake_simplification_distance"),
        Variant::Float(0.1)
    );

    // The per-layer accessors: (layer, value) then (layer).
    assert_eq!(arity("OccluderInstance3D", "set_bake_mask_value"), 2);
    assert_eq!(arity("OccluderInstance3D", "get_bake_mask_value"), 1);
    assert_eq!(arity("OccluderInstance3D", "set_occluder"), 1);
    assert_eq!(arity("OccluderInstance3D", "get_occluder"), 0);
}

/// The base's baked triangle soup — every shape lowers to vertices+indices, which
/// is what the cull pass would consume.
#[test]
fn the_base_exposes_the_baked_triangle_soup() {
    register_3d_classes();
    assert_eq!(arity("Occluder3D", "get_vertices"), 0);
    assert_eq!(arity("Occluder3D", "get_indices"), 0);
    // Inherited by every shape — that is the point of the base.
    for shape in SHAPES {
        assert!(class_has_method(shape, "get_vertices"), "{shape}");
        assert!(class_has_method(shape, "get_indices"), "{shape}");
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["Occluder3D", "BoxOccluder3D", "OccluderInstance3D"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_3d_classes();
    let first = ids();
    register_3d_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
