//! pat-s3rrv.220 (part 1) — `XRPose` and `XRControllerTracker` resolve in
//! ClassDB with the surface Godot documents.
//!
//! Ground truth: `upstream/godot/doc/classes/XRPose.xml`,
//! `XRControllerTracker.xml`, and `servers/xr/xr_pose.h`.
//!
//! # Scope
//!
//! This is the registration half of the bead. The script-facing half — an
//! `XRPose` handle family, `call_xr_pose_instance`, the `.new()` arm, the seven
//! signals, and per-class `tracker_type` seeding — lives in
//! `gdscript-interop/src/interpreter.rs`, which was under another agent's
//! exclusive lease while this landed.
//!
//! ⚠️ So `class_exists("XRControllerTracker")` is true here while
//! `XRControllerTracker.new()` still is not reachable from a script. That gap
//! is deliberate and tracked, not an oversight.

use gdobject::class_db::{
    class_exists, class_get_integer_constant, get_property_list, instantiate, register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn prop(class_name: &str, property: &str) -> Option<gdobject::class_db::PropertyInfo> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
}

/// 🔑 Both classes exist, with the parents Godot documents.
#[test]
fn a_xr_pose_and_controller_tracker_are_registered() {
    register_all_classes();

    assert!(class_exists("XRPose"), "XRPose must be registered");
    assert!(
        class_exists("XRControllerTracker"),
        "XRControllerTracker must be registered"
    );

    // ⚠️ XRControllerTracker inherits XRPositionalTracker, NOT XRTracker and
    // NOT RefCounted. It needs the pose/input/signal surface of the positional
    // tracker; a wrong parent still satisfies `class_exists` above and only
    // shows up as a broken walk.
    assert!(
        is_parent_class("XRControllerTracker", "XRPositionalTracker"),
        "XRControllerTracker inherits XRPositionalTracker"
    );
    for ancestor in ["XRPositionalTracker", "XRTracker", "RefCounted", "Object"] {
        assert!(
            is_parent_class("XRControllerTracker", ancestor),
            "chain broken: XRControllerTracker does not reach {ancestor}"
        );
    }

    assert!(is_parent_class("XRPose", "RefCounted"));
    assert!(is_parent_class("XRPose", "Object"));

    // Both are instantiable value types — `get_pose()` hands one back.
    assert!(instantiate("XRPose").is_some());
    assert!(instantiate("XRControllerTracker").is_some());
}

/// ⚠️ The control for the chain assertion above.
///
/// Reaching `Object` succeeds either way if `XRControllerTracker` had been
/// parented straight onto `XRTracker` — the walk still terminates at `Object`.
/// This pins the SHAPE: the `XRPositionalTracker` link must be present, and the
/// relation must not be symmetric.
#[test]
fn b_the_controller_tracker_chain_shape_not_just_its_endpoint() {
    register_all_classes();

    assert!(is_parent_class(
        "XRControllerTracker",
        "XRPositionalTracker"
    ));
    assert!(
        !is_parent_class("XRPositionalTracker", "XRControllerTracker"),
        "inheritance is not symmetric"
    );
    assert!(
        !is_parent_class("XRPose", "XRTracker"),
        "XRPose is a value, not a tracker"
    );
}

/// 🔑 `XRPose`'s members, transcribed from the doc XML and header.
///
/// ⚠️ The bool getter is `get_has_tracking_data`, **not** `is_has_tracking_data`.
/// Godot uses the `is_` prefix for a great many bool getters and not this one,
/// so deriving the name from the convention yields a plausible, wrong string.
#[test]
fn c_xr_pose_members_match_the_doc_xml() {
    register_all_classes();

    let p = prop("XRPose", "has_tracking_data").expect("has_tracking_data is registered");
    assert_eq!(p.getter, "get_has_tracking_data");
    assert_eq!(p.setter, "set_has_tracking_data");
    assert_eq!(p.default_value, Variant::Bool(false));

    for (name, getter, setter) in [
        ("name", "get_name", "set_name"),
        ("transform", "get_transform", "set_transform"),
        (
            "linear_velocity",
            "get_linear_velocity",
            "set_linear_velocity",
        ),
        (
            "angular_velocity",
            "get_angular_velocity",
            "set_angular_velocity",
        ),
        (
            "tracking_confidence",
            "get_tracking_confidence",
            "set_tracking_confidence",
        ),
    ] {
        let p = prop("XRPose", name).unwrap_or_else(|| panic!("{name} is registered"));
        assert_eq!(p.getter, getter, "{name} getter");
        assert_eq!(p.setter, setter, "{name} setter");
    }

    // ⚠️ `tracking_confidence` carries NO `default=` in the doc XML. The value
    // comes from `xr_pose.h:52`, which initialises it to
    // XR_TRACKING_CONFIDENCE_NONE. Inferring "no documented default" as "no
    // default" would have left it unset.
    assert_eq!(
        prop("XRPose", "tracking_confidence").unwrap().default_value,
        Variant::Int(0),
        "tracking_confidence starts at XR_TRACKING_CONFIDENCE_NONE"
    );
}

/// The `TrackingConfidence` enum the acceptance names.
#[test]
fn d_tracking_confidence_constants_are_exposed() {
    register_all_classes();

    assert_eq!(
        class_get_integer_constant("XRPose", "XR_TRACKING_CONFIDENCE_NONE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("XRPose", "XR_TRACKING_CONFIDENCE_LOW"),
        Some(1)
    );
    assert_eq!(
        class_get_integer_constant("XRPose", "XR_TRACKING_CONFIDENCE_HIGH"),
        Some(2),
        "the acceptance passes XR_TRACKING_CONFIDENCE_HIGH to set_pose"
    );
}
