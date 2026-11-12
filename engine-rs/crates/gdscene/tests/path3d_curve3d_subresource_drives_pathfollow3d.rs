//! pat-s3rrv.91 — PathFollow3D transform semantics: orientation, h/v offsets,
//! `progress_ratio`.
//!
//! ⚠️ **The bead's "Patina today" is stale.** It says the loader skips Curve3D
//! and that a PathFollow3D therefore never moves. Both halves already landed
//! (pat-s3rrv.83): `packed_scene.rs:732` decodes the bezier triples and `closed`
//! and writes `curve_points`, and `curve3d_path_follow3d_scene_wiring.rs` proves
//! the follower advances. What was genuinely missing is what this file covers —
//! the TRANSFORM.
//!
//! 🔑 **Every follower faced backwards.** The driver derived yaw as
//! `atan2(t.x, t.z)`, described in the source as an "internally consistent
//! Patina convention" — i.e. +Z forward. Godot puts **-Z along the tangent**
//! (`path_3d.cpp:290-305`, `correct_posture` at :371-400), which is
//! `atan2(-t.x, -t.z)`: **exactly π away on every tangent**. The existing
//! PathFollow3D test asserted position only, so nothing could see it.
//!
//! Straight-line curves are used deliberately. On a bezier the tangent at the
//! sampled progress is itself a derived quantity, so a wrong tangent and a wrong
//! convention could cancel; on a straight segment the tangent is known exactly
//! from the fixture, and the assertions are about the convention alone.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::MainLoop;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A straight Curve3D whose tangent is exactly `+X`, with rotation left at
/// Godot's default (absent `rotation_mode`, which is ROTATION_XYZ = 3).
const ALONG_X: &str = r#"[gd_scene load_steps=2 format=3]

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

