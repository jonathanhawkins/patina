//! pat-s3rrv.91 item (4) — `bake_interval` drives the bake, replacing a fixed
//! 16 samples per segment.
//!
//! `bake_interval` is "the distance in meters between two adjacent cached
//! points" (`doc/classes/Curve3D.xml:204`). A fixed step count cannot honour
//! that: a straight 10-unit segment cut into 16 leaves ~0.625 between adjacent
//! points, three times the 0.2 default.
//!
//! ⚠️ **This test asserts the GAP, never a point count.** A count is an artifact
//! of how the subdivision recursion happens to bottom out; the documented
//! contract is a distance, and `Curve::_bake` reaches it by halving a
//! sub-interval while its CHORD exceeds the interval (`curve.cpp:937`,
//! `max_stages = 10` — not the `5` on the public `tessellate_even_length`).
//! Pinning counts would re-assert the recursion's shape and go red on a
//! faithful change to it.
//!
//! ⚠️ The three arrays are checked for EQUAL LENGTH at both densities. They are
//! read index-for-index by the PathFollow3D driver, and `bake_curve_3d_tilts`
//! is handed no geometry — it can only stay aligned by consuming the same
//! schedule. A desync here does not error; it silently gives every sample a
//! neighbour's tilt, worst at exactly the tight corners a fine interval exists
//! to resolve.

use gdcore::math::Vector3;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

/// A straight 10-unit run with zero handles. Deliberately the simplest possible
/// geometry: the gap between adjacent baked points is then a pure function of
/// the subdivision, with no curvature to make a coarse bake look fine.
const STRAIGHT: &str = "0, 0, 0, 0, 0, 0, 0, 0, 0, \
                        0, 0, 0, 0, 0, 0, 10, 0, 0";

fn scene(bake_interval: Option<&str>) -> String {
    let line = match bake_interval {
        Some(v) => format!("bake_interval = {v}\n"),
        None => String::new(),
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve3D" id="Curve3D_1"]
{line}_data = {{
"points": PackedVector3Array({STRAIGHT}),
"tilts": PackedFloat32Array(0, 1.5)
}}

[node name="Main" type="Node3D"]

[node name="Path" type="Path3D" parent="."]
curve = SubResource("Curve3D_1")
"#
    )
}

struct Baked {
    points: Vec<Vector3>,
    tilts: Vec<f32>,
    ups: Vec<Vector3>,
}

