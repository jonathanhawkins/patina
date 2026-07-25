//! pat-0q2kx.27 — an enabled `Camera2D` scrolls and zooms the runtime canvas.
//!
//! Godot's `Camera2D::_update_scroll` writes the viewport's canvas transform every
//! frame, so content centres on the camera, zoom magnifies about that centre, and
//! limits clamp the scroll (upstream/godot/scene/2d/camera_2d.cpp). Patina had the
//! complete math in `node2d` (`Camera2DConfig`, `compute_camera2d_position`) with
//! **zero** runtime callers: the Play path mapped world coordinates 1:1 to pixels,
//! so a camera at (500,300) left the sprite drawn at pixel (500,300) instead of at
//! the viewport centre.
//!
//! These assert the transform the Play branch now applies.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{
    apply_camera2d_view, build_runtime_render_list, camera2d_view_transform, current_camera2d,
};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

const W: f32 = 400.0;
const H: f32 = 200.0;

/// `Root > { Sprite2D(500,300), Camera2D(500,300, enabled) }`, the bead's scene.
fn scene_with_camera(zoom: f32, limits: Option<(f32, f32)>) -> (SceneTree, gdscene::NodeId) {
    // The render list admits a node only if `is_class("CanvasItem")`, which resolves
    // through ClassDB — and ClassDB starts *empty*, filled only by explicit
    // `register_*_classes()` calls that the editor/runner make at startup. Without
    // this, `Sprite2D` is not known to derive from `CanvasItem` and the sprite never
    // enters the list. Registration overwrites by class name, so repeated calls
    // across tests in one process are harmless.
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let sprite = tree
        .add_child(scene_root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(sprite) {
        n.set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));
        // A ColorRect-free visual still needs a size for the scale assertion; the
        // sprite path falls back to a placeholder square when no texture is cached.
        n.set_property("texture", Variant::String("res://missing.png".into()));
    }

    let camera = tree
        .add_child(scene_root, Node::new("Camera", "Camera2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(camera) {
        n.set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));
        n.set_property("enabled", Variant::Bool(true));
        n.set_property("zoom", Variant::Vector2(Vector2::new(zoom, zoom)));
        if let Some((left, right)) = limits {
            n.set_property("limit_left", Variant::Float(left as f64));
            n.set_property("limit_right", Variant::Float(right as f64));
        }
    }

    (tree, sprite)
}

#[test]
fn camera2d_scroll_drives_runtime_canvas() {
    let (tree, sprite) = scene_with_camera(1.0, None);
    let size = Vector2::new(W, H);

    // The camera is found through Godot's `enabled` claim.
    assert!(
        current_camera2d(&tree).is_some(),
        "an enabled Camera2D must be discovered as the current camera"
    );

    let view = camera2d_view_transform(&tree, size, None, 1.0 / 60.0)
        .expect("an enabled camera must yield a canvas transform");

    let mut items = build_runtime_render_list(&tree);
    let before = items
        .iter()
        .find(|i| i.node_id == sprite.raw())
        .map(|i| i.global_transform.origin)
        .expect("the sprite is in the render list");
    // Without a camera the sprite sits at its world position, 1:1 in pixels.
    assert_eq!(before, Vector2::new(500.0, 300.0));

    apply_camera2d_view(&mut items, view);
    let after = items
        .iter()
        .find(|i| i.node_id == sprite.raw())
        .map(|i| i.global_transform.origin)
        .expect("the sprite survives the camera pass");

    // Camera centred on the sprite ⇒ the sprite lands at the viewport centre.
    assert!(
        (after.x - W / 2.0).abs() < 0.5 && (after.y - H / 2.0).abs() < 0.5,
        "sprite should be centred at ({}, {}), got {after:?}",
        W / 2.0,
        H / 2.0
    );
    assert_ne!(
        after, before,
        "the camera pass must move content, not leave world coords 1:1"
    );
}

#[test]
fn zoom_doubles_on_screen_size() {
    let (tree, _sprite) = scene_with_camera(2.0, None);
    let size = Vector2::new(W, H);
    let view = camera2d_view_transform(&tree, size, None, 1.0 / 60.0).expect("camera transform");

    // Zoom scales the linear part, which is what makes the rasterizer draw a
    // sprite/rect twice as large — it draws through the item transform.
    let unit_x = view.xform(Vector2::new(1.0, 0.0)) - view.xform(Vector2::ZERO);
    let unit_y = view.xform(Vector2::new(0.0, 1.0)) - view.xform(Vector2::ZERO);
    assert!(
        (unit_x.x - 2.0).abs() < 1e-4 && (unit_y.y - 2.0).abs() < 1e-4,
        "zoom=2 must double on-screen size, got x-scale {} y-scale {}",
        unit_x.x,
        unit_y.y
    );

    // A point one world-unit right of the camera lands one *zoomed* unit right of
    // the viewport centre.
    let p = view.xform(Vector2::new(501.0, 300.0));
    assert!(
        (p.x - (W / 2.0 + 2.0)).abs() < 0.5,
        "expected centre+2px for a 1-unit offset at zoom 2, got {p:?}"
    );
}

#[test]
fn camera_outside_limit_left_is_clamped() {
    // Camera at x=500 but limits allow only 900..1400: Godot clamps the scroll so
    // the visible region stays inside the limit rect.
    let (tree, _sprite) = scene_with_camera(1.0, Some((900.0, 1400.0)));
    let size = Vector2::new(W, H);

    let clamped = gdeditor::viewport_2d_render::camera2d_center(&tree, size, None, 1.0 / 60.0)
        .expect("camera centre");
    let unclamped = Vector2::new(500.0, 300.0);
    assert_ne!(
        clamped, unclamped,
        "a camera outside limit_left must be clamped, not used as-is"
    );
    // The left edge of the view cannot cross limit_left.
    let left_edge = clamped.x - W / 2.0;
    assert!(
        left_edge >= 900.0 - 0.5,
        "view left edge {left_edge} crossed limit_left 900"
    );
}
