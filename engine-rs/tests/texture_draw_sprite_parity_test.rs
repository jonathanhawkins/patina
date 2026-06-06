//! pat-xgm7: Cover texture draw and sprite property parity in renderer fixtures.
//!
//! Tests that texture drawing and sprite property changes produce the expected
//! pixel output from the software renderer, matching Godot's 2D rendering
//! contracts:
//!
//! 1. DrawTextureRect: solid textures, modulation, scaling, clipping
//! 2. DrawTextureRegion: atlas sub-regions, source rect clamping
//! 3. Sprite properties: flip_h, flip_v, texture modulate, visibility
//! 4. Texture operations: resize, flip, solid creation
//! 5. Alpha blending: semi-transparent textures, modulate alpha
//!
//! Acceptance: sprite and texture property changes affect output as expected.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::texture::Texture2D;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

// ===========================================================================
// Helpers
// ===========================================================================

fn pixel_at(frame: &gdserver2d::server::FrameData, x: u32, y: u32) -> Color {
    frame.pixels[(y * frame.width + x) as usize]
}

fn approx_eq(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() < tol
        && (a.g - b.g).abs() < tol
        && (a.b - b.b).abs() < tol
        && (a.a - b.a).abs() < tol
}

/// Creates a 4x4 checkerboard texture: top-left=color_a, top-right=color_b, etc.
fn checkerboard_2x2(color_a: Color, color_b: Color) -> Texture2D {
    // 2x2 texture: [A, B, B, A]
    Texture2D {
        width: 2,
        height: 2,
        pixels: vec![color_a, color_b, color_b, color_a],
    }
}

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

// ===========================================================================
// Part 1: DrawTextureRect basics
// ===========================================================================

/// Solid texture drawn at exact framebuffer size fills every pixel.
#[test]
fn draw_texture_rect_solid_fills_viewport() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(10, 10, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red());
    assert_eq!(pixel_at(&frame, 9, 9), red());
    assert_eq!(pixel_at(&frame, 5, 5), red());
}

/// Texture drawn at half viewport leaves surrounding area as clear color.
#[test]
fn draw_texture_rect_partial_coverage() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, green());
    renderer.register_texture("res://green.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://green.png".to_string(),
        rect: Rect2::new(Vector2::new(3.0, 3.0), Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 5, 5), green(), "inside texture rect");
    assert_eq!(pixel_at(&frame, 0, 0), Color::BLACK, "outside texture rect");
    assert_eq!(pixel_at(&frame, 9, 9), Color::BLACK, "outside texture rect");
}

/// Texture stretched to larger rect via nearest-neighbor sampling.
#[test]
fn draw_texture_rect_stretched() {
    let mut renderer = SoftwareRenderer::new();
    // 2x2 texture stretched to 10x10
    let tex = Texture2D::solid(2, 2, blue());
    renderer.register_texture("res://small.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://small.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // All pixels should be blue (nearest neighbor of 2x2 into 10x10)
    assert_eq!(pixel_at(&frame, 0, 0), blue());
    assert_eq!(pixel_at(&frame, 9, 9), blue());
    assert_eq!(pixel_at(&frame, 5, 5), blue());
}

/// Texture drawn partially offscreen clips correctly.
#[test]
fn draw_texture_rect_clips_at_edges() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(10, 10, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Rect extends from (5,5) to (15,15) — only (5,5)-(10,10) visible
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 7, 7),
        red(),
        "visible portion should render"
    );
    assert_eq!(
        pixel_at(&frame, 2, 2),
        Color::BLACK,
        "outside rect should be clear"
    );
}

/// Zero-size texture does not crash.
#[test]
fn draw_texture_rect_zero_texture_no_crash() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(0, 0, red());
    renderer.register_texture("res://zero.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://zero.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "zero-size texture draws nothing"
    );
}

/// Missing texture path draws nothing.
#[test]
fn draw_texture_rect_missing_texture_draws_nothing() {
    let mut renderer = SoftwareRenderer::new();

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://nonexistent.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 5, 5), Color::BLACK);
}

// ===========================================================================
// Part 2: Texture modulation (Sprite2D.modulate equivalent)
// ===========================================================================

/// White modulate preserves original texture colors.
#[test]
fn modulate_white_preserves_color() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, Color::rgb(0.5, 0.3, 0.8));
    renderer.register_texture("res://tex.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://tex.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    assert!(approx_eq(p, Color::rgb(0.5, 0.3, 0.8), 0.01));
}

/// Red modulate on white texture produces red.
#[test]
fn modulate_red_on_white_produces_red() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, white());
    renderer.register_texture("res://white.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://white.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: red(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 2, 2), red());
}

/// Modulate with half intensity dims the texture.
#[test]
fn modulate_half_intensity_dims_texture() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, white());
    renderer.register_texture("res://white.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://white.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::rgb(0.5, 0.5, 0.5),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    assert!(approx_eq(p, Color::rgb(0.5, 0.5, 0.5), 0.01));
}

/// Modulate with zero alpha makes texture invisible (writes transparent).
#[test]
fn modulate_zero_alpha() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::new(1.0, 1.0, 1.0, 0.0),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    // Alpha should be 0 (1.0 * 0.0 = 0.0)
    assert!(
        p.a < 0.01,
        "modulate alpha=0 should produce transparent pixel"
    );
}

