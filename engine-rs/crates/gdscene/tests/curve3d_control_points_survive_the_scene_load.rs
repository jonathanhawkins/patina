//! pat-s3rrv.91 item (5) — the loaded scene retains the AUTHORED control points,
//! not just the polyline baked from them.
//!
//! `sample`, `samplef` and `tessellate` all evaluate bezier handles. The baked
//! polyline has already thrown those away, so without this the three methods
//! could only ever be called on a hand-built curve — never on one that came from
//! a scene file.
//!
//! ⚠️ The load-bearing assertion here is the **count**. Godot's `_data["points"]`
//! does not contain the duplicated first point of a closed curve — `closed` is a
//! separate flag, and `sample`/`tessellate` add the wrap segment themselves. The
//! loader pushes that duplicate onto its own working copy before baking, so
//! writing the post-push array alongside `curve_closed = true` would make the
//! curve wrap TWICE, appending a closing segment the author never drew. A
//! 3-point closed curve must therefore arrive as 9 entries, never 12.

use gdcore::math::Vector3;
use gdscene::bezier_bake::CurvePoint3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{Curve3D, SceneTree};
use gdvariant::Variant;

/// Three control points with real handles on the first segment, so the authored
/// curve is genuinely a bezier and not recoverable from its chords.
const POINTS: &str = "0, 0, 0, 0, 9, 0, 0, 0, 0, \
                      -6, 0, 3, 0, 0, 0, 10, 0, 0, \
                      0, 0, 0, 0, 0, 0, 10, 0, 10";

fn scene(closed: bool) -> String {
    let closed_line = if closed { "closed = true\n" } else { "" };
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
{closed_line}_data = {{
"points": PackedVector3Array({POINTS}),
"tilts": PackedFloat32Array(0, 0, 0)
}}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")
"#
    )
}

/// The Path3D node's properties after a load.
fn load_path(closed: bool) -> (Vec<Vector3>, bool, Vec<Vector3>) {
    let tscn = scene(closed);
    let scene = PackedScene::from_tscn(&tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let id: NodeId = tree
        .get_node_by_path("/root/Main/Path")
        .expect("the Path3D is in the scene");
    let node = tree.get_node(id).expect("the Path3D node resolves");
    let control = match node.get_property("curve_control_points") {
        Variant::PackedVector3Array(v) => v,
        other => panic!(
            "the loader must carry the authored control points onto the Path3D \
             as `curve_control_points`; got {other:?}"
        ),
    };
    let is_closed = matches!(node.get_property("curve_closed"), Variant::Bool(true));
    let baked = match node.get_property("curve_points") {
        Variant::PackedVector3Array(v) => v,
        other => panic!("curve_points should still be baked too, got {other:?}"),
    };
    (control, is_closed, baked)
}

/// Rebuild the runtime curve the way `main_loop` does.
fn curve_from(control: &[Vector3], closed: bool) -> Curve3D {
    let mut c = Curve3D::new();
    c.control = control
        .chunks_exact(3)
        .map(|t| CurvePoint3 {
            in_handle: t[0],
            out_handle: t[1],
            position: t[2],
        })
        .collect();
    c.closed = closed;
    c
}

fn near(a: Vector3, b: Vector3) -> bool {
    (a - b).length() < 1e-4
}

/// 🔑 A closed curve keeps exactly its authored triples — the wrap duplicate is
/// the flag's job, not the array's.
#[test]
fn closed_curve_control_points_exclude_the_wrap_duplicate() {
    let (control, is_closed, baked) = load_path(true);

    assert!(
        is_closed,
        "`closed = true` in the scene must reach the node as `curve_closed`",
    );
    assert_eq!(
        control.len(),
        9,
        "three authored control points are nine (in, out, position) entries; \
         got {}. Twelve means the loader wrote its own working copy AFTER \
         pushing the wrap duplicate, so the curve would close twice — once from \
         the duplicated point and again from `curve_closed`",
        control.len(),
    );
    // The last authored point is the third one, not a copy of the first.
    assert!(
        near(control[8], Vector3::new(10.0, 0.0, 10.0)),
        "the final entry must be the third authored position, got {:?}. \
         (0, 0, 0) here is the wrap duplicate leaking through",
        control[8],
    );

    // The polyline is still baked alongside it — this replaces nothing.
    assert!(
        baked.len() > 9,
        "the baked polyline must still be written; got {} points",
        baked.len(),
    );
}

/// 🔑 The retained data is enough to rebuild the authored bezier and evaluate it.
///
/// ⚠️ This is what separates "the handles were stored" from "the handles were
/// stored correctly". `sample(0, 0.5)` is derived from the cubic bezier by hand:
/// with `c1 = (0,9,0)` and `c2 = (4,0,3)`,
/// `B(0.5) = 0.125*p0 + 0.375*c1 + 0.375*c2 + 0.125*p3 = (2.75, 3.375, 1.125)`.
/// A curve rebuilt from chords alone — handles dropped or zeroed — gives
/// `(5, 0, 0)`, which the second assertion rules out.
#[test]
fn control_points_rebuild_the_authored_bezier() {
    let (control, is_closed, _) = load_path(false);
    let curve = curve_from(&control, is_closed);

    assert_eq!(
        curve.control.len(),
        3,
        "three control points survive the load"
    );
    assert!(
        near(curve.sample(0, 0.5), Vector3::new(2.75, 3.375, 1.125)),
        "the rebuilt curve must reproduce the authored bezier midpoint; got {:?}",
        curve.sample(0, 0.5),
    );
    assert!(
        !near(curve.sample(0, 0.5), Vector3::new(5.0, 0.0, 0.0)),
        "…and it must not be the straight-chord midpoint, which is what a curve \
         rebuilt without handles would give",
    );
    assert!(
        near(curve.samplef(0.5), curve.sample(0, 0.5)),
        "samplef must agree with sample on the same segment",
    );
}

/// 🔑 `closed` reaches the rebuilt curve, so `tessellate` walks the wrap segment
/// exactly once and returns to the first point.
#[test]
fn closed_flag_makes_tessellate_walk_the_wrap_segment_once() {
    let (control, is_closed, _) = load_path(true);
    let curve = curve_from(&control, is_closed);
    assert!(curve.closed, "the rebuilt curve must be closed");

    let tess = curve.tessellate(5, 4.0);
    let first = *tess.first().expect("a tessellated point exists");
    let last = *tess.last().expect("a tessellated point exists");
    assert!(
        near(first, Vector3::new(0.0, 0.0, 0.0)) && near(last, Vector3::new(0.0, 0.0, 0.0)),
        "a closed curve must start and end at the first control point; got \
         {first:?} .. {last:?}",
    );

    // The open form of the same curve stops at the third point instead — which
    // is what proves the wrap came from the flag rather than from the fixture.
    let (open_control, open_closed, _) = load_path(false);
    let open = curve_from(&open_control, open_closed);
    assert!(!open.closed, "the control fixture must be open");
    let open_tess = open.tessellate(5, 4.0);
    assert!(
        near(
            *open_tess.last().expect("a tessellated point exists"),
            Vector3::new(10.0, 0.0, 10.0)
        ),
        "an open curve ends at its last control point, got {:?}",
        open_tess.last(),
    );
    assert!(
        open_tess.len() < tess.len(),
        "the closed curve has one more interval than the open one ({} vs {})",
        tess.len(),
        open_tess.len(),
    );
}
