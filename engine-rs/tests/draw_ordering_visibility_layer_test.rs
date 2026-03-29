//! pat-5w7j: Validate 2D draw ordering, visibility, and layer semantics.
//!
//! Tests Godot 2D rendering contracts for the software renderer:
//! 1. Z-index draw ordering (higher z renders on top)
//! 2. Stable draw order for equal z-index (insertion order)
//! 3. Negative z-index items render behind z=0
//! 4. Visibility toggling (self and inherited)
//! 5. Canvas layer ordering (layer z_order controls inter-layer draw order)
//! 6. Canvas layer visibility (hidden layer hides all its items)
//! 7. Canvas layer transform application
//! 8. Mixed layered and unlayered item rendering
//!
//! Acceptance: renderer fixtures compare those behaviors against expected output.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
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
fn cyan() -> Color {
    Color::rgb(0.0, 1.0, 1.0)
}
fn magenta() -> Color {
    Color::rgb(1.0, 0.0, 1.0)
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
// Part 1: Z-index draw ordering
// ===========================================================================

/// Higher z_index items render on top of lower z_index items.
#[test]
fn higher_z_renders_on_top() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Red at z=0 (full viewport)
    let mut bg = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    // Green at z=1 (full viewport, should cover red)
    let mut fg = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    fg.z_index = 1;
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "z=1 should render on top of z=0"
    );
}

/// Lower z_index items render behind higher z_index items regardless of insertion order.
#[test]
fn z_ordering_independent_of_insertion_order() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Insert high-z first, then low-z
    let mut fg = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, green());
    fg.z_index = 5;
    vp.add_canvas_item(fg);

    let mut bg = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    bg.z_index = -1;
    vp.add_canvas_item(bg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "z=5 should render on top of z=-1 regardless of insertion order"
    );
}

/// Negative z-index items render behind z=0 items.
#[test]
fn negative_z_renders_behind() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut front = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, blue());
    front.z_index = 0;
    vp.add_canvas_item(front);

    let mut behind = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    behind.z_index = -10;
    vp.add_canvas_item(behind);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "z=0 should be on top of z=-10"
    );
}

/// Items at same z_index: last inserted renders on top (Godot's stable sort behavior).
#[test]
fn same_z_last_inserted_on_top() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Both at z=0; green inserted last should render on top
    let bg = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    vp.add_canvas_item(bg);

    let fg = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "equal z: later insertion should be on top"
    );
}

/// Z-index ordering with partial overlap — only overlapping region shows top item.
#[test]
fn z_ordering_partial_overlap() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Red at z=0, covers left half
    let mut left = make_rect_item(1, 0.0, 0.0, 10.0, 20.0, red());
    left.z_index = 0;
    vp.add_canvas_item(left);

    // Green at z=1, covers right half (overlaps nothing)
    let mut right = make_rect_item(2, 10.0, 0.0, 10.0, 20.0, green());
    right.z_index = 1;
    vp.add_canvas_item(right);

    // Blue at z=2, covers center (overlaps both)
    let mut center = make_rect_item(3, 5.0, 5.0, 10.0, 10.0, blue());
    center.z_index = 2;
    vp.add_canvas_item(center);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 2, 10),
        red(),
        "left area should be red (z=0)"
    );
    assert_eq!(
        pixel_at(&frame, 17, 10),
        green(),
        "right area should be green (z=1)"
    );
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "center overlap should be blue (z=2)"
    );
}

/// Many z-levels — items at z=-5 through z=5 layer correctly.
#[test]
fn many_z_levels_order_correctly() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let colors = [red(), green(), blue(), yellow(), cyan()];
    for (i, color) in colors.iter().enumerate() {
        let mut item = make_rect_item(i as u64 + 1, 0.0, 0.0, 10.0, 10.0, *color);
        item.z_index = i as i32 - 2; // z: -2, -1, 0, 1, 2
        vp.add_canvas_item(item);
    }

    let frame = renderer.render_frame(&vp);
    // Highest z (2) = cyan should be visible
    assert_eq!(
        pixel_at(&frame, 5, 5),
        cyan(),
        "highest z-index should be on top"
    );
}

