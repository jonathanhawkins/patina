//! pat-s3rrv.86 items (3) and (4), END TO END — `scroll_limit_begin` /
//! `scroll_limit_end` and the `scroll_ignore_camera_zoom` screen-offset term,
//! driven from a real scene through `MainLoop::step`.
//!
//! Both fixes landed in `gdserver2d::parallax` with unit tests. This file is the
//! part that matters: it proves the runtime phase actually READS the node's
//! limit properties and the camera's anchor mode. Without it both would be
//! textbook registered-but-inert — correct maths that no `.tscn` can reach,
//! which is the exact failure mode this audit keeps turning up.
//!
//! ⚠️ Every assertion is on TRAVEL across a known camera move, never on an
//! absolute position. The absolute numbers depend on the canvas-transform
//! composition, which is not what these two items changed; travel is also what a
//! player actually sees.

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Two backgrounds, identical but for the limits, so the control differs in
/// exactly one property.
///
/// `scroll_limit_end` is `(2000, 0)`: x is bounded to `0..2000` in world space,
/// while y has `begin == end` and is therefore DISABLED — Godot's per-axis
/// `if (limit_begin.x < limit_end.x)` guard.
const LIMITED: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
current = true

[node name="Limited" type="ParallaxBackground" parent="."]
scroll_limit_begin = Vector2(0, 0)
scroll_limit_end = Vector2(2000, 0)

[node name="L" type="ParallaxLayer" parent="Limited"]
motion_scale = Vector2(1, 1)

[node name="Free" type="ParallaxBackground" parent="."]

[node name="L" type="ParallaxLayer" parent="Free"]
motion_scale = Vector2(1, 1)
"#;

/// A zoomed camera, a background that ignores the zoom, and a control that does
/// not.
const ZOOMED: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Cam" type="Camera2D" parent="."]
position = Vector2(0, 0)
zoom = Vector2(2, 2)
current = true

[node name="Ignoring" type="ParallaxBackground" parent="."]
scroll_ignore_camera_zoom = true

[node name="L" type="ParallaxLayer" parent="Ignoring"]
motion_scale = Vector2(1, 1)

[node name="Following" type="ParallaxBackground" parent="."]

[node name="L" type="ParallaxLayer" parent="Following"]
motion_scale = Vector2(1, 1)
"#;

fn load(src: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
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

fn move_camera_to(ml: &mut MainLoop, to: Vector2) {
    let cam = ml
        .tree()
        .get_node_by_path("/root/Main/Cam")
        .expect("the camera is in the scene");
    if let Some(node) = ml.tree_mut().get_node_mut(cam) {
        node.set_property("position", Variant::Vector2(to));
    }
    // Position smoothing is off by default, so the centre snaps; the extra steps
    // only make this independent of how many frames the phase needs.
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
}

/// Travel of `path` between two camera positions.
fn travel(ml: &mut MainLoop, path: &str, from: Vector2, to: Vector2) -> Vector2 {
    move_camera_to(ml, from);
    let start = layer_pos(ml, path);
    move_camera_to(ml, to);
    let end = layer_pos(ml, path);
    Vector2::new(end.x - start.x, end.y - start.y)
}

fn approx(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() < 1e-2,
        "{what}: got {got}, expected {want}"
    );
}

/// The fixture assumes the default viewport fits inside the 2000px limit range
/// with room to spare on both sides. Checked rather than assumed, so a changed
/// default window size fails loudly here instead of skewing every number below.
fn assert_fixture_fits(ml: &MainLoop) -> f32 {
    let width = ml.tree().root_window().visible_size().x;
    assert!(
        (0.0..=1600.0).contains(&width),
        "fixture assumes a viewport narrower than 1600px against its 2000px \
         limit range; got {width}"
    );
    width
}

/// **The acceptance test for item (3).** A limited background stops scrolling at
/// its limits while an identical unlimited one keeps going.
///
/// The camera is driven well past `scroll_limit_end` twice. The limited
/// background must not move between those two positions; the control, which
/// differs only in having no limits, must move the full amount.
#[test]
fn a_limited_background_stops_scrolling_and_an_unlimited_one_does_not() {
    let mut ml = load(LIMITED);
    assert_fixture_fits(&ml);
    let (near, far) = (Vector2::new(5000.0, 0.0), Vector2::new(9000.0, 0.0));

    let limited = travel(&mut ml, "/root/Main/Limited/L", near, far);
    let free = travel(&mut ml, "/root/Main/Free/L", near, far);

    approx(
        limited.x,
        0.0,
        "a background past scroll_limit_end must not scroll further",
    );
    // ⚠️ Asserted as an exact 4000 rather than merely "non-zero": if the
    // defaults `(0,0)`/`(0,0)` were ever read as an ACTIVE `0..0` range, this
    // control would freeze too, and every other assertion in the file would
    // still pass while every parallax background in a project sat still.
    approx(
        free.x,
        -4000.0,
        "the unlimited control must track the camera's full 4000px move",
    );
}

