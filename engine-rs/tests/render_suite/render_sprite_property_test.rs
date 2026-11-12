//! pat-22g: Sprite2D property parity in renderer fixtures.
//!
//! Pixel-level tests verifying:
//! - flip_h mirrors texture horizontally
//! - flip_v mirrors texture vertically
//! - flip_h + flip_v combined
//! - offset shifts the drawn texture position
//! - modulate tints texture pixels
//! - centered property (centering draw rect on node position)
//! - Combined properties (flip + offset + modulate)

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdrender2d::texture::Texture2D;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TOL: f32 = 0.02;
const W: u32 = 20;
const H: u32 = 20;

fn red() -> Color {
    Color::rgb(1.0, 0.0, 0.0)
}
fn green() -> Color {
    Color::rgb(0.0, 1.0, 0.0)
}
fn blue() -> Color {
    Color::rgb(0.0, 0.0, 1.0)
}
fn white() -> Color {
    Color::WHITE
}

/// Creates a 4x4 asymmetric test texture:
/// ```text
/// R R G G
/// R R G G
/// B B W W
/// B B W W
/// ```
/// Top-left = red, top-right = green, bottom-left = blue, bottom-right = white.
fn asymmetric_texture() -> Texture2D {
    let mut pixels = vec![Color::BLACK; 16];
    for y in 0..4u32 {
        for x in 0..4u32 {
            let color = match (x < 2, y < 2) {
                (true, true) => red(),     // top-left
                (false, true) => green(),  // top-right
                (true, false) => blue(),   // bottom-left
                (false, false) => white(), // bottom-right
            };
            pixels[(y * 4 + x) as usize] = color;
        }
    }
    Texture2D {
        width: 4,
        height: 4,
        pixels,
    }
}

/// Creates a canvas item that draws a texture at a given position with modulate.
fn sprite_item(
    id: u64,
    texture_path: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    modulate: Color,
) -> CanvasItem {
    let mut item = CanvasItem::new(CanvasItemId(id));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: texture_path.to_string(),
        rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, h)),
        modulate,
    });
    item
}

// ===========================================================================
// TEXTURE DRAWING BASICS
// ===========================================================================

#[test]
fn texture_draws_at_correct_position() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("test", Texture2D::solid(4, 4, red()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "test", 5.0, 5.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 5, 5, red(), TOL);
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 4, 4, Color::BLACK, TOL);
    assert_pixel_color(&fb, 9, 9, Color::BLACK, TOL);
}

#[test]
fn texture_asymmetric_pattern_reads_correctly() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("asym", asymmetric_texture());

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Draw at (0,0), same size as texture (4x4) → 1:1 mapping.
    vp.add_canvas_item(sprite_item(1, "asym", 0.0, 0.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // Top-left quadrant: red.
    assert_pixel_color(&fb, 0, 0, red(), TOL);
    // Top-right quadrant: green.
    assert_pixel_color(&fb, 2, 0, green(), TOL);
    // Bottom-left quadrant: blue.
    assert_pixel_color(&fb, 0, 2, blue(), TOL);
    // Bottom-right quadrant: white.
    assert_pixel_color(&fb, 2, 2, white(), TOL);
}

// ===========================================================================
// FLIP_H (horizontal flip)
// ===========================================================================

#[test]
fn flip_h_mirrors_texture_horizontally() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_horizontal();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("flipped", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "flipped", 0.0, 0.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // After horizontal flip: left↔right swapped.
    // Top-left was red → now green. Top-right was green → now red.
    assert_pixel_color(&fb, 0, 0, green(), TOL);
    assert_pixel_color(&fb, 2, 0, red(), TOL);
    // Bottom-left was blue → now white. Bottom-right was white → now blue.
    assert_pixel_color(&fb, 0, 2, white(), TOL);
    assert_pixel_color(&fb, 2, 2, blue(), TOL);
}

#[test]
fn flip_h_does_not_affect_vertical_order() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_horizontal();

    // Top row should still be top row (just mirrored).
    // Original top: R G → flipped: G R.
    assert_eq!(flipped.get_pixel(0, 0), green()); // was (3,0)=green
    assert_eq!(flipped.get_pixel(3, 0), red()); // was (0,0)=red

    // Bottom row: B W → flipped: W B.
    assert_eq!(flipped.get_pixel(0, 3), white());
    assert_eq!(flipped.get_pixel(3, 3), blue());
}

