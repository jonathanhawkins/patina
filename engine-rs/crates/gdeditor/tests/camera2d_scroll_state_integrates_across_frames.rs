//! pat-s3rrv.84 — the Camera2D's scroll state survives between frames.
//!
//! `Camera2D::_update_scroll` (upstream/godot/scene/2d/camera_2d.cpp) keeps
//! `camera_pos`/`smoothed_camera_pos` on the node and integrates them every
//! frame: the drag dead zone is measured from the PREVIOUS centre and position
//! smoothing eases from it.
//!
//! Patina had that math in `node2d` and unit-tested it green, but every
//! production caller passed `previous_center = None`. With `current == target`
//! the dead zone and the lerp both collapse to a hard snap, so
//! `position_smoothing_enabled`, `position_smoothing_speed` and all four
//! `drag_*_margin`s did nothing in a running game. The state now lives on the
//! node, republished each frame by `behavioral_2d::process_camera2d_scroll`.
//!
//! Every assertion below is paired with a control case that must move the
//! opposite way, so none of them can pass by accident.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{
    apply_camera2d_view, build_runtime_render_list, camera2d_view_transform,
};
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

const W: f32 = 400.0;
const H: f32 = 200.0;
const DT: f32 = 1.0 / 60.0;
/// Half the visible width at zoom 1 — the span the drag margins scale.
const HALF_W: f32 = W / 2.0;

