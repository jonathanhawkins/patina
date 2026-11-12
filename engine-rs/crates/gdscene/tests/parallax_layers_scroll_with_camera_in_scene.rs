//! pat-s3rrv.86 — ParallaxBackground/ParallaxLayer scroll from the live camera.
//!
//! `gdserver2d::parallax` has computed these offsets, with its own test file,
//! since it was written — and had **no production caller**. A `.tscn` with a
//! ParallaxBackground loaded, kept every property, and scrolled not one pixel.
//! This drives the real chain: scene → `MainLoop::step` → parallax phase.
//!
//! ⚠️ **A layer's `position` is a SCREEN position.** `ParallaxBackground` is a
//! `CanvasLayer` (`class_db.rs`), and the render list marks a CanvasLayer's items
//! `CanvasFollow::Fixed`, so `apply_camera2d_view` never applies the camera to
//! them. A layer therefore starts pinned to the screen and its offset is the only
//! thing that moves it — which is why the offset is Godot's canvas transform
//! origin *unmodified*, and why `motion_scale = 1` means "scrolls at the full
//! camera rate" while `0` means "does not scroll at all"
//! (`doc/classes/ParallaxLayer.xml`).
//!
//! ⚠️ An earlier pass shipped the offset NEGATED — every background scrolled the
//! wrong way — and this test passed anyway, because it only checked
//! `motion_scale = 0.5`. **At 0.5 both conventions produce 100px of travel**; the
//! `0` and `1` endpoints are the only fixtures that tell them apart, so all three
//! are pinned below. The assertions are on *travel* across a known camera move
//! rather than on absolute numbers, so they are grounded in what a player sees
//! rather than in the formula under test.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// One background, three layers spanning the whole `motion_scale` range.
const WORLD: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="Back" type="ParallaxBackground" parent="."]

