//! pat-0q2kx.263 — the XR tracker / modifier / interface class surface.
//!
//! Nine target classes plus three ancestors that did not exist (`XRTracker`,
//! `XRPositionalTracker`, `XRInterface`). Everything here — parents, properties
//! and 183 enum constants — was generated from
//! `upstream/godot/doc/classes/*.xml` rather than transcribed, because a wrong
//! constant is invisible to `class_exists`.
//!
//! **The doc XML corrects the bead text in three places**, and each is asserted
//! below so the correction cannot silently regress:
//!   * `XRHandTracker` / `XRBodyTracker` extend `XRPositionalTracker`, not
//!     `XRTracker` as the bead says
//!   * `XRFaceModifier3D` extends `Node3D`, not `SkeletonModifier3D`
//!   * `XRVRS` extends `Object`, not `RefCounted`
//!
//! Binary named `xr_trackers` to keep the verify command inside the 58-char
//! truncation limit.

use gdobject::class_db::{
    class_exists, get_property_list, inheritance_chain, instantiate, register_3d_classes,
};

/// Each class and the parent its Godot ground truth declares.
///
/// `MobileVRInterface` has no doc XML — it is a module class — so its parent
/// comes from `modules/mobile_vr/mobile_vr_interface.h`
/// (`class MobileVRInterface : public XRInterface`).
const FAMILY: [(&str, &str); 12] = [
    ("XRTracker", "RefCounted"),
    ("XRPositionalTracker", "XRTracker"),
    ("XRInterface", "RefCounted"),
    ("XRHandTracker", "XRPositionalTracker"),
    ("XRBodyTracker", "XRPositionalTracker"),
    ("XRFaceTracker", "XRTracker"),
    ("XRHandModifier3D", "SkeletonModifier3D"),
    ("XRBodyModifier3D", "SkeletonModifier3D"),
    ("XRFaceModifier3D", "Node3D"),
    ("XRVRS", "Object"),
    ("XRInterfaceExtension", "XRInterface"),
    ("MobileVRInterface", "XRInterface"),
];

// ===========================================================================
// The acceptance test (ClassDB half)
// ===========================================================================

/// **The acceptance test.** All twelve classes exist with Godot's parents, the
/// enum constants are present with Godot's values, and each instantiates.
///
/// The script-side half of the bead's acceptance — `XRServer.add_tracker` /
/// `get_tracker` round-tripping a joint transform — is interpreter work and is
/// **not** covered here; see the bead report.
#[test]
fn xr_tracker_class_surface() {
    register_3d_classes();

    for (class, parent) in FAMILY {
        assert!(class_exists(class), "{class} must be registered");

        // The DIRECT parent. `is_parent_class` is true for any ancestor, so it
        // would pass even with a class attached at the wrong depth — which is
        // exactly the mistake the bead text would have led to for the three
        // classes it mis-parents.
        let chain = inheritance_chain(class);
        assert_eq!(
            chain.get(1).map(String::as_str),
            Some(parent),
            "{class} must extend {parent} directly; chain was {chain:?}",
        );
    }
}

/// The enum constant the acceptance names explicitly, plus one from each of the
/// other enums that carry real semantics.
#[test]
fn enum_constants_match_godot_values() {
    register_3d_classes();

    let c = |class: &str, name: &str| -> Option<i64> {
        gdobject::class_db::class_get_integer_constant(class, name)
    };

    // Named in the bead's acceptance.
    assert_eq!(c("XRHandTracker", "HAND_JOINT_PALM"), Some(0));
    // A non-zero value proves the whole enum was carried across, not just the
    // first entry — a zero-initialised table would satisfy PALM alone.
    assert_eq!(c("XRHandTracker", "HAND_JOINT_WRIST"), Some(1));
    assert_eq!(
        c("XRPositionalTracker", "TRACKER_HAND_LEFT"),
        Some(1),
        "TrackerHand must carry Godot's values",
    );
    assert!(
        c("XRFaceTracker", "FT_EYE_LOOK_OUT_RIGHT").is_some()
            || c("XRFaceTracker", "FT_MAX").is_some(),
        "XRFaceTracker must carry its blend-shape constants",
    );
}

/// Every class the family declares instantiable actually instantiates.
///
/// `XRVRS` descends from `Object` rather than `RefCounted`, so this also guards
/// the one class whose base differs from the rest.
#[test]
fn the_family_instantiates() {
    register_3d_classes();

    for (class, _) in FAMILY {
        assert!(
            instantiate(class).is_some(),
            "{class} must instantiate through ClassDB",
        );
    }
}

/// Properties carry Godot's defaults, including the ones a zero-initialised
/// table would get wrong.
#[test]
fn properties_carry_godot_defaults() {
    register_3d_classes();

    let default_of = |class: &str, name: &str| -> Option<gdvariant::Variant> {
        // `get_property_list` includes inherited properties base-first without
        // deduping, so search from the end to take the most-derived
        // declaration rather than an ancestor's.
        get_property_list(class)
            .into_iter()
            .rev()
            .find(|p| p.name == name)
            .map(|p| p.default_value)
    };

    // XRTracker.type defaults to 128 (TRACKER_UNKNOWN), not 0.
    assert_eq!(
        default_of("XRTracker", "type"),
        Some(gdvariant::Variant::Int(128)),
        "XRTracker.type defaults to 128; 0 would be a different tracker kind",
    );
    // MobileVRInterface's lens parameters are non-zero floats — a zeroed table
    // would render a degenerate distortion.
    assert_eq!(
        default_of("MobileVRInterface", "k1"),
        Some(gdvariant::Variant::Float(0.215)),
    );
}