/// Different modulate per-channel tints correctly.
#[test]
fn modulate_per_channel_tint() {
    let mut renderer = SoftwareRenderer::new();
    // White texture modulated by (0.2, 0.8, 0.4, 1.0)
    let tex = Texture2D::solid(4, 4, white());
    renderer.register_texture("res://w.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://w.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::new(0.2, 0.8, 0.4, 1.0),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    assert!(approx_eq(p, Color::new(0.2, 0.8, 0.4, 1.0), 0.01));
}

// ===========================================================================
// Part 3: DrawTextureRegion (atlas/sprite sheet sub-regions)
// ===========================================================================

/// Sub-region draws only the specified portion of the texture.
#[test]
fn draw_texture_region_draws_subregion() {
    let mut renderer = SoftwareRenderer::new();
    // 2x2 checkerboard: TL=red, TR=blue, BL=blue, BR=red
    let checker = checkerboard_2x2(red(), blue());
    renderer.register_texture("res://checker.png", checker);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Draw only top-left quadrant (0,0)-(1,1) of the 2x2 texture → should be red
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://checker.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(1.0, 1.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "sub-region should sample top-left (red)"
    );
}

/// Sub-region with modulate applies tinting.
#[test]
fn draw_texture_region_with_modulate() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, white());
    renderer.register_texture("res://white.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://white.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: green(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 2, 2), green());
}

/// Zero-size source rect draws nothing.
#[test]
fn draw_texture_region_zero_source_draws_nothing() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(0.0, 0.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 2, 2), Color::BLACK);
}

// ===========================================================================
// Part 4: Sprite property parity (flip, transform, visibility)
// ===========================================================================

/// Texture flip_horizontal reverses pixels left-to-right.
#[test]
fn texture_flip_horizontal() {
    let mut pixels = vec![Color::BLACK; 4];
    pixels[0] = red(); // (0,0)
    pixels[1] = blue(); // (1,0)
    let tex = Texture2D {
        width: 2,
        height: 2,
        pixels,
    };
    let flipped = tex.flip_horizontal();
    assert_eq!(flipped.get_pixel(0, 0), blue(), "left becomes right");
    assert_eq!(flipped.get_pixel(1, 0), red(), "right becomes left");
}

/// Texture flip_vertical reverses pixels top-to-bottom.
#[test]
fn texture_flip_vertical() {
    let mut pixels = vec![Color::BLACK; 4];
    pixels[0] = red(); // (0,0) top
    pixels[2] = blue(); // (0,1) bottom
    let tex = Texture2D {
        width: 2,
        height: 2,
        pixels,
    };
    let flipped = tex.flip_vertical();
    assert_eq!(flipped.get_pixel(0, 0), blue(), "top becomes bottom");
    assert_eq!(flipped.get_pixel(0, 1), red(), "bottom becomes top");
}

/// Double flip_h restores original.
#[test]
fn double_flip_horizontal_restores() {
    let checker = checkerboard_2x2(red(), blue());
    let double_flipped = checker.flip_horizontal().flip_horizontal();
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(
                checker.get_pixel(x, y),
                double_flipped.get_pixel(x, y),
                "double flip_h should restore at ({x},{y})"
            );
        }
    }
}

/// Double flip_v restores original.
#[test]
fn double_flip_vertical_restores() {
    let checker = checkerboard_2x2(red(), blue());
    let double_flipped = checker.flip_vertical().flip_vertical();
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(
                checker.get_pixel(x, y),
                double_flipped.get_pixel(x, y),
                "double flip_v should restore at ({x},{y})"
            );
        }
    }
}

/// Flipped texture renders correctly through the renderer.
#[test]
fn flipped_texture_renders_correctly() {
    let mut renderer = SoftwareRenderer::new();

    // 2x1 texture: left=red, right=blue
    let tex = Texture2D {
        width: 2,
        height: 1,
        pixels: vec![red(), blue()],
    };
    let flipped = tex.flip_horizontal();
    renderer.register_texture("res://flipped.png", flipped);

    let mut vp = Viewport::new(10, 1, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://flipped.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 1.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // After horizontal flip: left=blue, right=red
    assert_eq!(
        pixel_at(&frame, 0, 0),
        blue(),
        "left should be blue after flip"
    );
    assert_eq!(
        pixel_at(&frame, 9, 0),
        red(),
        "right should be red after flip"
    );
}

/// Sprite item with transform translation offsets texture.
#[test]
fn sprite_transform_translation() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(5, 5, green());
    renderer.register_texture("res://green.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(10.0, 10.0));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://green.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 12, 12),
        green(),
        "translated sprite should render offset"
    );
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "origin should be clear"
    );
}

/// Hidden sprite with texture does not render.
#[test]
fn hidden_sprite_texture_not_rendered() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(10, 10, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.visible = false;
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 5, 5), Color::BLACK);
}

// ===========================================================================
// Part 5: Texture resize
// ===========================================================================

/// Resize preserves color with nearest-neighbor.
#[test]
fn texture_resize_preserves_color() {
    let tex = Texture2D::solid(2, 2, red());
    let resized = tex.resize(8, 8);
    assert_eq!(resized.width, 8);
    assert_eq!(resized.height, 8);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                resized.get_pixel(x, y),
                red(),
                "all pixels should be red at ({x},{y})"
            );
        }
    }
}

/// Resize to 1x1 picks a representative pixel.
#[test]
fn texture_resize_to_1x1() {
    let tex = Texture2D::solid(10, 10, blue());
    let resized = tex.resize(1, 1);
    assert_eq!(resized.width, 1);
    assert_eq!(resized.height, 1);
    assert_eq!(resized.get_pixel(0, 0), blue());
}

/// Resize to zero returns zero-size texture.
#[test]
fn texture_resize_to_zero() {
    let tex = Texture2D::solid(4, 4, red());
    let resized = tex.resize(0, 0);
    assert_eq!(resized.width, 0);
    assert_eq!(resized.height, 0);
    assert!(resized.pixels.is_empty());
}

// ===========================================================================
// Part 6: Multiple textures and z-ordering
// ===========================================================================