fn bake(bake_interval: Option<&str>) -> Baked {
    let tscn = scene(bake_interval);
    let scene = PackedScene::from_tscn(&tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let id: NodeId = tree
        .get_node_by_path("/root/Main/Path")
        .expect("the Path3D is in the scene");
    let node = tree.get_node(id).expect("the Path3D node resolves");
    let points = match node.get_property("curve_points") {
        Variant::PackedVector3Array(v) => v,
        other => panic!("curve_points should be a PackedVector3Array, got {other:?}"),
    };
    let tilts = match node.get_property("curve_tilts") {
        Variant::PackedFloat32Array(v) => v,
        other => panic!("curve_tilts should be a PackedFloat32Array, got {other:?}"),
    };
    let ups = match node.get_property("curve_up_vectors") {
        Variant::PackedVector3Array(v) => v,
        other => panic!("curve_up_vectors should be a PackedVector3Array, got {other:?}"),
    };
    Baked { points, tilts, ups }
}

fn max_gap(points: &[Vector3]) -> f32 {
    points
        .windows(2)
        .map(|p| (p[1] - p[0]).length())
        .fold(0.0f32, f32::max)
}

/// Every array the driver indexes must have one entry per baked point.
fn assert_aligned(b: &Baked, what: &str) {
    assert_eq!(
        b.tilts.len(),
        b.points.len(),
        "{what}: curve_tilts must have one entry per baked point ({} points, \
         {} tilts). These are read index-for-index; a mismatch gives each \
         sample a neighbour's tilt instead of failing",
        b.points.len(),
        b.tilts.len(),
    );
    assert_eq!(
        b.ups.len(),
        b.points.len(),
        "{what}: curve_up_vectors must have one entry per baked point ({} \
         points, {} ups)",
        b.points.len(),
        b.ups.len(),
    );
}

/// 🔑 The discriminating assertion: at a fine interval no two adjacent baked
/// points may be further apart than the interval.
///
/// A fixed 16 steps puts them 0.625 apart on this fixture — 12x the 0.05 asked
/// for — so this is the assertion that a step-count bake cannot pass at any
/// constant it is given. (Nor can `segment_length / interval`: the chord
/// recursion's worst gap peaks above the mean, which is why the contract is
/// checked as a MAXIMUM.)
#[test]
fn bake_interval_bounds_the_gap_between_adjacent_points() {
    let fine = bake(Some("0.05"));
    let gap = max_gap(&fine.points);
    assert!(
        gap <= 0.05 + 1e-5,
        "bake_interval = 0.05 must put adjacent baked points at most 0.05 \
         apart; the largest gap was {gap} across {} points. A bake that ignores \
         bake_interval and uses a fixed step count lands around 0.625 here",
        fine.points.len(),
    );
    assert_aligned(&fine, "fine");
}

/// 🔑 The interval actually drives density: coarser gives fewer points, finer
/// gives more, with the class default in between.
///
/// ⚠️ Ordering only. Exact counts are the recursion's business.
#[test]
fn bake_interval_orders_density_coarse_default_fine() {
    let coarse = bake(Some("2.0"));
    let default = bake(None);
    let fine = bake(Some("0.05"));

    assert!(
        coarse.points.len() < default.points.len(),
        "a 2.0 interval must bake fewer points than the 0.2 class default; got \
         {} vs {}",
        coarse.points.len(),
        default.points.len(),
    );
    assert!(
        default.points.len() < fine.points.len(),
        "the 0.2 class default must bake fewer points than a 0.05 interval; got \
         {} vs {}",
        default.points.len(),
        fine.points.len(),
    );

    assert!(
        max_gap(&coarse.points) <= 2.0 + 1e-5,
        "the coarse bake must honour its own interval too, got {}",
        max_gap(&coarse.points),
    );

    for (b, what) in [(&coarse, "coarse"), (&default, "default"), (&fine, "fine")] {
        assert_aligned(b, what);
    }

    // The curve itself is unchanged — only how densely it is sampled. Both
    // densities must still start and end at the control points, or the density
    // change has moved the path rather than resampled it.
    for (b, what) in [(&coarse, "coarse"), (&fine, "fine")] {
        let first = b.points.first().copied().expect("a baked point exists");
        let last = b.points.last().copied().expect("a baked point exists");
        assert!(
            (first - Vector3::new(0.0, 0.0, 0.0)).length() < 1e-4,
            "{what}: the bake must start at the first control point, got {first:?}",
        );
        assert!(
            (last - Vector3::new(10.0, 0.0, 0.0)).length() < 1e-4,
            "{what}: the bake must end at the last control point, got {last:?}",
        );
    }

    // Tilt spans the same 0 -> 1.5 at either density; only the step size moves.
    for (b, what) in [(&coarse, "coarse"), (&fine, "fine")] {
        let first = *b.tilts.first().expect("a baked tilt exists");
        let last = *b.tilts.last().expect("a baked tilt exists");
        assert!(
            first.abs() < 1e-4 && (last - 1.5).abs() < 1e-4,
            "{what}: the baked tilts must still run 0 -> 1.5, got {first} -> {last}",
        );
    }
}

/// 🔑 A non-positive interval falls back to the class default.
///
/// ⚠️ Not a defensive extra. `even_length_schedule_3d` reads `<= 0.0` as "do not
/// subdivide" — right for a caller that asked directly, but a scene file
/// carrying `0.0` has an out-of-range value, and honouring it literally would
/// collapse the path to its control-point chords: a 2-point polyline where the
/// follower would cut every corner.
#[test]
fn non_positive_bake_interval_falls_back_to_the_class_default() {
    let default = bake(None);
    for bad in ["0.0", "-1.0"] {
        let b = bake(Some(bad));
        assert_eq!(
            b.points.len(),
            default.points.len(),
            "bake_interval = {bad} must bake exactly as the class default does; \
             got {} points vs {}. Two points would mean the non-positive value \
             reached the subdivision and disabled it",
            b.points.len(),
            default.points.len(),
        );
        assert_aligned(&b, bad);
    }
}
