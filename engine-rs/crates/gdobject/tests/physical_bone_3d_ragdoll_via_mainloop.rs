//! pat-nzgdu.175 — `PhysicalBone3D` + `PhysicalBoneSimulator3D` class surface.
//!
//! Covers the registration half. The ragdoll itself — creating a rigid body per
//! `PhysicalBone3D`, joint-connecting it to its parent bone, stepping it in
//! MainLoop's physics sub-step, and writing the body transforms back to the
//! Skeleton3D bone poses — is follow-up, and the bead itself notes it "depends
//! on the joints bead". See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

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
fn ragdoll_classes_are_registered_with_godot_parents() {
    register_3d_classes();

    // A physical bone IS a body — that is what lets physics move it.
    assert!(class_exists("PhysicalBone3D"));
    assert!(
        is_parent_class("PhysicalBone3D", "PhysicsBody3D"),
        "PhysicalBone3D is a PhysicsBody3D"
    );
    assert!(is_parent_class("PhysicalBone3D", "Node3D"));
    assert!(is_parent_class("PhysicalBone3D", "Object"));

    // The simulator IS a skeleton modifier — that is how it takes pose control.
    assert!(class_exists("PhysicalBoneSimulator3D"));
    assert!(
        is_parent_class("PhysicalBoneSimulator3D", "SkeletonModifier3D"),
        "the simulator is a SkeletonModifier3D — that is what lets it drive poses"
    );
    assert!(is_parent_class("PhysicalBoneSimulator3D", "Node3D"));

    for class in ["PhysicalBone3D", "PhysicalBoneSimulator3D"] {
        assert!(!is_abstract(class));
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// The joint that ties a bone to its parent — the heart of a ragdoll.
#[test]
fn physical_bone_carries_its_joint_surface() {
    register_3d_classes();
    assert_props(
        "PhysicalBone3D",
        &["joint_type", "joint_offset", "joint_rotation", "body_offset"],
    );
    assert!(class_has_enum("PhysicalBone3D", "JointType"));
    for (name, value) in [
        ("JOINT_TYPE_NONE", 0),
        ("JOINT_TYPE_PIN", 1),
        ("JOINT_TYPE_CONE", 2),
        ("JOINT_TYPE_HINGE", 3),
        ("JOINT_TYPE_SLIDER", 4),
        ("JOINT_TYPE_6DOF", 5),
    ] {
        assert_eq!(
            class_get_integer_constant("PhysicalBone3D", name),
            Some(value),
            "JointType::{name}"
        );
    }
    // Unconstrained until the ragdoll is built — Godot's default.
    assert_eq!(default_of("PhysicalBone3D", "joint_type"), Variant::Int(0));
    // The offsets are identity, so a bone with no offset sits on its joint.
    assert!(matches!(
        default_of("PhysicalBone3D", "joint_offset"),
        Variant::Transform3D(_)
    ));
    assert!(matches!(
        default_of("PhysicalBone3D", "body_offset"),
        Variant::Transform3D(_)
    ));
}

/// The rigid-body surface a bone needs to actually fall.
#[test]
fn physical_bone_carries_a_full_rigid_body_surface() {
    register_3d_classes();
    assert_props(
        "PhysicalBone3D",
        &[
            "mass",
            "friction",
            "bounce",
            "gravity_scale",
            "custom_integrator",
            "linear_damp_mode",
            "linear_damp",
            "angular_damp_mode",
            "angular_damp",
            "linear_velocity",
            "angular_velocity",
            "can_sleep",
        ],
    );
    // A bone falls by default: unit mass, full gravity, no damping.
    assert_eq!(default_of("PhysicalBone3D", "mass"), Variant::Float(1.0));
    assert_eq!(
        default_of("PhysicalBone3D", "gravity_scale"),
        Variant::Float(1.0),
        "a gravity_scale of 0 would leave the ragdoll hanging in the air"
    );
    assert_eq!(default_of("PhysicalBone3D", "linear_damp"), Variant::Float(0.0));
    assert_eq!(default_of("PhysicalBone3D", "can_sleep"), Variant::Bool(true));
}

/// ⚠️ `PhysicalBone3D` is a `PhysicsBody3D`, **not** a `RigidBody3D` — despite
/// carrying a rigid body's surface. Assuming the inheritance from the property
/// list gets it wrong.
#[test]
fn physical_bone_is_a_physics_body_not_a_rigid_body() {
    register_3d_classes();
    assert!(is_parent_class("PhysicalBone3D", "PhysicsBody3D"));
    assert!(
        !is_parent_class("PhysicalBone3D", "RigidBody3D"),
        "PhysicalBone3D re-declares mass/gravity_scale itself; it does not derive RigidBody3D"
    );
}

#[test]
fn damp_mode_enum_matches_godot() {
    register_3d_classes();
    assert!(class_has_enum("PhysicalBone3D", "DampMode"));
    assert_eq!(
        class_get_integer_constant("PhysicalBone3D", "DAMP_MODE_COMBINE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("PhysicalBone3D", "DAMP_MODE_REPLACE"),
        Some(1)
    );
    // Both damp modes default to COMBINE (0) — REPLACE would silently override
    // the space's damping.
    assert_eq!(default_of("PhysicalBone3D", "linear_damp_mode"), Variant::Int(0));
    assert_eq!(default_of("PhysicalBone3D", "angular_damp_mode"), Variant::Int(0));
}

/// Godot gives the simulator no properties of its own — it is driven purely by
/// `physical_bones_start_simulation()`, and inherits `active`/`influence` from
/// SkeletonModifier3D. Inventing a property here would be fiction.
#[test]
fn simulator_adds_no_properties_and_inherits_the_modifier_surface() {
    register_3d_classes();
    let own: Vec<String> = props("PhysicalBoneSimulator3D")
        .into_iter()
        .filter(|p| !props("SkeletonModifier3D").contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "PhysicalBoneSimulator3D adds no properties in Godot; found {own:?}"
    );
    // But it does inherit the modifier's controls — that is how a ragdoll is
    // blended in and out.
    assert_props("PhysicalBoneSimulator3D", &["active", "influence"]);
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("PhysicalBone3D");
    register_3d_classes();
    assert_eq!(before, props("PhysicalBone3D"));
}