/// Tangent exactly `+Z` — a second, independent yaw.
const ALONG_Z: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10),
"tilts": PackedFloat32Array(0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// A 45° climb, so PITCH is non-zero and cannot be confused with yaw.
const CLIMBING: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 10, 0),
"tilts": PackedFloat32Array(0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// The `+X` path again, with offsets and `rotation_mode = 1` (ROTATION_Y).
const OFFSET_ROTATED: &str = r#"[gd_scene load_steps=2 format=3]

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
rotation_mode = 1
h_offset = 2.0
v_offset = 3.0
"#;

/// The same offsets with `rotation_mode = 0` (ROTATION_NONE), where Godot's
/// `translate_local` runs against an identity basis and becomes world x/y.
const OFFSET_UNROTATED: &str = r#"[gd_scene load_steps=2 format=3]

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
rotation_mode = 0
h_offset = 2.0
v_offset = 3.0
"#;

fn load(tscn: &str) -> (MainLoop, NodeId) {
    // ⚠️ Register FIRST: nextest gives each test its own process, so the ClassDB
    // is empty until something seeds it.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let follow = tree
        .find_child(root, "Follow", true)
        .expect("PathFollow3D exists");
    (MainLoop::new(tree), follow)
}

fn vec3_prop(ml: &MainLoop, id: NodeId, name: &str) -> Vector3 {
    match ml.tree().get_node(id).map(|n| n.get_property(name)) {
        Some(Variant::Vector3(v)) => v,
        other => panic!("{name} should be a Vector3, got {other:?}"),
    }
}

fn float_prop(ml: &MainLoop, id: NodeId, name: &str) -> f32 {
    match ml.tree().get_node(id).map(|n| n.get_property(name)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        other => panic!("{name} should be a number, got {other:?}"),
    }
}

fn step(ml: &mut MainLoop, frames: usize) {
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
}

/// The follower's local **-Z axis**, rebuilt from the euler the driver wrote.
///
/// Godot's rotation order is YXZ, and the driver locks roll to zero, so the
/// basis is `Ry(yaw) * Rx(pitch)`; its third column is `(sy*cp, -sp, cy*cp)`
/// and the forward axis is the negation of that.
///
/// Stated this way on purpose: it turns the assertions into the acceptance
/// criterion's own sentence — "-Z faces the tangent" — instead of a claim about
/// one particular euler encoding.
fn forward_axis(euler: Vector3) -> Vector3 {
    let (sin_y, cos_y) = euler.y.sin_cos();
    let (sin_x, cos_x) = euler.x.sin_cos();
    Vector3::new(-sin_y * cos_x, sin_x, -cos_y * cos_x)
}

fn approx_vec(got: Vector3, want: Vector3, what: &str) {
    assert!(
        (got - want).length() < 1e-3,
        "{what}: got {got:?}, expected {want:?}"
    );
}

/// **The acceptance test.** The follower's -Z axis points along the tangent.
///
/// 🔑 The old convention put **+Z** along it, so `forward_axis` came out as the
/// exact negation — `(-1, 0, 0)` here instead of `(1, 0, 0)`. Both are unit
/// vectors along the path, which is why every "does it rotate?" check passed.
#[test]
fn path3d_curve3d_subresource_drives_pathfollow3d() {
    let (mut ml, follow) = load(ALONG_X);
    step(&mut ml, 30);

    let euler = vec3_prop(&ml, follow, "rotation");
    approx_vec(
        forward_axis(euler),
        Vector3::new(1.0, 0.0, 0.0),
        "-Z must face the +X tangent",
    );
    // The raw yaw, named explicitly so the regression is unmistakable: Godot's
    // is -PI/2 and the convention this replaces gave +PI/2.
    assert!(
        (euler.y + std::f32::consts::FRAC_PI_2).abs() < 1e-3,
        "yaw must be -PI/2 for a +X tangent (the old convention gave +PI/2), got {}",
        euler.y
    );

    // ⚠️ Rotation happens with NO `rotation_mode` in the .tscn, because Godot's
    // default is ROTATION_XYZ (3). The driver used to fall to NONE when the
    // property was absent, so the common case did not rotate at all.
    assert!(
        euler.y.abs() > 1e-6,
        "an absent rotation_mode must default to XYZ, not NONE"
    );
}

/// A second, independent tangent — `+Z`, where yaw is PI rather than -PI/2.
///
/// One tangent cannot distinguish a sign error from an axis swap; two can.
#[test]
fn a_second_tangent_pins_the_same_convention() {
    let (mut ml, follow) = load(ALONG_Z);
    step(&mut ml, 30);

    let euler = vec3_prop(&ml, follow, "rotation");
    approx_vec(
        forward_axis(euler),
        Vector3::new(0.0, 0.0, 1.0),
        "-Z must face the +Z tangent",
    );
}

/// Pitch: a 45° climb must tip the forward axis up, not just yaw it.
///
/// Both flat fixtures above have `pitch == 0`, so a driver that dropped the
/// pitch term entirely would pass them.
#[test]
fn a_climbing_curve_pitches_the_follower_up() {
    let (mut ml, follow) = load(CLIMBING);
    step(&mut ml, 30);

    let euler = vec3_prop(&ml, follow, "rotation");
    let want = Vector3::new(
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
        0.0,
    );
    approx_vec(
        forward_axis(euler),
        want,
        "-Z must face the 45-degree tangent",
    );
    assert!(
        (euler.x - std::f32::consts::FRAC_PI_4).abs() < 1e-3,
        "pitch must be +PI/4 for a 45-degree climb, got {}",
        euler.x
    );
}

/// ⚠️ `h_offset`/`v_offset` shift along the follower's OWN axes, not the world's.
///
/// Godot applies them as `t.translate_local(Vector3(h, v, 0))` AFTER the posture
/// correction (`path_3d.cpp:310`). On a `+X` path rotated about Y, the local x
/// axis points along `+Z`, so `h_offset` must move the follower SIDEWAYS off the
/// path — the property's whole purpose. A driver that added them in world space
/// would slide it ALONG the path instead, which still "applies the offset" and
/// still moves the node.
#[test]
fn the_offsets_are_applied_in_the_followers_own_frame() {
    let (mut ml, rotated) = load(OFFSET_ROTATED);
    step(&mut ml, 30);
    let with_offset = vec3_prop(&ml, rotated, "position");

    let (mut plain_ml, plain) = load(ALONG_X);
    step(&mut plain_ml, 30);
    let without = vec3_prop(&plain_ml, plain, "position");

    // local x = (cos yaw, 0, -sin yaw) = (0, 0, 1) at yaw = -PI/2; local y = +Y.
    approx_vec(
        with_offset,
        Vector3::new(without.x, without.y + 3.0, without.z + 2.0),
        "h_offset must go sideways (+Z) and v_offset up (+Y)",
    );
    // Explicitly NOT along the path: the fixture would look 'offset applied'
    // either way, so the along-path component has to be pinned as unchanged.
    assert!(
        (with_offset.x - without.x).abs() < 1e-3,
        "h_offset must not slide the follower along its own +X path, moved {}",
        with_offset.x - without.x
    );
}

/// With ROTATION_NONE the basis is the identity, so the offsets are world x/y.
///
/// This is the control that stops the frame-relative test above from being
/// satisfiable by a hardcoded axis mapping.
#[test]
fn without_rotation_the_offsets_are_world_axes() {
    let (mut ml, unrotated) = load(OFFSET_UNROTATED);
    step(&mut ml, 30);
    let with_offset = vec3_prop(&ml, unrotated, "position");

    let (mut plain_ml, plain) = load(ALONG_X);
    step(&mut plain_ml, 30);
    let without = vec3_prop(&plain_ml, plain, "position");

    approx_vec(
        with_offset,
        Vector3::new(without.x + 2.0, without.y + 3.0, without.z),
        "with no rotation the offsets are plain world x/y",
    );
}

/// `progress_ratio` is republished every tick, as the 2D branch already did.
///
/// ⚠️ Before this it was never written for a PathFollow3D at all, so a script
/// reading it got `Nil` for ever — and writing it did nothing, because the
/// driver only ever read `progress`.
#[test]
fn progress_ratio_is_published_and_tracks_progress() {
    let (mut ml, follow) = load(ALONG_X);
    step(&mut ml, 30);

    let progress = float_prop(&ml, follow, "progress");
    let ratio = float_prop(&ml, follow, "progress_ratio");
    assert!(
        progress > 0.0,
        "the follower must have advanced, got {progress}"
    );
    // The curve is 10 units long, so the two views must agree on that scale.
    assert!(
        (ratio - progress / 10.0).abs() < 1e-2,
        "progress_ratio must be progress / baked_length: ratio {ratio}, progress {progress}"
    );
}
