//! pat-s3rrv.91 — Curve3D `tilts` reach PathFollow3D and roll it about the path.
//!
//! The loader used to drop `_data["tilts"]` on purpose, with the note *"inventing
//! a tilt would be worse than [none]"*. That was right while nothing consumed a
//! tilt. It is now parsed, baked index-for-index with the polyline, carried on
//! the Path3D as `curve_tilts`, and applied as Godot applies it.
//!
//! 🔑 **THE DEFINING PROPERTY OF A TILT IS WHAT IT DOES *NOT* CHANGE.** It is a
//! twist about the tangent, so the follower's forward axis must be **identical**
//! with and without it — only the up axis moves. Every test below asserts both
//! halves, because an implementation that rotated the whole frame would still
//! "apply the tilt" and still look like motion.
//!
//! Godot (`path_3d.cpp:299-305`), applied AFTER `correct_posture` and on the
//! LEFT:
//!
//! ```text
//! if (tilt_enabled) {
//!     const real_t tilt = c->sample_baked_tilt(progress);
//!     const Basis twist(tangent, tilt);
//!     t.basis = twist * t.basis;
//! }
//! ```

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::MainLoop;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A straight `+X` curve with a CONSTANT tilt of +PI/2, so the sampled angle is
/// the same wherever the follower has got to — interpolation is tested
/// separately, and this fixture must not depend on it.
const CONSTANT_TILT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0),
"tilts": PackedFloat32Array(1.5707964, 1.5707964)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// The same curve with NO tilt — the baseline the others are measured against.
const NO_TILT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0),
"tilts": PackedFloat32Array(0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// Tilt ramping 0 -> PI/2 along the curve, so the angle depends on `progress`.
///
/// 🔑 **THE HANDLES ARE `+/- 10/3` ON PURPOSE — ZERO HANDLES ARE NOT A UNIFORM
/// LINE.** A cubic bezier whose handles both sit on their endpoints reduces to
/// `10 * (3t^2 - 2t^3)`, a SMOOTHSTEP: it passes x = 1.5625 at t = 0.25 rather
/// than 2.5, bunching samples at the ends. Tilt interpolates in the bezier
/// PARAMETER (Godot's `curve.cpp:1749`) while the follower is placed by ARC
/// LENGTH, so on such a curve 40% of the distance is 43.3% of the parameter and
/// the sampled tilt is `0.433 * PI/2`, not `0.4 * PI/2`.
///
/// Spacing the handles at a third of the segment makes `x(t) = 10t` exactly, so
/// parameter and arc length coincide and the number in the assertion means what
/// it says. ⚠️ Do not "simplify" these back to zero — the other fixtures here
/// can afford zero handles only because their tilt is CONSTANT, which makes the
/// parameterisation irrelevant to them.
const RAMPED_TILT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 3.3333333, 0, 0, 0, 0, 0, -3.3333333, 0, 0, 0, 0, 0, 10, 0, 0),
"tilts": PackedFloat32Array(0, 1.5707964)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// The constant tilt with `tilt_enabled = false` — the control.
const TILT_DISABLED: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0),
"tilts": PackedFloat32Array(1.5707964, 1.5707964)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
tilt_enabled = false
"#;

fn load(tscn: &str) -> (MainLoop, NodeId, NodeId) {
    // ⚠️ Register FIRST: nextest gives each test its own process, so the ClassDB
    // is empty until something seeds it.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let path = tree.find_child(root, "Path", true).expect("Path3D exists");
    let follow = tree
        .find_child(root, "Follow", true)
        .expect("PathFollow3D exists");
    (MainLoop::new(tree), path, follow)
}

/// Runs 30 frames at `path_speed = 8`, i.e. `progress == 4.0` along a 10-unit
/// curve, and returns the follower's rotation euler.
fn euler_after_30_frames(tscn: &str) -> Vector3 {
    let (mut ml, _, follow) = load(tscn);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    match ml
        .tree()
        .get_node(follow)
        .map(|n| n.get_property("rotation"))
    {
        Some(Variant::Vector3(v)) => v,
        other => panic!("rotation should be a Vector3, got {other:?}"),
    }
}

/// Column 2 of `Ry(yaw) * Rx(pitch) * Rz(roll)`, negated — the follower's
/// forward (-Z) axis.
fn forward_axis(e: Vector3) -> Vector3 {
    let (sin_y, cos_y) = e.y.sin_cos();
    let (sin_x, cos_x) = e.x.sin_cos();
    Vector3::new(-sin_y * cos_x, sin_x, -cos_y * cos_x)
}

/// Column 1 of the same basis — the follower's up (+Y) axis, which is the axis
/// a tilt actually moves.
fn up_axis(e: Vector3) -> Vector3 {
    let (sin_y, cos_y) = e.y.sin_cos();
    let (sin_x, cos_x) = e.x.sin_cos();
    let (sin_z, cos_z) = e.z.sin_cos();
    Vector3::new(
        cos_y * -sin_z + sin_y * sin_x * cos_z,
        cos_x * cos_z,
        sin_y * sin_z + cos_y * sin_x * cos_z,
    )
}

