//! pat-0q2kx.40 — the Camera2D view moves the world canvas but leaves a
//! `CanvasLayer` HUD screen-fixed.
//!
//! Godot's viewport carries a canvas transform that the Camera2D rewrites each
//! frame (`scene/2d/camera_2d.cpp::_update_scroll`). It applies to the DEFAULT
//! canvas; a `CanvasLayer` is screen-fixed unless `follow_viewport_enabled` is
//! set, which is why a health bar stays put while the level scrolls under it.
//!
//! Patina folded the view into **every** render item, so the HUD scrolled and
//! zoomed with the world. Nothing errored — the HUD simply slid off-screen the
//! moment the camera moved, which reads as a layout bug rather than a missing
//! canvas-transform concept.
//!
//! These assert item transforms rather than rasterized pixels: the transform is
//! what the rasterizer draws through, so asserting it tests the same behaviour
//! without depending on glyph/placeholder pixel layout.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{
    apply_camera2d_view, build_runtime_render_list, camera2d_view_transform, Render2DItem,
};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

const W: f32 = 400.0;
const H: f32 = 200.0;

/// Viewport centre — where the camera's own position lands.
fn centre() -> Vector2 {
    Vector2::new(W / 2.0, H / 2.0)
}

/// Adds a textured `Sprite2D` at `pos`. A free function rather than a closure:
/// a closure capturing `tree` mutably would still hold that borrow across the
/// direct `tree.add_child` calls below.
fn add_sprite(
    tree: &mut SceneTree,
    parent: gdscene::NodeId,
    name: &str,
    pos: Vector2,
) -> gdscene::NodeId {
    let id = tree.add_child(parent, Node::new(name, "Sprite2D")).unwrap();
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property("position", Variant::Vector2(pos));
        // The sprite path falls back to a placeholder square when no texture is
        // cached, which is enough to carry a transform.
        n.set_property("texture", Variant::String("res://missing.png".into()));
    }
    id
}

/// `Root > { World(200,0), Far(300,0), Camera2D(camera_x,0), CanvasLayer > Hud(10,10) }`.
///
/// `Far` sits 100px right of the camera so zoom has an offset to magnify —
/// a node at the camera centre maps to the centre at every zoom, so it alone
/// could not tell 1x from 2x.
fn scene(camera_x: f32, zoom: f32) -> (SceneTree, u64, u64, u64) {
    // The render list admits a node only if `is_class("CanvasItem")`, which
    // resolves through ClassDB — and ClassDB starts EMPTY. Without this the
    // sprites never enter the list and every assertion below would be vacuous.
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let world = add_sprite(&mut tree, scene_root, "World", Vector2::new(200.0, 0.0));
    let far = add_sprite(&mut tree, scene_root, "Far", Vector2::new(300.0, 0.0));

    let camera = tree
        .add_child(scene_root, Node::new("Camera", "Camera2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(camera) {
        n.set_property("position", Variant::Vector2(Vector2::new(camera_x, 0.0)));
        n.set_property("enabled", Variant::Bool(true));
        n.set_property("zoom", Variant::Vector2(Vector2::new(zoom, zoom)));
    }

    // A CanvasLayer keeps its children out of the default canvas. `layer`
    // defaults to 1 in Godot, which is what marks these items screen-fixed.
    let layer = tree
        .add_child(scene_root, Node::new("Hud", "CanvasLayer"))
        .unwrap();
    let hud = add_sprite(&mut tree, layer, "Bar", Vector2::new(10.0, 10.0));

    (tree, world.raw(), far.raw(), hud.raw())
}

/// Builds the runtime list with the camera view applied, as the Play path does.
fn viewed(tree: &SceneTree) -> Vec<Render2DItem> {
    let mut items = build_runtime_render_list(tree);
    let view = camera2d_view_transform(tree, Vector2::new(W, H), None, 1.0 / 60.0)
        .expect("an enabled Camera2D must produce a view transform");
    apply_camera2d_view(&mut items, view);
    items
}

fn origin_of(items: &[Render2DItem], node_id: u64) -> Vector2 {
    items
        .iter()
        .find(|i| i.node_id == node_id)
        .unwrap_or_else(|| panic!("node {node_id} missing from the render list"))
        .global_transform
        .origin
}

/// **The acceptance test.**
#[test]
fn camera2d_drives_runtime_canvas_transform() {
    // -- Camera at (200,0): the world node authored there lands dead centre. --
    let (tree, world, _far, hud) = scene(200.0, 1.0);
    let items = viewed(&tree);

    let w0 = origin_of(&items, world);
    assert!(
        (w0.x - centre().x).abs() < 0.01 && (w0.y - centre().y).abs() < 0.01,
        "a world node at the camera's position must draw at the viewport centre \
         {:?}, got {w0:?}",
        centre()
    );

    // ⚠️ The HUD is untouched by the camera view.
    let h0 = origin_of(&items, hud);
    assert!(
        (h0.x - 10.0).abs() < 0.01 && (h0.y - 10.0).abs() < 0.01,
        "a CanvasLayer HUD must stay at its authored screen position (10,10), \
         got {h0:?} — the camera view was folded into it"
    );

    // -- Camera moved +50: world pixels shift -50, HUD does not move. --------
    let (tree2, world2, _far2, hud2) = scene(250.0, 1.0);
    let items2 = viewed(&tree2);

    let w1 = origin_of(&items2, world2);
    assert!(
        (w1.x - (w0.x - 50.0)).abs() < 0.01 && (w1.y - w0.y).abs() < 0.01,
        "moving the camera +50px must shift world pixels -50px: expected x={}, \
         got {w1:?}",
        w0.x - 50.0
    );

    let h1 = origin_of(&items2, hud2);
    assert!(
        (h1.x - h0.x).abs() < 0.01 && (h1.y - h0.y).abs() < 0.01,
        "HUD pixels must not move when the camera moves: was {h0:?}, now {h1:?}"
    );
}

/// Zoom magnifies the world about the camera centre and leaves the HUD alone.
#[test]
fn camera2d_zoom_scales_world_but_not_hud() {
    let (tree1x, _w1, far1, hud1) = scene(200.0, 1.0);
    let (tree2x, _w2, far2, hud2) = scene(200.0, 2.0);

    let at1 = viewed(&tree1x);
    let at2 = viewed(&tree2x);

    // `Far` is 100px right of the camera, so its offset from centre is what zoom
    // multiplies: 100px at 1x, 200px at 2x.
    let off1 = origin_of(&at1, far1).x - centre().x;
    let off2 = origin_of(&at2, far2).x - centre().x;
    assert!(
        (off1 - 100.0).abs() < 0.01,
        "at zoom 1x a node 100px from the camera sits 100px from centre, got {off1}"
    );
    assert!(
        (off2 - 200.0).abs() < 0.01,
        "at zoom 2x that offset must double to 200px, got {off2} — zoom is not \
         reaching the world canvas"
    );

    // ⚠️ The HUD is identical at both zoom levels.
    let h1 = origin_of(&at1, hud1);
    let h2 = origin_of(&at2, hud2);
    assert!(
        (h1.x - h2.x).abs() < 0.01 && (h1.y - h2.y).abs() < 0.01,
        "zoom must not scale a CanvasLayer HUD: {h1:?} at 1x vs {h2:?} at 2x"
    );
}
