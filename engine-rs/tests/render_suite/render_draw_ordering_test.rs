//! pat-wb3: Validate 2D draw ordering, visibility, and layer semantics.
//!
//! Pixel-level tests verifying:
//! - z_index draw ordering (higher z renders on top)
//! - visible=false hides nodes entirely
//! - Tree-order (insertion order) rendering within the same z_index
//! - CanvasLayer z_order and visibility semantics
//! - Cross-layer and unlayered item ordering

use std::path::PathBuf;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::compare::compare_framebuffers;
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdrender2d::texture::load_png;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const W: u32 = 16;
const H: u32 = 16;
const TOL: f32 = 0.001;

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

/// Creates a canvas item that fills the entire viewport with the given color.
fn full_rect(id: u64, z: i32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.z_index = z;
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(W as f32, H as f32)),
        color,
        filled: true,
    });
    item
}

/// Creates a canvas item drawing a rect at a specific position.
fn rect_at(id: u64, z: i32, x: f32, y: f32, w: f32, h: f32, color: Color) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.z_index = z;
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        color,
        filled: true,
    });
    item
}

// ===========================================================================
// Z-INDEX DRAW ORDERING
// ===========================================================================

#[test]
fn z_index_higher_renders_on_top() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Red at z=0, green at z=1 — green should be the final visible color.
    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(full_rect(2, 1, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, green(), TOL);
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
    assert_pixel_color(&fb, W - 1, H - 1, green(), TOL);
}

#[test]
fn z_index_lower_renders_underneath() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Green at z=10, red at z=-5 — green on top everywhere.
    vp.add_canvas_item(full_rect(1, -5, red()));
    vp.add_canvas_item(full_rect(2, 10, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, green(), TOL);
}

#[test]
fn z_index_respects_order_regardless_of_insertion() {
    // Even if the high-z item is added first, it should still render on top.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Add blue (z=5) first, then red (z=0).
    vp.add_canvas_item(full_rect(1, 5, blue()));
    vp.add_canvas_item(full_rect(2, 0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    // Blue has higher z, so blue should be on top despite being added first.
    assert_pixel_color(&fb, 0, 0, blue(), TOL);
    assert_pixel_color(&fb, W - 1, H - 1, blue(), TOL);
}

#[test]
fn z_index_negative_values_render_below_zero() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, -10, red()));
    vp.add_canvas_item(full_rect(2, 0, green()));
    vp.add_canvas_item(full_rect(3, -5, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    // z=0 (green) is highest, should be on top.
    assert_pixel_color(&fb, 0, 0, green(), TOL);
}

#[test]
fn z_index_three_layer_sandwich() {
    // Three overlapping items: only the partial regions should show through.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // z=0: red fills everything.
    vp.add_canvas_item(full_rect(1, 0, red()));

    // z=1: green fills left half.
    vp.add_canvas_item(rect_at(2, 1, 0.0, 0.0, (W / 2) as f32, H as f32, green()));

    // z=2: blue fills top-left quadrant.
    vp.add_canvas_item(rect_at(
        3,
        2,
        0.0,
        0.0,
        (W / 2) as f32,
        (H / 2) as f32,
        blue(),
    ));

    let fb = capture_frame(&mut renderer, &vp);

    // Top-left quadrant → blue (z=2).
    assert_pixel_color(&fb, 1, 1, blue(), TOL);
    // Bottom-left → green (z=1, blue doesn't cover here).
    assert_pixel_color(&fb, 1, H - 2, green(), TOL);
    // Right half → red (z=0, nothing covers here).
    assert_pixel_color(&fb, W - 2, H / 2, red(), TOL);
}

#[test]
fn z_index_pixel_proof_five_overlapping_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let colors = [red(), green(), blue(), yellow(), cyan()];
    for (i, &color) in colors.iter().enumerate() {
        vp.add_canvas_item(full_rect((i + 1) as u64, i as i32, color));
    }

    let fb = capture_frame(&mut renderer, &vp);
    // Cyan (z=4) is highest, should be the final color.
    assert_pixel_color(&fb, W / 2, H / 2, cyan(), TOL);
}

// ===========================================================================
// TREE-ORDER (INSERTION ORDER) WITHIN SAME Z_INDEX
// ===========================================================================

