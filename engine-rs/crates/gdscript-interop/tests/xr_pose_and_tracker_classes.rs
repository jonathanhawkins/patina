//! pat-s3rrv.220 (part 2) — the script-facing half: `XRPose` objects, the
//! `XRPositionalTracker` pose/input API, its seven signals, and the per-class
//! `tracker_type` seeding.
//!
//! The ClassDB half landed separately in
//! `gdobject/tests/xr_pose_and_controller_tracker_registered.rs`; this is the
//! wiring half the bead's acceptance calls for.
//!
//! # Ground truth
//!
//! Transcribed from `upstream/godot/servers/xr/xr_positional_tracker.cpp`,
//! `xr_pose.cpp`, `xr_tracker.h` and `xr_controller_tracker.h` — **not** from
//! the bead's prose summary, which is wrong in two places that matter:
//!
//! - ⚠️ The bead says `get_adjusted_transform` "multiplies by XRServer
//!   world_scale". Godot multiplies **only `.origin`** and then
//!   left-multiplies the **reference frame**. Scaling the whole transform
//!   would grow the controller's basis rather than move it further away.
//! - ⚠️ The bead says `invalidate_pose` should make the pose unavailable.
//!   Godot *keeps* the entry and only clears `has_tracking_data`, with an
//!   explicit comment: "we leave our current transform and velocity data as is
//!   so controllers don't suddenly jump to origin". `has_pose` therefore stays
//!   **true** after an invalidate.
//!
//! # Test ordering
//!
//! ⚠️ Names carry `a_`/`b_`/… prefixes so the **acceptance sorts first**. Test
//! order is alphabetical and a runner stopping at the first failure would
//! otherwise spend its budget on a control and leave the acceptance unrun.
//!
//! ⚠️ Array literals are single-line throughout — this GDScript parser rejects
//! a multi-line `[` (pat-0q2kx.253).

