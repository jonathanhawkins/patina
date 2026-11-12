//! pat-s3rrv.83 — a `.tscn` Curve3D bakes on load and drives PathFollow3D.
//!
//! # What was missing
//!
//! The `.tscn` curve bake (pat-0q2kx.104) handled Curve2D and **explicitly
//! skipped Curve3D**:
//!
//! ```text
//! if !is_2d {
//!     // Curve3D bakes through the 3D path driver; only the 2D half is
//!     // in scope here, and guessing at the other would be worse than
//!     // leaving it untouched.
//!     continue;
//! }
//! ```
//!
//! So a Godot-authored `curve = SubResource("Curve3D_…")` was parsed and
//! dropped, `curve_points` never appeared on the Path3D, and `PathFollow3D` sat
//! still — even though `process_path_follows` had handled the 3D case all
//! along. `Curve3D` was also absent from ClassDB while `Curve2D` and
//! `PathFollow3D` were both registered.
//!
//! # Format, from the vendored C++
//!
//! `Curve3D::_set_data` (`curve.cpp:2278-2299`) takes the SAME in/out/position
//! triple as the 2D form, over a `PackedVector3Array`, plus a `tilts` array —
//! `ERR_FAIL_COND(pc % 3 != 0)` and `points[i].in/out/position = r[i*3+0..2]`.
//!
//! ⚠️ `tilts` is present in the fixture because Godot always writes it, but the
//! bake deliberately ignores it: tilt rotates the follower ABOUT the curve and
//! does not move the polyline. Inventing a rotation from it would be a second
//! behaviour smuggled into a geometry change.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

/// A bezier from (0,0,0) to (10,0,10) bulging through +x.
///
/// Triples are (in_handle, out_handle, position); the first point's in-handle
/// and the last point's out-handle are unused by the bake, as in Godot.
const PATH_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
_data = {
"points": PackedVector3Array(0, 0, 0, 6, 0, 0, 0, 0, 0, -2, 0, -4, 0, 0, 0, 10, 0, 10),
"tilts": PackedFloat32Array(0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// The same scene with no curve — the control proving the motion below comes
/// from the baked curve and not from any incidental default.
const NO_CURVE_TSCN: &str = r#"[gd_scene load_steps=1 format=3]

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

fn load(tscn: &str) -> (MainLoop, NodeId, NodeId) {
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

fn position(ml: &MainLoop, id: NodeId) -> Vector3 {
    match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector3(v)) => v,
        _ => Vector3::new(0.0, 0.0, 0.0),
    }
}

fn baked(ml: &MainLoop, path: NodeId) -> Vec<Vector3> {
    match ml
        .tree()
        .get_node(path)
        .map(|n| n.get_property("curve_points"))
    {
        Some(Variant::PackedVector3Array(v)) => v,
        _ => Vec::new(),
    }
}

/// **The acceptance test.**
#[test]
fn curve3d_path_follow3d_scene_wiring() {
    // ⚠️ Register FIRST. nextest gives each test its own process, so the ClassDB
    // is empty until something seeds it — asserting before `load()` (which
    // registers) tested an empty registry and failed for a reason that had
    // nothing to do with the registration under test.
    gdobject::class_db::register_all_classes();
    assert!(
        gdobject::class_db::class_exists("Curve3D"),
        "Curve3D must be in ClassDB — a .tscn can carry one, and Curve2D and \
         PathFollow3D were both registered while it was not"
    );

    let (mut ml, path, follow) = load(PATH_TSCN);

    // The bake happened on LOAD, before any frame ran.
    let poly = baked(&ml, path);
    assert!(
        poly.len() > 2,
        "the Curve3D sub-resource must bake into a sampled polyline on load; \
         got {} points. Empty means the `curve = SubResource(..)` was parsed \
         and dropped, which is the bug this closes",
        poly.len()
    );
    assert!(
        (poly[0] - Vector3::new(0.0, 0.0, 0.0)).length() < 1e-3,
        "the polyline starts at the first control point, got {:?}",
        poly[0]
    );
    assert!(
        (poly[poly.len() - 1] - Vector3::new(10.0, 0.0, 10.0)).length() < 1e-3,
        "…and ends at the last, got {:?}",
        poly[poly.len() - 1]
    );
    // ⚠️ The bulge is what proves the BEZIER was sampled rather than the control
    // points being copied straight through: the straight line from (0,0,0) to
    // (10,0,10) has x == z everywhere, and the +x out-handle pushes the middle
    // off it.
    let bulged = poly.iter().any(|p| p.x - p.z > 0.5);
    assert!(
        bulged,
        "a sampled bezier must bow away from the chord; got {poly:?}"
    );

    let start = position(&ml, follow);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let moved = position(&ml, follow);
    assert!(
        (moved - start).length() > 0.1,
        "PathFollow3D must advance along the baked curve across frames — \
         start {start:?}, after 30 frames {moved:?}"
    );

    // …and it is ON the curve, not merely somewhere else: the nearest baked
    // sample must be close. Sampling density (16 steps/segment) bounds how far
    // a point on the curve can sit from the nearest sample.
    let nearest = poly
        .iter()
        .map(|p| (*p - moved).length())
        .fold(f32::INFINITY, f32::min);
    assert!(
        nearest < 0.75,
        "the follower must sit on the curve; nearest baked sample is {nearest} \
         away from {moved:?}"
    );
}

/// The control: with no curve there is nothing to follow, so the node must not
/// drift. Without this, "position changed" could be satisfied by any unrelated
/// per-frame motion.
#[test]
fn without_a_curve_the_follower_does_not_move() {
    let (mut ml, path, follow) = load(NO_CURVE_TSCN);
    assert!(
        baked(&ml, path).is_empty(),
        "no curve sub-resource means no baked polyline"
    );
    let start = position(&ml, follow);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        (position(&ml, follow) - start).length() < 1e-3,
        "a PathFollow3D with no curve must stay put"
    );
}