#[test]
fn same_z_insertion_order_last_wins() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // All at z=0; last inserted (blue) should be on top.
    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(full_rect(2, 0, green()));
    vp.add_canvas_item(full_rect(3, 0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, blue(), TOL);
    assert_pixel_color(&fb, W - 1, H - 1, blue(), TOL);
}

#[test]
fn same_z_partial_overlap_insertion_determines_top() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Both at z=0. Red fills everything, green fills right half.
    // Green is added second → green wins in overlap region.
    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(rect_at(
        2,
        0,
        (W / 2) as f32,
        0.0,
        (W / 2) as f32,
        H as f32,
        green(),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    // Left half: only red.
    assert_pixel_color(&fb, 1, H / 2, red(), TOL);
    // Right half: green on top.
    assert_pixel_color(&fb, W - 2, H / 2, green(), TOL);
}

#[test]
fn same_z_many_items_last_inserted_wins() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // 10 items all at z=0, each a different shade. Last one should win.
    for i in 0..10u64 {
        let intensity = (i + 1) as f32 / 10.0;
        vp.add_canvas_item(full_rect(i + 1, 0, Color::rgb(intensity, 0.0, 0.0)));
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(0, 0);
    // Last item has intensity 1.0.
    assert!(
        (pixel.r - 1.0).abs() < 0.02,
        "Expected r≈1.0, got {}",
        pixel.r
    );
}

#[test]
fn stable_sort_mixed_z_and_insertion() {
    // Items: A(z=0), B(z=1), C(z=0), D(z=1).
    // Expected render order: A, C (z=0 in insertion order), then B, D (z=1 in insertion order).
    // Final pixel at overlap: D (last drawn).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red())); // A
    vp.add_canvas_item(full_rect(2, 1, green())); // B
    vp.add_canvas_item(full_rect(3, 0, blue())); // C
    vp.add_canvas_item(full_rect(4, 1, yellow())); // D

    let fb = capture_frame(&mut renderer, &vp);
    // D (z=1, inserted after B) should be on top.
    assert_pixel_color(&fb, W / 2, H / 2, yellow(), TOL);
}

// ===========================================================================
// VISIBILITY: visible=false HIDES NODES
// ===========================================================================

#[test]
fn invisible_item_not_rendered() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let mut item = full_rect(1, 0, red());
    item.visible = false;
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Should remain black — invisible item should not draw.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
    assert_pixel_color(&fb, W / 2, H / 2, Color::BLACK, TOL);
}

#[test]
fn invisible_item_between_visible_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // z=0: red, z=1: green (invisible), z=2: blue covers left half.
    vp.add_canvas_item(full_rect(1, 0, red()));
    let mut mid = full_rect(2, 1, green());
    mid.visible = false;
    vp.add_canvas_item(mid);
    vp.add_canvas_item(rect_at(3, 2, 0.0, 0.0, (W / 2) as f32, H as f32, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    // Left half: blue on top of red (green skipped).
    assert_pixel_color(&fb, 1, H / 2, blue(), TOL);
    // Right half: red only (green invisible, blue doesn't cover).
    assert_pixel_color(&fb, W - 2, H / 2, red(), TOL);
}

#[test]
fn invisible_highest_z_item_reveals_lower() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));
    let mut top = full_rect(2, 10, green());
    top.visible = false;
    vp.add_canvas_item(top);

    let fb = capture_frame(&mut renderer, &vp);
    // Green is invisible, so red shows through.
    assert_pixel_color(&fb, W / 2, H / 2, red(), TOL);
}

#[test]
fn all_items_invisible_shows_clear_color() {
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.2, 0.2, 0.2);
    let mut vp = Viewport::new(W, H, clear);

    let mut a = full_rect(1, 0, red());
    a.visible = false;
    let mut b = full_rect(2, 1, green());
    b.visible = false;

    vp.add_canvas_item(a);
    vp.add_canvas_item(b);

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, clear, TOL);
    assert_pixel_color(&fb, W - 1, H - 1, clear, TOL);
}

#[test]
fn toggle_visibility_affects_rendering() {
    // Render once visible, then set invisible and re-render.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));

    let fb1 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb1, 0, 0, red(), TOL);

    // Make invisible via viewport mutation.
    vp.get_canvas_item_mut(CanvasItemId(1)).unwrap().visible = false;

    let fb2 = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb2, 0, 0, Color::BLACK, TOL);
}