#[test]
fn flip_h_double_flip_restores_original() {
    let tex = asymmetric_texture();
    let double_flipped = tex.flip_horizontal().flip_horizontal();

    for y in 0..4 {
        for x in 0..4 {
            let orig = tex.get_pixel(x, y);
            let restored = double_flipped.get_pixel(x, y);
            assert_pixel_color_eq(orig, restored, x, y, "double flip_h");
        }
    }
}

// ===========================================================================
// FLIP_V (vertical flip)
// ===========================================================================

#[test]
fn flip_v_mirrors_texture_vertically() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_vertical();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("vflip", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "vflip", 0.0, 0.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // After vertical flip: top↔bottom swapped.
    // Top-left was red → now blue. Bottom-left was blue → now red.
    assert_pixel_color(&fb, 0, 0, blue(), TOL);
    assert_pixel_color(&fb, 0, 2, red(), TOL);
    // Top-right was green → now white. Bottom-right was white → now green.
    assert_pixel_color(&fb, 2, 0, white(), TOL);
    assert_pixel_color(&fb, 2, 2, green(), TOL);
}

#[test]
fn flip_v_does_not_affect_horizontal_order() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_vertical();

    // Left column should still be left column (just vertically mirrored).
    // Original col 0: R(top), B(bottom) → flipped: B(top), R(bottom).
    assert_eq!(flipped.get_pixel(0, 0), blue());
    assert_eq!(flipped.get_pixel(0, 3), red());
}

#[test]
fn flip_v_double_flip_restores_original() {
    let tex = asymmetric_texture();
    let double_flipped = tex.flip_vertical().flip_vertical();

    for y in 0..4 {
        for x in 0..4 {
            let orig = tex.get_pixel(x, y);
            let restored = double_flipped.get_pixel(x, y);
            assert_pixel_color_eq(orig, restored, x, y, "double flip_v");
        }
    }
}

// ===========================================================================
// FLIP_H + FLIP_V COMBINED
// ===========================================================================

#[test]
fn flip_h_and_v_rotates_180() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_horizontal().flip_vertical();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("hv", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "hv", 0.0, 0.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // H+V flip = 180° rotation.
    // Top-left was red → now white (was bottom-right).
    assert_pixel_color(&fb, 0, 0, white(), TOL);
    // Top-right was green → now blue (was bottom-left).
    assert_pixel_color(&fb, 2, 0, blue(), TOL);
    // Bottom-left was blue → now green (was top-right).
    assert_pixel_color(&fb, 0, 2, green(), TOL);
    // Bottom-right was white → now red (was top-left).
    assert_pixel_color(&fb, 2, 2, red(), TOL);
}

#[test]
fn flip_h_then_v_equals_flip_v_then_h() {
    let tex = asymmetric_texture();
    let hv = tex.flip_horizontal().flip_vertical();
    let vh = tex.flip_vertical().flip_horizontal();

    for y in 0..4 {
        for x in 0..4 {
            assert_pixel_color_eq(hv.get_pixel(x, y), vh.get_pixel(x, y), x, y, "hv vs vh");
        }
    }
}

// ===========================================================================
// OFFSET
// ===========================================================================

