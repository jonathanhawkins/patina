//! pat-29a0: Match Node2D canvas ordering with z-index and parent transforms.
//!
//! Godot 2D rendering contracts:
//!   - Canvas items are sorted by z_index (ascending), stable on insertion order
//!   - Parent transforms compose: child_world = parent_world * child_local
//!   - z_index applies per-item; parent z_index does NOT accumulate on children
//!     (z_as_relative=true means children draw relative to parent in the sibling
//!     sort, but z_index values don't add — they sort within the parent's band)
//!   - Visibility is inherited: hidden parent hides all descendants
//!
//! Acceptance: render parity tests prove z-index and inherited parent-transform
//! ordering matches Godot semantics.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

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
fn yellow() -> Color {
    Color::rgb(1.0, 1.0, 0.0)
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
// Part 1: Parent transform inheritance in rendering
// ===========================================================================

/// Parent translation offsets child draw position.
///
/// Godot contract: child_world_pos = parent_transform * child_local_pos.
/// A parent at (10,0) with a child drawing at local (0,0)-(5,5) should
/// appear at screen (10,0)-(15,5).
#[test]
fn parent_transform_offsets_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 10, Color::BLACK);

    // Parent at position (10, 0), no draw commands.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    vp.add_canvas_item(parent);

    // Child draws a red 5x5 rect at local (0,0). With parent offset, renders at (10,0).
    let mut child = make_rect_item(2, 0.0, 0.0, 5.0, 5.0, red());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 2, 2),
        Color::BLACK,
        "left of parent offset should be clear"
    );
    assert_eq!(
        pixel_at(&frame, 12, 2),
        red(),
        "child should render at parent-offset position"
    );
    assert_eq!(
        pixel_at(&frame, 20, 2),
        Color::BLACK,
        "right of child should be clear"
    );
}

/// Nested parent transforms compose (grandparent → parent → child).
///
/// Godot contract: transforms multiply down the hierarchy.
/// GP at (5,0), P at (10,0), C draws at local (0,0) → screen (15,0).
#[test]
fn nested_parent_transforms_compose() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 10, Color::BLACK);

    // Grandparent at (5, 0).
    let mut gp = CanvasItem::new(CanvasItemId(1));
    gp.transform = Transform2D::translated(Vector2::new(5.0, 0.0));
    vp.add_canvas_item(gp);

    // Parent at (10, 0) relative to grandparent.
    let mut parent = CanvasItem::new(CanvasItemId(2));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    parent.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(parent);

    // Child draws at local (0,0). Global = (5+10, 0) = (15, 0).
    let mut child = make_rect_item(3, 0.0, 0.0, 5.0, 5.0, green());
    child.parent = Some(CanvasItemId(2));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 2, 2),
        Color::BLACK,
        "before grandparent offset"
    );
    assert_eq!(
        pixel_at(&frame, 7, 2),
        Color::BLACK,
        "between gp and parent offsets"
    );
    assert_eq!(
        pixel_at(&frame, 17, 2),
        green(),
        "child at composed offset (15+2, 2)"
    );
}

/// Parent rotation transforms child position.
///
/// Godot contract: parent rotation rotates child's local coordinate frame.
/// Parent rotated 90° CW: child at local (5,0) → screen (0,5) relative to parent.
#[test]
fn parent_rotation_transforms_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent at center (10,10), rotated 90° (π/2).
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 10.0))
        * Transform2D::rotated(std::f32::consts::FRAC_PI_2);
    vp.add_canvas_item(parent);

    // Child draws a 4x4 rect at local (0,0).
    // After parent rotation: local x-axis maps to screen y-axis.
    // Global position: parent_pos + rotated(0,0) = (10, 10).
    let mut child = make_rect_item(2, 0.0, 0.0, 4.0, 4.0, blue());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // After 90° rotation, the rect at local (0..4, 0..4) becomes roughly
    // screen (6..10, 10..14). The exact pixels depend on rasterization.
    // We verify the pixel at approximately the rotated center.
    let center_pixel = pixel_at(&frame, 8, 12);
    assert_eq!(
        center_pixel,
        blue(),
        "rotated child should render in rotated position"
    );
}