// ===========================================================================
// CANVAS LAYER Z-ORDER AND VISIBILITY
// ===========================================================================

#[test]
fn layer_z_order_determines_draw_order() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Layer 1 (z_order=0), Layer 2 (z_order=1).
    let mut layer1 = CanvasLayer::new(1);
    layer1.z_order = 0;
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 1;
    vp.add_canvas_layer(layer1);
    vp.add_canvas_layer(layer2);

    // Red in layer 1, green in layer 2.
    let mut item_a = full_rect(1, 0, red());
    item_a.layer_id = Some(1);
    let mut item_b = full_rect(2, 0, green());
    item_b.layer_id = Some(2);

    vp.add_canvas_item(item_a);
    vp.add_canvas_item(item_b);

    let fb = capture_frame(&mut renderer, &vp);
    // Layer 2 (z_order=1) renders after layer 1 → green on top.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn layer_z_order_reverse_insertion() {
    // Add higher z_order layer first — should still render in correct order.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let mut layer_top = CanvasLayer::new(1);
    layer_top.z_order = 10;
    let mut layer_bottom = CanvasLayer::new(2);
    layer_bottom.z_order = -5;
    // Add top first.
    vp.add_canvas_layer(layer_top);
    vp.add_canvas_layer(layer_bottom);

    let mut item_top = full_rect(1, 0, blue());
    item_top.layer_id = Some(1);
    let mut item_bottom = full_rect(2, 0, red());
    item_bottom.layer_id = Some(2);

    vp.add_canvas_item(item_top);
    vp.add_canvas_item(item_bottom);

    let fb = capture_frame(&mut renderer, &vp);
    // Layer 1 has z_order=10, should render on top → blue.
    assert_pixel_color(&fb, W / 2, H / 2, blue(), TOL);
}

#[test]
fn invisible_layer_hides_all_its_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.visible = false;
    vp.add_canvas_layer(layer);

    let mut item = full_rect(1, 0, red());
    item.layer_id = Some(1);
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Layer invisible → item not rendered.
    assert_pixel_color(&fb, 0, 0, Color::BLACK, TOL);
    assert_pixel_color(&fb, W / 2, H / 2, Color::BLACK, TOL);
}

