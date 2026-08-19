//! pat-s3rrv.89: the CSG family carries its documented property set in ClassDB.
//!
//! ⚠️ THIS IS NOT COSMETIC. ClassDB is what seeds a `.tscn` node's defaults
//! (`packed_scene.rs` reads `get_property_list` when instancing) and what
//! GDScript introspection answers from. A property that is not registered is
//! absent from BOTH — so `CSGBox3D.size` authored in a scene had no default to
//! fall back on, and a script asking for the property list got nothing.
//!
//! Every default here is transcribed from the vendored
//! `upstream/godot/modules/csg/doc_classes/*.xml`, which is the oracle for this
//! repo — not from memory of Godot's API.
//!
//! ⚠️ `snap` is deliberately absent. The XML marks it deprecated ("The CSG
//! library no longer uses snapping", "This property does nothing") and gives it
//! no default, so registering it would mean inventing a value for a no-op.

use gdobject::class_db::{class_exists, get_property_list, is_parent_class, register_all_classes};
use gdvariant::Variant;

fn setup() {
    register_all_classes();
}

fn default_of(class_name: &str, prop: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == prop)
        .map(|p| p.default_value)
}

/// The shared base: operation, collision and tangent controls.
#[test]
fn csg_shape3d_carries_the_shared_property_set() {
    setup();

    assert!(class_exists("CSGShape3D"));
    // 0 = UNION, Godot's default operation.
    assert_eq!(default_of("CSGShape3D", "operation"), Some(Variant::Int(0)));
    assert_eq!(
        default_of("CSGShape3D", "use_collision"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("CSGShape3D", "collision_layer"),
        Some(Variant::Int(1)),
        "Godot's default is layer 1, NOT 0 — a shape on no layer collides with \
         nothing, which is a different scene"
    );
    assert_eq!(
        default_of("CSGShape3D", "collision_mask"),
        Some(Variant::Int(1))
    );
    assert_eq!(
        default_of("CSGShape3D", "collision_priority"),
        Some(Variant::Float(1.0))
    );
    assert_eq!(
        default_of("CSGShape3D", "calculate_tangents"),
        Some(Variant::Bool(true))
    );
}

/// The leaves carry their own geometry, and INHERIT the base's set.
#[test]
fn the_csg_primitives_carry_their_geometry_properties() {
    setup();

    assert_eq!(
        default_of("CSGBox3D", "size"),
        Some(Variant::Vector3(gdcore::math::Vector3::ONE)),
        "CSGBox3D.size defaults to Vector3(1, 1, 1)"
    );

    assert_eq!(
        default_of("CSGSphere3D", "radius"),
        Some(Variant::Float(0.5))
    );
    assert_eq!(
        default_of("CSGSphere3D", "radial_segments"),
        Some(Variant::Int(12))
    );
    assert_eq!(default_of("CSGSphere3D", "rings"), Some(Variant::Int(6)));

    assert_eq!(
        default_of("CSGCylinder3D", "radius"),
        Some(Variant::Float(0.5))
    );
    assert_eq!(
        default_of("CSGCylinder3D", "height"),
        Some(Variant::Float(2.0))
    );
    assert_eq!(default_of("CSGCylinder3D", "sides"), Some(Variant::Int(8)));
    assert_eq!(
        default_of("CSGCylinder3D", "cone"),
        Some(Variant::Bool(false))
    );
}

/// 🔑 INHERITANCE, asserted separately.
///
/// The leaves must see the base's properties through the parent chain. Without
/// this, registering the geometry on each leaf would look complete while
/// `operation` — the property that makes CSG *CSG* — was invisible on the very
/// nodes a scene actually instantiates.
#[test]
fn a_csg_primitive_inherits_the_shape_property_set() {
    setup();

    for cls in ["CSGBox3D", "CSGSphere3D", "CSGCylinder3D"] {
        assert!(
            is_parent_class(cls, "CSGShape3D"),
            "{cls} must descend from CSGShape3D"
        );
        assert_eq!(
            default_of(cls, "operation"),
            Some(Variant::Int(0)),
            "{cls} must inherit `operation` from CSGShape3D — it is what selects \
             union/intersection/subtraction, and a scene sets it on the leaf"
        );
        assert_eq!(
            default_of(cls, "use_collision"),
            Some(Variant::Bool(false)),
            "{cls} must inherit `use_collision`"
        );
    }
}

/// CONTROL: the deprecated `snap` is NOT registered.
///
/// Pins the deliberate omission so a future "completeness" pass does not add it
/// back with an invented default. The XML gives it none and says it does
/// nothing.
#[test]
fn the_deprecated_snap_property_is_not_registered() {
    setup();

    assert_eq!(
        default_of("CSGShape3D", "snap"),
        None,
        "`snap` is deprecated in Godot ('This property does nothing') and has no \
         documented default; registering it would mean inventing one"
    );
}
