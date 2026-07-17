//! pat-nzgdu.144 — the GPU particle attractor classes are ClassDB-registered.
//!
//! This is step (1) of the bead's own implementation plan. The simulator half
//! (collecting attractor nodes from the SceneTree and applying their force in
//! `gdscene/src/particle3d.rs`) is follow-up — see the bead notes; that crate
//! currently does not compile for reasons unrelated to this bead, and `gdobject`
//! has no dependency on it, so these verify regardless.
//!
//! Schemas are diffed against `extension_api.json`.

use gdobject::{class_exists, get_property_list, is_parent_class, register_3d_classes};
use gdobject::class_db::is_abstract;
use gdvariant::Variant;

/// The three concrete attractor shapes and the property that defines each volume.
const SHAPES: &[(&str, &str)] = &[
    ("GPUParticlesAttractorBox3D", "size"),
    ("GPUParticlesAttractorSphere3D", "radius"),
    ("GPUParticlesAttractorVectorField3D", "size"),
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
fn attractor_family_is_registered_with_godot_parents() {
    register_3d_classes();

    assert!(class_exists("GPUParticlesAttractor3D"));
    assert!(
        is_parent_class("GPUParticlesAttractor3D", "VisualInstance3D"),
        "an attractor is a VisualInstance3D — it has a volume in the world, \
         which is what cull_mask filters against"
    );

    for (shape, _) in SHAPES {
        assert!(class_exists(shape), "ClassDB must know {shape}");
        assert!(
            is_parent_class(shape, "GPUParticlesAttractor3D"),
            "{shape} derives the attractor base"
        );
        // The chain must actually reach the root — an unregistered intermediate
        // severs it silently and `class_exists` alone would not notice
        // (pat-nzgdu.98).
        assert!(
            is_parent_class(shape, "Node3D"),
            "{shape} must reach Node3D through VisualInstance3D"
        );
        assert!(is_parent_class(shape, "Object"));
    }
}

/// Godot only instantiates the shapes; the base carries the shared force
/// parameters and is abstract.
#[test]
fn the_base_is_abstract_and_the_shapes_are_not() {
    register_3d_classes();
    assert!(
        is_abstract("GPUParticlesAttractor3D"),
        "the base has no volume of its own, so it cannot attract anything"
    );
    for (shape, _) in SHAPES {
        assert!(!is_abstract(shape), "{shape} is instantiable in Godot");
    }
}

/// The four parameters that define the force, inherited by every shape.
#[test]
fn base_carries_the_shared_force_parameters() {
    register_3d_classes();
    let want = ["strength", "attenuation", "directionality", "cull_mask"];
    assert_props("GPUParticlesAttractor3D", &want);
    // Inherited, not redeclared, by each shape.
    for (shape, _) in SHAPES {
        assert_props(shape, &want);
    }
}

/// Each shape adds exactly the property that defines its volume — that is the
/// only difference between them, and it is what the containment test reads.
#[test]
fn each_shape_defines_its_own_volume() {
    register_3d_classes();
    for (shape, volume_prop) in SHAPES {
        assert_props(shape, &[volume_prop]);
    }
    // A sphere is defined by a radius and a box by a size; neither borrows the
    // other's.
    assert!(
        !props("GPUParticlesAttractorSphere3D").iter().any(|p| p == "size"),
        "a sphere has no size — it has a radius"
    );
    assert!(
        !props("GPUParticlesAttractorBox3D").iter().any(|p| p == "radius"),
        "a box has no radius — it has a size"
    );
    // The vector-field variant is a box that also samples a texture for the
    // force direction.
    assert_props("GPUParticlesAttractorVectorField3D", &["size", "texture"]);
}

#[test]
fn defaults_describe_a_usable_attractor() {
    register_3d_classes();
    // A default attractor pulls (positive strength) rather than doing nothing.
    assert_eq!(default_of("GPUParticlesAttractor3D", "strength"), Variant::Float(1.0));
    assert_eq!(default_of("GPUParticlesAttractor3D", "attenuation"), Variant::Float(1.0));
    // Fully radial by default: 0 = pull toward the centre, 1 = uniform direction.
    assert_eq!(
        default_of("GPUParticlesAttractor3D", "directionality"),
        Variant::Float(0.0)
    );
    // All 20 render layers on, so an attractor affects every emitter until the
    // mask is narrowed. A default of 0 would silently attract nothing.
    assert_eq!(
        default_of("GPUParticlesAttractor3D", "cull_mask"),
        Variant::Int(0xFFFFF)
    );

    // The volumes have real extents, not zeroes.
    assert_eq!(
        default_of("GPUParticlesAttractorSphere3D", "radius"),
        Variant::Float(1.0)
    );
    match default_of("GPUParticlesAttractorBox3D", "size") {
        Variant::Vector3(v) => {
            assert!(v.x > 0.0 && v.y > 0.0 && v.z > 0.0, "a box volume must have extent")
        }
        other => panic!("size must be a Vector3, got {other:?}"),
    }
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("GPUParticlesAttractorSphere3D");
    register_3d_classes();
    assert_eq!(before, props("GPUParticlesAttractorSphere3D"));
}