#[test]
fn invisible_layer_does_not_affect_other_layers() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let mut layer1 = CanvasLayer::new(1);
    layer1.visible = false;
    layer1.z_order = 0;
    let mut layer2 = CanvasLayer::new(2);
    layer2.z_order = 1;

    vp.add_canvas_layer(layer1);
    vp.add_canvas_layer(layer2);

    let mut item_a = full_rect(1, 0, red());
    item_a.layer_id = Some(1);
    let mut item_b = full_rect(2, 0, green());
    item_b.layer_id = Some(2);

    vp.add_canvas_item(item_a);
    vp.add_canvas_item(item_b);

    let fb = capture_frame(&mut renderer, &vp);
    // Layer 1 invisible, layer 2 visible → green shows.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn layers_render_before_unlayered_items() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Red in layer 1.
    let mut layered = full_rect(1, 0, red());
    layered.layer_id = Some(1);
    vp.add_canvas_item(layered);

    // Green unlayered.
    vp.add_canvas_item(full_rect(2, 0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // Unlayered items render after all layers → green on top.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn unlayered_items_on_top_even_with_high_layer_z() {
    // Even if a layer has high z_order, unlayered items render after all layers.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let mut layer = CanvasLayer::new(1);
    layer.z_order = 1000;
    vp.add_canvas_layer(layer);

    let mut layered = full_rect(1, 0, red());
    layered.layer_id = Some(1);
    vp.add_canvas_item(layered);

    vp.add_canvas_item(full_rect(2, 0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    // Unlayered renders after layers → green on top.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn z_index_within_same_layer() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    let layer = CanvasLayer::new(1);
    vp.add_canvas_layer(layer);

    // Two items in same layer, different z.
    let mut low = full_rect(1, 0, red());
    low.layer_id = Some(1);
    let mut high = full_rect(2, 5, green());
    high.layer_id = Some(1);

    vp.add_canvas_item(low);
    vp.add_canvas_item(high);

    let fb = capture_frame(&mut renderer, &vp);
    // Higher z within layer → green on top.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn multiple_layers_interleaved_with_unlayered() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Three layers: z_order -1, 0, 5.
    let mut l_neg = CanvasLayer::new(1);
    l_neg.z_order = -1;
    let mut l_zero = CanvasLayer::new(2);
    l_zero.z_order = 0;
    let mut l_high = CanvasLayer::new(3);
    l_high.z_order = 5;

    vp.add_canvas_layer(l_neg);
    vp.add_canvas_layer(l_zero);
    vp.add_canvas_layer(l_high);

    // Items cover overlapping regions:
    // Layer 1 (z_order=-1): red, full viewport.
    let mut i1 = full_rect(1, 0, red());
    i1.layer_id = Some(1);
    // Layer 2 (z_order=0): green, full viewport.
    let mut i2 = full_rect(2, 0, green());
    i2.layer_id = Some(2);
    // Layer 3 (z_order=5): blue, left half only.
    let mut i3 = rect_at(3, 0, 0.0, 0.0, (W / 2) as f32, H as f32, blue());
    i3.layer_id = Some(3);
    // Unlayered: yellow, top-left quadrant.
    let i4 = rect_at(4, 0, 0.0, 0.0, (W / 4) as f32, (H / 4) as f32, yellow());

    vp.add_canvas_item(i1);
    vp.add_canvas_item(i2);
    vp.add_canvas_item(i3);
    vp.add_canvas_item(i4);

    let fb = capture_frame(&mut renderer, &vp);

    // Top-left quadrant: yellow (unlayered, rendered last).
    assert_pixel_color(&fb, 1, 1, yellow(), TOL);
    // Left half outside yellow: blue (layer 3, z_order=5).
    assert_pixel_color(&fb, 1, H - 2, blue(), TOL);
    // Right half: green (layer 2, z_order=0 > layer 1 z_order=-1).
    assert_pixel_color(&fb, W - 2, H / 2, green(), TOL);
}

// ===========================================================================
// COMBINED Z-INDEX + VISIBILITY EDGE CASES
// ===========================================================================

#[test]
fn invisible_item_at_highest_z_does_not_block_lower() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(full_rect(2, 1, green()));

    let mut top = full_rect(3, 100, blue());
    top.visible = false;
    vp.add_canvas_item(top);

    let fb = capture_frame(&mut renderer, &vp);
    // Blue at z=100 is invisible → green at z=1 is the visible top.
    assert_pixel_color(&fb, W / 2, H / 2, green(), TOL);
}

#[test]
fn invisible_item_same_z_does_not_affect_sibling() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));
    let mut hidden = full_rect(2, 0, green());
    hidden.visible = false;
    vp.add_canvas_item(hidden);
    vp.add_canvas_item(full_rect(3, 0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    // Render order at z=0: red, (green skipped), blue → blue wins.
    assert_pixel_color(&fb, W / 2, H / 2, blue(), TOL);
}

#[test]
fn only_visible_item_among_many_invisible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    for i in 0..5u64 {
        let mut item = full_rect(i + 1, i as i32, magenta());
        item.visible = false;
        vp.add_canvas_item(item);
    }

    // One visible item at z=3.
    vp.add_canvas_item(full_rect(10, 3, cyan()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, W / 2, H / 2, cyan(), TOL);
}

// ===========================================================================
// STRESS: LARGE ITEM COUNTS WITH ORDERING VERIFICATION
// ===========================================================================

#[test]
fn stress_50_items_alternating_z_and_visibility() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // Add 50 items. Even z-index items are visible, odd are invisible.
    // Among visible items, the highest z should win.
    for i in 0..50u64 {
        let mut item = full_rect(i + 1, i as i32, Color::rgb(i as f32 / 49.0, 0.0, 0.0));
        item.visible = i % 2 == 0;
        vp.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(W / 2, H / 2);
    // Highest visible z = 48 (even), intensity = 48/49.
    let expected_r = 48.0 / 49.0;
    assert!(
        (pixel.r - expected_r).abs() < 0.02,
        "Expected r≈{}, got {}",
        expected_r,
        pixel.r
    );
}

#[test]
fn stress_layered_and_unlayered_mix() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(W, H, Color::BLACK);

    // 3 layers with 5 items each, plus 5 unlayered items.
    for l in 0..3u64 {
        let mut layer = CanvasLayer::new(l + 1);
        layer.z_order = l as i32;
        vp.add_canvas_layer(layer);

        for i in 0..5u64 {
            let id = l * 10 + i + 1;
            let intensity = (l * 5 + i) as f32 / 19.0;
            let mut item = full_rect(id, i as i32, Color::rgb(intensity, 0.0, 0.0));
            item.layer_id = Some(l + 1);
            vp.add_canvas_item(item);
        }
    }

    // Unlayered items render last.
    for i in 0..5u64 {
        let id = 100 + i;
        vp.add_canvas_item(full_rect(id, i as i32, Color::rgb(0.0, 0.0, 1.0)));
    }

    let fb = capture_frame(&mut renderer, &vp);
    let pixel = fb.get_pixel(W / 2, H / 2);
    // Unlayered items render last; highest z among them is z=4 → all blue.
    assert_pixel_color(&fb, W / 2, H / 2, Color::rgb(0.0, 0.0, 1.0), TOL);
    assert_eq!(pixel.r, 0.0);
}