// ===========================================================================
// Part 2: Visibility semantics
// ===========================================================================

/// Self-hidden item does not render.
#[test]
fn self_hidden_item_does_not_render() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    item.visible = false;
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "hidden item must not render"
    );
}

/// Toggling visibility: item renders when visible, not when hidden.
#[test]
fn visibility_toggle_affects_rendering() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut item = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, green());
    item.visible = true;
    vp.add_canvas_item(item);

    // Frame 1: visible
    let frame1 = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame1, 5, 5), green());

    // Toggle hidden
    vp.get_canvas_item_mut(CanvasItemId(1)).unwrap().visible = false;
    let frame2 = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame2, 5, 5), Color::BLACK);

    // Toggle back
    vp.get_canvas_item_mut(CanvasItemId(1)).unwrap().visible = true;
    let frame3 = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame3, 5, 5), green());
}

/// Hidden item at high z does not prevent lower z items from rendering.
#[test]
fn hidden_item_does_not_block_lower_z() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut bg = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    let mut hidden_fg = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green());
    hidden_fg.z_index = 100;
    hidden_fg.visible = false;
    vp.add_canvas_item(hidden_fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "lower z item should render when higher z is hidden"
    );
}

/// Visibility is per-item — hiding one does not affect siblings.
#[test]
fn sibling_visibility_independent() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 10, Color::BLACK);

    // Left: visible red
    let left = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    vp.add_canvas_item(left);

    // Right: hidden green
    let mut right = make_rect_item(2, 10.0, 0.0, 10.0, 10.0, green());
    right.visible = false;
    vp.add_canvas_item(right);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "visible sibling should render"
    );
    assert_eq!(
        pixel_at(&frame, 15, 5),
        Color::BLACK,
        "hidden sibling should not render"
    );
}

// ===========================================================================
// Part 3: Canvas layer ordering
// ===========================================================================

/// Items in a higher z_order layer render on top of a lower z_order layer.
#[test]
fn layer_z_order_controls_draw_order() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Layer 1: z_order=0
    let mut layer1 = CanvasLayer::new(1);
    layer1.z_order = 0;
    vp.add_canvas_layer(layer1);

    // Layer 2: z_order=1
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 1;
    vp.add_canvas_layer(layer2);

    // Red in layer 1
    let mut item1 = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    item1.layer_id = Some(1);
    vp.add_canvas_item(item1);

    // Green in layer 2 (should render on top)
    let mut item2 = make_rect_item(20, 0.0, 0.0, 20.0, 20.0, green());
    item2.layer_id = Some(2);
    vp.add_canvas_item(item2);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "layer z_order=1 should render on top of z_order=0"
    );
}

/// Layer z_order overrides item z_index across layers.
#[test]
fn layer_z_order_overrides_item_z_index() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Layer A: z_order=10
    let mut layer_a = CanvasLayer::new(1);
    layer_a.z_order = 10;
    vp.add_canvas_layer(layer_a);

    // Layer B: z_order=0
    let mut layer_b = CanvasLayer::new(2);
    layer_b.z_order = 0;
    vp.add_canvas_layer(layer_b);

    // Item in layer A (z_order=10) with low z_index=0
    let mut item_a = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    item_a.layer_id = Some(1);
    item_a.z_index = 0;
    vp.add_canvas_item(item_a);

    // Item in layer B (z_order=0) with high z_index=100
    let mut item_b = make_rect_item(20, 0.0, 0.0, 20.0, 20.0, green());
    item_b.layer_id = Some(2);
    item_b.z_index = 100;
    vp.add_canvas_item(item_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        red(),
        "layer z_order=10 should beat item z_index=100 in layer z_order=0"
    );
}

/// Items within the same layer are sorted by z_index.
#[test]
fn items_within_layer_sorted_by_z_index() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Red at z=0 in layer
    let mut item1 = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    item1.layer_id = Some(1);
    item1.z_index = 0;
    vp.add_canvas_item(item1);

    // Blue at z=5 in same layer (should be on top)
    let mut item2 = make_rect_item(20, 0.0, 0.0, 20.0, 20.0, blue());
    item2.layer_id = Some(1);
    item2.z_index = 5;
    vp.add_canvas_item(item2);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "higher z_index in same layer should render on top"
    );
}

