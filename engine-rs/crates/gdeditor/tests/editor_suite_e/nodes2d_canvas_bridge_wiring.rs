//! pat-nzgdu.11 — SceneTree -> CanvasItem render bridge produces real pixels.
//!
//! Godot's viewport rasterizes each CanvasItem's draw commands at the node's
//! global transform and z-order. This test drives the editor's runtime canvas
//! bridge (`viewport_2d_render::render_runtime_canvas`, which the served
//! viewport's Play/runtime path uses) over a scene of positioned primitive
//! nodes and asserts the framebuffer carries their real pixels at their real
//! transforms — not an icon placeholder — with correct z-order.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdscene::node::Node;
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const BLACK: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn set_prop(tree: &mut SceneTree, id: gdscene::NodeId, key: &str, v: Variant) {
    if let Some(n) = tree.get_node_mut(id) {
        n.set_property(key, v);
    }
}

/// A positioned ColorRect and a Line2D rasterize their real pixels at their
/// global transforms into the runtime framebuffer.
#[test]
fn nodes2d_canvas_bridge_wiring() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Red ColorRect at world (40, 40), size 20x20 -> covers (40,40)..(60,60).
    let rect = tree
        .add_child(root, Node::new("Rect", "ColorRect"))
        .unwrap();
    node2d::set_position(&mut tree, rect, Vector2::new(40.0, 40.0));
    set_prop(
        &mut tree,
        rect,
        "size",
        Variant::Vector2(Vector2::new(20.0, 20.0)),
    );
    set_prop(
        &mut tree,
        rect,
        "color",
        Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
    );

    // Green vertical Line2D at x=10 from y=10 to y=90.
    let line = tree.add_child(root, Node::new("Line", "Line2D")).unwrap();
    set_prop(
        &mut tree,
        line,
        "points",
        Variant::Array(
            vec![
                Variant::Vector2(Vector2::new(10.0, 10.0)),
                Variant::Vector2(Vector2::new(10.0, 90.0)),
            ]
            .into(),
        ),
    );
    set_prop(&mut tree, line, "width", Variant::Float(4.0));
    set_prop(
        &mut tree,
        line,
        "default_color",
        Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
    );

    let fb = render_runtime_canvas(&tree, 100, 100, BLACK, None);

    // Empty region stays the clear/background color.
    let bg = fb.get_pixel(85, 15);
    assert!(
        bg.r < 0.15 && bg.g < 0.15 && bg.b < 0.15,
        "empty region should be background, got {bg:?}",
    );

    // The ColorRect renders red inside its rect — at its real transform, not an
    // icon placeholder centered on the node origin.
    let inside = fb.get_pixel(50, 50);
    assert!(
        inside.r > 0.7 && inside.g < 0.3 && inside.b < 0.3,
        "ColorRect must render red at world (50,50), got {inside:?}",
    );

    // The Line2D column renders green.
    let on_line = fb.get_pixel(10, 50);
    assert!(
        on_line.g > 0.4 && on_line.b < 0.4,
        "Line2D must render green near x=10, got {on_line:?}",
    );
}

/// Overlapping ColorRects composite in z-order: the higher `z_index` draws on
/// top.
#[test]
fn nodes2d_canvas_bridge_z_order() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Blue rect underneath (z=0), red rect on top (z=10), both (30,30)+40x40.
    let blue = tree
        .add_child(root, Node::new("Blue", "ColorRect"))
        .unwrap();
    node2d::set_position(&mut tree, blue, Vector2::new(30.0, 30.0));
    set_prop(
        &mut tree,
        blue,
        "size",
        Variant::Vector2(Vector2::new(40.0, 40.0)),
    );
    set_prop(
        &mut tree,
        blue,
        "color",
        Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
    );
    node2d::set_z_index(&mut tree, blue, 0);

    let red = tree.add_child(root, Node::new("Red", "ColorRect")).unwrap();
    node2d::set_position(&mut tree, red, Vector2::new(30.0, 30.0));
    set_prop(
        &mut tree,
        red,
        "size",
        Variant::Vector2(Vector2::new(40.0, 40.0)),
    );
    set_prop(
        &mut tree,
        red,
        "color",
        Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
    );
    node2d::set_z_index(&mut tree, red, 10);

    let fb = render_runtime_canvas(&tree, 100, 100, BLACK, None);

    // The overlap shows red — the higher z_index composited last, on top.
    let px = fb.get_pixel(50, 50);
    assert!(
        px.r > 0.7 && px.b < 0.3,
        "higher z_index (red) must draw over lower (blue), got {px:?}",
    );
}