/// A closed 3-point curve loops back to its first point.
///
/// Handles are all zero, so each segment is a straight line and the baked
/// polyline is exactly the triangle — which makes "does it return to the
/// start?" a clean question with no sampling tolerance to argue about.
const CLOSED_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_C"]
closed = true
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 10),
"tilts": PackedFloat32Array(0, 0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_C")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

/// ⚠️ TWO control points, `closed = true`. Godot does **not** close this one.
const CLOSED_TWO_POINT_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_C2"]
closed = true
_data = {
"points": PackedVector3Array(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0),
"tilts": PackedFloat32Array(0, 0)
}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_C2")

[node name="Follow" type="PathFollow3D" parent="Path"]
progress = 0.0
path_speed = 8.0
"#;

#[test]
fn a_closed_curve_loops_back_to_its_first_point() {
    let (ml, path, _) = load(CLOSED_TSCN);
    let poly = baked(&ml, path);
    assert!(poly.len() > 3, "the closed curve must bake, got {poly:?}");
    let (first, last) = (poly[0], poly[poly.len() - 1]);
    assert!(
        (last - first).length() < 1e-3,
        "`closed = true` connects the last control point back to the first, so \
         the polyline ends where it began — first {first:?}, last {last:?}. \
         Ending at (5, 0, 10) means the closing segment was never baked"
    );
    // …and it really went the long way round rather than collapsing: the
    // triangle's far corner must still appear.
    assert!(
        poly.iter().any(|p| (p.z - 10.0).abs() < 0.5),
        "the closing segment is added to the curve, not substituted for it"
    );
}

/// ⚠️ The rule's own boundary. `closed = true` on a TWO-point curve must be
/// ignored — "if the curve has more than 2 control points"
/// (`doc/classes/Curve3D.xml:207`).
///
/// An implementation that closes unconditionally passes the test above and
/// fails here, which is the only reason this case exists.
#[test]
fn a_closed_two_point_curve_is_left_open() {
    let (ml, path, _) = load(CLOSED_TWO_POINT_TSCN);
    let poly = baked(&ml, path);
    assert!(poly.len() > 1, "the curve must still bake, got {poly:?}");
    let last = poly[poly.len() - 1];
    assert!(
        (last - Vector3::new(10.0, 0.0, 0.0)).length() < 1e-3,
        "a 2-point curve stays open however it is flagged, so it ends at the \
         second control point — got {last:?}"
    );
}