/// Texture at higher z renders on top of texture at lower z.
#[test]
fn texture_z_ordering() {
    let mut renderer = SoftwareRenderer::new();
    let tex_red = Texture2D::solid(10, 10, red());
    let tex_green = Texture2D::solid(10, 10, green());
    renderer.register_texture("res://red.png", tex_red);
    renderer.register_texture("res://green.png", tex_green);

    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut bg = CanvasItem::new(CanvasItemId(1));
    bg.z_index = 0;
    bg.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(bg);

    let mut fg = CanvasItem::new(CanvasItemId(2));
    fg.z_index = 1;
    fg.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://green.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "higher z texture should be on top"
    );
}

/// Multiple draw commands on same item render in command order.
#[test]
fn multiple_draw_commands_same_item() {
    let mut renderer = SoftwareRenderer::new();
    let tex_red = Texture2D::solid(10, 10, red());
    let tex_blue = Texture2D::solid(5, 5, blue());
    renderer.register_texture("res://red.png", tex_red);
    renderer.register_texture("res://blue.png", tex_blue);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // First: red fills entire viewport
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    // Second: blue center (overwrites center)
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://blue.png".to_string(),
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(5.0, 5.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 0, 0),
        red(),
        "corner should be red (first draw)"
    );
    assert_eq!(
        pixel_at(&frame, 4, 4),
        blue(),
        "center should be blue (second draw overwrites)"
    );
}

// ===========================================================================
// Part 7: Mixed DrawRect + DrawTextureRect
// ===========================================================================

/// DrawRect and DrawTextureRect compose correctly on same item.
#[test]
fn mixed_rect_and_texture_draw() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(5, 5, green());
    renderer.register_texture("res://green.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // First: red rect background
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: red(),
        filled: true,
    });
    // Second: green texture in center
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://green.png".to_string(),
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(5.0, 5.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red(), "corner should be red rect");
    assert_eq!(
        pixel_at(&frame, 4, 4),
        green(),
        "center should be green texture"
    );
}

/// Texture on lower z, rect on higher z — rect wins.
#[test]
fn rect_over_texture_z_ordering() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(10, 10, blue());
    renderer.register_texture("res://blue.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut tex_item = CanvasItem::new(CanvasItemId(1));
    tex_item.z_index = 0;
    tex_item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://blue.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(tex_item);

    let mut rect_item = CanvasItem::new(CanvasItemId(2));
    rect_item.z_index = 1;
    rect_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: red(),
        filled: true,
    });
    vp.add_canvas_item(rect_item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "higher z rect should be on top of lower z texture"
    );
}

// ===========================================================================
// Part 8: Texture scaling down (large texture → small rect)
// ===========================================================================

/// Large texture drawn into a smaller rect samples correctly.
#[test]
fn draw_texture_rect_scaled_down() {
    let mut renderer = SoftwareRenderer::new();
    // 10x10 solid red texture drawn into 4x4 rect.
    let tex = Texture2D::solid(10, 10, red());
    renderer.register_texture("res://big.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://big.png".to_string(),
        rect: Rect2::new(Vector2::new(1.0, 1.0), Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 2, 2),
        red(),
        "scaled-down texture should render"
    );
    assert_eq!(
        pixel_at(&frame, 0, 0),
        Color::BLACK,
        "outside should be clear"
    );
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "outside should be clear"
    );
}

