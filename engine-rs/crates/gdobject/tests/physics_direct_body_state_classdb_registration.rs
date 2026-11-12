//! pat-s3rrv.94 acceptance 1 — `PhysicsDirectBodyState2D/3D` are registered
//! non-instantiable with their doc-XML surface, and `custom_integrator` is
//! registered on both RigidBody classes with accessors.
//!
//! Ground truth: `doc/classes/PhysicsDirectBodyState2D.xml` (14 properties,
//! 28 methods) and `PhysicsDirectBodyState3D.xml` (16, 28). Both are
//! `is_instantiable=false` in `extension_api.json` — a script RECEIVES one from
//! `_integrate_forces`, it never constructs one.

use gdobject::class_db::{
    can_instantiate, class_exists, class_has_method, get_property_list, is_abstract,
    register_all_classes,
};

fn setup() {
    // ⚠️ ClassDB is empty in a fresh test process; every query below would
    // answer "absent" for reasons that have nothing to do with registration.
    register_all_classes();
}

fn property_names(class_name: &str) -> Vec<String> {
    get_property_list(class_name)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn accessors(class_name: &str, prop: &str) -> (String, String) {
    let p = get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == prop)
        .unwrap_or_else(|| panic!("{class_name} should register `{prop}`"));
    (p.getter, p.setter)
}

/// 🔑 Both state classes exist and are NOT instantiable.
#[test]
fn direct_body_state_classes_are_registered_non_instantiable() {
    setup();
    for cls in ["PhysicsDirectBodyState2D", "PhysicsDirectBodyState3D"] {
        assert!(class_exists(cls), "{cls} must be registered in ClassDB");
        assert!(
            is_abstract(cls),
            "{cls} is `is_instantiable=false` in Godot — registering it as \
             instantiable would let ClassDB.instantiate() hand back an object \
             with no body behind it",
        );
        assert!(
            !can_instantiate(cls),
            "{cls} must refuse instantiation, not merely be flagged abstract",
        );
    }
}

/// 🔑 The property tables match the doc XML, including the 3D-only members.
#[test]
fn direct_body_state_property_tables_match_the_doc_xml() {
    setup();

    let two = property_names("PhysicsDirectBodyState2D");
    for name in [
        "step",
        "inverse_mass",
        "inverse_inertia",
        "total_gravity",
        "total_linear_damp",
        "total_angular_damp",
        "center_of_mass",
        "center_of_mass_local",
        "collision_layer",
        "collision_mask",
        "linear_velocity",
        "angular_velocity",
        "transform",
        "sleeping",
    ] {
        assert!(
            two.contains(&name.to_string()),
            "PhysicsDirectBodyState2D must declare `{name}`; it has {two:?}",
        );
    }

    let three = property_names("PhysicsDirectBodyState3D");
    // ⚠️ The two classes are NOT the same table. These three exist only in 3D,
    // where inertia is a tensor rather than a scalar — a registration that
    // copied the 2D list would pass every assertion above and fail here.
    for name in [
        "inverse_inertia_tensor",
        "principal_inertia_axes",
        "total_gravity",
    ] {
        assert!(
            three.contains(&name.to_string()),
            "PhysicsDirectBodyState3D must declare `{name}`; it has {three:?}",
        );
    }
    assert!(
        !two.contains(&"inverse_inertia_tensor".to_string()),
        "…and `inverse_inertia_tensor` is 3D-only; 2D inertia is a scalar",
    );
}

/// 🔑 `sleeping`'s setter is `set_sleep_state`, which does not follow from the
/// property name — the one accessor here that cannot be guessed.
#[test]
fn sleeping_uses_godots_irregular_setter_name() {
    setup();
    for cls in ["PhysicsDirectBodyState2D", "PhysicsDirectBodyState3D"] {
        let (getter, setter) = accessors(cls, "sleeping");
        assert_eq!(
            getter, "is_sleeping",
            "{cls}.sleeping uses the `is_` getter form",
        );
        assert_eq!(
            setter, "set_sleep_state",
            "{cls}.sleeping's setter is `set_sleep_state` in Godot, NOT \
             `set_sleeping` — a name derived from the property would be wrong",
        );
    }
}

/// The contact and impulse methods a script calls on the state are declared.
#[test]
fn direct_body_state_declares_its_contact_and_impulse_methods() {
    setup();
    for cls in ["PhysicsDirectBodyState2D", "PhysicsDirectBodyState3D"] {
        for m in [
            "get_contact_count",
            "get_contact_local_position",
            "get_contact_local_normal",
            "get_contact_impulse",
            "get_contact_collider",
            "get_contact_collider_id",
            "apply_central_impulse",
            "apply_impulse",
            "apply_torque_impulse",
            "apply_force",
            "add_constant_force",
            "get_velocity_at_local_position",
        ] {
            assert!(class_has_method(cls, m), "{cls} must declare `{m}`");
        }
    }
}

/// 🔑 `custom_integrator` is registered on BOTH RigidBody classes, WITH
/// accessors.
///
/// ⚠️ The accessors are the assertion that matters. The property itself was
/// already registered on both — what RigidBody3D lacked was
/// `is_using_custom_integrator` / `set_use_custom_integrator`, so a test that
/// only checked the property existed would have passed before the fix and
/// proved nothing.
#[test]
fn custom_integrator_is_registered_with_accessors_on_both_rigid_bodies() {
    setup();
    for cls in ["RigidBody2D", "RigidBody3D"] {
        let (getter, setter) = accessors(cls, "custom_integrator");
        assert_eq!(
            getter, "is_using_custom_integrator",
            "{cls}.custom_integrator needs Godot's `is_` getter; an empty \
             getter means the property is registered but not addressable \
             through ClassDB",
        );
        assert_eq!(
            setter, "set_use_custom_integrator",
            "{cls}.custom_integrator needs Godot's setter name",
        );
    }
}
