//! pat-0q2kx.238 (partial) — `AimModifier3D` in the ClassDB.
//!
//! Godot ground truth: `register_scene_types.cpp` and
//! `doc/classes/AimModifier3D.xml`.
//!
//! ⚠️ SCOPE: registration only. The bead also asks that the aim rotation be
//! driven from the skeleton's modifier tick so poses actually update — NOT
//! delivered, so the acceptance test
//! `aim_modifier_3d_rotates_bone_toward_target` does not exist. See the report.

use gdobject::class_db::{
    class_exists, get_class_info, instantiate, is_abstract, is_parent_class, register_3d_classes,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

#[test]
fn aim_modifier_3d_is_registered_under_bone_constraint() {
    register_3d_classes();

    assert!(class_exists("AimModifier3D"));
    assert!(
        is_parent_class("AimModifier3D", "BoneConstraint3D"),
        "AimModifier3D.xml declares inherits=\"BoneConstraint3D\""
    );
    // The full chain, not just the direct link: AimModifier3D <-
    // BoneConstraint3D <- SkeletonModifier3D. A wrongly-parented intermediate
    // satisfies the direct check while severing the chain.
    assert!(
        is_parent_class("AimModifier3D", "SkeletonModifier3D"),
        "it must reach SkeletonModifier3D, which is what puts it in the \
         skeleton's modifier phase at all"
    );
}

/// Concrete, unlike its virtual parent.
#[test]
fn aim_modifier_is_concrete_while_its_parent_is_virtual() {
    register_3d_classes();

    assert!(
        !is_abstract("AimModifier3D"),
        "GDREGISTER_CLASS(AimModifier3D) — a scene must be able to create one"
    );
}

/// `setting_count` is the only real property; everything else is an indexed
/// accessor. Copying a flat property list from the doc's *methods* would invent
/// properties Godot does not have.
#[test]
fn setting_count_is_the_only_property() {
    register_3d_classes();

    let obj = instantiate("AimModifier3D").expect("instantiates");
    assert_eq!(
        obj.get_property("setting_count"),
        Variant::Int(0),
        "no settings configured by default"
    );

    let own = get_class_info("AimModifier3D").expect("registered");
    let names: Vec<&str> = own.properties.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["setting_count"],
        "the per-setting values are indexed accessors, not properties"
    );
}

/// The five getter/setter pairs, with the arities the doc gives: a setter takes
/// (index, value) and a getter takes (index).
#[test]
fn the_indexed_accessor_pairs_have_the_documented_arities() {
    register_3d_classes();
    let info = get_class_info("AimModifier3D").expect("registered");

    for (setter, getter) in [
        ("set_forward_axis", "get_forward_axis"),
        ("set_primary_rotation_axis", "get_primary_rotation_axis"),
        ("set_use_secondary_rotation", "is_using_secondary_rotation"),
        ("set_relative", "is_relative"),
        ("set_use_euler", "is_using_euler"),
    ] {
        let s = info
            .methods
            .iter()
            .find(|m| m.name == setter)
            .unwrap_or_else(|| panic!("must declare `{setter}`"));
        assert_eq!(s.argument_count, 2, "`{setter}` takes (index, value)");

        let g = info
            .methods
            .iter()
            .find(|m| m.name == getter)
            .unwrap_or_else(|| panic!("must declare `{getter}`"));
        assert_eq!(g.argument_count, 1, "`{getter}` takes (index)");
    }

    assert_eq!(
        info.methods.len(),
        10,
        "exactly the ten the doc declares — no invented extras"
    );
}