/// Scale-down preserves quadrant pattern from a multi-colored texture.
#[test]
fn draw_texture_rect_scaled_down_preserves_pattern() {
    let mut renderer = SoftwareRenderer::new();
    // 8x8 texture: left half red, right half blue.
    let mut pixels = vec![Color::BLACK; 64];
    for y in 0..8u32 {
        for x in 0..8u32 {
            pixels[(y * 8 + x) as usize] = if x < 4 { red() } else { blue() };
        }
    }
    let tex = Texture2D {
        width: 8,
        height: 8,
        pixels,
    };
    renderer.register_texture("res://halves.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Draw 8x8 texture into 4x4 rect → 2x scale down.
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://halves.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red(), "left half should be red");
    assert_eq!(pixel_at(&frame, 3, 0), blue(), "right half should be blue");
}

// ===========================================================================
// Part 9: Non-square (rectangular) textures
// ===========================================================================

/// Wide texture drawn into matching aspect rect.
#[test]
fn wide_texture_draws_correctly() {
    let mut renderer = SoftwareRenderer::new();
    // 8x2 texture: left 4 pixels red, right 4 pixels green per row.
    let mut pixels = vec![Color::BLACK; 16];
    for y in 0..2u32 {
        for x in 0..8u32 {
            pixels[(y * 8 + x) as usize] = if x < 4 { red() } else { green() };
        }
    }
    let tex = Texture2D {
        width: 8,
        height: 2,
        pixels,
    };
    renderer.register_texture("res://wide.png", tex);

    let mut vp = Viewport::new(16, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://wide.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 2, 1), red(), "left side should be red");
    assert_eq!(
        pixel_at(&frame, 12, 1),
        green(),
        "right side should be green"
    );
}

/// Tall texture drawn into matching aspect rect.
#[test]
fn tall_texture_draws_correctly() {
    let mut renderer = SoftwareRenderer::new();
    // 2x8 texture: top 4 rows blue, bottom 4 rows red.
    let mut pixels = vec![Color::BLACK; 16];
    for y in 0..8u32 {
        for x in 0..2u32 {
            pixels[(y * 2 + x) as usize] = if y < 4 { blue() } else { red() };
        }
    }
    let tex = Texture2D {
        width: 2,
        height: 8,
        pixels,
    };
    renderer.register_texture("res://tall.png", tex);

    let mut vp = Viewport::new(4, 16, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://tall.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 16.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 1, 2), blue(), "top should be blue");
    assert_eq!(pixel_at(&frame, 1, 12), red(), "bottom should be red");
}

/// Non-square texture stretched into a square rect distorts aspect ratio.
#[test]
fn nonsquare_texture_stretched_to_square() {
    let mut renderer = SoftwareRenderer::new();
    // 4x2 texture: left half red, right half green.
    let mut pixels = vec![Color::BLACK; 8];
    for y in 0..2u32 {
        for x in 0..4u32 {
            pixels[(y * 4 + x) as usize] = if x < 2 { red() } else { green() };
        }
    }
    let tex = Texture2D {
        width: 4,
        height: 2,
        pixels,
    };
    renderer.register_texture("res://4x2.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Stretch 4x2 into 10x10 (aspect distortion).
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://4x2.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Left half (x < 5) should sample from left half of texture → red.
    assert_eq!(pixel_at(&frame, 2, 5), red(), "left should map to red");
    // Right half (x >= 5) → green.
    assert_eq!(pixel_at(&frame, 7, 5), green(), "right should map to green");
}

// ===========================================================================
// Part 10: DrawTextureRegion — all quadrants of asymmetric texture
// ===========================================================================

/// Create a 4x4 asymmetric texture: TL=red, TR=green, BL=blue, BR=white.
fn asymmetric_4x4() -> Texture2D {
    let mut pixels = vec![Color::BLACK; 16];
    for y in 0..4u32 {
        for x in 0..4u32 {
            let color = match (x < 2, y < 2) {
                (true, true) => red(),
                (false, true) => green(),
                (true, false) => blue(),
                (false, false) => white(),
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

/// Region: top-right quadrant (green).
#[test]
fn draw_texture_region_top_right_quadrant() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://asym.png", asymmetric_4x4());

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://asym.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "top-right quadrant should be green"
    );
}

/// Region: bottom-left quadrant (blue).
#[test]
fn draw_texture_region_bottom_left_quadrant() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://asym.png", asymmetric_4x4());

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://asym.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::new(0.0, 2.0), Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        blue(),
        "bottom-left quadrant should be blue"
    );
}

/// Region: bottom-right quadrant (white).
#[test]
fn draw_texture_region_bottom_right_quadrant() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://asym.png", asymmetric_4x4());

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://asym.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        white(),
        "bottom-right quadrant should be white"
    );
}

// ===========================================================================
// Part 11: Flipped texture with region
// ===========================================================================

/// Horizontally flipped texture sampled via region still maps correctly.
#[test]
fn flipped_texture_region_horizontal() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4().flip_horizontal();
    renderer.register_texture("res://flipped.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // After H-flip: TL=green, TR=red, BL=white, BR=blue.
    // Draw top-left quadrant (0,0)-(2,2) → should be green.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://flipped.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "H-flipped TL should be green"
    );
}

/// Vertically flipped texture sampled via region.
#[test]
fn flipped_texture_region_vertical() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4().flip_vertical();
    renderer.register_texture("res://vflip.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // After V-flip: TL=blue, TR=white, BL=red, BR=green.
    // Draw top-left quadrant → should be blue.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://vflip.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        blue(),
        "V-flipped TL should be blue"
    );
}

// ===========================================================================
// Part 12: Combined flip + region + modulate
// ===========================================================================

/// Flipped texture region with color modulation.
#[test]
fn flipped_region_with_modulate() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4().flip_horizontal();
    renderer.register_texture("res://hflip.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // H-flipped TL = green. Modulate by half → (0, 0.5, 0).
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://hflip.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        modulate: Color::rgb(0.5, 0.5, 0.5),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 5, 5);
    // Green * 0.5 = (0, 0.5, 0).
    assert!(
        approx_eq(p, Color::rgb(0.0, 0.5, 0.0), 0.02),
        "flipped green region modulated by 0.5: expected ~(0, 0.5, 0), got {:?}",
        p
    );
}

// ===========================================================================
// Part 13: Multiple overlapping textured sprites
// ===========================================================================

/// Three overlapping textures at different z-indices — correct layering.
#[test]
fn three_overlapping_textures_z_order() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://r.png", Texture2D::solid(10, 10, red()));
    renderer.register_texture("res://g.png", Texture2D::solid(6, 6, green()));
    renderer.register_texture("res://b.png", Texture2D::solid(2, 2, blue()));

    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Red full-viewport background z=0.
    let mut bg = CanvasItem::new(CanvasItemId(1));
    bg.z_index = 0;
    bg.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://r.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(bg);

    // Green center z=1.
    let mut mid = CanvasItem::new(CanvasItemId(2));
    mid.z_index = 1;
    mid.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://g.png".to_string(),
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(6.0, 6.0)),
        modulate: white(),
    });
    vp.add_canvas_item(mid);

    // Blue tiny center z=2.
    let mut fg = CanvasItem::new(CanvasItemId(3));
    fg.z_index = 2;
    fg.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://b.png".to_string(),
        rect: Rect2::new(Vector2::new(4.0, 4.0), Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red(), "corner: red background");
    assert_eq!(pixel_at(&frame, 3, 3), green(), "mid ring: green");
    assert_eq!(pixel_at(&frame, 5, 5), blue(), "center: blue foreground");
}

/// Same z-index sprites render in tree order (later added = on top).
#[test]
fn same_z_texture_tree_order() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://r.png", Texture2D::solid(10, 10, red()));
    renderer.register_texture("res://g.png", Texture2D::solid(10, 10, green()));

    let mut vp = Viewport::new(10, 10, Color::BLACK);

    let mut first = CanvasItem::new(CanvasItemId(1));
    first.z_index = 0;
    first.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://r.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(first);

    let mut second = CanvasItem::new(CanvasItemId(2));
    second.z_index = 0;
    second.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://g.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(second);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 5, 5),
        green(),
        "same z: later item renders on top"
    );
}

