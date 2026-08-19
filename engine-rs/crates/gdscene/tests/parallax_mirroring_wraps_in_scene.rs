//! pat-s3rrv.86 — `motion_mirroring` folds a layer's offset back into one
//! repeat period, driven from a real scene.
//!
//! The wrap direction was wrong: this port used `floor` (folding into
//! `[0, period)`) while Godot uses `ceil` (folding into `(-period, 0]`), with a
//! comment beside it citing Godot as the source. The two are exactly one period
//! apart, so every periodicity test passes under both — `f(x + period) == f(x)`
//! holds either way — and only a test that pins the wrapped VALUE separates them.
//!
//! ⚠️ The sign is not cosmetic. Godot's canvas repeats extend in the POSITIVE
//! direction from the item (`renderer_canvas_cull.cpp:388-405`), so the wrapped
//! item must sit at or just LEFT of the origin for its copies to cover the
//! screen to the right. Folding into `[0, period)` leaves a gap of up to one
//! full period at the left edge — a seam that only becomes visible once tile
//! emission lands, which is still open on this bead. That is precisely why
//! nothing caught it.
//!
//! A NEW FILE, so a red elsewhere cannot cancel these and vice versa.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// One mirrored layer and one plain layer, so the control differs in exactly one
/// property. `motion_mirroring.y` is 0 — mirroring is per-axis, and the vertical
/// axis must keep scrolling freely.
const WORLD: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="Back" type="ParallaxBackground" parent="."]

[node name="Tiled" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(1, 1)
motion_mirroring = Vector2(256, 0)

[node name="Plain" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(1, 1)
"#;

fn load() -> MainLoop {
    let scene = PackedScene::from_tscn(WORLD).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    MainLoop::new(tree)
}

fn layer_pos(ml: &MainLoop, path: &str) -> Vector2 {
    match ml
        .tree()
        .get_node_by_path(path)
        .and_then(|id| ml.tree().get_node(id))
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v,
        other => panic!("{path} position should be a Vector2, got {other:?}"),
    }
}

fn at_camera(ml: &mut MainLoop, x: f32) -> Vector2 {
    let cam = ml
        .tree()
        .get_node_by_path("/root/Main/Cam")
        .expect("the camera is in the scene");
    if let Some(node) = ml.tree_mut().get_node_mut(cam) {
        node.set_property("position", Variant::Vector2(Vector2::new(x, 0.0)));
    }
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    layer_pos(ml, "/root/Main/Back/Tiled")
}

/// 🔑 THE WRAPPED OFFSET LANDS IN `(-period, 0]`, NOT `[0, period)`.
///
/// This is the only assertion that separates Godot's `ceil` wrap from the
/// `floor` one this port shipped. It is stated as a range over many camera
/// positions rather than as one value, so it cannot be satisfied by a sign flip
/// that happens to be right at a single point.
#[test]
fn a_mirrored_layer_folds_to_the_left_of_the_origin() {
    let mut ml = load();

    for camera_x in [-100_000.0f32, -3000.0, -1.0, 0.0, 40.0, 5000.0, 100_000.0] {
        let pos = at_camera(&mut ml, camera_x);
        assert!(
            pos.x > -256.0 && pos.x <= 0.0,
            "camera {camera_x}: mirrored offset {} must fold into (-256, 0]",
            pos.x
        );
    }
}

/// The point of mirroring: the layer never runs away, however far the camera
/// travels.
///
/// The control layer, identical but unmirrored, must run away — otherwise this
/// test would pass against an engine that had simply stopped scrolling
/// backgrounds altogether.
#[test]
fn mirroring_bounds_the_layer_while_an_unmirrored_one_runs_away() {
    let mut ml = load();

    at_camera(&mut ml, 0.0);
    let plain_near = layer_pos(&ml, "/root/Main/Back/Plain").x;
    at_camera(&mut ml, 100_000.0);
    let plain_far = layer_pos(&ml, "/root/Main/Back/Plain").x;

    assert!(
        (plain_far - plain_near + 100_000.0).abs() < 1e-1,
        "the unmirrored control must track the camera's full travel, got {}",
        plain_far - plain_near
    );
}

/// Seamlessness: one full period of camera travel returns the identical offset.
///
/// ⚠️ This holds under BOTH wrap directions and so proves nothing about the sign
/// — it is here because it is the property a player actually sees (no jump at
/// the tile boundary), and because a wrap that lost periodicity would break the
/// feature outright while still satisfying the range test above.
#[test]
fn one_period_of_camera_travel_repeats_the_offset() {
    let mut ml = load();

    let a = at_camera(&mut ml, 1000.0).x;
    let forward = at_camera(&mut ml, 1000.0 + 256.0).x;
    let backward = at_camera(&mut ml, 1000.0 - 256.0).x;

    assert!(
        (a - forward).abs() < 1e-2,
        "one period forward must repeat: {a} vs {forward}"
    );
    assert!(
        (a - backward).abs() < 1e-2,
        "one period backward must repeat: {a} vs {backward}"
    );
}

/// Mirroring is per-axis: `motion_mirroring.y` is 0, so vertical scroll is
/// untouched.
///
/// The camera only moves horizontally in the tests above, so without this a
/// wrap that folded BOTH axes regardless of their period would go unnoticed.
#[test]
fn an_unmirrored_axis_still_scrolls() {
    let mut ml = load();

    let cam = ml
        .tree()
        .get_node_by_path("/root/Main/Cam")
        .expect("the camera is in the scene");
    for y in [0.0f32, 5000.0] {
        if let Some(node) = ml.tree_mut().get_node_mut(cam) {
            node.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
        }
        for _ in 0..4 {
            ml.step(1.0 / 60.0);
        }
        if y == 0.0 {
            continue;
        }
        let pos = layer_pos(&ml, "/root/Main/Back/Tiled");
        assert!(
            pos.y < -4000.0,
            "y has no mirroring period, so it must follow the camera's 5000px \
             move; got {}",
            pos.y
        );
    }
}
