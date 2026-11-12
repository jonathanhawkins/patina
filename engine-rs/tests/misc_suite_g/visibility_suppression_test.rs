//! pat-uapa: Visibility false suppresses draw calls.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

#[test]
fn visible_item_draws() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.visible = true;
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);
    let frame = renderer.render_frame(&vp);
    assert!(frame.pixels[0].r > 0.9, "visible item should draw red");
}

#[test]
fn invisible_item_suppressed() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.visible = false;
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);
    let frame = renderer.render_frame(&vp);
    assert!(frame.pixels[0].r < 0.1, "invisible item should not draw");
}

#[test]
fn invisible_item_leaves_background() {
    let mut renderer = SoftwareRenderer::new();
    let bg = Color::rgb(0.0, 0.0, 1.0);
    let mut vp = Viewport::new(10, 10, bg);
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.visible = false;
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);
    let frame = renderer.render_frame(&vp);
    assert!(frame.pixels[0].b > 0.9, "background should show through");
    assert!(frame.pixels[0].r < 0.1, "red should not appear");
}

#[test]
fn server_api_visibility_suppression() {
    let mut renderer = SoftwareRenderer::new();
    let id = renderer.create_canvas_item();

    renderer.canvas_item_add_draw_command(
        id,
        DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            filled: true,
        },
    );

    // Hide the item via the RenderingServer2D trait method.
    renderer.canvas_item_set_visible(id, false);

    let viewport = Viewport::new(20, 20, Color::BLACK);
    let frame = renderer.render_frame(&viewport);

    // Pixel at (5,5) must remain black (clear color), not red.
    let idx = 5 * 20 + 5;
    let pixel = frame.pixels[idx];
    assert!(
        pixel.r < 0.01,
        "invisible server item must not draw, got pixel {:?}",
        pixel
    );
}

#[test]
fn server_api_visible_item_draws() {
    let mut renderer = SoftwareRenderer::new();
    let id = renderer.create_canvas_item();

    renderer.canvas_item_add_draw_command(
        id,
        DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            filled: true,
        },
    );

    // Item is visible by default.
    let viewport = Viewport::new(20, 20, Color::BLACK);
    let frame = renderer.render_frame(&viewport);

    let idx = 5 * 20 + 5;
    let pixel = frame.pixels[idx];
    assert!(
        pixel.r > 0.9,
        "visible server item should draw red, got {:?}",
        pixel
    );
}

#[test]
fn mixed_visible_and_invisible() {
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(10, 10, Color::BLACK);

    // Invisible red rect
    let mut item1 = CanvasItem::new(CanvasItemId(1));
    item1.visible = false;
    item1.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item1);

    // Visible green rect
    let mut item2 = CanvasItem::new(CanvasItemId(2));
    item2.visible = true;
    item2.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
        color: Color::rgb(0.0, 1.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item2);

    let frame = renderer.render_frame(&vp);
    assert!(frame.pixels[0].g > 0.9, "green visible item should draw");
    assert!(
        frame.pixels[0].r < 0.1,
        "red invisible item should not draw"
    );
}