// ===========================================================================
// Part 14: Modulate alpha (semi-transparent) via DrawTextureRect
// ===========================================================================

/// Half-alpha modulate produces semi-transparent pixel.
#[test]
fn modulate_half_alpha_produces_semitransparent() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::new(1.0, 1.0, 1.0, 0.5),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    // Red(1,0,0,1) * modulate(1,1,1,0.5) → (1,0,0,0.5).
    assert!(
        approx_eq(p, Color::new(1.0, 0.0, 0.0, 0.5), 0.02),
        "half-alpha modulate: expected (1,0,0,0.5), got {:?}",
        p
    );
}

/// Modulate with per-channel alpha and color tint.
#[test]
fn modulate_tint_with_alpha() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, white());
    renderer.register_texture("res://white.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://white.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::new(0.0, 1.0, 0.0, 0.25),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    // White * (0, 1, 0, 0.25) = (0, 1, 0, 0.25).
    assert!(
        approx_eq(p, Color::new(0.0, 1.0, 0.0, 0.25), 0.02),
        "green tint + quarter alpha: expected (0,1,0,0.25), got {:?}",
        p
    );
}

// ===========================================================================
// Part 15: Texture with negative offset (partially offscreen left/top)
// ===========================================================================

/// Texture drawn at negative coordinates clips to visible area.
#[test]
fn texture_negative_position_clips() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(10, 10, green());
    renderer.register_texture("res://green.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Rect from (-5,-5) to (5,5) — only (0,0)-(5,5) visible.
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://green.png".to_string(),
        rect: Rect2::new(Vector2::new(-5.0, -5.0), Vector2::new(10.0, 10.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 0, 0),
        green(),
        "clipped but visible at origin"
    );
    assert_eq!(
        pixel_at(&frame, 4, 4),
        green(),
        "clipped but visible at edge"
    );
    assert_eq!(
        pixel_at(&frame, 5, 5),
        Color::BLACK,
        "outside rect should be clear"
    );
}

// ===========================================================================
// Part 16: Region source rect extends beyond texture bounds (clamping)
// ===========================================================================

/// Source rect partially beyond texture edge clamps to valid texels.
#[test]
fn draw_texture_region_source_rect_clamped() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, red());
    renderer.register_texture("res://red4.png", tex);

    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Source rect (2,2)-(6,6) extends beyond the 4x4 texture → should clamp.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://red4.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(6.0, 6.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Should render red (clamped to edge texels), not crash or show garbage.
    assert_eq!(
        pixel_at(&frame, 5, 5),
        red(),
        "clamped source rect should still produce valid color"
    );
}

// ===========================================================================
// Part 17: Texture resize + flip combined
// ===========================================================================

/// Resize then flip preserves correct pixel mapping.
#[test]
fn texture_resize_then_flip_preserves_pattern() {
    // 2x2: TL=red, TR=green, BL=blue, BR=white.
    let tex = checkerboard_2x2(red(), green());
    // Wait — checkerboard_2x2(A,B) → [A,B,B,A].
    // So: (0,0)=red, (1,0)=green, (0,1)=green, (1,1)=red.

    let resized = tex.resize(4, 4);
    assert_eq!(resized.width, 4);
    assert_eq!(resized.height, 4);

    let flipped = resized.flip_horizontal();
    // After H-flip of 4x4 that was resized from [R,G / G,R]:
    // (0,0) was red → after resize (0,0)=red. After H-flip, (0,0)=green (was right side).
    assert_eq!(
        flipped.get_pixel(0, 0),
        green(),
        "H-flip of resized: left should be green"
    );
    assert_eq!(
        flipped.get_pixel(3, 0),
        red(),
        "H-flip of resized: right should be red"
    );
}

/// Flip then resize produces same result as resize then flip.
#[test]
fn flip_then_resize_equals_resize_then_flip() {
    let tex = checkerboard_2x2(red(), blue());

    let flip_first = tex.flip_horizontal().resize(4, 4);
    let resize_first = tex.resize(4, 4).flip_horizontal();

    for y in 0..4u32 {
        for x in 0..4u32 {
            assert!(
                approx_eq(
                    flip_first.get_pixel(x, y),
                    resize_first.get_pixel(x, y),
                    0.02
                ),
                "flip→resize vs resize→flip mismatch at ({x},{y}): {:?} vs {:?}",
                flip_first.get_pixel(x, y),
                resize_first.get_pixel(x, y),
            );
        }
    }
}

// ===========================================================================
// Part 18: Determinism with multiple sprites and properties
// ===========================================================================

/// Complex scene with multiple textures, flips, modulates is deterministic.
#[test]
fn deterministic_multi_sprite_scene() {
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("res://r.png", Texture2D::solid(4, 4, red()));
        renderer.register_texture(
            "res://g.png",
            Texture2D::solid(4, 4, green()).flip_horizontal(),
        );
        renderer.register_texture("res://b.png", Texture2D::solid(4, 4, blue()));

        let mut vp = Viewport::new(20, 20, Color::BLACK);

        let mut item1 = CanvasItem::new(CanvasItemId(1));
        item1.z_index = 0;
        item1.commands.push(DrawCommand::DrawTextureRect {
            texture_path: "res://r.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            modulate: Color::rgb(0.8, 0.8, 0.8),
        });
        vp.add_canvas_item(item1);

        let mut item2 = CanvasItem::new(CanvasItemId(2));
        item2.z_index = 1;
        item2.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        item2.commands.push(DrawCommand::DrawTextureRect {
            texture_path: "res://g.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)),
            modulate: white(),
        });
        vp.add_canvas_item(item2);

        let mut item3 = CanvasItem::new(CanvasItemId(3));
        item3.z_index = 2;
        item3.commands.push(DrawCommand::DrawTextureRegion {
            texture_path: "res://b.png".to_string(),
            rect: Rect2::new(Vector2::new(8.0, 8.0), Vector2::new(4.0, 4.0)),
            source_rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            modulate: Color::rgb(0.5, 0.5, 1.0),
        });
        vp.add_canvas_item(item3);

        renderer.render_frame(&vp)
    };

    let f1 = make_frame();
    let f2 = make_frame();
    assert_eq!(
        f1.pixels, f2.pixels,
        "complex multi-sprite scene must be deterministic"
    );
}