[node name="Full" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(1, 1)

[node name="Half" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(0.5, 0.5)

[node name="Pinned" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(0, 0)
"#;

fn load(src: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn layer_x(ml: &MainLoop, path: &str) -> f32 {
    match ml
        .tree()
        .get_node_by_path(path)
        .and_then(|id| ml.tree().get_node(id))
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v.x,
        other => panic!("{path} position should be a Vector2, got {other:?}"),
    }
}

fn move_camera_to(ml: &mut MainLoop, x: f32) {
    let cam = ml
        .tree()
        .get_node_by_path("/root/Main/Cam")
        .expect("the camera is in the scene");
    if let Some(node) = ml.tree_mut().get_node_mut(cam) {
        node.set_property("position", Variant::Vector2(Vector2::new(x, 0.0)));
    }
    // Position smoothing is off by default, so the centre snaps; several steps
    // only make that independent of how many frames the phase needs.
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
}

/// **The acceptance test.**
#[test]
fn parallax_layers_scroll_with_camera_in_scene() {
    let mut ml = MainLoop::new(load(WORLD));
    let half_x = ml.tree().root_window().visible_size().x * 0.5;

    move_camera_to(&mut ml, 0.0);
    let at_zero = (
        layer_x(&ml, "/root/Main/Back/Full"),
        layer_x(&ml, "/root/Main/Back/Half"),
        layer_x(&ml, "/root/Main/Back/Pinned"),
    );

    // ⚠️ With the camera at the origin a world-space node at the origin renders
    // at the centre of the viewport, so that is where a full-rate layer belongs.
    // The travel assertions below cannot see this constant — a layer offset by a
    // fixed amount travels correctly and still sits in the wrong place.
    assert!(
        (at_zero.0 - half_x).abs() < 1e-3,
        "with the camera at the origin a motion_scale of 1 must land where a \
         world object at the origin renders, the viewport centre {half_x}; got {}",
        at_zero.0,
    );

    move_camera_to(&mut ml, 200.0);

    // Travel, not position: the camera moved 200px right, so the world sweeps
    // 200px LEFT across the screen. That is the number a player sees.
    let full = layer_x(&ml, "/root/Main/Back/Full") - at_zero.0;
    let half = layer_x(&ml, "/root/Main/Back/Half") - at_zero.1;
    let pinned = layer_x(&ml, "/root/Main/Back/Pinned") - at_zero.2;

    assert!(
        (full - -200.0).abs() < 1e-3,
        "motion_scale 1 must scroll at the full camera rate, -200px; got {full} \
         (+200 means the offset is negated and every background scrolls the \
         wrong way)",
    );
    assert!(
        (half - -100.0).abs() < 1e-3,
        "motion_scale 0.5 must scroll at half rate, -100px; got {half}",
    );
    assert!(
        pinned.abs() < 1e-3,
        "motion_scale 0 must not scroll at all — `doc/classes/ParallaxLayer.xml`: \
         'If an axis is set to 0, it will not scroll'; got {pinned} (a -200 here \
         is the inverted convention, under which 0 behaves as 1 does)",
    );
}

/// An unconfigured layer scrolls at the full camera rate, because Godot's
/// `motion_scale` default is `(1, 1)`.
///
/// ⚠️ Defaulting it to zero — the natural choice for an absent Vector2 — pins
/// every layer to the screen, which reads as the camera having stopped affecting
/// the background rather than as a missing default.
const NO_SCALE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="Back" type="ParallaxBackground" parent="."]

[node name="Plain" type="ParallaxLayer" parent="Back"]
"#;

#[test]
fn a_layer_with_no_motion_scale_scrolls_at_the_full_rate() {
    let mut ml = MainLoop::new(load(NO_SCALE));

    move_camera_to(&mut ml, 0.0);
    let at_zero = layer_x(&ml, "/root/Main/Back/Plain");
    move_camera_to(&mut ml, 80.0);
    let travelled = layer_x(&ml, "/root/Main/Back/Plain") - at_zero;

    assert!(
        (travelled - -80.0).abs() < 1e-3,
        "Godot's motion_scale default is (1, 1), so an unconfigured layer \
         scrolls the full 80px the camera moved; got {travelled} (0 means the \
         default was read as (0, 0) and the layer is pinned to the screen)",
    );
}

/// A layer's **authored** position survives, added on top of the scroll —
/// Godot's `orig_offset` term (`parallax_layer.cpp`).
///
/// ⚠️ This phase overwrites `position` every frame, so without a shadow of the
/// authored value frame 2 reads back frame 1's write and the authored offset is
/// gone for good. Two identical layers differing only in authored `y` make that
/// visible without depending on the scroll value itself; sampling twice, either
/// side of a camera move, is what proves the value is not consumed by the first
/// overwrite.
const AUTHORED: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="Back" type="ParallaxBackground" parent="."]

[node name="Plain" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(0.5, 0.5)

[node name="Nudged" type="ParallaxLayer" parent="Back"]
motion_scale = Vector2(0.5, 0.5)
position = Vector2(0, 100)
"#;

fn layer_y(ml: &MainLoop, path: &str) -> f32 {
    match ml
        .tree()
        .get_node_by_path(path)
        .and_then(|id| ml.tree().get_node(id))
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v.y,
        other => panic!("{path} position should be a Vector2, got {other:?}"),
    }
}

#[test]
fn an_authored_layer_position_is_kept_and_added_to_the_scroll() {
    let mut ml = MainLoop::new(load(AUTHORED));

    move_camera_to(&mut ml, 0.0);
    let first = layer_y(&ml, "/root/Main/Back/Nudged") - layer_y(&ml, "/root/Main/Back/Plain");
    assert!(
        (first - 100.0).abs() < 1e-3,
        "the layer authored 100px down must stay 100px below its identical \
         sibling; got {first}",
    );

    move_camera_to(&mut ml, 200.0);
    let later = layer_y(&ml, "/root/Main/Back/Nudged") - layer_y(&ml, "/root/Main/Back/Plain");
    assert!(
        (later - 100.0).abs() < 1e-3,
        "the authored offset must survive later frames; got {later} (0 means the \
         first frame's overwrite of `position` destroyed it)",
    );
}
