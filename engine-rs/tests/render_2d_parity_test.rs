//! pat-1rcm: 2D rendering parity — visibility propagation and z-index semantics.
//!
//! Validates Godot rendering contracts:
//! 1. Parent visibility propagation — hiding a parent hides all children
//! 2. Z-index ordering with visibility interaction
//! 3. Deep hierarchy visibility propagation (grandparent → grandchild)

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

use gdrender2d::renderer::SoftwareRenderer;

// ===========================================================================
// Helpers
// ===========================================================================

fn red() -> Color {
    Color::rgb(1.0, 0.0, 0.0)
}
fn green() -> Color {
    Color::rgb(0.0, 1.0, 0.0)
}
fn blue() -> Color {
    Color::rgb(0.0, 0.0, 1.0)
}

fn make_rect_item(id: u64, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
}

fn pixel_at(frame: &gdserver2d::server::FrameData, x: u32, y: u32) -> Color {
    frame.pixels[(y * frame.width + x) as usize]
}

// ===========================================================================
// 1. Parent visibility propagation
// ===========================================================================

/// When a parent is hidden, its child should not render even if the child is visible.
#[test]
fn hidden_parent_hides_visible_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent: invisible, no draw commands
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.visible = false;
    vp.add_canvas_item(parent);

    // Child: visible, draws green rect at (0,0)-(10,10)
    let mut child = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);

    // Child should NOT render because parent is hidden
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "child of hidden parent should not render"
    );
}

/// When a parent is visible, its visible child should render normally.
#[test]
fn visible_parent_allows_visible_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent: visible, no draw commands
    let parent = CanvasItem::new(CanvasItemId(1));
    vp.add_canvas_item(parent);

    // Child: visible, draws green rect
    let mut child = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "child of visible parent should render"
    );
}

/// Hidden parent with multiple children — none should render.
#[test]
fn hidden_parent_hides_all_children() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.visible = false;
    vp.add_canvas_item(parent);

    // Child A: red at left
    let mut child_a = make_rect_item(2, 0.0, 0.0, 10.0, 20.0, red());
    child_a.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child_a);

    // Child B: blue at right
    let mut child_b = make_rect_item(3, 10.0, 0.0, 10.0, 20.0, blue());
    child_b.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 10),
        Color::BLACK,
        "child A should be hidden"
    );
    assert_eq!(
        pixel_at(&frame, 15, 10),
        Color::BLACK,
        "child B should be hidden"
    );
}

// ===========================================================================
// 2. Deep hierarchy visibility propagation
// ===========================================================================

/// Grandparent hidden → grandchild should not render.
#[test]
fn hidden_grandparent_hides_grandchild() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Grandparent: hidden
    let mut grandparent = CanvasItem::new(CanvasItemId(1));
    grandparent.visible = false;
    vp.add_canvas_item(grandparent);

    // Parent: visible, child of grandparent
    let mut parent = CanvasItem::new(CanvasItemId(2));
    parent.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(parent);

    // Grandchild: visible, draws red rect
    let mut grandchild = make_rect_item(3, 0.0, 0.0, 10.0, 10.0, red());
    grandchild.parent = Some(CanvasItemId(2));
    vp.add_canvas_item(grandchild);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "grandchild of hidden grandparent should not render"
    );
}

/// Middle of hierarchy hidden — grandchild should not render but sibling of
/// the hidden node should render.
#[test]
fn hidden_middle_node_hides_subtree_only() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Root: visible
    let root = CanvasItem::new(CanvasItemId(1));
    vp.add_canvas_item(root);

    // Branch A: hidden
    let mut branch_a = CanvasItem::new(CanvasItemId(2));
    branch_a.parent = Some(CanvasItemId(1));
    branch_a.visible = false;
    vp.add_canvas_item(branch_a);

    // Leaf under A: should be hidden
    let mut leaf_a = make_rect_item(3, 0.0, 0.0, 10.0, 10.0, red());
    leaf_a.parent = Some(CanvasItemId(2));
    vp.add_canvas_item(leaf_a);

    // Branch B: visible
    let mut branch_b = CanvasItem::new(CanvasItemId(4));
    branch_b.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(branch_b);

    // Leaf under B: should render
    let mut leaf_b = make_rect_item(5, 10.0, 0.0, 10.0, 10.0, green());
    leaf_b.parent = Some(CanvasItemId(4));
    vp.add_canvas_item(leaf_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "leaf under hidden branch should not render"
    );
    assert_eq!(
        pixel_at(&frame, 15, 5),
        green(),
        "leaf under visible branch should render"
    );
}

// ===========================================================================
// 3. Z-index with visibility interaction
// ===========================================================================

/// A higher-z item that is hidden via parent visibility should not occlude
/// a lower-z visible item.
#[test]
fn hidden_parent_high_z_child_does_not_occlude() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Visible red at z=0, covers full viewport
    let mut bg = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    // Hidden parent
    let mut hidden_parent = CanvasItem::new(CanvasItemId(2));
    hidden_parent.visible = false;
    vp.add_canvas_item(hidden_parent);

    // High-z green child (would occlude red if visible)
    let mut occluder = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, green());
    occluder.z_index = 10;
    occluder.parent = Some(CanvasItemId(2));
    vp.add_canvas_item(occluder);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        red(),
        "hidden high-z child should not occlude lower-z visible item"
    );
}

/// Self-hidden item at high z does not occlude (baseline — already tested
/// elsewhere, included here for completeness of the z+visibility matrix).
#[test]
fn self_hidden_high_z_does_not_occlude() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut bg = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    let mut occluder = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    occluder.z_index = 10;
    occluder.visible = false;
    vp.add_canvas_item(occluder);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 10, 10), red());
}

// ===========================================================================
// 4. Edge cases
// ===========================================================================

/// Item with no parent and visible=true renders normally (no false positive
/// from ancestor check on orphan items).
#[test]
fn orphan_visible_item_renders() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    vp.add_canvas_item(make_rect_item(1, 0.0, 0.0, 10.0, 10.0, blue()));

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 5, 5), blue());
}

/// Item with a parent ID that doesn't exist in the viewport — should still
/// render (dangling parent reference doesn't suppress visibility).
#[test]
fn dangling_parent_ref_does_not_hide() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    item.parent = Some(CanvasItemId(999)); // nonexistent parent
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "dangling parent ref should not suppress rendering"
    );
}
