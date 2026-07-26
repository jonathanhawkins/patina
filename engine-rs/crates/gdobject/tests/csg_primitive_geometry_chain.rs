//! pat-0q2kx.161 — the CSG inheritance chain matches Godot's.
//!
//! Godot (`modules/csg/csg_shape.h`):
//!
//! ```text
//! VisualInstance3D <- GeometryInstance3D <- CSGShape3D <- CSGPrimitive3D <- CSGBox3D/Sphere3D/Cylinder3D
//!                                                      <- CSGCombiner3D
//! ```
//!
//! Patina parented `CSGShape3D` to `Node3D`, which cut every CSG node off from
//! the VisualInstance3D branch — so a CSG node, which is by definition rendered
//! geometry, did not answer `is_class("VisualInstance3D")` and inherited none of
//! GeometryInstance3D's properties. `CSGPrimitive3D` was missing outright, so
//! the solids hung directly off `CSGShape3D` and `smooth_faces` (which Godot
//! defines on the primitive base) had nowhere to live.

use gdobject::class_db::{class_exists, get_class_info, is_abstract};
use gdobject::{inheritance_chain, is_parent_class, register_3d_classes};

/// **The acceptance test.**
#[test]
fn csg_primitive_geometry_chain() {
    // ClassDB is not populated implicitly — the 3D registrar owns the CSG family.
    register_3d_classes();

    // -- the missing base now exists, and is abstract ------------------------
    assert!(
        class_exists("CSGPrimitive3D"),
        "CSGPrimitive3D must be registered",
    );
    assert!(
        is_abstract("CSGPrimitive3D"),
        "CSGPrimitive3D is an abstract base in Godot — it must not be instantiable",
    );
    assert!(
        is_abstract("CSGShape3D"),
        "CSGShape3D stays abstract after the reparent",
    );

    // -- CSGShape3D reaches the render branch --------------------------------
    assert!(
        is_parent_class("CSGShape3D", "GeometryInstance3D"),
        "CSGShape3D must inherit GeometryInstance3D",
    );
    assert!(
        is_parent_class("CSGShape3D", "VisualInstance3D"),
        "…and therefore VisualInstance3D — this is what the Node3D parenting broke",
    );

    // -- the solids sit under the primitive base, and still under CSGShape3D --
    for solid in ["CSGBox3D", "CSGSphere3D", "CSGCylinder3D"] {
        assert!(
            is_parent_class(solid, "CSGPrimitive3D"),
            "{solid} must inherit CSGPrimitive3D",
        );
        assert!(
            is_parent_class(solid, "CSGShape3D"),
            "{solid} must still reach CSGShape3D transitively",
        );
        assert!(
            is_parent_class(solid, "VisualInstance3D"),
            "{solid} must reach the render branch through the full chain",
        );
    }

    // -- the combiner is NOT a primitive -------------------------------------
    //
    // This is the assertion that stops an over-eager fix: Godot parents
    // CSGCombiner3D straight to CSGShape3D because it is a boolean grouping
    // node, not a solid. Sweeping every CSG* class under CSGPrimitive3D would
    // satisfy every other check here and still be wrong.
    assert!(
        is_parent_class("CSGCombiner3D", "CSGShape3D"),
        "CSGCombiner3D must still inherit CSGShape3D",
    );
    assert!(
        !is_parent_class("CSGCombiner3D", "CSGPrimitive3D"),
        "CSGCombiner3D is a grouping node, NOT a primitive",
    );

    // -- smooth_faces lives on the primitive base ----------------------------
    let primitive = get_class_info("CSGPrimitive3D").expect("CSGPrimitive3D resolves");
    assert!(
        primitive
            .properties
            .iter()
            .any(|p| p.name == "smooth_faces"),
        "CSGPrimitive3D must expose `smooth_faces`; got {:?}",
        primitive
            .properties
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>(),
    );
}

/// The full ordered chain, so a future reparent that keeps every pairwise
/// `is_parent_class` true but scrambles the order still fails.
#[test]
fn csg_box_chain_is_ordered_correctly() {
    register_3d_classes();

    let chain = inheritance_chain("CSGBox3D");
    let pos = |name: &str| {
        chain
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("{name} missing from CSGBox3D chain: {chain:?}"))
    };

    // Nearest ancestor first: Primitive, then Shape, then the render bases.
    assert!(
        pos("CSGPrimitive3D") < pos("CSGShape3D"),
        "CSGPrimitive3D is nearer than CSGShape3D: {chain:?}",
    );
    assert!(
        pos("CSGShape3D") < pos("GeometryInstance3D"),
        "CSGShape3D is nearer than GeometryInstance3D: {chain:?}",
    );
    assert!(
        pos("GeometryInstance3D") < pos("VisualInstance3D"),
        "GeometryInstance3D is nearer than VisualInstance3D: {chain:?}",
    );
    assert!(
        pos("VisualInstance3D") < pos("Node3D"),
        "the render bases sit between the CSG classes and Node3D: {chain:?}",
    );
}