/// The axes are independent — `begin < end` is tested per-axis.
///
/// The camera runs far out of range DIAGONALLY. x is limited and must hold; y
/// has `begin == end`, is therefore disabled, and must travel the full distance.
/// A guard that covered both axes at once passes every other test in this file
/// and fails here.
#[test]
fn one_axis_can_be_limited_while_the_other_is_free() {
    let mut ml = load(LIMITED);
    assert_fixture_fits(&ml);

    let moved = travel(
        &mut ml,
        "/root/Main/Limited/L",
        Vector2::new(5000.0, 5000.0),
        Vector2::new(9000.0, 9000.0),
    );

    approx(moved.x, 0.0, "x is limited and must hold at the clamp");
    approx(
        moved.y,
        -4000.0,
        "y has equal limits, so it is disabled and scrolls freely",
    );
}

/// Inside its limits, a limited background scrolls exactly like an unlimited one.
///
/// Without this, a clamp that fired unconditionally — pinning the layer whenever
/// limits were configured at all — would pass both tests above.
///
/// The camera positions are derived from the viewport width, because "inside the
/// range" is `[width/2, 2000 - width/2]` in camera coordinates: the limits bound
/// the VIEW's world-space edges, not the camera's centre.
#[test]
fn within_its_limits_a_limited_background_scrolls_normally() {
    let mut ml = load(LIMITED);
    let half = assert_fixture_fits(&ml) * 0.5;

    let moved = travel(
        &mut ml,
        "/root/Main/Limited/L",
        Vector2::new(half + 100.0, 0.0),
        Vector2::new(half + 400.0, 0.0),
    );

    approx(
        moved.x,
        -300.0,
        "inside its limits, a limited background tracks the camera exactly",
    );
}

/// 🔑 THE LEFT LIMIT IS REACHED BY DRIVING THE CAMERA *NEGATIVE*.
///
/// The scroll offset runs opposite to the camera, and Godot clamps the NEGATED
/// offset before negating back. Dropping that negation still produces a clamp
/// that fires — just at the opposite end of the level — so only exercising both
/// ends separates the two. A camera at -3000 and one at -7000 are both past
/// `scroll_limit_begin`, so the layer must not move between them.
#[test]
fn the_begin_limit_holds_when_the_camera_runs_off_the_left_edge() {
    let mut ml = load(LIMITED);
    assert_fixture_fits(&ml);

    let moved = travel(
        &mut ml,
        "/root/Main/Limited/L",
        Vector2::new(-3000.0, 0.0),
        Vector2::new(-7000.0, 0.0),
    );

    approx(
        moved.x,
        0.0,
        "a background past scroll_limit_begin must not scroll further",
    );
}

/// **The acceptance test for item (4).** With `scroll_ignore_camera_zoom`, a
/// layer scrolls at the UNZOOMED rate; without it, at the zoomed rate.
///
/// ⚠️ This is the assertion the earlier implementation could not pass, and the
/// reason it survived so long: it dropped both the division by the zoom and the
/// `screen_offset * (zoom - 1)` term, and those cancel exactly at `zoom == 1`.
/// At zoom 2 an ignoring background travelled the full 200px — identical to the
/// control it is supposed to differ from.
///
/// This is also what reaches the runtime half of the fix: the phase must read
/// the camera's anchor mode and pass `viewport * 0.5` for the default
/// DRAG_CENTER.
#[test]
fn ignoring_the_camera_zoom_scrolls_at_the_unzoomed_rate() {
    let mut ml = load(ZOOMED);
    let (start, end) = (Vector2::ZERO, Vector2::new(100.0, 0.0));

    let ignoring = travel(&mut ml, "/root/Main/Ignoring/L", start, end);
    let following = travel(&mut ml, "/root/Main/Following/L", start, end);

    approx(
        ignoring.x,
        -100.0,
        "ignoring the zoom, a full-rate layer travels exactly the camera's 100px",
    );
    approx(
        following.x,
        -200.0,
        "following the zoom, the same layer travels 100px x zoom 2",
    );
    assert!(
        (ignoring.x - following.x).abs() > 1.0,
        "scroll_ignore_camera_zoom must change the scroll rate under a zoomed \
         camera; both travelled {}",
        ignoring.x
    );
}