// ===========================================================================
// DETERMINISM
// ===========================================================================

#[test]
fn deterministic_rendering_with_complex_ordering() {
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(W, H, Color::BLACK);

        let layer = CanvasLayer::new(1);
        vp.add_canvas_layer(layer);

        let mut l_item = full_rect(1, 0, red());
        l_item.layer_id = Some(1);
        vp.add_canvas_item(l_item);

        vp.add_canvas_item(full_rect(2, 0, green()));
        vp.add_canvas_item(full_rect(3, 5, blue()));

        let mut invisible = full_rect(4, 10, yellow());
        invisible.visible = false;
        vp.add_canvas_item(invisible);

        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_frame();
    let fb2 = make_frame();
    assert_eq!(fb1.pixels, fb2.pixels, "Rendering must be deterministic");
}

// ===========================================================================
// GOLDEN RENDER REGRESSION TESTS
//
// These tests render specific draw-ordering / visibility / layer scenarios at
// a fixed resolution, save the result as a golden PNG on first run, and
// compare against it on subsequent runs.  A mismatch means the rendering
// pipeline changed its draw-ordering or visibility behaviour.
// ===========================================================================

/// Render resolution for golden draw-ordering tests.
const GOLDEN_W: u32 = 32;
const GOLDEN_H: u32 = 32;

/// Pixel tolerance for golden comparison (Euclidean RGB distance).
const GOLDEN_TOL: f64 = 0.02;

/// Minimum match ratio to pass golden comparison.
const GOLDEN_MIN_MATCH: f64 = 1.0; // exact match required

/// Returns the golden render directory for draw-ordering tests.
fn golden_draw_order_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
        .join("golden")
        .join("render")
        .join("draw_ordering")
}

/// Saves a framebuffer as a golden PNG reference.
fn save_draw_order_golden(fb: &FrameBuffer, name: &str) {
    let dir = golden_draw_order_dir();
    std::fs::create_dir_all(&dir).expect("failed to create golden draw_ordering dir");
    let path = dir.join(format!("{name}.png"));
    fb.save_png(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("failed to save golden PNG {}: {e}", path.display()));
}

/// Loads a golden PNG reference. Returns None if the file doesn't exist.
fn load_draw_order_golden(name: &str) -> Option<FrameBuffer> {
    let path = golden_draw_order_dir().join(format!("{name}.png"));
    let tex = load_png(path.to_str().unwrap())?;
    Some(FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    })
}