// ===========================================================================
// Part 4: Canvas layer visibility
// ===========================================================================

/// Hidden canvas layer hides all items in it.
#[test]
fn hidden_layer_hides_all_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    let mut item = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        Color::BLACK,
        "items in hidden layer should not render"
    );
}

/// Visible layer allows items to render normally.
#[test]
fn visible_layer_renders_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    let mut item = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, green());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "items in visible layer should render"
    );
}

/// Hidden layer does not affect items in other layers.
#[test]
fn hidden_layer_does_not_affect_other_layers() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Layer 1: hidden
    let mut layer1 = CanvasLayer::new(1);
    layer1.visible = false;
    vp.add_canvas_layer(layer1);

    // Layer 2: visible
    let layer2 = CanvasLayer::new(2);
    vp.add_canvas_layer(layer2);

    let mut item1 = make_rect_item(10, 0.0, 0.0, 10.0, 20.0, red());
    item1.layer_id = Some(1);
    vp.add_canvas_item(item1);

    let mut item2 = make_rect_item(20, 10.0, 0.0, 10.0, 20.0, green());
    item2.layer_id = Some(2);
    vp.add_canvas_item(item2);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 10),
        Color::BLACK,
        "hidden layer item should not render"
    );
    assert_eq!(
        pixel_at(&frame, 15, 10),
        green(),
        "visible layer item should render"
    );
}

// ===========================================================================
// Part 5: Canvas layer transform
// ===========================================================================

/// Canvas layer transform offsets all items within the layer.
#[test]
fn layer_transform_offsets_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 10, Color::BLACK);

    // Layer with a +10px X offset
    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(10.0, 0.0));
    vp.add_canvas_layer(layer);

    // Item at (0,0)-(10,10) in layer — should render at (10,0)-(20,10) on screen
    let mut item = make_rect_item(10, 0.0, 0.0, 10.0, 10.0, red());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "left of layer offset should be clear"
    );
    assert_eq!(
        pixel_at(&frame, 15, 5),
        red(),
        "item should be rendered at layer-offset position"
    );
    assert_eq!(
        pixel_at(&frame, 25, 5),
        Color::BLACK,
        "right of item should be clear"
    );
}

/// Layer transform applies to all items within the layer.
#[test]
fn layer_transform_applies_to_multiple_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 20, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    vp.add_canvas_layer(layer);

    // Item A at (0,0) in layer → screen (5,5)
    let mut item_a = make_rect_item(10, 0.0, 0.0, 5.0, 5.0, red());
    item_a.layer_id = Some(1);
    vp.add_canvas_item(item_a);

    // Item B at (10,0) in layer → screen (15,5)
    let mut item_b = make_rect_item(20, 10.0, 0.0, 5.0, 5.0, green());
    item_b.layer_id = Some(1);
    vp.add_canvas_item(item_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 7, 7),
        red(),
        "item A offset by layer transform"
    );
    assert_eq!(
        pixel_at(&frame, 17, 7),
        green(),
        "item B offset by layer transform"
    );
    assert_eq!(
        pixel_at(&frame, 0, 0),
        Color::BLACK,
        "origin should be clear (offset by +5,+5)"
    );
}

// ===========================================================================
// Part 6: Mixed layered and unlayered items
// ===========================================================================

/// Unlayered items render after all layered items.
#[test]
fn unlayered_items_render_after_layers() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Layer at z_order=0
    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Red in layer
    let mut layered = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    layered.layer_id = Some(1);
    vp.add_canvas_item(layered);

    // Green unlayered (should render on top)
    let unlayered = make_rect_item(20, 0.0, 0.0, 20.0, 20.0, green());
    vp.add_canvas_item(unlayered);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "unlayered items render after layered items"
    );
}

