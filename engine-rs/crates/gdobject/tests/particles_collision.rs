//! pat-0q2kx.231 (partial) — the GPUParticlesCollision* ClassDB family.
//!
//! Godot ground truth: `register_scene_types.cpp` (GDREGISTER macros) and each
//! class's doc XML.
//!
//! The attractor half of the particle-volume family was registered; the
//! collision half was not, so a `.tscn` carrying a collider failed class
//! resolution outright.
//!
//! ⚠️ SCOPE: registration only. The bead's stage 1 also asks for attractor
//! forces and box/sphere bounce in the CPU sim, wired through
//! `process_particles` — NOT delivered. See the completion report.

use gdobject::class_db::{
    class_exists, get_property_list, instantiate, is_abstract, is_parent_class, register_3d_classes,
};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject`.
use gdobject::GodotObject;
use gdvariant::Variant;

const COLLIDERS: [&str; 4] = [
    "GPUParticlesCollisionBox3D",
    "GPUParticlesCollisionSphere3D",
    "GPUParticlesCollisionHeightField3D",
    "GPUParticlesCollisionSDF3D",
];

#[test]
fn the_collision_family_is_registered_under_visual_instance() {
    register_3d_classes();

    assert!(class_exists("GPUParticlesCollision3D"));
    assert!(
        is_parent_class("GPUParticlesCollision3D", "VisualInstance3D"),
        "the base inherits VisualInstance3D, matching its attractor sibling"
    );

    for c in COLLIDERS {
        assert!(class_exists(c), "{c} must be registered");
        assert!(
            is_parent_class(c, "GPUParticlesCollision3D"),
            "{c} must inherit the collision base"
        );
        // Through the base to VisualInstance3D — a direct-parent-only check
        // passes even when the intermediate is parented wrongly.
        assert!(
            is_parent_class(c, "VisualInstance3D"),
            "{c} must reach VisualInstance3D through the base"
        );
    }
}

/// Only the base is abstract; all four volumes are instantiable.
#[test]
fn only_the_collision_base_is_abstract() {
    register_3d_classes();

    assert!(
        is_abstract("GPUParticlesCollision3D"),
        "GDREGISTER_ABSTRACT_CLASS(GPUParticlesCollision3D)"
    );
    for c in COLLIDERS {
        assert!(
            !is_abstract(c),
            "{c} is a plain GDREGISTER_CLASS — a scene must be able to create it"
        );
    }
}

/// The masks are all-bits-set defaults, and `heightfield_mask` is a DIFFERENT
/// width from `cull_mask`. Zeroing either would make the collider ignore every
/// particle — silently, with no error anywhere.
#[test]
fn mask_defaults_are_not_zero_and_not_interchangeable() {
    register_3d_classes();

    // Inherited by every collider from the base.
    for c in COLLIDERS {
        let obj = instantiate(c).unwrap_or_else(|| panic!("{c} instantiates"));
        assert_eq!(
            obj.get_property("cull_mask"),
            Variant::Int(4294967295),
            "{c} must inherit cull_mask = all 32 layers, not 0"
        );
    }

    let hf = instantiate("GPUParticlesCollisionHeightField3D").expect("instantiates");
    assert_eq!(
        hf.get_property("heightfield_mask"),
        Variant::Int(1048575),
        "heightfield_mask is 2^20-1, NOT cull_mask's 2^32-1 — copying the \
         neighbouring mask's value is the natural mistake"
    );
    assert_ne!(
        hf.get_property("heightfield_mask"),
        hf.get_property("cull_mask"),
        "the two masks must not have been given the same value"
    );

    let sdf = instantiate("GPUParticlesCollisionSDF3D").expect("instantiates");
    assert_eq!(sdf.get_property("bake_mask"), Variant::Int(4294967295));
}

/// Volume dimensions and the non-zero `resolution` enum default.
#[test]
fn volume_defaults_match_the_doc_xml() {
    register_3d_classes();

    let two = Variant::Vector3(gdcore::math::Vector3::new(2.0, 2.0, 2.0));
    for c in [
        "GPUParticlesCollisionBox3D",
        "GPUParticlesCollisionHeightField3D",
        "GPUParticlesCollisionSDF3D",
    ] {
        let obj = instantiate(c).unwrap_or_else(|| panic!("{c} instantiates"));
        assert_eq!(
            obj.get_property("size"),
            two,
            "{c}.size defaults to Vector3(2, 2, 2); a zero box collides with nothing"
        );
    }

    let sphere = instantiate("GPUParticlesCollisionSphere3D").expect("instantiates");
    assert_eq!(sphere.get_property("radius"), Variant::Float(1.0));

    // `resolution` is an enum whose default is 2, not its zeroth member.
    for c in [
        "GPUParticlesCollisionHeightField3D",
        "GPUParticlesCollisionSDF3D",
    ] {
        let obj = instantiate(c).expect("instantiates");
        assert_eq!(
            obj.get_property("resolution"),
            Variant::Int(2),
            "{c}.resolution defaults to 2, not 0"
        );
    }

    let hf = instantiate("GPUParticlesCollisionHeightField3D").expect("instantiates");
    assert_eq!(hf.get_property("update_mode"), Variant::Int(0));
    assert_eq!(
        hf.get_property("follow_camera_enabled"),
        Variant::Bool(false)
    );
    let sdf = instantiate("GPUParticlesCollisionSDF3D").expect("instantiates");
    assert_eq!(sdf.get_property("thickness"), Variant::Float(1.0));
}

/// `cull_mask` lives on the base only, so it appears once per collider rather
/// than twice — `get_property_list` concatenates the chain without deduping.
#[test]
fn cull_mask_is_declared_once_on_the_base() {
    register_3d_classes();

    for c in COLLIDERS {
        assert_eq!(
            get_property_list(c)
                .iter()
                .filter(|p| p.name == "cull_mask")
                .count(),
            1,
            "{c} must report cull_mask exactly once"
        );
    }
}