/// Compares a rendered framebuffer against a golden reference.
/// Generates the golden on first run; asserts exact match on subsequent runs.
fn assert_draw_order_golden(fb: &FrameBuffer, name: &str) {
    match load_draw_order_golden(name) {
        Some(golden) => {
            let result = compare_framebuffers(fb, &golden, GOLDEN_TOL);
            assert!(
                result.match_ratio() >= GOLDEN_MIN_MATCH,
                "golden draw-ordering comparison failed for '{}': {:.2}% match \
                 (need {:.0}%), max_diff={:.4}, avg_diff={:.4}",
                name,
                result.match_ratio() * 100.0,
                GOLDEN_MIN_MATCH * 100.0,
                result.max_diff,
                result.avg_diff,
            );
        }
        None => {
            save_draw_order_golden(fb, name);
            eprintln!(
                "Generated golden draw-ordering reference: {}/{}.png",
                golden_draw_order_dir().display(),
                name,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Golden: Z-index ordering
// ---------------------------------------------------------------------------

#[test]
fn golden_z_index_sandwich() {
    // Three overlapping layers: red(z=0), green left-half(z=1), blue top-left(z=2).
    // Verifies z-index draw order is stable across renderer changes.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(rect_at(
        2,
        1,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    ));
    vp.add_canvas_item(rect_at(
        3,
        2,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        (GOLDEN_H / 2) as f32,
        blue(),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "z_index_sandwich");
}

#[test]
fn golden_z_index_reverse_insertion() {
    // High-z item added first, low-z added second.
    // Verifies ordering is by z-index, not insertion order.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 5, cyan()));
    vp.add_canvas_item(full_rect(2, 0, red()));
    vp.add_canvas_item(full_rect(3, 3, yellow()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "z_index_reverse_insertion");
}

#[test]
fn golden_z_index_negative() {
    // Negative z-index items below zero.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, -10, red()));
    vp.add_canvas_item(full_rect(2, -5, green()));
    vp.add_canvas_item(full_rect(3, 0, blue()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "z_index_negative");
}

// ---------------------------------------------------------------------------
// Golden: Sibling insertion order within same z-index
// ---------------------------------------------------------------------------

#[test]
fn golden_same_z_insertion_order() {
    // All items at z=0; last inserted wins in overlap regions.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    // Red fills everything, green fills right half, blue fills bottom half.
    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(rect_at(
        2,
        0,
        (GOLDEN_W / 2) as f32,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    ));
    vp.add_canvas_item(rect_at(
        3,
        0,
        0.0,
        (GOLDEN_H / 2) as f32,
        GOLDEN_W as f32,
        (GOLDEN_H / 2) as f32,
        blue(),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "same_z_insertion_order");
}

// ---------------------------------------------------------------------------
// Golden: Visibility toggling
// ---------------------------------------------------------------------------

#[test]
fn golden_visibility_hidden_top_layer() {
    // Top-z item is invisible, revealing lower items.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red()));
    vp.add_canvas_item(rect_at(
        2,
        1,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    ));

    let mut hidden_top = full_rect(3, 10, magenta());
    hidden_top.visible = false;
    vp.add_canvas_item(hidden_top);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "visibility_hidden_top");
}

#[test]
fn golden_visibility_all_hidden() {
    // All items invisible — only clear color should show.
    let mut renderer = SoftwareRenderer::new();
    let clear = Color::rgb(0.2, 0.3, 0.4);
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, clear);

    let mut a = full_rect(1, 0, red());
    a.visible = false;
    let mut b = full_rect(2, 1, green());
    b.visible = false;
    let mut c = full_rect(3, 2, blue());
    c.visible = false;

    vp.add_canvas_item(a);
    vp.add_canvas_item(b);
    vp.add_canvas_item(c);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "visibility_all_hidden");
}

#[test]
fn golden_visibility_mixed() {
    // Some items visible, some hidden, at different z-indices.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    vp.add_canvas_item(full_rect(1, 0, red())); // visible
    let mut hidden = full_rect(2, 1, green()); // hidden
    hidden.visible = false;
    vp.add_canvas_item(hidden);
    vp.add_canvas_item(rect_at(
        3,
        2,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        blue(),
    )); // visible

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "visibility_mixed");
}

// ---------------------------------------------------------------------------
// Golden: Canvas layer ordering and visibility
// ---------------------------------------------------------------------------

#[test]
fn golden_layer_z_order() {
    // Two layers with different z_order, items in each.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut layer_bg = CanvasLayer::new(1);
    layer_bg.z_order = 0;
    let mut layer_fg = CanvasLayer::new(2);
    layer_fg.z_order = 1;
    vp.add_canvas_layer(layer_bg);
    vp.add_canvas_layer(layer_fg);

    let mut bg_item = full_rect(1, 0, red());
    bg_item.layer_id = Some(1);
    let mut fg_item = rect_at(
        2,
        0,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    );
    fg_item.layer_id = Some(2);

    vp.add_canvas_item(bg_item);
    vp.add_canvas_item(fg_item);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "layer_z_order");
}

#[test]
fn golden_layer_invisible() {
    // Layer 1 is invisible; its items should not render.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut layer1 = CanvasLayer::new(1);
    layer1.visible = false;
    let layer2 = CanvasLayer::new(2);
    vp.add_canvas_layer(layer1);
    vp.add_canvas_layer(layer2);

    let mut item_hidden = full_rect(1, 0, red());
    item_hidden.layer_id = Some(1);
    let mut item_visible = rect_at(
        2,
        0,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    );
    item_visible.layer_id = Some(2);

    vp.add_canvas_item(item_hidden);
    vp.add_canvas_item(item_visible);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "layer_invisible");
}