/// Multiple layers plus unlayered items form a complete draw order.
#[test]
fn full_draw_order_layers_then_unlayered() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Layer 1: z_order=-1 (drawn first)
    let mut layer1 = CanvasLayer::new(1);
    layer1.z_order = -1;
    vp.add_canvas_layer(layer1);

    // Layer 2: z_order=5 (drawn second)
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 5;
    vp.add_canvas_layer(layer2);

    // Red in layer 1 (bottom)
    let mut item1 = make_rect_item(10, 0.0, 0.0, 20.0, 20.0, red());
    item1.layer_id = Some(1);
    vp.add_canvas_item(item1);

    // Green in layer 2 (middle)
    let mut item2 = make_rect_item(20, 0.0, 0.0, 20.0, 20.0, green());
    item2.layer_id = Some(2);
    vp.add_canvas_item(item2);

    // Blue unlayered (top)
    let item3 = make_rect_item(30, 0.0, 0.0, 20.0, 20.0, blue());
    vp.add_canvas_item(item3);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        blue(),
        "unlayered item should be on top of all layers"
    );
}

/// Unlayered items do NOT render on top when layers have higher z_order —
/// verify the current rendering order: layers first, then unlayered.
#[test]
fn layer_with_only_unlayered_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // No layers — just unlayered items
    let bg = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    vp.add_canvas_item(bg);

    let mut fg = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green());
    fg.z_index = 1;
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "without layers, z_index still works"
    );
}

// ===========================================================================
// Part 7: Edge cases and stress
// ===========================================================================

/// Empty viewport renders clear color.
#[test]
fn empty_viewport_renders_clear_color() {
    let mut renderer = SoftwareRenderer::new();
    let vp = Viewport::new(10, 10, cyan());

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        cyan(),
        "empty viewport should show clear color"
    );
}

/// Empty layer does not crash or affect rendering.
#[test]
fn empty_layer_no_crash() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    vp.add_canvas_layer(CanvasLayer::new(1));
    vp.add_canvas_layer(CanvasLayer::new(2));

    // Only an unlayered item
    let item = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "empty layers should not affect unlayered items"
    );
}

/// Item visible=false within a visible layer does not render.
#[test]
fn hidden_item_in_visible_layer() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    let mut item = make_rect_item(10, 0.0, 0.0, 10.0, 10.0, red());
    item.layer_id = Some(1);
    item.visible = false;
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "hidden item in visible layer should not render"
    );
}

/// Many items across multiple layers render correctly.
#[test]
fn stress_multiple_layers_many_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 10, Color::BLACK);

    // 10 layers, each with 1 item drawing a 10px-wide column
    let colors = [
        red(),
        green(),
        blue(),
        yellow(),
        cyan(),
        magenta(),
        red(),
        green(),
        blue(),
        yellow(),
    ];

    for i in 0..10u64 {
        let mut layer = CanvasLayer::new(i + 1);
        layer.z_order = i as i32;
        vp.add_canvas_layer(layer);

        let mut item = make_rect_item(
            (i + 1) * 100,
            (i * 10) as f32,
            0.0,
            10.0,
            10.0,
            colors[i as usize],
        );
        item.layer_id = Some(i + 1);
        vp.add_canvas_item(item);
    }

    let frame = renderer.render_frame(&vp);

    // Each 10px column should have its layer's color
    for i in 0..10 {
        let x = i * 10 + 5;
        assert_eq!(
            pixel_at(&frame, x, 5),
            colors[i as usize],
            "column {i} should have its layer color"
        );
    }
}

/// Z-index range: items with extreme z values (i32::MIN, i32::MAX) work.
#[test]
fn extreme_z_index_values() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut bottom = make_rect_item(1, 0.0, 0.0, 10.0, 10.0, red());
    bottom.z_index = i32::MIN;
    vp.add_canvas_item(bottom);

    let mut top = make_rect_item(2, 0.0, 0.0, 10.0, 10.0, green());
    top.z_index = i32::MAX;
    vp.add_canvas_item(top);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "i32::MAX z should be on top of i32::MIN"
    );
}

// ===========================================================================
// Part 5: Parent transform inheritance (pat-s4d)
// ===========================================================================