#[test]
fn offset_shifts_texture_position() {
    // Simulate Sprite2D offset by shifting the draw rect position.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", Texture2D::solid(4, 4, red()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Node at (5,5), offset (3,3) → draw at (8,8).
    let offset = Vector2::new(3.0, 3.0);
    let node_pos = Vector2::new(5.0, 5.0);
    let draw_pos = Vector2::new(node_pos.x + offset.x, node_pos.y + offset.y);
    vp.add_canvas_item(sprite_item(
        1,
        "tex",
        draw_pos.x,
        draw_pos.y,
        4.0,
        4.0,
        white(),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    // Texture should be at (8,8) not (5,5).
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 5, 5, Color::BLACK, TOL);
}

#[test]
fn offset_negative_shifts_upward_left() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", Texture2D::solid(4, 4, green()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Node at (10,10), offset (-3,-3) → draw at (7,7).
    vp.add_canvas_item(sprite_item(1, "tex", 7.0, 7.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 7, 7, green(), TOL);
    assert_pixel_color(&fb, 10, 10, green(), TOL);
    assert_pixel_color(&fb, 6, 6, Color::BLACK, TOL);
}

#[test]
fn offset_zero_draws_at_node_position() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", Texture2D::solid(4, 4, blue()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // No offset → draw at node position.
    vp.add_canvas_item(sprite_item(1, "tex", 3.0, 3.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 3, 3, blue(), TOL);
    assert_pixel_color(&fb, 6, 6, blue(), TOL);
}

// ===========================================================================
// MODULATE
// ===========================================================================

#[test]
fn modulate_tints_texture_red() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("white", Texture2D::solid(4, 4, white()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // White texture with red modulate → pure red.
    vp.add_canvas_item(sprite_item(1, "white", 0.0, 0.0, 4.0, 4.0, red()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 1, 1, red(), TOL);
}

#[test]
fn modulate_tints_texture_green() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("white", Texture2D::solid(4, 4, white()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "white", 0.0, 0.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 1, 1, green(), TOL);
}

#[test]
fn modulate_half_intensity() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("white", Texture2D::solid(4, 4, white()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let half = Color::rgb(0.5, 0.5, 0.5);
    vp.add_canvas_item(sprite_item(1, "white", 0.0, 0.0, 4.0, 4.0, half));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 1, 1, half, TOL);
}

#[test]
fn modulate_multiplies_with_texture_color() {
    // Red texture modulated by green → black (1,0,0) * (0,1,0) = (0,0,0).
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("red", Texture2D::solid(4, 4, red()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "red", 0.0, 0.0, 4.0, 4.0, green()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 1, 1, Color::BLACK, TOL);
}

#[test]
fn modulate_white_preserves_texture() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_texture();
    renderer.register_texture("asym", tex);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    vp.add_canvas_item(sprite_item(1, "asym", 0.0, 0.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 0, 0, red(), TOL);
    assert_pixel_color(&fb, 2, 0, green(), TOL);
    assert_pixel_color(&fb, 0, 2, blue(), TOL);
    assert_pixel_color(&fb, 2, 2, white(), TOL);
}

#[test]
fn modulate_partial_channels() {
    // Modulate (1, 0.5, 0) on white texture → (1, 0.5, 0).
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("white", Texture2D::solid(4, 4, white()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let mod_color = Color::rgb(1.0, 0.5, 0.0);
    vp.add_canvas_item(sprite_item(1, "white", 0.0, 0.0, 4.0, 4.0, mod_color));

    let fb = capture_frame(&mut renderer, &vp);
    assert_pixel_color(&fb, 1, 1, mod_color, TOL);
}

// ===========================================================================
// CENTERED PROPERTY
// ===========================================================================

#[test]
fn centered_shifts_draw_rect_by_half_size() {
    // Godot's Sprite2D centered=true draws the texture centered on the node position.
    // This means the draw rect starts at (node_pos - texture_size/2).
    let mut renderer = SoftwareRenderer::new();
    let tex_size = 4.0;
    renderer.register_texture("tex", Texture2D::solid(4, 4, red()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let node_pos = Vector2::new(10.0, 10.0);
    // centered=true: draw at (10 - 4/2, 10 - 4/2) = (8, 8).
    let centered_pos = Vector2::new(node_pos.x - tex_size / 2.0, node_pos.y - tex_size / 2.0);
    vp.add_canvas_item(sprite_item(
        1,
        "tex",
        centered_pos.x,
        centered_pos.y,
        tex_size,
        tex_size,
        white(),
    ));

    let fb = capture_frame(&mut renderer, &vp);
    // Texture centered on (10,10) → rect from (8,8) to (12,12).
    assert_pixel_color(&fb, 8, 8, red(), TOL);
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    assert_pixel_color(&fb, 11, 11, red(), TOL);
    assert_pixel_color(&fb, 7, 7, Color::BLACK, TOL);
    assert_pixel_color(&fb, 12, 12, Color::BLACK, TOL);
}

#[test]
fn not_centered_draws_from_top_left() {
    // centered=false: draw rect starts at node position (top-left corner).
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", Texture2D::solid(4, 4, red()));

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Not centered: draw at node position (10,10).
    vp.add_canvas_item(sprite_item(1, "tex", 10.0, 10.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // Rect from (10,10) to (14,14).
    assert_pixel_color(&fb, 10, 10, red(), TOL);
    assert_pixel_color(&fb, 13, 13, red(), TOL);
    assert_pixel_color(&fb, 9, 9, Color::BLACK, TOL);
    assert_pixel_color(&fb, 14, 14, Color::BLACK, TOL);
}

#[test]
fn centered_vs_not_centered_differ() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", Texture2D::solid(4, 4, red()));

    let node_pos = Vector2::new(10.0, 10.0);
    let tex_size = 4.0;

    // Centered.
    let mut vp1 = Viewport::new(W, H, Color::BLACK);
    let cp = Vector2::new(node_pos.x - tex_size / 2.0, node_pos.y - tex_size / 2.0);
    vp1.add_canvas_item(sprite_item(
        1,
        "tex",
        cp.x,
        cp.y,
        tex_size,
        tex_size,
        white(),
    ));
    let fb1 = capture_frame(&mut renderer, &vp1);

    // Not centered.
    let mut vp2 = Viewport::new(W, H, Color::BLACK);
    vp2.add_canvas_item(sprite_item(
        1,
        "tex",
        node_pos.x,
        node_pos.y,
        tex_size,
        tex_size,
        white(),
    ));
    let fb2 = capture_frame(&mut renderer, &vp2);

    assert_ne!(
        fb1.pixels, fb2.pixels,
        "Centered and non-centered should produce different output"
    );
}

// ===========================================================================
// COMBINED PROPERTIES
// ===========================================================================

#[test]
fn flip_h_with_modulate() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_horizontal();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("flip_mod", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Half-intensity modulate.
    let half = Color::rgb(0.5, 0.5, 0.5);
    vp.add_canvas_item(sprite_item(1, "flip_mod", 0.0, 0.0, 4.0, 4.0, half));

    let fb = capture_frame(&mut renderer, &vp);
    // Top-left after H-flip is green → modulated: (0, 0.5, 0).
    let expected_tl = Color::rgb(0.0, 0.5, 0.0);
    assert_pixel_color(&fb, 0, 0, expected_tl, TOL);
    // Top-right after H-flip is red → modulated: (0.5, 0, 0).
    let expected_tr = Color::rgb(0.5, 0.0, 0.0);
    assert_pixel_color(&fb, 2, 0, expected_tr, TOL);
}

#[test]
fn flip_v_with_offset() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_vertical();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("vflip_off", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Node at (5,5) with offset (3,3) → draw at (8,8).
    vp.add_canvas_item(sprite_item(1, "vflip_off", 8.0, 8.0, 4.0, 4.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // After V-flip, top-left was red → now blue.
    assert_pixel_color(&fb, 8, 8, blue(), TOL);
    // Bottom-left was blue → now red.
    assert_pixel_color(&fb, 8, 10, red(), TOL);
    // Position before draw area should be black.
    assert_pixel_color(&fb, 7, 7, Color::BLACK, TOL);
}

#[test]
fn centered_with_flip_h_and_modulate() {
    let tex = asymmetric_texture();
    let flipped = tex.flip_horizontal();

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("combo", flipped);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let node_pos = Vector2::new(10.0, 10.0);
    let tex_size = 4.0;
    // Centered + H-flip + half modulate.
    let cp = Vector2::new(node_pos.x - tex_size / 2.0, node_pos.y - tex_size / 2.0);
    let half = Color::rgb(0.5, 0.5, 0.5);
    vp.add_canvas_item(sprite_item(
        1, "combo", cp.x, cp.y, tex_size, tex_size, half,
    ));

    let fb = capture_frame(&mut renderer, &vp);
    // Rect at (8,8) to (12,12). Top-left after H-flip is green → (0, 0.5, 0).
    assert_pixel_color(&fb, 8, 8, Color::rgb(0.0, 0.5, 0.0), TOL);
    // Node position (10,10) is inside the rect.
    let pixel = fb.get_pixel(10, 10);
    assert!(
        pixel.r > 0.0 || pixel.g > 0.0 || pixel.b > 0.0,
        "Center should have content"
    );
}

// ===========================================================================
// TEXTURE REGION (atlas sub-rect)
// ===========================================================================

#[test]
fn texture_region_draws_subregion_only() {
    // 4x4 asymmetric texture, draw only the top-left quadrant (red).
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("asym", asymmetric_texture());

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "asym".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // Entire drawn region should be red (from top-left quadrant of texture).
    assert_pixel_color(&fb, 0, 0, red(), TOL);
    assert_pixel_color(&fb, 3, 3, red(), TOL);
}

#[test]
fn texture_region_with_modulate() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("asym", asymmetric_texture());

    let mut vp = Viewport::new(W, H, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Draw bottom-right quadrant (white) with green modulate → green.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "asym".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0)),
        modulate: green(),
    });
    vp.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &vp);
    // White * green = green.
    assert_pixel_color(&fb, 1, 1, green(), TOL);
}

// ===========================================================================
// TEXTURE SCALING
// ===========================================================================

#[test]
fn texture_scaled_up_uses_nearest_neighbor() {
    // Draw a 2x2 texture into a larger rect — should scale with nearest-neighbor.
    let mut pixels = vec![Color::BLACK; 4];
    pixels[0] = red(); // (0,0)
    pixels[1] = green(); // (1,0)
    pixels[2] = blue(); // (0,1)
    pixels[3] = white(); // (1,1)
    let tex = Texture2D {
        width: 2,
        height: 2,
        pixels,
    };

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("small", tex);

    let mut vp = Viewport::new(W, H, Color::BLACK);
    // Draw 2x2 texture into 8x8 rect → each texel covers 4x4 screen pixels.
    vp.add_canvas_item(sprite_item(1, "small", 0.0, 0.0, 8.0, 8.0, white()));

    let fb = capture_frame(&mut renderer, &vp);
    // Top-left 4x4 block should be red.
    assert_pixel_color(&fb, 0, 0, red(), TOL);
    assert_pixel_color(&fb, 3, 3, red(), TOL);
    // Top-right 4x4 block should be green.
    assert_pixel_color(&fb, 4, 0, green(), TOL);
    assert_pixel_color(&fb, 7, 3, green(), TOL);
    // Bottom-left 4x4 block should be blue.
    assert_pixel_color(&fb, 0, 4, blue(), TOL);
    // Bottom-right 4x4 block should be white.
    assert_pixel_color(&fb, 4, 4, white(), TOL);
}

// ===========================================================================
// DETERMINISM
// ===========================================================================

#[test]
fn deterministic_sprite_rendering() {
    let make_frame = || {
        let tex = asymmetric_texture();
        let flipped = tex.flip_horizontal();
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("tex", flipped);

        let mut vp = Viewport::new(W, H, Color::BLACK);
        vp.add_canvas_item(sprite_item(
            1,
            "tex",
            5.0,
            5.0,
            4.0,
            4.0,
            Color::rgb(0.8, 0.6, 0.4),
        ));
        capture_frame(&mut renderer, &vp)
    };

    let fb1 = make_frame();
    let fb2 = make_frame();
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Sprite rendering must be deterministic"
    );
}

// ===========================================================================
// Utility
// ===========================================================================

fn assert_pixel_color_eq(a: Color, b: Color, x: u32, y: u32, label: &str) {
    assert!(
        (a.r - b.r).abs() < TOL
            && (a.g - b.g).abs() < TOL
            && (a.b - b.b).abs() < TOL
            && (a.a - b.a).abs() < TOL,
        "{label}: pixel ({x},{y}) mismatch: expected {:?}, got {:?}",
        a,
        b,
    );
}