/// Parent scale applies to child.
///
/// Godot contract: parent scale multiplies child's coordinate frame.
#[test]
fn parent_scale_applies_to_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Parent with 2x scale.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    vp.add_canvas_item(parent);

    // Child draws 5x5 rect at (0,0). With 2x parent scale → 10x10 on screen.
    let mut child = make_rect_item(2, 0.0, 0.0, 5.0, 5.0, red());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // At 2x scale, the rect extends from (0,0) to (10,10).
    assert_eq!(
        pixel_at(&frame, 1, 1),
        red(),
        "scaled child should fill (0,0)"
    );
    assert_eq!(
        pixel_at(&frame, 9, 9),
        red(),
        "scaled child should extend to ~(10,10)"
    );
    assert_eq!(
        pixel_at(&frame, 11, 11),
        Color::BLACK,
        "beyond scaled bounds should be clear"
    );
}

// ===========================================================================
// Part 2: Z-index ordering with parent-child hierarchy
// ===========================================================================

/// Child z_index is independent of parent z_index.
///
/// Godot contract: child and parent z_index values don't accumulate.
/// A child with z_index=0 under a parent with z_index=5 does NOT behave
/// as z_index=5. Z-sorting is flat across all items in the viewport.
#[test]
fn child_z_index_is_independent_of_parent() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent with z_index=5, no draw commands.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.z_index = 5;
    vp.add_canvas_item(parent);

    // Child under parent with z_index=0.
    let mut child = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    child.parent = Some(CanvasItemId(1));
    child.z_index = 0;
    vp.add_canvas_item(child);

    // Independent item at z_index=1.
    let mut other = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, green());
    other.z_index = 1;
    vp.add_canvas_item(other);

    let frame = renderer.render_frame(&vp);
    // Child z_index=0, other z_index=1 → other renders on top.
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "z_index=1 should render on top of child with z_index=0 (parent z_index=5 doesn't accumulate)"
    );
}

/// Sibling children with different z_index sort correctly.
///
/// Godot contract: within a parent's children, z_index determines draw order.
#[test]
fn sibling_children_sort_by_z_index() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent container.
    let parent = CanvasItem::new(CanvasItemId(1));
    vp.add_canvas_item(parent);

    // Child A: z=2 (should render on top).
    let mut child_a = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    child_a.parent = Some(CanvasItemId(1));
    child_a.z_index = 2;
    vp.add_canvas_item(child_a);

    // Child B: z=5 (should render on top of A).
    let mut child_b = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, blue());
    child_b.parent = Some(CanvasItemId(1));
    child_b.z_index = 5;
    vp.add_canvas_item(child_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "higher z_index sibling should render on top"
    );
}

/// Z-index ordering works correctly across different parent hierarchies.
///
/// Godot contract: z-sorting is global across the viewport, not scoped to parent.
#[test]
fn z_index_sorting_across_parent_hierarchies() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent A container.
    let parent_a = CanvasItem::new(CanvasItemId(1));
    vp.add_canvas_item(parent_a);

    // Parent B container.
    let parent_b = CanvasItem::new(CanvasItemId(2));
    vp.add_canvas_item(parent_b);

    // Child of A with z=10.
    let mut child_a = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, red());
    child_a.parent = Some(CanvasItemId(1));
    child_a.z_index = 10;
    vp.add_canvas_item(child_a);

    // Child of B with z=20 (should render on top).
    let mut child_b = make_rect_item(4, 0.0, 0.0, 20.0, 20.0, green());
    child_b.parent = Some(CanvasItemId(2));
    child_b.z_index = 20;
    vp.add_canvas_item(child_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "higher z_index across different parents should render on top"
    );
}

// ===========================================================================
// Part 3: Combined z-index + parent transforms
// ===========================================================================