/// Helper to create a parent-child pair of canvas items.
fn make_parent_child_pair(
    parent_id: u64,
    child_id: u64,
    parent_translate: Vector2,
    child_rect: Rect2,
    child_color: Color,
) -> (CanvasItem, CanvasItem) {
    let mut parent = CanvasItem::new(CanvasItemId(parent_id));
    parent.transform = Transform2D::translated(parent_translate);
    parent.children.push(CanvasItemId(child_id));

    let mut child = CanvasItem::new(CanvasItemId(child_id));
    child.parent = Some(CanvasItemId(parent_id));
    child.commands.push(DrawCommand::DrawRect {
        rect: child_rect,
        color: child_color,
        filled: true,
    });
    (parent, child)
}

/// Child item inherits parent's translation transform.
#[test]
fn child_inherits_parent_translation() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent translated 5px right and 5px down. Child draws at (0,0)-(5,5).
    // With parent transform, child should appear at (5,5)-(10,10).
    let (parent, child) = make_parent_child_pair(
        1,
        2,
        Vector2::new(5.0, 5.0),
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0)),
        red(),
    );
    vp.add_canvas_item(parent);
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // (2,2) should be black (outside child's transformed rect)
    assert_eq!(
        pixel_at(&frame, 2, 2),
        Color::BLACK,
        "outside parent transform should be black"
    );
    // (7,7) should be red (inside child's transformed rect)
    assert_eq!(
        pixel_at(&frame, 7, 7),
        red(),
        "inside parent+child transform should be red"
    );
}

/// Parent's scale transform scales child's draw commands.
#[test]
fn child_inherits_parent_scale() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent scales 2x. Child draws a 5x5 rect at (0,0).
    // Result: child should appear as 10x10 rect.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    parent.children.push(CanvasItemId(2));

    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0)),
        color: green(),
        filled: true,
    });
    vp.add_canvas_item(parent);
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // At 2x scale, child's 5x5 rect becomes 10x10.
    assert_eq!(
        pixel_at(&frame, 1, 1),
        green(),
        "(1,1) should be green (inside scaled rect)"
    );
    assert_eq!(
        pixel_at(&frame, 9, 9),
        green(),
        "(9,9) should be green (inside 10x10 scaled rect)"
    );
    // Just outside the scaled rect
    assert_eq!(
        pixel_at(&frame, 11, 11),
        Color::BLACK,
        "(11,11) outside scaled rect"
    );
}

/// Chained parent transforms: grandparent -> parent -> child.
#[test]
fn grandparent_parent_child_transform_chain() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Grandparent translates (5,5), parent translates (3,3), child draws at (0,0)-(4,4).
    // Result: child should appear at (8,8)-(12,12).
    let mut grandparent = CanvasItem::new(CanvasItemId(1));
    grandparent.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    grandparent.children.push(CanvasItemId(2));

    let mut parent = CanvasItem::new(CanvasItemId(2));
    parent.parent = Some(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(3.0, 3.0));
    parent.children.push(CanvasItemId(3));

    let mut child = CanvasItem::new(CanvasItemId(3));
    child.parent = Some(CanvasItemId(2));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(4.0, 4.0)),
        color: blue(),
        filled: true,
    });
    vp.add_canvas_item(grandparent);
    vp.add_canvas_item(parent);
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // Translated by (5+3, 5+3) = (8,8).
    assert_eq!(
        pixel_at(&frame, 7, 7),
        Color::BLACK,
        "just outside transformed rect"
    );
    assert_eq!(
        pixel_at(&frame, 9, 9),
        blue(),
        "inside transformed rect (8,8)-(12,12)"
    );
    assert_eq!(pixel_at(&frame, 11, 11), blue(), "still inside");
    assert_eq!(
        pixel_at(&frame, 13, 13),
        Color::BLACK,
        "outside transformed rect"
    );
}

/// Parent with no draw commands but with transform still affects child rendering.
#[test]
fn invisible_parent_transform_still_applies_to_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Parent has transform but no draw commands itself.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    parent.children.push(CanvasItemId(2));

    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0)),
        color: yellow(),
        filled: true,
    });
    vp.add_canvas_item(parent);
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "before parent offset should be black"
    );
    assert_eq!(
        pixel_at(&frame, 12, 12),
        yellow(),
        "child should render at parent offset"
    );
}

// ===========================================================================
// Part 6: Z-index with parent-child hierarchy (pat-s4d)
// ===========================================================================