fn approx(got: Vector3, want: Vector3, what: &str) {
    assert!(
        (got - want).length() < 1e-3,
        "{what}: got {got:?}, expected {want:?}"
    );
}

/// **The acceptance test.** A tilted curve rolls the follower about the path.
///
/// The up axis turns a quarter turn from `+Y` to `+Z`, and the forward axis does
/// not move at all — that pair is what makes it a TILT rather than a rotation.
#[test]
fn path3d_tilt_rolls_the_follower_about_the_path() {
    let tilted = euler_after_30_frames(CONSTANT_TILT);
    let plain = euler_after_30_frames(NO_TILT);

    approx(
        up_axis(plain),
        Vector3::new(0.0, 1.0, 0.0),
        "with no tilt the follower is upright",
    );
    approx(
        up_axis(tilted),
        Vector3::new(0.0, 0.0, 1.0),
        "a +PI/2 tilt about the +X tangent turns up from +Y to +Z",
    );
    // 🔑 The half that catches a whole-frame rotation masquerading as a tilt.
    approx(
        forward_axis(tilted),
        forward_axis(plain),
        "a twist about the tangent CANNOT move the forward axis",
    );
}

/// ⚠️ The twist is LEFT-multiplied, about the world-space tangent.
///
/// Right-multiplying instead would twist about the follower's own local Z —
/// which for this frame is the negated tangent, so forward stays put there too
/// and only the SIGN of the roll differs. Pinning up as exactly `+Z` rather than
/// `-Z` is therefore the assertion that separates the two.
#[test]
fn the_twist_direction_follows_godots_left_multiply() {
    let up = up_axis(euler_after_30_frames(CONSTANT_TILT));
    assert!(
        up.z > 0.9,
        "a positive tilt must roll up toward +Z, not -Z; got {up:?}"
    );
}

/// Tilt is interpolated along the curve, not applied whole.
///
/// The ramp runs 0 -> PI/2 over the 10-unit curve and the follower is at
/// `progress == 4`, so the angle is `0.4 * PI/2` and up is `(0, cos, sin)` of
/// it. A constant-tilt implementation would give `(0, 0, 1)` here and pass the
/// acceptance test above.
///
/// ⚠️ This depends on `RAMPED_TILT`'s handles making the curve uniformly
/// parameterised — see the note on that fixture. With the zero handles the
/// other fixtures use, the correct answer here is `0.433 * PI/2`, and an
/// implementation matching Godot exactly would "fail" this assertion.
#[test]
fn tilt_is_interpolated_along_the_curve() {
    let ramped = euler_after_30_frames(RAMPED_TILT);
    approx(
        up_axis(ramped),
        Vector3::new(0.0, 0.809_017, 0.587_785),
        "at 40% along a 0 -> PI/2 ramp the tilt is 0.4 * PI/2",
    );
}

/// `tilt_enabled = false` switches it off — and its DEFAULT is true.
///
/// ⚠️ Both directions matter: every other fixture here omits the property, so if
/// an absent `tilt_enabled` read as false they would all sit upright and the
/// acceptance test would fail — while a version that ignored the property
/// entirely passes everything except this one.
#[test]
fn tilt_enabled_defaults_to_true_and_can_be_switched_off() {
    let disabled = euler_after_30_frames(TILT_DISABLED);
    approx(
        up_axis(disabled),
        Vector3::new(0.0, 1.0, 0.0),
        "tilt_enabled = false must leave the follower upright",
    );

    // The same curve with the property absent DOES tilt, which is what proves
    // the default is true rather than the tilt simply never arriving.
    let enabled_by_default = euler_after_30_frames(CONSTANT_TILT);
    assert!(
        (up_axis(enabled_by_default) - up_axis(disabled)).length() > 0.5,
        "an absent tilt_enabled must default to TRUE, so these must differ"
    );
}

/// The loader must actually put the baked tilts on the node, index-for-index
/// with the polyline.
///
/// ⚠️ A length mismatch between `curve_points` and `curve_tilts` would rotate
/// the follower by an angle belonging to a different point on the path —
/// plausible motion, wrong everywhere — so the driver drops mismatched tilts and
/// this pins that they match in the first place.
#[test]
fn the_loader_bakes_tilts_alongside_the_polyline() {
    let (ml, path, _) = load(CONSTANT_TILT);
    let node = ml.tree().get_node(path).expect("Path3D is in the tree");

    let points = match node.get_property("curve_points") {
        Variant::PackedVector3Array(v) => v,
        other => panic!("curve_points should be a PackedVector3Array, got {other:?}"),
    };
    let tilts = match node.get_property("curve_tilts") {
        Variant::PackedFloat32Array(v) => v,
        other => panic!(
            "curve_tilts should be a PackedFloat32Array — the loader used to drop \
             _data[\"tilts\"] entirely; got {other:?}"
        ),
    };
    assert_eq!(
        points.len(),
        tilts.len(),
        "the two arrays must describe the same samples"
    );
    assert!(
        tilts
            .iter()
            .all(|t| (t - std::f32::consts::FRAC_PI_2).abs() < 1e-4),
        "a constant authored tilt must bake to a constant array, got {tilts:?}"
    );
}
