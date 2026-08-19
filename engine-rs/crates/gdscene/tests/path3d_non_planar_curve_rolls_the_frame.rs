//! pat-s3rrv.91 item (1) — up vectors on a NON-PLANAR curve, at the scene level.
//!
//! ⚠️ **Up vectors only change the output where the curve has torsion.** Every
//! existing scene fixture on this bead is planar — a straight run along an axis,
//! or a climb in one plane — and on those the parallel-transported up vector and
//! a naive world-`+Y` agree exactly. So the transport branch was covered at the
//! data level (the bake tests) and only for no-regression at the scene level.
//! This is the fixture where the two answers diverge.
//!
//! ⚠️ Asserted through POSITION, not Euler angles. `v_offset` is applied as
//! Godot's `translate_local(Vector3(h_offset, v_offset, 0))`, so the follower's
//! displacement when `v_offset` goes from 0 to 1 **is** the frame's up column —
//! one unit of it. That keeps the test independent of how a basis is decomposed
//! into `rotation`, which is a convention the sibling tests already pin and
//! which would otherwise be re-asserted here by accident.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::MainLoop;
use gdscene::SceneTree;
use gdvariant::Variant;

/// Four points of a helix: a quarter turn about Z per segment while climbing.
/// Consecutive segments are not coplanar, which is the whole point.
const HELIX_POINTS: &str = "0, 0, 0, 0, 0, 0, 1, 0, 0, \
                            0, 0, 0, 0, 0, 0, 0, 1, 1, \
                            0, 0, 0, 0, 0, 0, -1, 0, 2, \
                            0, 0, 0, 0, 0, 0, 0, -1, 3";

/// A straight run along +X — planar, and the control for the helix below.
const STRAIGHT_POINTS: &str = "0, 0, 0, 0, 0, 0, 0, 0, 0, \
                               0, 0, 0, 0, 0, 0, 10, 0, 0";

fn scene(points: &str, tilts: &str, progress: f32, v_offset: f32) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {{
"points": PackedVector3Array({points}),
"tilts": PackedFloat32Array({tilts})
}}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = {progress}
v_offset = {v_offset}
"#
    )
}

fn follower_position(tscn: &str) -> Vector3 {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let id: NodeId = tree
        .get_node_by_path("/root/Main/Path/Follow")
        .expect("the PathFollow3D is in the scene");
    let mut ml = MainLoop::new(tree);
    // One frame is enough: the driver writes the transform every frame.
    ml.step(1.0 / 60.0);
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector3(v)) => v,
        other => panic!("the follower's position should be a Vector3, got {other:?}"),
    }
}

/// The frame's up column at `progress`, measured as the displacement one unit of
/// `v_offset` produces.
fn up_at(points: &str, tilts: &str, progress: f32) -> Vector3 {
    let base = follower_position(&scene(points, tilts, progress, 0.0));
    let lifted = follower_position(&scene(points, tilts, progress, 1.0));
    Vector3::new(lifted.x - base.x, lifted.y - base.y, lifted.z - base.z)
}

fn length(v: Vector3) -> f32 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

fn dot(a: Vector3, b: Vector3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// 🔑 On a torsioned curve the frame's up is transported, not world `+Y`.
#[test]
fn path3d_non_planar_curve_rolls_the_frame() {
    let progress = 2.0;
    let up = up_at(HELIX_POINTS, "0, 0, 0, 0", progress);

    assert!(
        (length(up) - 1.0).abs() < 1e-2,
        "one unit of v_offset must move the follower exactly one unit along its \
         own up axis; got a displacement of {} — if this is 0 the offset never \
         reached the 3D driver",
        length(up),
    );

    // The tangent, measured the same way: two nearby samples of the curve.
    let here = follower_position(&scene(HELIX_POINTS, "0, 0, 0, 0", progress, 0.0));
    let ahead = follower_position(&scene(HELIX_POINTS, "0, 0, 0, 0", progress + 0.05, 0.0));
    let mut tangent = Vector3::new(ahead.x - here.x, ahead.y - here.y, ahead.z - here.z);
    let t_len = length(tangent);
    assert!(
        t_len > 1e-4,
        "the follower must actually advance between the two samples, or there is \
         no tangent to be perpendicular to",
    );
    tangent = Vector3::new(tangent.x / t_len, tangent.y / t_len, tangent.z / t_len);

    // 🔑 The discriminator. A frame carrying world +Y as its up is NOT
    // perpendicular to a tangent that climbs and turns — only a transported up
    // vector is. This is the assertion a planar fixture can never make.
    assert!(
        dot(up, tangent).abs() < 0.05,
        "the up axis must stay perpendicular to the tangent along a torsioned \
         curve; got dot = {} (up {:?}, tangent {:?}). A naive world-up frame \
         fails here and passes every planar fixture on this bead",
        dot(up, tangent),
        up,
        tangent,
    );
    assert!(
        length(Vector3::new(up.x, up.y - 1.0, up.z)) > 0.1,
        "…and it must have actually rotated away from world +Y, got {up:?}",
    );
}

/// 🔑 The control: on a PLANAR curve the same measurement gives world `+Y`.
///
/// ⚠️ This is what proves the helix result comes from the curve's torsion rather
/// than from the measurement being noisy or the up axis being arbitrary. Both
/// tests read the identical quantity; only the fixture's shape differs.
#[test]
fn path3d_planar_curve_keeps_world_up() {
    let up = up_at(STRAIGHT_POINTS, "0, 0", 3.0);

    assert!(
        (length(up) - 1.0).abs() < 1e-2,
        "the control must measure a unit up axis too, got {}",
        length(up),
    );
    assert!(
        up.y > 0.9,
        "a straight, untilted run along +X keeps world +Y as its up; got {up:?}",
    );
}