#[test]
fn golden_layers_with_unlayered() {
    // Three layers plus unlayered items; unlayered render last.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::BLACK);

    let mut l1 = CanvasLayer::new(1);
    l1.z_order = -1;
    let mut l2 = CanvasLayer::new(2);
    l2.z_order = 5;
    vp.add_canvas_layer(l1);
    vp.add_canvas_layer(l2);

    // Layer 1: red, full viewport.
    let mut i1 = full_rect(1, 0, red());
    i1.layer_id = Some(1);
    // Layer 2: green, left half.
    let mut i2 = rect_at(
        2,
        0,
        0.0,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    );
    i2.layer_id = Some(2);
    // Unlayered: blue, top-left quadrant (renders last = on top).
    let i3 = rect_at(
        3,
        0,
        0.0,
        0.0,
        (GOLDEN_W / 4) as f32,
        (GOLDEN_H / 4) as f32,
        blue(),
    );

    vp.add_canvas_item(i1);
    vp.add_canvas_item(i2);
    vp.add_canvas_item(i3);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "layers_with_unlayered");
}

// ---------------------------------------------------------------------------
// Golden: Combined z-index + visibility + layers (complex scenario)
// ---------------------------------------------------------------------------

#[test]
fn golden_complex_ordering_scenario() {
    // Complex scenario mixing layers, z-index, visibility, and insertion order.
    // This is the "kitchen sink" test: if this golden matches, the ordering
    // pipeline is regression-free.
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(GOLDEN_W, GOLDEN_H, Color::rgb(0.1, 0.1, 0.1));

    // Layer A (z_order=0)
    let mut la = CanvasLayer::new(1);
    la.z_order = 0;
    // Layer B (z_order=2, invisible)
    let mut lb = CanvasLayer::new(2);
    lb.z_order = 2;
    lb.visible = false;
    // Layer C (z_order=1)
    let mut lc = CanvasLayer::new(3);
    lc.z_order = 1;
    vp.add_canvas_layer(la);
    vp.add_canvas_layer(lb);
    vp.add_canvas_layer(lc);

    // Layer A items:
    let mut a1 = full_rect(1, 0, red());
    a1.layer_id = Some(1);
    let mut a2 = rect_at(
        2,
        1,
        0.0,
        0.0,
        GOLDEN_W as f32,
        (GOLDEN_H / 2) as f32,
        yellow(),
    );
    a2.layer_id = Some(1);

    // Layer B items (invisible layer — should not render):
    let mut b1 = full_rect(3, 0, magenta());
    b1.layer_id = Some(2);

    // Layer C items:
    let mut c1 = rect_at(
        4,
        0,
        (GOLDEN_W / 2) as f32,
        0.0,
        (GOLDEN_W / 2) as f32,
        GOLDEN_H as f32,
        green(),
    );
    c1.layer_id = Some(3);

    // Unlayered items (render after all layers):
    let u1 = rect_at(
        5,
        0,
        0.0,
        0.0,
        (GOLDEN_W / 4) as f32,
        (GOLDEN_H / 4) as f32,
        cyan(),
    );
    let mut u2 = full_rect(6, 5, blue());
    u2.visible = false; // invisible unlayered at high z

    vp.add_canvas_item(a1);
    vp.add_canvas_item(a2);
    vp.add_canvas_item(b1);
    vp.add_canvas_item(c1);
    vp.add_canvas_item(u1);
    vp.add_canvas_item(u2);

    let fb = capture_frame(&mut renderer, &vp);
    assert_draw_order_golden(&fb, "complex_ordering_scenario");
}
