//! pat-s3rrv.22: `CLIP_CHILDREN_ONLY` masks without drawing itself.
//!
//! # What was wrong
//!
//! Godot has three clip modes (`doc/classes/CanvasItem.xml:806-814`):
//!
//! - `CLIP_CHILDREN_DISABLED` (0) — children drawn over, not clipped.
//! - `CLIP_CHILDREN_ONLY` (1) — "This node is used as a mask and is **not**
//!   drawn." Children are clipped to it.
//! - `CLIP_CHILDREN_AND_DRAW` (2) — mask, and the node **is** drawn.
//!
//! `gdscene::canvas_order::clip_children_mode` returned the right 0/1/2 all
//! along, but it had exactly ONE production consumer — `clip_rect_for`, which
//! tests it with `!= 0`. That is correct for the clipping half, and it means
//! nothing anywhere could tell mode 1 from mode 2. A node set to "mask only"
//! drew itself anyway, painting over the scene with what the author intended as
//! an invisible stencil.
//!
//! # What these assertions separate
//!
//! Both cases render the SAME tree; only the mode differs. The sample point sits
//! inside the clipper but outside its child, so it isolates the clipper's own
//! drawing from the child's. The third assertion is the one that stops a lazy
//! fix: skipping the subtree entirely would also blank that pixel, so the child
//! must still be drawn in mode 1.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{build_runtime_render_list, rasterize_render_list};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn color_rect(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    pos: Vector2,
    size: Vector2,
    color: Color,
) -> NodeId {
    let mut node = Node::new(name, "ColorRect");
    node.set_property("position", Variant::Vector2(pos));
    node.set_property("size", Variant::Vector2(size));
    node.set_property("color", Variant::Color(color));
    tree.add_child(parent, node).unwrap()
}

/// Without a populated ClassDB the render list filters on `is_class("CanvasItem")`
/// and comes back empty, so every assertion would pass vacuously.
fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    (tree, root)
}

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

fn close(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.05 && (a.g - b.g).abs() < 0.05 && (a.b - b.b).abs() < 0.05
}

/// A 20x20 red clipper with a 4x4 blue child, rendered at `clip_children = mode`.
fn render_with_mode(mode: i64) -> gdrender2d::renderer::FrameBuffer {
    let (mut tree, root) = new_tree();
    let clipper = color_rect(
        &mut tree,
        root,
        "Clipper",
        Vector2::ZERO,
        Vector2::new(20.0, 20.0),
        Color::new(1.0, 0.0, 0.0, 1.0),
    );
    tree.get_node_mut(clipper)
        .unwrap()
        .set_property("clip_children", Variant::Int(mode));
    color_rect(
        &mut tree,
        clipper,
        "Child",
        Vector2::ZERO,
        Vector2::new(4.0, 4.0),
        Color::new(0.0, 0.0, 1.0, 1.0),
    );
    let items = build_runtime_render_list(&tree);
    rasterize_render_list(&items, W, H, CLEAR, None)
}

#[test]
fn clip_children_and_draw_still_paints_the_mask() {
    // The control. Mode 2 is "mask AND draw", so the clipper's own red must be
    // there — without this, the mode-1 assertion below could pass simply because
    // the clipper never drew under any mode.
    let fb = render_with_mode(2);
    let got = pixel(&fb, 10, 10);
    assert!(
        close(got, Color::new(1.0, 0.0, 0.0, 1.0)),
        "CLIP_CHILDREN_AND_DRAW must draw the mask node itself; got {got:?}"
    );
}

#[test]
fn clip_children_only_does_not_paint_the_mask() {
    // **The headline.** Same tree, mode 1: the clipper is a stencil and must not
    // appear. Red here is the old behaviour, where nothing could distinguish the
    // two modes.
    let fb = render_with_mode(1);
    let got = pixel(&fb, 10, 10);
    assert!(
        close(got, CLEAR),
        "CLIP_CHILDREN_ONLY must NOT draw the mask node; got {got:?}. Red means \
         mode 1 was treated as mode 2"
    );
}

#[test]
fn clip_children_only_still_draws_its_children() {
    // ⚠️ The anti-cheat. Dropping the whole subtree would also clear the pixel
    // asserted above, so mode 1 must still draw the child — it is a mask, not a
    // delete.
    let fb = render_with_mode(1);
    let got = pixel(&fb, 2, 2);
    assert!(
        close(got, Color::new(0.0, 0.0, 1.0, 1.0)),
        "the child of a CLIP_CHILDREN_ONLY node must still be drawn; got {got:?}"
    );
}