// ===========================================================================
// Part 19: NinePatch through the renderer pipeline
// ===========================================================================

/// NinePatch draw command renders through the full renderer pipeline.
#[test]
fn nine_patch_through_renderer() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(6, 6, green());
    renderer.register_texture("res://np.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Corners, edges, and center should all be green.
    assert_eq!(pixel_at(&frame, 0, 0), green(), "NP top-left corner");
    assert_eq!(pixel_at(&frame, 19, 19), green(), "NP bottom-right corner");
    assert_eq!(pixel_at(&frame, 10, 0), green(), "NP top edge (stretched)");
    assert_eq!(pixel_at(&frame, 10, 10), green(), "NP center (stretched)");
}

/// NinePatch with draw_center=false leaves center clear.
#[test]
fn nine_patch_no_center_through_renderer() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(6, 6, red());
    renderer.register_texture("res://np_nc.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_nc.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: false,
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red(), "NP corner should render");
    assert_eq!(
        pixel_at(&frame, 10, 10),
        Color::BLACK,
        "NP center should be empty"
    );
}

/// NinePatch with modulate tints all regions.
#[test]
fn nine_patch_with_modulate_through_renderer() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(6, 6, white());
    renderer.register_texture("res://np_mod.png", tex);

    let mut vp = Viewport::new(12, 12, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_mod.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(12.0, 12.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: blue(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // White * blue = blue.
    assert_eq!(
        pixel_at(&frame, 0, 0),
        blue(),
        "NP corner modulated to blue"
    );
    assert_eq!(
        pixel_at(&frame, 6, 6),
        blue(),
        "NP center modulated to blue"
    );
}

/// NinePatch with asymmetric texture preserves corner pixel identity.
#[test]
fn nine_patch_asymmetric_corners() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4();
    renderer.register_texture("res://np_asym.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_asym.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Top-left corner of asymmetric texture = red.
    assert_eq!(pixel_at(&frame, 0, 0), red(), "NP TL corner = red");
    // Top-right corner of asymmetric texture = green.
    assert_eq!(pixel_at(&frame, 19, 0), green(), "NP TR corner = green");
    // Bottom-left corner = blue.
    assert_eq!(pixel_at(&frame, 0, 19), blue(), "NP BL corner = blue");
    // Bottom-right corner = white.
    assert_eq!(pixel_at(&frame, 19, 19), white(), "NP BR corner = white");
}

// ===========================================================================
// Part 20: Transform scale with texture
// ===========================================================================

/// Texture drawn with a scale transform enlarges the output.
#[test]
fn transform_scale_enlarges_texture() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, red());
    renderer.register_texture("res://red.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Scale 2x: a 4x4 texture rect becomes 8x8 on screen.
    item.transform = Transform2D::scaled(Vector2::new(2.0, 2.0));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://red.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Scaled rect: (0,0) to (8,8).
    assert_eq!(pixel_at(&frame, 0, 0), red(), "scaled origin");
    assert_eq!(pixel_at(&frame, 7, 7), red(), "scaled far corner");
    assert_eq!(pixel_at(&frame, 8, 8), Color::BLACK, "outside scaled rect");
}

/// Transform scale + translation positions texture correctly.
#[test]
fn transform_scale_and_translate() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(4, 4, blue());
    renderer.register_texture("res://blue.png", tex);

    let mut vp = Viewport::new(30, 30, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Translate (10,10) then conceptually the 4x4 rect at origin + scale 2x
    // → rect becomes (0,0)-(8,8) in local space, then translated to (10,10)-(18,18).
    let scale = Transform2D::scaled(Vector2::new(2.0, 2.0));
    let translate = Transform2D::translated(Vector2::new(10.0, 10.0));
    item.transform = translate * scale;
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://blue.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(
        pixel_at(&frame, 12, 12),
        blue(),
        "scaled+translated interior"
    );
    assert_eq!(
        pixel_at(&frame, 9, 9),
        Color::BLACK,
        "before translated rect"
    );
    assert_eq!(pixel_at(&frame, 18, 18), Color::BLACK, "after scaled rect");
}

// ===========================================================================
// Part 21: Flipped texture at non-1:1 scale through renderer
// ===========================================================================