/// `Root > { Sprite2D(0,0), Camera2D(0,0, enabled) }`.
///
/// The sprite sits at the world origin so its rendered x reads the camera
/// centre directly: at zoom 1 the view maps world `p` to `p - centre + half`,
/// so `rendered_x == HALF_W - centre.x`.
fn scene(camera_props: &[(&str, Variant)]) -> (MainLoop, NodeId, NodeId) {
    // ClassDB starts empty in every nextest process, and the render list admits
    // a node only if `is_class("CanvasItem")` resolves — without this the
    // sprite never enters the list at all.
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    // The scroll pass reads the viewport from the root window while the render
    // below is given W x H explicitly; the drag dead zone and the limit clamp
    // are both fractions of the half-view, so they must agree.
    tree.root_window_mut().size = Vector2::new(W, H);
    assert_eq!(
        tree.root_window().visible_size(),
        Vector2::new(W, H),
        "the fixture depends on the root window reporting the size it renders at"
    );

    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let sprite = tree
        .add_child(scene_root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(sprite) {
        n.set_property("position", Variant::Vector2(Vector2::ZERO));
        n.set_property("texture", Variant::String("res://missing.png".into()));
    }

    let camera = tree
        .add_child(scene_root, Node::new("Camera", "Camera2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(camera) {
        n.set_property("position", Variant::Vector2(Vector2::ZERO));
        n.set_property("enabled", Variant::Bool(true));
        for (key, value) in camera_props.iter() {
            n.set_property(*key, value.clone());
        }
    }

    (MainLoop::new(tree), camera, sprite)
}

/// The sprite's rendered origin, through the exact path the Play branch uses:
/// build the list, fold in the camera view, read the item back.
fn rendered_origin(ml: &MainLoop, sprite: NodeId) -> Vector2 {
    let tree = ml.tree();
    let size = Vector2::new(W, H);
    let mut items = build_runtime_render_list(tree);
    let view = camera2d_view_transform(tree, size, None, DT)
        .expect("an enabled Camera2D must yield a canvas transform");
    apply_camera2d_view(&mut items, view);
    items
        .iter()
        .find(|i| i.node_id == sprite.raw())
        .map(|i| i.global_transform.origin)
        .expect("the sprite survives the camera pass")
}

fn move_camera(ml: &mut MainLoop, camera: NodeId, to: Vector2) {
    ml.tree_mut()
        .get_node_mut(camera)
        .unwrap()
        .set_property("position", Variant::Vector2(to));
}

#[test]
fn camera2d_scroll_state_integrates_across_frames() {
    smoothing_eases_over_frames_instead_of_snapping();
    smoothing_off_snaps_in_one_frame();
    drag_margin_is_a_dead_zone_across_frames();
    limit_enabled_false_leaves_the_camera_unclamped();
    ignore_rotation_false_rotates_the_world();
}

/// Clause 1: a teleported camera lands STRICTLY between old and new after one
/// frame, and converges over ~30.
fn smoothing_eases_over_frames_instead_of_snapping() {
    let (mut ml, camera, sprite) = scene(&[
        ("position_smoothing_enabled", Variant::Bool(true)),
        ("position_smoothing_speed", Variant::Float(5.0)),
    ]);

    // Establish the scroll state at the origin. The first frame seeds rather
    // than eases (Godot's `first` branch), so the teleport below starts from a
    // settled centre rather than from nothing.
    for _ in 0..3 {
        ml.step(DT as f64);
    }
    let at_old = rendered_origin(&ml, sprite);
    assert_eq!(
        at_old.x, HALF_W,
        "a camera on the sprite must centre it in the viewport"
    );

    move_camera(&mut ml, camera, Vector2::new(1000.0, 0.0));
    ml.step(DT as f64);
    let after_one = rendered_origin(&ml, sprite);

    // Where the two ends of the move render.
    let at_new = HALF_W - 1000.0;
    assert!(
        after_one.x < at_old.x && after_one.x > at_new,
        "after ONE frame the view must sit strictly between the old centre \
         ({at_old:?}) and the new one ({at_new}), got {after_one:?} — equal to \
         the new position means the scroll state was not carried across frames"
    );

    // The exact Godot step: lerp by `speed * dt` = 5/60.
    let expected = HALF_W - 1000.0 * (5.0 * DT);
    assert!(
        (after_one.x - expected).abs() < 0.5,
        "one frame of smoothing must lerp by speed*dt: expected x~{expected}, got {after_one:?}"
    );

    for _ in 0..40 {
        ml.step(DT as f64);
    }
    let converged = rendered_origin(&ml, sprite);
    assert!(
        converged.x < at_new + 100.0,
        "~40 further frames must converge on the target ({at_new}), got {converged:?}"
    );
    assert!(
        converged.x > at_new,
        "an ease must never overshoot its target ({at_new}), got {converged:?}"
    );
}

/// Control for clause 1: with smoothing off the same teleport lands exactly on
/// the target in one frame. Without this, an implementation that simply moved
/// the camera slowly would satisfy the test above.
fn smoothing_off_snaps_in_one_frame() {
    let (mut ml, camera, sprite) = scene(&[]);
    for _ in 0..3 {
        ml.step(DT as f64);
    }
    move_camera(&mut ml, camera, Vector2::new(1000.0, 0.0));
    ml.step(DT as f64);

    let after_one = rendered_origin(&ml, sprite);
    assert!(
        (after_one.x - (HALF_W - 1000.0)).abs() < 0.001,
        "with smoothing disabled the camera must snap in one frame, got {after_one:?}"
    );
}

/// Clause 2: a nudge inside the dead zone does not move the view at all; the
/// control nudge outside it does.
fn drag_margin_is_a_dead_zone_across_frames() {
    // Margin 0.2 of a 200px half-view => a 40px dead zone on each side.
    let props = [
        ("drag_horizontal_enabled", Variant::Bool(true)),
        ("drag_left_margin", Variant::Float(0.2)),
        ("drag_right_margin", Variant::Float(0.2)),
    ];
    let dead_zone = 0.2 * HALF_W;

    let (mut ml, camera, sprite) = scene(&props);
    for _ in 0..3 {
        ml.step(DT as f64);
    }
    let settled = rendered_origin(&ml, sprite);

    move_camera(&mut ml, camera, Vector2::new(dead_zone * 0.5, 0.0));
    ml.step(DT as f64);
    let nudged = rendered_origin(&ml, sprite);
    assert_eq!(
        nudged, settled,
        "a nudge inside the {dead_zone}px dead zone must not move the view at all"
    );

    // Control: past the margin the camera is dragged along, trailing by exactly
    // the margin.
    let (mut ml, camera, sprite) = scene(&props);
    for _ in 0..3 {
        ml.step(DT as f64);
    }
    let settled = rendered_origin(&ml, sprite);
    move_camera(&mut ml, camera, Vector2::new(dead_zone * 2.5, 0.0));
    ml.step(DT as f64);
    let dragged = rendered_origin(&ml, sprite);
    assert!(
        dragged.x < settled.x - 1.0,
        "a nudge PAST the dead zone must move the view, got {dragged:?} from {settled:?}"
    );
    let expected = HALF_W - (dead_zone * 2.5 - dead_zone);
    assert!(
        (dragged.x - expected).abs() < 0.5,
        "the dragged camera must trail the target by exactly the margin: \
         expected x~{expected}, got {dragged:?}"
    );
}

/// Clause 3: `limit_enabled = false` lets the camera leave its limits.
fn limit_enabled_false_leaves_the_camera_unclamped() {
    let limits = [
        ("limit_left", Variant::Float(-100.0)),
        ("limit_right", Variant::Float(100.0)),
        ("limit_top", Variant::Float(-100.0)),
        ("limit_bottom", Variant::Float(100.0)),
    ];

    let mut unclamped = limits.to_vec();
    unclamped.push(("limit_enabled", Variant::Bool(false)));
    let (mut ml, camera, sprite) = scene(&unclamped);
    move_camera(&mut ml, camera, Vector2::new(5000.0, 0.0));
    ml.step(DT as f64);
    let free = rendered_origin(&ml, sprite);
    assert!(
        (free.x - (HALF_W - 5000.0)).abs() < 0.001,
        "with limit_enabled = false the camera must NOT be clamped, got {free:?}"
    );

    // Control: the same scene with limits on is pulled back hard.
    let (mut ml, camera, sprite) = scene(&limits);
    move_camera(&mut ml, camera, Vector2::new(5000.0, 0.0));
    ml.step(DT as f64);
    let clamped = rendered_origin(&ml, sprite);
    assert!(
        clamped.x > free.x + 1000.0,
        "with limits enabled the camera must be clamped well short of 5000, \
         got {clamped:?} against the unclamped {free:?}"
    );
}

/// Clause 4: `ignore_rotation = false` folds the camera's rotation into the
/// view, so the world counter-rotates around the centre.
fn ignore_rotation_false_rotates_the_world() {
    // Put the sprite off-centre so a rotation is observable: a point ON the
    // centre of rotation renders identically at every angle, which would make
    // this assertion vacuous.
    let quarter_turn = std::f32::consts::FRAC_PI_2;

    let rotated = [
        ("ignore_rotation", Variant::Bool(false)),
        ("rotation", Variant::Float(quarter_turn as f64)),
        ("position", Variant::Vector2(Vector2::new(-100.0, 0.0))),
    ];
    let (mut ml, _camera, sprite) = scene(&rotated);
    ml.step(DT as f64);
    let turned = rendered_origin(&ml, sprite);

    // Camera at (-100,0) looking a quarter-turn over: the sprite, 100px to the
    // camera's right in world space, renders a quarter-turn away from there —
    // straight up the screen rather than across it.
    assert!(
        (turned.x - HALF_W).abs() < 0.5,
        "a quarter-turned camera must put the sprite on the centre line, got {turned:?}"
    );
    assert!(
        (turned.y - (H / 2.0 - 100.0)).abs() < 0.5,
        "a quarter-turned camera must swing the sprite onto the vertical axis, got {turned:?}"
    );

    // Control: the default (`ignore_rotation = true`) keeps the view upright,
    // so the same rotated camera leaves the sprite across the screen.
    let upright = [
        ("rotation", Variant::Float(quarter_turn as f64)),
        ("position", Variant::Vector2(Vector2::new(-100.0, 0.0))),
    ];
    let (mut ml, _camera, sprite) = scene(&upright);
    ml.step(DT as f64);
    let flat = rendered_origin(&ml, sprite);
    assert!(
        (flat.x - (HALF_W + 100.0)).abs() < 0.5 && (flat.y - H / 2.0).abs() < 0.5,
        "with ignore_rotation = true (the default) the view stays upright, got {flat:?}"
    );
}
