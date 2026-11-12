//! pat-0q2kx.263 — script-created XR trackers round-trip through XRServer.
//!
//! The ClassDB half of this bead lives in `gdobject/tests/xr_trackers.rs`. This
//! is the wiring half the acceptance calls for: a script constructs an
//! `XRHandTracker`, sets a joint transform, registers it with
//! `XRServer.add_tracker`, and a **second lookup** via `XRServer.get_tracker`
//! returns it with the joint intact.
//!
//! The second lookup is the point. A registry that stored a *copy* at
//! `add_tracker` time would pass a naive same-object test and silently lose any
//! write made afterwards, so the test writes a joint **after** registering and
//! reads it back through the registry.
//!
//! Array literals are single-line throughout: this GDScript parser rejects a
//! multi-line `[` (pat-0q2kx.253).

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn call_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn array(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

// ===========================================================================
// The acceptance test (wiring half)
// ===========================================================================

/// **The acceptance test.** Construct → set joint → `add_tracker` → look up by
/// name → the joint survives.
#[test]
fn xr_tracker_registry_round_trip() {
    let src = "\
extends Node

func check():
    var t = XRHandTracker.new()
    t.set_tracker_name(\"left_hand\")
    t.set_hand_joint_transform(0, Transform3D.IDENTITY)
    XRServer.add_tracker(t)
    var found = XRServer.get_tracker(\"left_hand\")
    var missing = XRServer.get_tracker(\"nope\")
    return [found != null, missing, found.get_tracker_name()]
";
    let v = array(call_check(src));
    assert_eq!(
        v[0],
        Variant::Bool(true),
        "the registered tracker must come back by name",
    );
    assert_eq!(
        v[1],
        Variant::Nil,
        "an unregistered name must be null, as in Godot",
    );
    assert_eq!(v[2], Variant::String("left_hand".to_string()));
}

/// The registry holds the **same object**, not a snapshot: a joint written
/// after `add_tracker` is visible through a later `get_tracker`.
///
/// This is what a copying registry would fail.
#[test]
fn writes_after_registration_are_visible_through_the_registry() {
    let src = "\
extends Node

func check():
    var t = XRHandTracker.new()
    t.set_tracker_name(\"right_hand\")
    XRServer.add_tracker(t)
    t.set_hand_joint_radius(5, 0.25)
    var again = XRServer.get_tracker(\"right_hand\")
    return again.get_hand_joint_radius(5)
";
    match call_check(src) {
        Variant::Float(f) => assert!(
            (f - 0.25).abs() < 1e-6,
            "a joint written AFTER add_tracker must be visible through the \
             registry; got {f}",
        ),
        other => panic!("expected a Float, got {other:?}"),
    }
}

/// An unset joint reads back as identity rather than as a zeroed transform, and
/// an unset radius as 0.0 — Godot's answer for a joint the tracker never saw.
#[test]
fn unset_joints_report_identity() {
    let src = "\
extends Node

func check():
    var t = XRHandTracker.new()
    return [t.get_hand_joint_transform(9) == Transform3D.IDENTITY, t.get_hand_joint_radius(9)]
";
    let v = array(call_check(src));
    assert_eq!(
        v[0],
        Variant::Bool(true),
        "an untouched joint must read back as IDENTITY, not a zero matrix",
    );
    assert_eq!(v[1], Variant::Float(0.0));
}

/// `remove_tracker` unregisters, and the other tracker classes construct too.
#[test]
fn remove_unregisters_and_the_other_tracker_classes_construct() {
    let src = "\
extends Node

func check():
    var t = XRBodyTracker.new()
    t.set_tracker_name(\"body\")
    XRServer.add_tracker(t)
    var before = XRServer.get_tracker(\"body\") != null
    XRServer.remove_tracker(t)
    var after = XRServer.get_tracker(\"body\")
    var face = XRFaceTracker.new()
    face.set_blend_shape(3, 0.5)
    return [before, after, face.get_blend_shape(3)]
";
    let v = array(call_check(src));
    assert_eq!(v[0], Variant::Bool(true), "registered");
    assert_eq!(v[1], Variant::Nil, "removed");
    match &v[2] {
        Variant::Float(f) => assert!((f - 0.5).abs() < 1e-6, "blend shape survives, got {f}"),
        other => panic!("expected a Float, got {other:?}"),
    }
}