/// Horizontally flipped texture drawn at 2x scale preserves flip.
#[test]
fn flipped_texture_at_2x_scale() {
    let mut renderer = SoftwareRenderer::new();
    // 2x1: left=red, right=green.
    let tex = Texture2D {
        width: 2,
        height: 1,
        pixels: vec![red(), green()],
    };
    let flipped = tex.flip_horizontal();
    renderer.register_texture("res://flipped_2x.png", flipped);

    let mut vp = Viewport::new(20, 2, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Draw into a 20x2 rect (10x scale).
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://flipped_2x.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // After H-flip: left=green, right=red. Scaled 10x.
    assert_eq!(
        pixel_at(&frame, 2, 0),
        green(),
        "left side after flip+scale = green"
    );
    assert_eq!(
        pixel_at(&frame, 15, 0),
        red(),
        "right side after flip+scale = red"
    );
}

/// Vertically flipped asymmetric texture at 2x scale.
#[test]
fn flipped_asymmetric_at_2x_scale() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4().flip_vertical();
    renderer.register_texture("res://vf_2x.png", tex);

    let mut vp = Viewport::new(16, 16, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // 4x4 texture drawn into 8x8 (2x scale).
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://vf_2x.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // After V-flip: TL=blue, TR=white, BL=red, BR=green. Each quadrant is 4x4 pixels.
    assert_eq!(pixel_at(&frame, 1, 1), blue(), "V-flipped 2x TL = blue");
    assert_eq!(pixel_at(&frame, 5, 1), white(), "V-flipped 2x TR = white");
    assert_eq!(pixel_at(&frame, 1, 5), red(), "V-flipped 2x BL = red");
    assert_eq!(pixel_at(&frame, 5, 5), green(), "V-flipped 2x BR = green");
}

// ===========================================================================
// Part 22: Semi-transparent texture pixels
// ===========================================================================

/// Texture with per-pixel alpha writes transparent pixels correctly.
#[test]
fn texture_with_per_pixel_alpha() {
    let mut renderer = SoftwareRenderer::new();
    // 2x2 texture: TL = red fully opaque, TR = green half-transparent.
    let tex = Texture2D {
        width: 2,
        height: 2,
        pixels: vec![
            Color::new(1.0, 0.0, 0.0, 1.0),  // TL: fully opaque red
            Color::new(0.0, 1.0, 0.0, 0.5),  // TR: half-alpha green
            Color::new(0.0, 0.0, 1.0, 0.0),  // BL: fully transparent blue
            Color::new(1.0, 1.0, 1.0, 0.25), // BR: quarter-alpha white
        ],
    };
    renderer.register_texture("res://alpha.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // 2x2 texture → 4x4 rect, each texel covers 2x2 screen pixels.
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://alpha.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // TL: fully opaque red.
    assert_eq!(pixel_at(&frame, 0, 0), Color::new(1.0, 0.0, 0.0, 1.0));
    // TR: half-alpha green.
    let tr = pixel_at(&frame, 2, 0);
    assert!(
        approx_eq(tr, Color::new(0.0, 1.0, 0.0, 0.5), 0.02),
        "half-alpha green: got {:?}",
        tr
    );
    // BL: fully transparent blue → alpha channel = 0.
    let bl = pixel_at(&frame, 0, 2);
    assert!(
        bl.a < 0.01,
        "fully transparent pixel should have near-zero alpha, got {}",
        bl.a
    );
}

/// Modulate alpha multiplies with texture alpha.
#[test]
fn modulate_alpha_multiplies_with_texture_alpha() {
    let mut renderer = SoftwareRenderer::new();
    // Half-alpha red texture.
    let tex = Texture2D::solid(4, 4, Color::new(1.0, 0.0, 0.0, 0.5));
    renderer.register_texture("res://half_alpha.png", tex);

    let mut vp = Viewport::new(4, 4, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    // Modulate with half alpha → final alpha = 0.5 * 0.5 = 0.25.
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://half_alpha.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: Color::new(1.0, 1.0, 1.0, 0.5),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    let p = pixel_at(&frame, 2, 2);
    // (1,0,0,0.5) * modulate(1,1,1,0.5) = (1,0,0,0.25).
    assert!(
        approx_eq(p, Color::new(1.0, 0.0, 0.0, 0.25), 0.02),
        "alpha multiply: expected (1,0,0,0.25), got {:?}",
        p
    );
}

// ===========================================================================
// Part 23: NinePatch with transform
// ===========================================================================

/// NinePatch drawn with a translation transform.
#[test]
fn nine_patch_with_transform() {
    let mut renderer = SoftwareRenderer::new();
    let tex = Texture2D::solid(6, 6, green());
    renderer.register_texture("res://np_t.png", tex);

    let mut vp = Viewport::new(30, 30, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_t.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 4, 4), Color::BLACK, "before translated NP");
    assert_eq!(pixel_at(&frame, 5, 5), green(), "translated NP start");
    assert_eq!(pixel_at(&frame, 14, 14), green(), "translated NP end");
    assert_eq!(
        pixel_at(&frame, 15, 15),
        Color::BLACK,
        "after translated NP"
    );
}

// ===========================================================================
// Part 24: Mixed DrawCommand types on single item
// ===========================================================================

/// Single canvas item with rect, texture, and nine-patch commands all render.
#[test]
fn mixed_rect_texture_ninepatch_on_single_item() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://g_tex.png", Texture2D::solid(4, 4, green()));
    renderer.register_texture("res://np_tex.png", Texture2D::solid(6, 6, blue()));

    let mut vp = Viewport::new(30, 30, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));

    // 1. Red background rect.
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(30.0, 30.0)),
        color: red(),
        filled: true,
    });
    // 2. Green texture in top-left.
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://g_tex.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        modulate: white(),
    });
    // 3. Blue nine-patch in bottom-right.
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_tex.png".to_string(),
        rect: Rect2::new(Vector2::new(20.0, 20.0), Vector2::new(10.0, 10.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Top-left: green texture overwrites red rect.
    assert_eq!(pixel_at(&frame, 1, 1), green(), "TL = green texture");
    // Middle: red rect (no overwrite).
    assert_eq!(pixel_at(&frame, 15, 15), red(), "mid = red rect");
    // Bottom-right: blue nine-patch overwrites red rect.
    assert_eq!(pixel_at(&frame, 25, 25), blue(), "BR = blue nine-patch");
}

// ===========================================================================
// Part 25: Texture region with transform
// ===========================================================================

/// DrawTextureRegion respects canvas item transform.
#[test]
fn texture_region_with_transform() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://asym.png", asymmetric_4x4());

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    // Draw top-right quadrant (green) of asymmetric texture.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://asym.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(6.0, 6.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // Region drawn at (5,5) to (11,11) should be green.
    assert_eq!(
        pixel_at(&frame, 7, 7),
        green(),
        "transformed region interior = green"
    );
    assert_eq!(
        pixel_at(&frame, 4, 4),
        Color::BLACK,
        "before transformed region"
    );
}