use gdcore::math3d::{Basis, Transform3D};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn call_check(src: &str) -> Variant {
    // ⚠️ REQUIRED, and easy to omit: `XRPose.XR_TRACKING_CONFIDENCE_HIGH`
    // resolves through ClassDB, which `Interpreter::new()` does NOT populate.
    // Without this the constant is not found, the static-member chain falls
    // through, and the script dies with `UndefinedVariable("XRPose")` — naming
    // the CLASS, which reads like a missing registration rather than an unseeded
    // ClassDB. `class_enum_constants_from_script.rs` opens its harness the same
    // way for the same reason; `xr_tracker.rs`, which this harness was copied
    // from, needs no class constant and so does not.
    //
    // ⚠️ A STALE BINARY REPRODUCES THIS SYMPTOM EXACTLY, and telling the two
    // apart cost three agents an afternoon (pat-s3rrv.220). This seeding line
    // landed in `735bb84` at 08-20 05:54; a binary built before that reports all
    // four failures as `UndefinedVariable("XRPose")` — identical to a genuinely
    // missing registration. Two of us "eliminated" a stale verdict by dating the
    // SOURCE (`git log -S'XR_TRACKING_CONFIDENCE_HIGH' -- ...class_db.rs`, which
    // predates the binary and looks current) and concluded the failure was real.
    // It was not: nobody dated the TEST.
    //
    // Before diagnosing a failure here, date BOTH halves — a test binary is built
    // from source AND fixture:
    //
    //   git log -S'<symbol>'            -- crates/gdobject/src/class_db.rs
    //   git log -S'register_all_classes' -- <this file>
    //
    // Take the NEWER of the two. `git log -S` dates the change; a file mtime dates
    // the file, and a later unrelated edit moves it and inverts the answer.
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

fn array(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn transform(v: &Variant) -> Transform3D {
    match v {
        Variant::Transform3D(t) => *t,
        other => panic!("expected a Transform3D, got {other:?}"),
    }
}

fn float(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn assert_basis_eq(got: Basis, want: Basis, what: &str) {
    for (g, w, axis) in [
        (got.x, want.x, "x"),
        (got.y, want.y, "y"),
        (got.z, want.z, "z"),
    ] {
        assert!(
            (g.x - w.x).abs() < 1e-5 && (g.y - w.y).abs() < 1e-5 && (g.z - w.z).abs() < 1e-5,
            "{what}: basis {axis} column is {g:?}, expected {w:?}",
        );
    }
}

// ===========================================================================
// The acceptance
// ===========================================================================

/// 🔑 **THE ACCEPTANCE**, clause by clause, in one script run.
///
/// `XRControllerTracker.new()` → seeded type 2 → `set_pose` → `has_pose` →
/// `get_pose(...).transform` round-trips → `get_adjusted_transform()` reflects
/// `XRServer.world_scale` → `invalidate_pose` clears `has_tracking_data` and
/// fires `pose_lost_tracking` → `set_input` fires `input_float_changed` once.
///
/// ⚠️ `world_scale` is **2.0**, the pose origin is non-zero and the basis is a
/// 45° yaw. All three matter: at scale 1.0 the origin-only bug is invisible,
/// at origin zero the scale term is erased entirely, and with an identity
/// basis a whole-transform multiply looks exactly like an origin-only one.
///
/// ⚠️ `.transform` and `.has_tracking_data` are spelled as **properties**, the
/// way the acceptance and Godot's own docs spell them (`ADD_PROPERTY` in
/// `xr_pose.cpp`), not as `get_transform()` calls. A handle that only answered
/// the getters would pass a test written the other way and still fail every
/// real script.
#[test]
fn a_xr_pose_and_tracker_classes() {
    let src = "\
extends Node

var lost_count = 0
var lost_conf = -1
var lost_tracking = true
var float_count = 0
var float_name = \"\"
var float_value = -1.0

func _on_lost(pose):
    lost_count += 1
    lost_conf = pose.tracking_confidence
    lost_tracking = pose.has_tracking_data

func _on_float(name, value):
    float_count += 1
    float_name = name
    float_value = value

func check():
    XRServer.set_world_scale(2.0)
    var t = XRControllerTracker.new()
    t.set_tracker_name(\"left_hand\")
    var seeded_type = t.get_tracker_type()
    t.connect(\"pose_lost_tracking\", _on_lost)
    t.connect(\"input_float_changed\", _on_float)
    var xf = Transform3D(Basis.from_euler(Vector3(0, 0.7853981634, 0), EULER_ORDER_XYZ), Vector3(0.3, 1.2, -0.4))
    t.set_pose(\"default\", xf, Vector3(1, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_HIGH)
    var posed = t.has_pose(\"default\")
    var pose = t.get_pose(\"default\")
    var round_trip = pose.transform
    var adjusted = pose.get_adjusted_transform()
    var vel = pose.linear_velocity
    var conf = pose.tracking_confidence
    var tracking_before = pose.has_tracking_data
    t.set_input(\"trigger\", 0.8)
    t.invalidate_pose(\"default\")
    var after = t.get_pose(\"default\")
    var exists = ClassDB.class_exists(\"XRPose\") and ClassDB.class_exists(\"XRControllerTracker\")
    return [seeded_type, posed, round_trip, adjusted, vel, conf, tracking_before, float_count, float_name, float_value, t.get_input(\"trigger\"), lost_count, after.has_tracking_data, lost_tracking, lost_conf, exists, xf]
";
    let v = array(call_check(src));

    // `XRControllerTracker()` sets type = TRACKER_CONTROLLER, with no explicit
    // `set_tracker_type` anywhere in the script.
    assert_eq!(
        v[0],
        Variant::Int(2),
        "XRControllerTracker.new() must seed tracker_type = TRACKER_CONTROLLER (2). \
         0 was the old value and is not a legal TrackerType at all, so \
         XRServer.get_trackers(TRACKER_CONTROLLER) could never match it",
    );
    assert_eq!(v[1], Variant::Bool(true), "has_pose after set_pose");

    let original = transform(&v[16]);
    let round_trip = transform(&v[2]);
    assert!(
        (round_trip.origin - original.origin).length() < 1e-5,
        "get_pose(...).transform must round-trip the origin: {:?} vs {:?}",
        round_trip.origin,
        original.origin,
    );
    assert_basis_eq(round_trip.basis, original.basis, "get_pose(...).transform");

    // 🔑 world_scale = 2.0 moves the pose twice as far from the origin…
    let adjusted = transform(&v[3]);
    let want = original.origin * 2.0;
    assert!(
        (adjusted.origin - want).length() < 1e-5,
        "get_adjusted_transform() must scale the ORIGIN by XRServer.world_scale: \
         got {:?}, expected {:?}",
        adjusted.origin,
        want,
    );
    // …and 🔑 leaves the basis alone. This is the half the bead's one-line
    // summary ("multiplies by world_scale") gets wrong: a whole-transform
    // multiply passes the origin assertion above and doubles the basis, which
    // would silently resize the controller instead of moving it.
    assert_basis_eq(
        adjusted.basis,
        original.basis,
        "get_adjusted_transform() must NOT scale the basis",
    );

    assert_eq!(
        v[4],
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 0.0, 0.0)),
        "linear_velocity round-trips",
    );
    assert_eq!(
        v[5],
        Variant::Int(2),
        "tracking_confidence round-trips as XR_TRACKING_CONFIDENCE_HIGH",
    );
    assert_eq!(
        v[6],
        Variant::Bool(true),
        "set_pose marks the pose as tracking",
    );

    // 🔑 Exactly once. `set_input` is edge-triggered on a CHANGED value in
    // Godot; `>= 1` would pass a level-triggered build too.
    assert_eq!(
        v[7],
        Variant::Int(1),
        "set_input must fire input_float_changed exactly once",
    );
    assert_eq!(v[8], Variant::String("trigger".to_string()));
    assert!(
        (float(&v[9]) - 0.8).abs() < 1e-6,
        "the signal carries the new value, got {:?}",
        v[9],
    );
    assert!(
        (float(&v[10]) - 0.8).abs() < 1e-6,
        "get_input(\"trigger\") == 0.8, got {:?}",
        v[10],
    );

    assert_eq!(
        v[11],
        Variant::Int(1),
        "invalidate_pose must fire pose_lost_tracking exactly once",
    );
    assert_eq!(
        v[12],
        Variant::Bool(false),
        "after invalidate_pose the pose must still be readable with \
         has_tracking_data false — Godot keeps the entry so the controller \
         does not jump to the origin",
    );
    // The signal carries the XRPose itself, already updated. A build that
    // emitted before clearing the flag would hand the callback a pose still
    // claiming to be tracked.
    assert_eq!(
        v[13],
        Variant::Bool(false),
        "the pose handed to pose_lost_tracking must already read false",
    );
    assert_eq!(
        v[14],
        Variant::Int(2),
        "…while keeping its tracking_confidence, which invalidate does not touch",
    );

    assert_eq!(
        v[15],
        Variant::Bool(true),
        "ClassDB.class_exists is true for both new classes",
    );
}

// ===========================================================================
// Controls
// ===========================================================================

/// 🔑 The edge-trigger control the acceptance's single `set_input` cannot give.
///
/// Godot only emits when `inputs[name] != value`. Setting the same value twice
/// must fire **once**; changing it must fire again. A level-triggered build
/// passes the acceptance and fails here.
///
/// Also pins the signal *choice*, which Godot makes on the Variant **type**:
/// bool → `button_pressed`/`button_released`, float → `input_float_changed`,
/// Vector2 → `input_vector2_changed`.
#[test]
fn b_set_input_is_edge_triggered_and_picks_the_signal_by_type() {
    let src = "\
extends Node

var floats = 0
var pressed = 0
var released = 0
var vectors = 0
var last_vector = Vector2(0, 0)

func _on_float(name, value):
    floats += 1

func _on_pressed(name):
    pressed += 1

func _on_released(name):
    released += 1

func _on_vector(name, vector):
    vectors += 1
    last_vector = vector

func check():
    var t = XRControllerTracker.new()
    t.connect(\"input_float_changed\", _on_float)
    t.connect(\"button_pressed\", _on_pressed)
    t.connect(\"button_released\", _on_released)
    t.connect(\"input_vector2_changed\", _on_vector)
    t.set_input(\"trigger\", 0.8)
    t.set_input(\"trigger\", 0.8)
    var same_value_fires = floats
    t.set_input(\"trigger\", 0.4)
    t.set_input(\"grip\", true)
    t.set_input(\"grip\", true)
    t.set_input(\"grip\", false)
    t.set_input(\"stick\", Vector2(0.5, -0.25))
    return [same_value_fires, floats, pressed, released, vectors, last_vector]
";
    let v = array(call_check(src));

    assert_eq!(
        v[0],
        Variant::Int(1),
        "setting the SAME float twice must fire input_float_changed once, not twice",
    );
    assert_eq!(
        v[1],
        Variant::Int(2),
        "…and changing it fires again — edge, not level, and not suppressed entirely",
    );
    assert_eq!(
        v[2],
        Variant::Int(1),
        "a bool input fires button_pressed once, and the repeat is swallowed",
    );
    assert_eq!(
        v[3],
        Variant::Int(1),
        "flipping it to false fires button_released, NOT button_pressed again",
    );
    assert_eq!(
        v[4],
        Variant::Int(1),
        "a Vector2 input fires input_vector2_changed"
    );
    assert_eq!(
        v[5],
        Variant::Vector2(gdcore::math::Vector2::new(0.5, -0.25)),
        "the vector2 signal carries the vector — Godot names this parameter \
         `vector`, not `value` like its float sibling",
    );
}

/// Every tracker class seeds the `tracker_type` its Godot constructor sets,
/// and `XRTracker`'s `name` starts at `"Unknown"`.
///
/// ⚠️ The name default is not cosmetic. `XRServer.add_tracker` keys its
/// registry by tracker name, so with the old `""` default two trackers added
/// before `set_tracker_name` would silently overwrite each other.
#[test]
fn c_tracker_types_and_name_default_per_class() {
    let src = "\
extends Node

func check():
    return [XRControllerTracker.new().get_tracker_type(), XRHandTracker.new().get_tracker_type(), XRBodyTracker.new().get_tracker_type(), XRFaceTracker.new().get_tracker_type(), XRTracker.new().get_tracker_type(), XRPositionalTracker.new().get_tracker_type(), XRTracker.new().get_tracker_name()]
";
    let v = array(call_check(src));

    assert_eq!(
        v[0],
        Variant::Int(2),
        "XRControllerTracker -> TRACKER_CONTROLLER"
    );
    assert_eq!(
        v[1],
        Variant::Int(16),
        "XRHandTracker -> TRACKER_HAND (0x10)"
    );
    assert_eq!(
        v[2],
        Variant::Int(32),
        "XRBodyTracker -> TRACKER_BODY (0x20)"
    );
    assert_eq!(
        v[3],
        Variant::Int(64),
        "XRFaceTracker -> TRACKER_FACE (0x40)"
    );
    assert_eq!(
        v[4],
        Variant::Int(128),
        "plain XRTracker -> TRACKER_UNKNOWN (0x80)",
    );
    // XRPositionalTracker has no constructor of its own, so it inherits
    // XRTracker's TRACKER_UNKNOWN rather than getting a type of its own.
    assert_eq!(
        v[5],
        Variant::Int(128),
        "XRPositionalTracker inherits TRACKER_UNKNOWN — it sets no type itself",
    );
    assert_eq!(
        v[6],
        Variant::String("Unknown".to_string()),
        "xr_tracker.h:46 starts `name` at \"Unknown\", not the empty string",
    );
}

/// `invalidate_pose` keeps the transform and the pose **object**.
///
/// 🔑 Godot reuses one `XRPose` per pose name for the lifetime of the tracker:
/// `set_pose` looks the name up and mutates the existing object. A build that
/// minted a fresh handle per call would pass every value assertion in the
/// acceptance and still break `pose_changed` consumers holding a reference.
#[test]
fn d_invalidate_keeps_the_transform_and_the_same_pose_object() {
    let src = "\
extends Node

func check():
    var t = XRControllerTracker.new()
    var xf = Transform3D(Basis.IDENTITY, Vector3(5, 6, 7))
    t.set_pose(\"aim\", xf, Vector3(0, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_LOW)
    var first = t.get_pose(\"aim\")
    t.set_pose(\"aim\", Transform3D(Basis.IDENTITY, Vector3(8, 9, 10)), Vector3(0, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_HIGH)
    var second = t.get_pose(\"aim\")
    t.invalidate_pose(\"aim\")
    var third = t.get_pose(\"aim\")
    return [first == second, second == third, third.transform, t.has_pose(\"aim\"), third.has_tracking_data, first.has_tracking_data, t.get_pose(\"nope\")]
";
    let v = array(call_check(src));

    assert_eq!(
        v[0],
        Variant::Bool(true),
        "a second set_pose under the same name must reuse the SAME XRPose object",
    );
    assert_eq!(
        v[1],
        Variant::Bool(true),
        "invalidate_pose must not replace the object either",
    );
    let t = transform(&v[2]);
    assert!(
        (t.origin - gdcore::math::Vector3::new(8.0, 9.0, 10.0)).length() < 1e-5,
        "invalidate_pose must LEAVE the transform as it was — Godot's comment: \
         \"so controllers don't suddenly jump to origin\". Got {:?}",
        t.origin,
    );
    assert_eq!(
        v[3],
        Variant::Bool(true),
        "has_pose stays true after invalidate — the entry is kept, only the \
         tracking flag is cleared. A build that REMOVED the pose passes the \
         acceptance's `has_tracking_data == false` clause by returning null \
         and fails here",
    );
    assert_eq!(v[4], Variant::Bool(false), "…with has_tracking_data false");
    assert_eq!(
        v[5],
        Variant::Bool(false),
        "and the handle captured BEFORE the invalidate sees it too, because it \
         is the same object",
    );
    assert_eq!(
        v[6],
        Variant::Nil,
        "an unknown pose name is null, as in Godot's empty Ref<XRPose>",
    );
}

/// `pose_changed` fires on every `set_pose` and carries the pose object.
///
/// ⚠️ Unlike the input signals this one is **not** edge-triggered: Godot emits
/// unconditionally at the end of `set_pose`, because a pose arriving with the
/// same value is still a fresh sample from the runtime.
#[test]
fn e_pose_changed_fires_on_every_set_pose_and_carries_the_pose() {
    let src = "\
extends Node

var changes = 0
var seen_origin = Vector3(0, 0, 0)
var seen_name = \"\"

func _on_changed(pose):
    changes += 1
    seen_origin = pose.transform.origin
    seen_name = pose.name

func check():
    var t = XRControllerTracker.new()
    t.connect(\"pose_changed\", _on_changed)
    var xf = Transform3D(Basis.IDENTITY, Vector3(1, 2, 3))
    t.set_pose(\"grip\", xf, Vector3(0, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_HIGH)
    t.set_pose(\"grip\", xf, Vector3(0, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_HIGH)
    return [changes, seen_origin, seen_name]
";
    let v = array(call_check(src));

    assert_eq!(
        v[0],
        Variant::Int(2),
        "pose_changed is level, not edge — two set_pose calls, two emissions",
    );
    assert_eq!(
        v[1],
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        "the signal carries the pose, and the pose carries the transform",
    );
    assert_eq!(
        v[2],
        Variant::String("grip".to_string()),
        "set_pose names the XRPose after the action, as Godot's does",
    );
}

/// `description` and `profile` accessors, plus the `profile_changed` edge.
///
/// ⚠️ The accessors are `get_tracker_desc`/`set_tracker_desc` — abbreviated,
/// and *not* derivable from the `description` member name. The signal
/// parameter is named `role`, not `profile`.
#[test]
fn f_tracker_desc_and_profile_round_trip_and_profile_changed_is_an_edge() {
    let src = "\
extends Node

var profile_events = 0
var last_role = \"\"

func _on_profile(role):
    profile_events += 1
    last_role = role

func check():
    var t = XRControllerTracker.new()
    t.connect(\"profile_changed\", _on_profile)
    t.set_tracker_desc(\"Left hand controller\")
    t.set_tracker_profile(\"/interaction_profiles/oculus/touch_controller\")
    t.set_tracker_profile(\"/interaction_profiles/oculus/touch_controller\")
    return [t.get_tracker_desc(), t.get_tracker_profile(), profile_events, last_role]
";
    let v = array(call_check(src));

    assert_eq!(v[0], Variant::String("Left hand controller".to_string()));
    assert_eq!(
        v[1],
        Variant::String("/interaction_profiles/oculus/touch_controller".to_string()),
    );
    assert_eq!(
        v[2],
        Variant::Int(1),
        "set_tracker_profile only emits when the profile actually changes",
    );
    assert_eq!(
        v[3],
        Variant::String("/interaction_profiles/oculus/touch_controller".to_string()),
        "profile_changed carries the profile string",
    );
}

/// `XRServer.world_scale` is real server state with Godot's default and clamp.
///
/// ⚠️ Default **1.0**, not 0.0 — a zero default would collapse every adjusted
/// pose origin onto the origin point, and every "reflects world_scale" test
/// written against a fresh server would then pass for the wrong reason.
#[test]
fn g_xr_server_world_scale_defaults_to_one_and_clamps() {
    let src = "\
extends Node

func check():
    var initial = XRServer.get_world_scale()
    XRServer.set_world_scale(3.5)
    var set_ok = XRServer.get_world_scale()
    XRServer.set_world_scale(0.0)
    var low = XRServer.get_world_scale()
    XRServer.set_world_scale(5000.0)
    var high = XRServer.get_world_scale()
    return [initial, set_ok, low, high]
";
    let v = array(call_check(src));

    assert!(
        (float(&v[0]) - 1.0).abs() < 1e-9,
        "a fresh XRServer reports world_scale 1.0, got {:?}",
        v[0],
    );
    assert!((float(&v[1]) - 3.5).abs() < 1e-6);
    assert!(
        (float(&v[2]) - 0.01).abs() < 1e-9,
        "XRServer::set_world_scale clamps below at 0.01, got {:?}",
        v[2],
    );
    assert!(
        (float(&v[3]) - 1000.0).abs() < 1e-6,
        "…and above at 1000.0, got {:?}",
        v[3],
    );
}

/// The pose handle family must not answer for another family's handles.
///
/// ⚠️ `is_*_handle` is a masked tag compare, so a tag byte reused by another
/// family would make both predicates claim the same objects and misroute every
/// call — silently, with no error anywhere. `0xE0` was the last free byte in
/// the `0xE0..0xEF` block; this is the assertion that notices if someone later
/// reuses it.
#[test]
fn h_pose_handles_do_not_collide_with_tracker_handles() {
    let src = "\
extends Node

func check():
    var t = XRControllerTracker.new()
    t.set_pose(\"default\", Transform3D.IDENTITY, Vector3(0, 0, 0), Vector3(0, 0, 0), XRPose.XR_TRACKING_CONFIDENCE_HIGH)
    var pose = t.get_pose(\"default\")
    var direct = XRPose.new()
    direct.transform = Transform3D(Basis.IDENTITY, Vector3(1, 1, 1))
    return [pose != t, direct != pose, direct.transform, t.get_tracker_type(), pose.tracking_confidence]
";
    let v = array(call_check(src));

    assert_eq!(
        v[0],
        Variant::Bool(true),
        "a pose handle and its tracker handle are different objects",
    );
    assert_eq!(
        v[1],
        Variant::Bool(true),
        "a standalone XRPose.new() is not the tracker's pose",
    );
    let t = transform(&v[2]);
    assert!(
        (t.origin - gdcore::math::Vector3::new(1.0, 1.0, 1.0)).length() < 1e-5,
        "XRPose.transform is writable as a property, got {:?}",
        t.origin,
    );
    // 🔑 The collision check with teeth: if the two families shared a tag,
    // whichever predicate ran first would swallow both handles and these two
    // reads would answer from the wrong table (Nil, or the wrong number).
    assert_eq!(
        v[3],
        Variant::Int(2),
        "the tracker still dispatches as a tracker after poses exist",
    );
    assert_eq!(
        v[4],
        Variant::Int(2),
        "and the pose still dispatches as a pose",
    );
}
