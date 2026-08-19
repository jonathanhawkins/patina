//! pat-s3rrv.91 item (4), 2D half — `bake_interval` drives the Curve2D bake.
//!
//! The 2D twin of `curve3d_bake_interval_controls_bake_density`. `bake_interval`
//! is "the distance in pixels between two adjacent cached points"
//! (`doc/classes/Curve2D.xml:174`), and the loader previously passed a hardcoded
//! 16 samples per segment, which cannot honour a distance at any constant.
//!
//! ⚠️ The 2D default is **5.0 pixels** — twenty-five times the Curve3D default
//! of 0.2 metres. The two are separate constants because they count different
//! units; a shared value would resample every 2D path at a metre-scale density.
//!
//! ⚠️ Gaps and ordering are asserted, never exact point counts — a count is an
//! artifact of where the chord recursion bottoms out.

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

/// A straight 100px run with zero handles: the gap between adjacent baked
/// points is then a pure function of the subdivision, with no curvature to make
/// a coarse bake look adequate.
const STRAIGHT: &str = "0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100";

fn scene(bake_interval: Option<&str>) -> String {
    let line = match bake_interval {
        Some(v) => format!("bake_interval = {v}\n"),
        None => String::new(),
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve2D" id="Curve2D_1"]
{line}_data = {{
"points": PackedVector2Array({STRAIGHT})
}}

[node name="Main" type="Node2D"]

[node name="Path" type="Path2D" parent="."]
curve = SubResource("Curve2D_1")
"#
    )
}

fn baked(bake_interval: Option<&str>) -> Vec<Vector2> {
    let tscn = scene(bake_interval);
    let scene = PackedScene::from_tscn(&tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let id: NodeId = tree
        .get_node_by_path("/root/Main/Path")
        .expect("the Path2D is in the scene");
    match tree
        .get_node(id)
        .map(|n| n.get_property("curve_points"))
        .expect("the Path2D node resolves")
    {
        Variant::PackedVector2Array(v) => v,
        other => panic!("curve_points should be a PackedVector2Array, got {other:?}"),
    }
}

fn max_gap(points: &[Vector2]) -> f32 {
    points
        .windows(2)
        .map(|p| (p[1] - p[0]).length())
        .fold(0.0f32, f32::max)
}

/// 🔑 The discriminating assertion: at a fine interval no two adjacent baked
/// points may be further apart than the interval.
///
/// The old hardcoded 16 puts them 6.25px apart on this fixture — six times the
/// 1.0 asked for — so this is the assertion no fixed step count can pass.
#[test]
fn curve2d_bake_interval_bounds_the_gap_between_adjacent_points() {
    let fine = baked(Some("1.0"));
    let gap = max_gap(&fine);
    assert!(
        gap <= 1.0 + 1e-4,
        "bake_interval = 1.0 must put adjacent baked points at most 1px apart; \
         the largest gap was {gap} across {} points. The previous hardcoded 16 \
         samples per segment lands at 6.25px here",
        fine.len(),
    );
}

/// 🔑 The interval drives density, with the 5px class default in between.
///
/// ⚠️ Ordering only — exact counts belong to the recursion.
#[test]
fn curve2d_bake_interval_orders_density_coarse_default_fine() {
    let coarse = baked(Some("25.0"));
    let default = baked(None);
    let fine = baked(Some("1.0"));

    assert!(
        coarse.len() < default.len(),
        "a 25px interval must bake fewer points than the 5px class default; got \
         {} vs {}",
        coarse.len(),
        default.len(),
    );
    assert!(
        default.len() < fine.len(),
        "the 5px class default must bake fewer points than a 1px interval; got \
         {} vs {}",
        default.len(),
        fine.len(),
    );
    assert!(
        max_gap(&coarse) <= 25.0 + 1e-4,
        "the coarse bake must honour its own interval too, got {}",
        max_gap(&coarse),
    );

    // Only the sampling density changes — the path itself does not move.
    for (pts, what) in [(&coarse, "coarse"), (&default, "default"), (&fine, "fine")] {
        let first = *pts.first().expect("a baked point exists");
        let last = *pts.last().expect("a baked point exists");
        assert!(
            (first - Vector2::new(0.0, 0.0)).length() < 1e-3,
            "{what}: the bake must start at the first control point, got {first:?}",
        );
        assert!(
            (last - Vector2::new(0.0, 100.0)).length() < 1e-3,
            "{what}: the bake must end at the last control point, got {last:?}",
        );
    }
}

/// 🔑 A non-positive interval falls back to the class default rather than
/// collapsing the path to its control-point chords.
#[test]
fn curve2d_non_positive_bake_interval_falls_back_to_the_class_default() {
    let default = baked(None);
    for bad in ["0.0", "-1.0"] {
        let pts = baked(Some(bad));
        assert_eq!(
            pts.len(),
            default.len(),
            "bake_interval = {bad} must bake exactly as the class default does; \
             got {} points vs {}. Two points would mean the non-positive value \
             reached the subdivision and disabled it",
            pts.len(),
            default.len(),
        );
    }
}