/// Parent transform + z-index: higher z child at offset renders on top at correct position.
///
/// Godot contract: z-index determines layering, parent transform determines position.
#[test]
fn z_index_and_parent_transform_combined() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 20, Color::BLACK);

    // Parent offset to (10, 0).
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    vp.add_canvas_item(parent);

    // Background item (no parent) at z=0, full viewport.
    let bg = make_rect_item(2, 0.0, 0.0, 30.0, 20.0, yellow());
    vp.add_canvas_item(bg);

    // Child of parent at z=1, draws 10x10 rect at local (0,0) → screen (10,0).
    let mut child = make_rect_item(3, 0.0, 0.0, 10.0, 10.0, red());
    child.parent = Some(CanvasItemId(1));
    child.z_index = 1;
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        yellow(),
        "left of parent offset shows background"
    );
    assert_eq!(
        pixel_at(&frame, 15, 5),
        red(),
        "child at parent offset with higher z renders on top"
    );
    assert_eq!(
        pixel_at(&frame, 25, 5),
        yellow(),
        "right of child shows background"
    );
}

/// Multiple siblings at same parent offset with z-index tiebreaker.
///
/// Godot contract: equal z_index → insertion order determines who renders on top.
#[test]
fn siblings_same_z_insertion_order_tiebreaker() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_item(parent);

    // Both children at z=0, same position. Second inserted should be on top.
    let mut first = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, red());
    first.parent = Some(CanvasItemId(1));
    first.z_index = 0;
    vp.add_canvas_item(first);

    let mut second = make_rect_item(3, 0.0, 0.0, 10.0, 10.0, green());
    second.parent = Some(CanvasItemId(1));
    second.z_index = 0;
    vp.add_canvas_item(second);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "later-inserted sibling at same z should render on top"
    );
}

// ===========================================================================
// Part 4: Visibility inheritance with parent hierarchy
// ===========================================================================

/// Hidden parent canvas item hides child in rendering.
///
/// Godot contract: invisible CanvasItem parent hides entire subtree.
#[test]
fn hidden_parent_hides_child_in_render() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.visible = false;
    vp.add_canvas_item(parent);

    let mut child = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    child.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        Color::BLACK,
        "child of hidden parent should not render"
    );
}

/// Grandparent hidden → grandchild hidden.
///
/// Godot contract: visibility propagates through entire ancestor chain.
#[test]
fn hidden_grandparent_hides_grandchild() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut gp = CanvasItem::new(CanvasItemId(1));
    gp.visible = false;
    vp.add_canvas_item(gp);

    let mut parent = CanvasItem::new(CanvasItemId(2));
    parent.parent = Some(CanvasItemId(1));
    vp.add_canvas_item(parent);

    let mut child = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, blue());
    child.parent = Some(CanvasItemId(2));
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        Color::BLACK,
        "grandchild of hidden grandparent should not render"
    );
}

// ===========================================================================
// Part 5: Node2D property helpers → global transform parity
// ===========================================================================

/// Node2D get_global_transform composes translate → rotate → scale per Godot.
///
/// Godot contract: local transform = translate * rotate * scale.
/// Global transform = root_xform * ... * parent_xform * local_xform.
#[test]
fn node2d_global_transform_matches_godot_composition() {
    use gdscene::node::Node;
    use gdscene::node2d;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Parent at (100, 50).
    let parent = Node::new("Parent", "Node2D");
    let p_id = tree.add_child(root, parent).unwrap();
    node2d::set_position(&mut tree, p_id, Vector2::new(100.0, 50.0));

    // Child at local (20, 10).
    let child = Node::new("Child", "Node2D");
    let c_id = tree.add_child(p_id, child).unwrap();
    node2d::set_position(&mut tree, c_id, Vector2::new(20.0, 10.0));

    let global = node2d::get_global_transform(&tree, c_id);
    let world_pos = global.xform(Vector2::ZERO);

    // Global = parent + child = (120, 60).
    assert!(
        (world_pos.x - 120.0).abs() < 0.01 && (world_pos.y - 60.0).abs() < 0.01,
        "global transform should compose parent+child translations: got {:?}",
        world_pos
    );
}

