//! pat-0q2kx.104 acceptance: a Godot-authored `Path2D` + `Curve2D` sub-resource
//! actually drives `PathFollow2D` at runtime.
//!
//! Godot ground truth: `Path2D.curve` is a `Curve2D` whose
//! `_data = { "points": PackedVector2Array }` interleaves **in-handle,
//! out-handle, position** triples, handles relative to the position
//! (upstream/godot/scene/resources/curve.cpp `Curve2D::_set_data`). Godot bakes
//! that into an arc-length table on load and `PathFollow2D` samples it.
//!
//! Patina had both halves and no seam: `process_path_follows`
//! (gdscene/src/main_loop.rs) read a `curve_points` polyline off the Path2D, and
//! `bezier_bake.rs` could bake one — but nothing produced it from a scene.
//! `grep curve_points` found writers only in `main_loop.rs`'s own test fixtures,
//! so a real `.tscn` left PathFollow2D inert while unit tests stayed green.
//!
//! ⚠️ These inject **no** properties. The curve arrives as `.tscn` text through
//! `PackedScene`, because test-side injection of `curve_points` is exactly what
//! hid this gap.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

/// A quarter-circle-ish bezier from (0,0) to (100,100), bulging through +x.
///
/// Triples are (in_handle, out_handle, position). The first point's in-handle
/// and the last point's out-handle are unused by the bake, as in Godot.
const PATH_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="Curve2D" id="Curve2D_1"]
_data = {
"points": PackedVector2Array(0, 0, 60, 0, 0, 0, -20, -40, 0, 0, 100, 100)
}

[node name="Main" type="Node2D"]

[node name="Path" type="Path2D" parent="."]
curve = SubResource("Curve2D_1")

[node name="Follow" type="PathFollow2D" parent="Path"]
progress = 0.0
path_speed = 40.0
"#;

/// The same scene with no curve at all — the control that proves the assertions
/// below are reading the baked curve rather than any incidental motion.
const NO_CURVE_TSCN: &str = r#"[gd_scene load_steps=1 format=3]

[node name="Main" type="Node2D"]

[node name="Path" type="Path2D" parent="."]

[node name="Follow" type="PathFollow2D" parent="Path"]
progress = 0.0
path_speed = 40.0
"#;

fn tree_from(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

/// The node's position, defaulting to the origin when unset.
///
/// A `PathFollow2D` that has never been stepped has no `position` property at
/// all — the driver writes it on the first tick — so reading the raw Variant
/// yields `Nil` before the first frame. `node2d::get_position` is the engine's
/// own accessor and applies Godot's "absent means (0,0)" default, which is what
/// the driver itself uses.
fn position_of(tree: &SceneTree, id: NodeId) -> Vector2 {
    gdscene::node2d::get_position(tree, id)
}

/// **The acceptance test.** The `.tscn`'s own Curve2D reaches the Path2D as a
/// baked polyline, and the follower advances along it each frame.
#[test]
fn pathfollow2d_curve2d_scene_wiring() {
    let tree = tree_from(PATH_TSCN);
    let path = node(&tree, "/root/Main/Path");
    let follow = node(&tree, "/root/Main/Path/Follow");

    // The bake happened at instance time, from the scene text. Without the
    // SubResource resolution this property is absent and everything below
    // degrades to "the follower never moves" — which is the bug.
    let baked = match tree.get_node(path).map(|n| n.get_property("curve_points")) {
        Some(Variant::PackedVector2Array(pts)) => pts,
        other => panic!(
            "PackedScene must bake `curve = SubResource(Curve2D)` onto the Path2D \
             as `curve_points`; got {other:?}"
        ),
    };
    assert!(
        baked.len() > 2,
        "a two-segment bezier must bake to a polyline, not its control points; \
         got {} point(s)",
        baked.len()
    );
    // The bake must follow the curve, not the straight chord. The chord from
    // (0,0) to (100,100) has every point on y == x; the authored handles bulge
    // the path off that line, so at least one baked point must leave it.
    assert!(
        baked.iter().any(|p| (p.x - p.y).abs() > 1.0),
        "the baked polyline must follow the bezier handles, not the straight \
         chord between the endpoints"
    );
    // Endpoints are exact in Godot's bake.
    assert!(
        (baked[0] - Vector2::ZERO).length() < 0.01,
        "the bake must start at the first control point, got {:?}",
        baked[0]
    );
    assert!(
        (baked[baked.len() - 1] - Vector2::new(100.0, 100.0)).length() < 0.01,
        "the bake must end at the last control point, got {:?}",
        baked[baked.len() - 1]
    );

    // Now drive it: the follower advances along the baked curve each frame.
    let mut ml = MainLoop::new(tree);
    let start = position_of(ml.tree(), follow);
    ml.step(0.1);
    let after_one = position_of(ml.tree(), follow);
    assert!(
        (after_one - start).length() > 0.1,
        "PathFollow2D must move along the curve once the scene supplies one; \
         start {start:?}, after one step {after_one:?}"
    );

    for _ in 0..5 {
        ml.step(0.1);
    }
    let later = position_of(ml.tree(), follow);
    assert!(
        (later - after_one).length() > 0.1,
        "the follower must keep advancing, not stall after the first frame"
    );
    // It travels the curve rather than teleporting past its end.
    assert!(
        later.x <= 101.0 && later.y <= 101.0,
        "the follower must stay on the authored curve, got {later:?}"
    );
}

/// A Path2D with no curve leaves its follower where it is — the control that
/// keeps the assertions above honest.
#[test]
fn pathfollow2d_without_a_curve_does_not_move() {
    let tree = tree_from(NO_CURVE_TSCN);
    let path = node(&tree, "/root/Main/Path");
    assert!(
        tree.get_node(path)
            .map(|n| n.get_property("curve_points").is_nil())
            .unwrap_or(false),
        "a Path2D with no `curve` must not gain a baked polyline"
    );

    let follow = node(&tree, "/root/Main/Path/Follow");
    let mut ml = MainLoop::new(tree);
    let start = position_of(ml.tree(), follow);
    for _ in 0..5 {
        ml.step(0.1);
    }
    assert_eq!(
        position_of(ml.tree(), follow),
        start,
        "with no curve there is nothing to follow, so the node stays put"
    );
}
