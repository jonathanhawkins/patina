//! pat-s3rrv.42 acceptance: `Class.CONSTANT` resolves through ClassDB.
//!
//! ClassDB already stored every registered class's integer constants, and the
//! interpreter already read them — but ONLY through an explicit
//! `ClassDB.class_get_integer_constant("XRServer", "TRACKER_HAND")` call. The
//! member-access spelling every real script uses resolved to nothing, so
//! `XRServer.TRACKER_HAND` raised UndefinedVariable.
//!
//! The lookup is deliberately LAST in the static-member chain, after
//! `variant_math` and @GlobalScope, so a global name that also exists as a class
//! constant keeps its global value.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` on a fresh instance through real VM dispatch.
fn call_check(src: &str) -> Variant {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn constant(expr: &str) -> Variant {
    call_check(&format!(
        "extends Node\n\nfunc check():\n    return {expr}\n"
    ))
}

/// **The acceptance list**, each value from
/// `upstream/godot/doc/classes/*.xml` rather than from memory.
#[test]
fn class_enum_constants_from_script() {
    assert_eq!(
        constant("XRServer.TRACKER_HAND"),
        Variant::Int(16),
        "XRServer.TRACKER_HAND is 16 (TrackerType is a bitmask)"
    );
    assert_eq!(
        constant("XRServer.TRACKER_UNKNOWN"),
        Variant::Int(128),
        "XRServer.TRACKER_UNKNOWN is 128"
    );
    assert_eq!(constant("XRHandTracker.HAND_JOINT_WRIST"), Variant::Int(1));
    assert_eq!(constant("XRHandTracker.HAND_JOINT_MAX"), Variant::Int(26));
    assert_eq!(
        constant("XRPositionalTracker.TRACKER_HAND_LEFT"),
        Variant::Int(1)
    );
}

/// No `ClassDB.*` call is involved — the point of the bead. If member access
/// silently fell back to the explicit API this would still pass, so it is
/// asserted alongside the negative case below rather than alone.
#[test]
fn the_bitmask_values_are_distinct_powers_of_two() {
    // A registration that collapsed the enum to sequential 0,1,2… would still
    // satisfy "some integer came back"; these relationships would not survive it.
    let hand = constant("XRServer.TRACKER_HAND");
    let body = constant("XRServer.TRACKER_BODY");
    let any_known = constant("XRServer.TRACKER_ANY_KNOWN");

    assert_eq!(hand, Variant::Int(16));
    assert_eq!(body, Variant::Int(32));
    assert_eq!(
        any_known,
        Variant::Int(127),
        "TRACKER_ANY_KNOWN is the OR of the known bits, not a counter"
    );
}

/// **The guard.** An unknown member must still raise, not answer 0.
///
/// Returning `unwrap_or(0)` here — which the explicit `ClassDB` call does — would
/// turn every typo into a silent zero, which is worse than the original gap.
#[test]
fn an_unknown_member_still_raises() {
    let mut interp = Interpreter::new();
    gdobject::class_db::register_all_classes();
    let class_def = interp
        .run_class("extends Node\n\nfunc check():\n    return XRServer.NOT_A_CONSTANT\n")
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");

    assert!(
        interp
            .call_instance_method(&mut inst, "check", &[])
            .is_err(),
        "an unregistered constant must raise, not silently evaluate to 0"
    );
}

/// A @GlobalScope name that ALSO exists as a class constant keeps the global
/// value — which is why the ClassDB lookup is last in the chain rather than
/// first.
#[test]
fn a_global_scope_name_still_wins() {
    assert_eq!(
        constant("Key.KEY_SPACE"),
        constant("KEY_SPACE"),
        "the enum-qualified global must agree with the bare global; a ClassDB \
         entry must not shadow it"
    );
}