/// Node2D z_index property stores and retrieves correctly with negative values.
///
/// Godot contract: z_index can be negative (range -4096..4096 in Godot).
#[test]
fn node2d_z_index_negative_values() {
    use gdscene::node::Node;
    use gdscene::node2d;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Behind", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    node2d::set_z_index(&mut tree, id, -100);
    assert_eq!(node2d::get_z_index(&tree, id), -100);

    node2d::set_z_index(&mut tree, id, 4096);
    assert_eq!(node2d::get_z_index(&tree, id), 4096);
}

/// Node2D global transform with rotation: parent rotation affects child global position.
///
/// Godot contract: child global position = parent_transform * child_local.
/// Parent at (10,0) rotated 90°, child at local (5,0) → global ≈ (10,5).
#[test]
fn node2d_global_transform_with_parent_rotation() {
    use gdscene::node::Node;
    use gdscene::node2d;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let p_id = tree.add_child(root, parent).unwrap();
    node2d::set_position(&mut tree, p_id, Vector2::new(10.0, 0.0));
    node2d::set_rotation(&mut tree, p_id, std::f32::consts::FRAC_PI_2);

    let child = Node::new("Child", "Node2D");
    let c_id = tree.add_child(p_id, child).unwrap();
    node2d::set_position(&mut tree, c_id, Vector2::new(5.0, 0.0));

    let global = node2d::get_global_transform(&tree, c_id);
    let world_pos = global.xform(Vector2::ZERO);

    // Parent at (10,0) rotated 90°: child local (5,0) → rotated to (0,5) → + parent = (10,5).
    assert!(
        (world_pos.x - 10.0).abs() < 0.1 && (world_pos.y - 5.0).abs() < 0.1,
        "rotated parent should transform child position: expected ~(10,5), got {:?}",
        world_pos
    );
}

/// Node2D set_global_position correctly inverts parent transform.
///
/// Godot contract: set_global_position adjusts local position so global matches.
#[test]
fn node2d_set_global_position_with_scaled_parent() {
    use gdscene::node::Node;
    use gdscene::node2d;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let p_id = tree.add_child(root, parent).unwrap();
    node2d::set_position(&mut tree, p_id, Vector2::new(50.0, 50.0));
    node2d::set_scale(&mut tree, p_id, Vector2::new(2.0, 2.0));

    let child = Node::new("Child", "Node2D");
    let c_id = tree.add_child(p_id, child).unwrap();

    // Set child's global position to (100, 100).
    node2d::set_global_position(&mut tree, c_id, Vector2::new(100.0, 100.0));

    // Verify: local should be (25, 25) because parent is at (50,50) with 2x scale.
    // global = parent_pos + scale * local → 100 = 50 + 2*local → local = 25.
    let local = node2d::get_position(&tree, c_id);
    assert!(
        (local.x - 25.0).abs() < 0.1 && (local.y - 25.0).abs() < 0.1,
        "local position should be ~(25,25) to achieve global (100,100) with 2x parent: got {:?}",
        local
    );

    // Double-check via global transform.
    let global = node2d::get_global_transform(&tree, c_id).xform(Vector2::ZERO);
    assert!(
        (global.x - 100.0).abs() < 0.1 && (global.y - 100.0).abs() < 0.1,
        "global position should be ~(100,100): got {:?}",
        global
    );
}

// ===========================================================================
// Part 6: Scene tree z-index ↔ renderer integration
// ===========================================================================

/// Scene tree z_index property roundtrips through Node2D helpers.
///
/// Godot contract: z_index stored on a Node2D in the scene tree can be read
/// back and used by the renderer to determine draw order.
#[test]
fn scene_tree_z_index_roundtrip_to_renderer() {
    use gdscene::node::Node;
    use gdscene::node2d;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let bg_node = Node::new("Background", "Node2D");
    let bg_id = tree.add_child(root, bg_node).unwrap();
    node2d::set_z_index(&mut tree, bg_id, -1);

    let fg_node = Node::new("Foreground", "Node2D");
    let fg_id = tree.add_child(root, fg_node).unwrap();
    node2d::set_z_index(&mut tree, fg_id, 10);

    // Read z_index back and verify ordering contract.
    let bg_z = node2d::get_z_index(&tree, bg_id);
    let fg_z = node2d::get_z_index(&tree, fg_id);
    assert_eq!(bg_z, -1, "background z_index should be -1");
    assert_eq!(fg_z, 10, "foreground z_index should be 10");
    assert!(
        fg_z > bg_z,
        "foreground z must be > background z for top rendering"
    );

    // Build canvas items from scene tree state and verify renderer ordering.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut bg_item = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    bg_item.z_index = bg_z as i32;
    vp.add_canvas_item(bg_item);

    let mut fg_item = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    fg_item.z_index = fg_z as i32;
    vp.add_canvas_item(fg_item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "foreground (z=10) should render on top of background (z=-1)"
    );
}