/// Child z-index orders relative to siblings under same parent.
#[test]
fn child_z_index_orders_among_siblings() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    // Red child at z=0, Green child at z=1. Both full viewport.
    let mut child_red = make_rect_item(1, 0.0, 0.0, 20.0, 20.0, red());
    child_red.z_index = 0;
    vp.add_canvas_item(child_red);

    let mut child_green = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, green());
    child_green.z_index = 1;
    vp.add_canvas_item(child_green);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "higher z child should render on top"
    );
}

/// Parent transform + child z-index: transform applies but z-order still correct.
#[test]
fn parent_transform_with_child_z_ordering() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Background rect at z=0 (no parent).
    let mut bg = make_rect_item(1, 0.0, 0.0, 30.0, 30.0, red());
    bg.z_index = 0;
    vp.add_canvas_item(bg);

    // Parent translated, child at z=2 should render on top of bg.
    let mut parent = CanvasItem::new(CanvasItemId(10));
    parent.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    parent.children.push(CanvasItemId(11));
    vp.add_canvas_item(parent);

    let mut child = CanvasItem::new(CanvasItemId(11));
    child.parent = Some(CanvasItemId(10));
    child.z_index = 2;
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)),
        color: green(),
        filled: true,
    });
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    // Green child at z=2 should be on top of red bg at z=0, offset by parent transform.
    assert_eq!(
        pixel_at(&frame, 2, 2),
        red(),
        "outside child's transformed area should be bg"
    );
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "child should render at parent offset, on top of bg"
    );
}

/// Hidden parent hides its child (inherited visibility).
#[test]
fn hidden_parent_hides_child() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.visible = false;
    parent.children.push(CanvasItemId(2));
    vp.add_canvas_item(parent);

    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(20.0, 20.0)),
        color: red(),
        filled: true,
    });
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 10, 10),
        Color::BLACK,
        "child of hidden parent should not render"
    );
}

/// Multiple children under same parent with different z-indices and transforms.
#[test]
fn multiple_children_z_and_transform_combined() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Parent at (0,0) with identity transform.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.children.push(CanvasItemId(2));
    parent.children.push(CanvasItemId(3));
    vp.add_canvas_item(parent);

    // Child A: red, z=0, at (0,0)-(20,20)
    let mut child_a = make_rect_item(2, 0.0, 0.0, 20.0, 20.0, red());
    child_a.parent = Some(CanvasItemId(1));
    child_a.z_index = 0;
    vp.add_canvas_item(child_a);

    // Child B: green, z=1, at (10,10)-(20,20) — overlaps child_a in corner
    let mut child_b = make_rect_item(3, 10.0, 10.0, 20.0, 20.0, green());
    child_b.parent = Some(CanvasItemId(1));
    child_b.z_index = 1;
    vp.add_canvas_item(child_b);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "non-overlapping area should be red"
    );
    assert_eq!(
        pixel_at(&frame, 15, 15),
        green(),
        "overlap area should show higher-z green"
    );
    assert_eq!(pixel_at(&frame, 25, 25), green(), "green-only area");
}

/// Parent scale + child translation compose correctly.
#[test]
fn parent_scale_child_translate_compose() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(30, 30, Color::BLACK);

    // Parent scales 2x.
    let mut parent = CanvasItem::new(CanvasItemId(1));
    parent.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    parent.children.push(CanvasItemId(2));
    vp.add_canvas_item(parent);

    // Child translates by (3,3) in local coords, draws 2x2 rect.
    // Global: translate (3,3)*2 = (6,6), size 2*2 = 4x4.
    // So rect should be at (6,6)-(10,10).
    let mut child = CanvasItem::new(CanvasItemId(2));
    child.parent = Some(CanvasItemId(1));
    child.transform = Transform2D::translated(Vector2::new(3.0, 3.0));
    child.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(2.0, 2.0)),
        color: cyan(),
        filled: true,
    });
    vp.add_canvas_item(child);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 4, 4), Color::BLACK, "before rect");
    assert_eq!(
        pixel_at(&frame, 7, 7),
        cyan(),
        "inside scaled+translated rect"
    );
    assert_eq!(pixel_at(&frame, 11, 11), Color::BLACK, "after rect");
}
