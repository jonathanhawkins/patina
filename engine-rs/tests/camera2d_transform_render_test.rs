//! pat-e3w5: Camera2D transform applied correctly to render output.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::RenderingServer2D;
use gdserver2d::viewport::Viewport;

fn make_viewport(cam_pos: Vector2, cam_zoom: Vector2) -> Viewport {
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    vp.camera_position = cam_pos;
    vp.camera_zoom = cam_zoom;
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);
    vp
}

fn px(frame: &gdserver2d::server::FrameData, x: u32, y: u32) -> Color {
    frame.pixels[(y * 20 + x) as usize]
}

#[test]
fn camera_offset_shifts_content() {
    let mut renderer = SoftwareRenderer::new();
    let vp = make_viewport(Vector2::new(10.0, 10.0), Vector2::ONE);
    let f = renderer.render_frame(&vp);
    assert!(
        px(&f, 0, 0).r > 0.9,
        "world origin should map to screen(0,0)"
    );
    assert!(px(&f, 10, 10).r < 0.1, "screen center should be empty");
}

#[test]
fn camera_pushes_content_offscreen() {
    let mut renderer = SoftwareRenderer::new();
    let vp = make_viewport(Vector2::new(-10.0, -10.0), Vector2::ONE);
    let f = renderer.render_frame(&vp);
    assert!(px(&f, 0, 0).r < 0.1, "rect should be off-screen");
}

#[test]
fn camera_zoom_scales_content() {
    let mut renderer = SoftwareRenderer::new();
    let vp = make_viewport(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0));
    let f = renderer.render_frame(&vp);
    assert!(px(&f, 10, 10).r > 0.9, "center should be red at 2x zoom");
    assert!(px(&f, 7, 7).r > 0.9, "inside zoomed rect");
    assert!(px(&f, 5, 5).r < 0.1, "outside zoomed rect");
    assert!(px(&f, 15, 15).r < 0.1, "past zoomed rect");
}

#[test]
fn camera_identity_renders_rect_at_origin() {
    // With no camera offset and no zoom, the rect at world(0,0) should
    // appear at screen(0,0) (no camera transform applied).
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(20, 20, Color::BLACK);
    // Leave camera_position at default (0,0) and zoom at default (1,1)
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(4.0, 4.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    vp.add_canvas_item(item);
    let f = renderer.render_frame(&vp);
    // Rect at (2,2)-(6,6) should have red pixels somewhere in that range
    let p = px(&f, 3, 3);
    assert!(p.r > 0.5, "rect pixel should be visible, got r={}", p.r);
}