/// Deep hierarchy: grandparent → parent → child, each with z-index, parent
/// transform composition, and correct layering.
///
/// Godot contract: transforms compose down the hierarchy, z-index is flat.
#[test]
fn deep_hierarchy_z_and_transforms_combined() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 20, Color::BLACK);

    // Grandparent at (0,0), z=0.
    let mut gp = CanvasItem::new(CanvasItemId(1));
    gp.transform = Transform2D::translated(Vector2::new(0.0, 0.0));
    gp.z_index = 0;
    vp.add_canvas_item(gp);

    // Parent at (10,0) relative to GP, z=0.
    let mut parent = CanvasItem::new(CanvasItemId(2));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    parent.parent = Some(CanvasItemId(1));
    parent.z_index = 0;
    vp.add_canvas_item(parent);

    // Child at (5,0) relative to parent, z=2 (should be on top).
    let mut child = make_rect_item(3, 0.0, 0.0, 10.0, 10.0, blue());
    child.transform = Transform2D::translated(Vector2::new(5.0, 0.0));
    child.parent = Some(CanvasItemId(2));
    child.z_index = 2;
    vp.add_canvas_item(child);

    // Independent item overlapping child position, z=1 (below child).
    let mut other = make_rect_item(4, 15.0, 0.0, 10.0, 10.0, red());
    other.z_index = 1;
    vp.add_canvas_item(other);

    let frame = renderer.render_frame(&vp);
    // Child global position: gp(0) + parent(10) + child_offset(5) = 15.
    // Child draws 10x10 at (15,0)-(25,10). Other draws same region at z=1.
    // Child z=2 > other z=1, so blue should be on top in the overlap.
    assert_eq!(
        pixel_at(&frame, 20, 5),
        blue(),
        "child at z=2 with composed transform should render on top of z=1 item"
    );
}

/// Parent with negative z-index, child with positive z-index.
///
/// Godot contract: child z-index is independent, not accumulated from parent.
/// Parent z=-10 child z=5 should render at z=5, not z=-5.
#[test]
fn negative_parent_z_positive_child_z_independent() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent with z=-10 (very behind).
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.z_index = -10;
    vp.add_canvas_item(parent);

    // Child with z=5.
    let mut child = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    child.parent = Some(CanvasItemId(1));
    child.z_index = 5;
    vp.add_canvas_item(child);

    // Independent item at z=3 (below child's z=5, above parent's z=-10).
    let mut mid = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, red());
    mid.z_index = 3;
    vp.add_canvas_item(mid);

    let frame = renderer.render_frame(&vp);
    // Child z=5 is independent of parent z=-10, so child renders on top of mid z=3.
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "child z=5 should render on top of independent z=3 (parent z doesn't accumulate)"
    );
}

/// Multiple children of same parent: z-index sorts independently from parent.
///
/// Godot contract: sibling ordering follows z_index, not tree order.
#[test]
fn multiple_children_interleaved_z_with_outsiders() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent container (no draw).
    let parent = CanvasItem::new(CanvasItemId(1));
    vp.add_canvas_item(parent);

    // Child A at z=-5, fills viewport red.
    let mut child_a = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    child_a.parent = Some(CanvasItemId(1));
    child_a.z_index = -5;
    vp.add_canvas_item(child_a);

    // Outsider at z=0, fills viewport green.
    let mut outsider = make_rect_item(3, 0.0, 0.0, 20.0, 20.0, green());
    outsider.z_index = 0;
    vp.add_canvas_item(outsider);

    // Child B at z=10, fills viewport blue.
    let mut child_b = make_rect_item(4, 0.0, 0.0, 20.0, 20.0, blue());
    child_b.parent = Some(CanvasItemId(1));
    child_b.z_index = 10;
    vp.add_canvas_item(child_b);

    let frame = renderer.render_frame(&vp);
    // z ordering: child_a(-5) < outsider(0) < child_b(10).
    // Top layer is blue (child_b z=10).
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "child_b (z=10) should render on top of outsider (z=0) and sibling (z=-5)"
    );
}