// ===========================================================================
// Part 26: Texture z-ordering with NinePatch
// ===========================================================================

/// NinePatch at higher z renders on top of texture at lower z.
#[test]
fn nine_patch_over_texture_z_order() {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://bg.png", Texture2D::solid(20, 20, red()));
    renderer.register_texture("res://np_fg.png", Texture2D::solid(6, 6, green()));

    let mut vp = Viewport::new(20, 20, Color::BLACK);

    let mut bg = CanvasItem::new(CanvasItemId(1));
    bg.z_index = 0;
    bg.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "res://bg.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)),
        modulate: white(),
    });
    vp.add_canvas_item(bg);

    let mut fg = CanvasItem::new(CanvasItemId(2));
    fg.z_index = 1;
    fg.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://np_fg.png".to_string(),
        rect: Rect2::new(Vector2::new(5.0, 5.0), Vector2::new(10.0, 10.0)),
        margin_left: 2.0,
        margin_top: 2.0,
        margin_right: 2.0,
        margin_bottom: 2.0,
        draw_center: true,
        modulate: white(),
    });
    vp.add_canvas_item(fg);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 0, 0), red(), "corner: bg texture");
    assert_eq!(
        pixel_at(&frame, 10, 10),
        green(),
        "center: fg nine-patch on top"
    );
}

// ===========================================================================
// Part 27: Texture flip + modulate + region + transform (full combo)
// ===========================================================================

/// All sprite properties combined: flip + modulate + region + transform.
#[test]
fn full_sprite_property_combo() {
    let mut renderer = SoftwareRenderer::new();
    // H-flip the asymmetric texture: TL=green, TR=red, BL=white, BR=blue.
    let tex = asymmetric_4x4().flip_horizontal();
    renderer.register_texture("res://combo.png", tex);

    let mut vp = Viewport::new(30, 30, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
    // Draw bottom-right quadrant of flipped texture (= blue), with red modulate.
    // blue(0,0,1) * red(1,0,0) = black(0,0,0).
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://combo.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)),
        source_rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0)),
        modulate: red(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    // blue * red = black. Drawn at (5,5) to (13,13).
    let p = pixel_at(&frame, 8, 8);
    assert!(
        approx_eq(p, Color::rgb(0.0, 0.0, 0.0), 0.02),
        "blue region * red modulate = black, got {:?}",
        p
    );
    assert_eq!(pixel_at(&frame, 4, 4), Color::BLACK, "before region");
}

/// Full combo with white modulate preserves expected color.
#[test]
fn full_combo_white_modulate_preserves_color() {
    let mut renderer = SoftwareRenderer::new();
    let tex = asymmetric_4x4().flip_vertical();
    renderer.register_texture("res://combo2.png", tex);

    let mut vp = Viewport::new(20, 20, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(Vector2::new(2.0, 2.0));
    // After V-flip: TL=blue. Draw TL quadrant with white modulate → blue.
    item.commands.push(DrawCommand::DrawTextureRegion {
        texture_path: "res://combo2.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(6.0, 6.0)),
        source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
        modulate: white(),
    });
    vp.add_canvas_item(item);

    let frame = renderer.render_frame(&vp);
    assert_eq!(pixel_at(&frame, 4, 4), blue(), "V-flipped TL region = blue");
}

// ===========================================================================
// Part 28: Determinism with nine-patch and mixed commands
// ===========================================================================

/// Complex scene with nine-patch, textures, and transforms is deterministic.
#[test]
fn deterministic_ninepatch_scene() {
    let make_frame = || {
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("res://r.png", Texture2D::solid(4, 4, red()));
        renderer.register_texture("res://np.png", Texture2D::solid(6, 6, green()));
        renderer.register_texture("res://asym.png", asymmetric_4x4().flip_horizontal());

        let mut vp = Viewport::new(30, 30, Color::BLACK);

        // Background red texture.
        let mut bg = CanvasItem::new(CanvasItemId(1));
        bg.z_index = 0;
        bg.commands.push(DrawCommand::DrawTextureRect {
            texture_path: "res://r.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(30.0, 30.0)),
            modulate: Color::rgb(0.9, 0.9, 0.9),
        });
        vp.add_canvas_item(bg);

        // NinePatch foreground.
        let mut np = CanvasItem::new(CanvasItemId(2));
        np.z_index = 1;
        np.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        np.commands.push(DrawCommand::DrawNinePatch {
            texture_path: "res://np.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(15.0, 15.0)),
            margin_left: 2.0,
            margin_top: 2.0,
            margin_right: 2.0,
            margin_bottom: 2.0,
            draw_center: true,
            modulate: Color::rgb(0.5, 1.0, 0.5),
        });
        vp.add_canvas_item(np);

        // Flipped asymmetric region on top.
        let mut top = CanvasItem::new(CanvasItemId(3));
        top.z_index = 2;
        top.commands.push(DrawCommand::DrawTextureRegion {
            texture_path: "res://asym.png".to_string(),
            rect: Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(8.0, 8.0)),
            source_rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
            modulate: white(),
        });
        vp.add_canvas_item(top);

        renderer.render_frame(&vp)
    };

    let f1 = make_frame();
    let f2 = make_frame();
    assert_eq!(
        f1.pixels, f2.pixels,
        "NP + texture + region scene must be deterministic"
    );
}
