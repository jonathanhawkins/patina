//! pat-nzgdu.127 — `VehicleWheel3D` exposes its full property surface.
//!
//! Godot gives the wheel 14 properties; only 5 were registered, so a `.tscn`
//! carrying `suspension_stiffness` or `wheel_friction_slip` had nowhere to put
//! them and the inspector could not show them. Names and types are diffed
//! against `extension_api.json`.
//!
//! Not covered here (see the bead notes): the sim bridge, the MainLoop tick, and
//! raycasting against real colliders. The acceptance test
//! `vehicle_body_3d_drives_via_mainloop` needs `gdscene`, which is currently
//! broken by an unrelated in-flight E0119 — this crate does not depend on
//! gdscene, so these verify regardless.

use gdobject::{class_exists, get_property_list, is_parent_class, register_3d_classes};
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

#[test]
fn vehicle_wheel_3d_exposes_the_full_suspension_surface() {
    register_3d_classes();
    assert!(class_exists("VehicleWheel3D"));
    assert!(
        is_parent_class("VehicleWheel3D", "Node3D"),
        "a wheel is a Node3D — it is not a body"
    );
    assert!(is_parent_class("VehicleWheel3D", "Object"));

    // Presence, not an exact count: `get_property_list` walks the whole chain,
    // so a wheel also reports Node3D's 5 (transform/visible/...). Asserting a
    // bare length here would be asserting the ancestors' surface too.
    for want in [
        // Drive inputs (per-wheel overrides of the body's).
        "engine_force",
        "brake",
        "steering",
        "use_as_traction",
        "use_as_steering",
        // Wheel geometry / grip.
        "wheel_roll_influence",
        "wheel_radius",
        "wheel_rest_length",
        "wheel_friction_slip",
        // Suspension.
        "suspension_travel",
        "suspension_stiffness",
        "suspension_max_force",
        "damping_compression",
        "damping_relaxation",
    ] {
        assert!(
            props("VehicleWheel3D").iter().any(|p| p == want),
            "VehicleWheel3D must expose `{want}`"
        );
    }
}

/// Godot splits damping into two coefficients — resisting compression and
/// resisting extension. A single "damping" value is a *different* model, so
/// collapsing them would silently change the ride.
#[test]
fn damping_is_a_pair_not_a_single_coefficient() {
    register_3d_classes();
    assert_eq!(default_of("VehicleWheel3D", "damping_compression"), Variant::Float(0.83));
    assert_eq!(default_of("VehicleWheel3D", "damping_relaxation"), Variant::Float(0.88));
    assert!(
        !props("VehicleWheel3D").iter().any(|p| p == "suspension_damping"),
        "`suspension_damping` is the *sim's* field name, not the node's — the \
         node has the compression/relaxation pair"
    );
}

/// The defaults describe a usable wheel rather than zeroes: a wheel with radius
/// 0 or no spring would be nonsense, and a `.tscn` that omits these relies on
/// them.
#[test]
fn defaults_describe_a_usable_wheel() {
    register_3d_classes();
    assert_eq!(default_of("VehicleWheel3D", "wheel_radius"), Variant::Float(0.5));
    assert_eq!(default_of("VehicleWheel3D", "wheel_rest_length"), Variant::Float(0.15));
    assert_eq!(default_of("VehicleWheel3D", "suspension_travel"), Variant::Float(0.2));
    assert_eq!(default_of("VehicleWheel3D", "suspension_stiffness"), Variant::Float(5.88));
    assert_eq!(default_of("VehicleWheel3D", "suspension_max_force"), Variant::Float(6000.0));
    assert_eq!(default_of("VehicleWheel3D", "wheel_friction_slip"), Variant::Float(10.5));
    assert_eq!(default_of("VehicleWheel3D", "wheel_roll_influence"), Variant::Float(0.1));

    // Drive inputs default to inert — the body drives, unless a wheel overrides.
    assert_eq!(default_of("VehicleWheel3D", "engine_force"), Variant::Float(0.0));
    assert_eq!(default_of("VehicleWheel3D", "brake"), Variant::Float(0.0));
    assert_eq!(default_of("VehicleWheel3D", "steering"), Variant::Float(0.0));
    assert_eq!(default_of("VehicleWheel3D", "use_as_traction"), Variant::Bool(false));
    assert_eq!(default_of("VehicleWheel3D", "use_as_steering"), Variant::Bool(false));
}

/// ⚠️ The node's `suspension_stiffness` is NOT the sim's. Godot's default is
/// 5.88 in its own units; `gdphysics2d::vehicle3d`'s is 5000.0 as
/// force-per-unit-compression. The bridge (follow-up) must MAP these, not copy
/// them — feeding 5.88 to the sim under-springs the chassis by ~3 orders of
/// magnitude and the car sinks through the floor. This test exists to make that
/// trap loud if anyone later "aligns" the two by changing this default.
#[test]
fn node_suspension_stiffness_is_godots_unit_not_the_sims() {
    register_3d_classes();
    let stiffness = default_of("VehicleWheel3D", "suspension_stiffness");
    assert_eq!(
        stiffness,
        Variant::Float(5.88),
        "this is Godot's node default; the sim's 5000.0 is a different unit and \
         must not be copied here"
    );
    // Sanity: it is nowhere near the sim's force-per-unit value.
    match stiffness {
        Variant::Float(f) => assert!(
            f < 100.0,
            "a node stiffness in the thousands means someone pasted the sim's \
             force-per-unit value into the node's surface"
        ),
        other => panic!("expected a float, got {other:?}"),
    }
}

/// The body's own surface is already complete — it is only the wheel that was short.
#[test]
fn vehicle_body_3d_surface_is_complete_and_is_a_rigid_body() {
    register_3d_classes();
    assert!(
        is_parent_class("VehicleBody3D", "RigidBody3D"),
        "the chassis is a RigidBody3D"
    );
    // Its own three drive inputs (the list also carries RigidBody3D's and
    // Node3D's, since get_property_list walks the chain).
    for want in ["engine_force", "brake", "steering"] {
        assert!(props("VehicleBody3D").iter().any(|p| p == want));
    }
    // The chassis inherits a body's surface — that is what makes it drivable at
    // all, and it is the difference between it and the wheel.
    assert!(
        props("VehicleBody3D").iter().any(|p| p == "mass"),
        "VehicleBody3D inherits RigidBody3D's mass"
    );
    assert!(
        !props("VehicleWheel3D").iter().any(|p| p == "mass"),
        "a wheel is a Node3D and has no mass of its own"
    );
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("VehicleWheel3D");
    register_3d_classes();
    assert_eq!(before, props("VehicleWheel3D"));
}