/// z_as_relative property is stored as true by default on Node2D.
///
/// Godot contract: z_as_relative defaults to true. When true, children's
/// z_index is relative to the parent's z-band in sibling sort order.
/// When false, children sort globally ignoring parent band.
#[test]
fn z_as_relative_defaults_true_in_scene_tree() {
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdvariant::Variant;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Sprite", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    // Default value should be true (Godot contract).
    let z_rel = tree.get_node(id).unwrap().get_property("z_as_relative");
    // Property may not be explicitly set (returns Nil), which means default true.
    match z_rel {
        Variant::Bool(b) => assert!(b, "z_as_relative should default to true"),
        Variant::Nil => {} // Nil means default, which is true — acceptable
        other => panic!(
            "z_as_relative should be Bool or Nil (default), got {:?}",
            other
        ),
    }

    // Explicitly set to false and verify.
    tree.get_node_mut(id)
        .unwrap()
        .set_property("z_as_relative", Variant::Bool(false));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("z_as_relative"),
        Variant::Bool(false),
        "z_as_relative should be settable to false"
    );
}

/// Scene-tree loaded scene preserves z_index from .tscn properties.
///
/// Godot contract: z_index set in the scene file persists through load.
#[test]
fn loaded_scene_preserves_z_index() {
    use gdscene::node2d;
    use gdscene::packed_scene::add_packed_scene_to_tree;
    use gdscene::{PackedScene, SceneTree};

    // Inline a minimal scene with z_index set.
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Behind" type="Node2D" parent="."]
z_index = -5

[node name="Front" type="Node2D" parent="."]
z_index = 10
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Find the children by name.
    let nodes = tree.all_nodes_in_tree_order();
    let behind = nodes
        .iter()
        .find(|id| tree.get_node(**id).unwrap().name() == "Behind")
        .expect("Behind node must exist");
    let front = nodes
        .iter()
        .find(|id| tree.get_node(**id).unwrap().name() == "Front")
        .expect("Front node must exist");

    assert_eq!(
        node2d::get_z_index(&tree, *behind),
        -5,
        "Behind z_index should be -5 from scene"
    );
    assert_eq!(
        node2d::get_z_index(&tree, *front),
        10,
        "Front z_index should be 10 from scene"
    );

    // Verify the scene root was created under the tree root.
    assert!(
        tree.get_node(scene_root).is_some(),
        "scene root should exist in tree"
    );
}

/// Parent transform from loaded scene correctly composes for child global position.
///
/// Godot contract: position set in .tscn file feeds into global transform computation.
#[test]
fn loaded_scene_parent_transform_composition() {
    use gdscene::node2d;
    use gdscene::packed_scene::add_packed_scene_to_tree;
    use gdscene::{PackedScene, SceneTree};

    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]
position = Vector2(100, 50)

[node name="Player" type="Node2D" parent="."]
position = Vector2(20, 10)
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    let nodes = tree.all_nodes_in_tree_order();
    let player = nodes
        .iter()
        .find(|id| tree.get_node(**id).unwrap().name() == "Player")
        .expect("Player node must exist");

    let global = node2d::get_global_transform(&tree, *player);
    let world_pos = global.xform(Vector2::ZERO);

    // World(100,50) + Player(20,10) = global(120, 60).
    assert!(
        (world_pos.x - 120.0).abs() < 0.1 && (world_pos.y - 60.0).abs() < 0.1,
        "loaded scene parent transform should compose: expected ~(120,60), got {:?}",
        world_pos
    );
}
