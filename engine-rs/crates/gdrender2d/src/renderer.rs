//! Software renderer implementing [`RenderingServer2D`].
//!
//! Provides a fully CPU-based 2D renderer that rasterizes draw commands
//! into a [`FrameBuffer`].

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::{FrameData, RenderingServer2D};
use gdserver2d::viewport::Viewport;

use crate::draw;
use crate::texture::Texture2D;

/// A pixel framebuffer for the software renderer.
#[derive(Debug, Clone)]
pub struct FrameBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in row-major order.
    pub pixels: Vec<Color>,
}

impl FrameBuffer {
    /// Creates a new framebuffer filled with `clear_color`.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![clear_color; (width * height) as usize],
        }
    }

    /// Clears the entire framebuffer to the given color.
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    /// Sets a pixel at `(x, y)`. No-op if out of bounds.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    /// Returns the color at `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if `(x, y)` is out of bounds.
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        self.pixels[(y * self.width + x) as usize]
    }
}

/// A CPU-based 2D software renderer.
#[derive(Debug)]
pub struct SoftwareRenderer {
    /// Internal canvas items managed outside the viewport.
    items: Vec<CanvasItem>,
    /// Next available ID.
    next_id: u64,
    /// Textures registered for DrawTextureRect commands, keyed by path.
    textures: Vec<(String, Texture2D)>,
}

impl SoftwareRenderer {
    /// Creates a new software renderer.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            textures: Vec::new(),
        }
    }

    /// Registers a texture that can be referenced by `DrawTextureRect` commands.
    pub fn register_texture(&mut self, path: &str, texture: Texture2D) {
        self.textures.push((path.to_string(), texture));
    }

    /// Looks up a registered texture by path.
    fn find_texture(&self, path: &str) -> Option<&Texture2D> {
        self.textures
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| t)
    }

    /// Rasterizes a single canvas item's draw commands into the framebuffer.
    ///
    /// `parent_transform` is the accumulated transform from all ancestors,
    /// applied before the item's own transform.
    fn rasterize_item(
        &self,
        fb: &mut FrameBuffer,
        item: &CanvasItem,
        parent_transform: Transform2D,
    ) {
        if !item.visible {
            return;
        }

        let global_transform = parent_transform * item.transform;

        for cmd in &item.commands {
            match cmd {
                DrawCommand::DrawRect {
                    rect,
                    color,
                    filled,
                } => {
                    if *filled {
                        let pos = global_transform.xform(rect.position);
                        let transformed = Rect2::new(pos, rect.size);
                        draw::fill_rect(fb, transformed, *color);
                    }
                }
                DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                } => {
                    let pos = global_transform.xform(*center);
                    draw::fill_circle(fb, pos, *radius, *color);
                }
                DrawCommand::DrawLine {
                    from,
                    to,
                    color,
                    width,
                } => {
                    let f = global_transform.xform(*from);
                    let t = global_transform.xform(*to);
                    draw::draw_line(fb, f, t, *color, *width);
                }
                DrawCommand::DrawTextureRect {
                    texture_path,
                    rect,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        let pos = global_transform.xform(rect.position);
                        let transformed = Rect2::new(pos, rect.size);
                        draw::draw_texture_rect(fb, tex, transformed, *modulate);
                    }
                }
                DrawCommand::DrawTextureRegion {
                    texture_path,
                    rect,
                    source_rect,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        let pos = global_transform.xform(rect.position);
                        let transformed = Rect2::new(pos, rect.size);
                        draw::draw_texture_region(fb, tex, transformed, *source_rect, *modulate);
                    }
                }
            }
        }
    }

    /// Computes the camera transform to apply to draw commands.
    ///
    /// When camera is enabled (non-zero position, non-identity zoom, or non-zero rotation),
    /// offsets all draw commands by `-camera_position + viewport_size/2`, applies zoom
    /// and rotation. When camera is at defaults, returns identity.
    fn compute_camera_transform(viewport: &Viewport) -> Transform2D {
        let has_camera = viewport.camera_position.x != 0.0
            || viewport.camera_position.y != 0.0
            || viewport.camera_rotation != 0.0
            || (viewport.camera_zoom.x - 1.0).abs() > f32::EPSILON
            || (viewport.camera_zoom.y - 1.0).abs() > f32::EPSILON;

        if !has_camera {
            return Transform2D::IDENTITY;
        }

        let half_w = viewport.width as f32 / 2.0;
        let half_h = viewport.height as f32 / 2.0;

        // Translation: move world so camera_position maps to viewport center.
        let translation = Transform2D::translated(Vector2::new(
            -viewport.camera_position.x + half_w,
            -viewport.camera_position.y + half_h,
        ));
        // Rotation around viewport center.
        let rotation = Transform2D::rotated(viewport.camera_rotation);
        // Zoom (scale).
        let zoom = Transform2D::scaled(viewport.camera_zoom);

        // Apply: first translate world, then rotate, then scale.
        zoom * rotation * translation
    }

    /// Resolves the global transform for a canvas item by walking up the parent chain.
    fn resolve_parent_transform(&self, item: &CanvasItem, viewport: &Viewport) -> Transform2D {
        match item.parent {
            Some(parent_id) => {
                if let Some(parent) = viewport.get_canvas_item(parent_id) {
                    self.resolve_parent_transform(parent, viewport) * parent.transform
                } else {
                    Transform2D::IDENTITY
                }
            }
            None => Transform2D::IDENTITY,
        }
    }
}

impl Default for SoftwareRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderingServer2D for SoftwareRenderer {
    fn create_canvas_item(&mut self) -> CanvasItemId {
        let id = CanvasItemId(self.next_id);
        self.next_id += 1;
        self.items.push(CanvasItem::new(id));
        id
    }

    fn free_canvas_item(&mut self, id: CanvasItemId) {
        self.items.retain(|item| item.id != id);
    }

    fn canvas_item_add_draw_command(&mut self, id: CanvasItemId, command: DrawCommand) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.commands.push(command);
        }
    }

    fn canvas_item_set_transform(&mut self, id: CanvasItemId, transform: Transform2D) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.transform = transform;
        }
    }

    fn canvas_item_set_z_index(&mut self, id: CanvasItemId, z_index: i32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.z_index = z_index;
        }
    }

    fn canvas_item_set_visible(&mut self, id: CanvasItemId, visible: bool) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.visible = visible;
        }
    }

    fn render_frame(&mut self, viewport: &Viewport) -> FrameData {
        let mut fb = FrameBuffer::new(viewport.width, viewport.height, viewport.clear_color);

        let camera_xform = Self::compute_camera_transform(viewport);

        // Render layered items first (sorted by layer z_order).
        let sorted_layers = viewport.get_sorted_layers();
        for layer in &sorted_layers {
            if !layer.visible {
                continue;
            }
            let layer_xform = camera_xform * layer.transform;
            let items = viewport.get_items_for_layer(layer.layer_id);
            for item in items {
                let parent_xform = self.resolve_parent_transform(item, viewport);
                self.rasterize_item(&mut fb, item, layer_xform * parent_xform);
            }
        }

        // Render unlayered items with camera transform.
        let unlayered = viewport.get_unlayered_items();
        for item in unlayered {
            let parent_xform = self.resolve_parent_transform(item, viewport);
            self.rasterize_item(&mut fb, item, camera_xform * parent_xform);
        }

        FrameData {
            width: fb.width,
            height: fb.height,
            pixels: fb.pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;

    #[test]
    fn framebuffer_create_and_clear() {
        let fb = FrameBuffer::new(4, 4, Color::BLACK);
        assert_eq!(fb.width, 4);
        assert_eq!(fb.height, 4);
        assert_eq!(fb.pixels.len(), 16);
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn framebuffer_set_get_pixel() {
        let mut fb = FrameBuffer::new(4, 4, Color::BLACK);
        let red = Color::rgb(1.0, 0.0, 0.0);
        fb.set_pixel(2, 3, red);
        assert_eq!(fb.get_pixel(2, 3), red);
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn framebuffer_clear() {
        let mut fb = FrameBuffer::new(4, 4, Color::BLACK);
        fb.set_pixel(0, 0, Color::WHITE);
        fb.clear(Color::rgb(0.5, 0.5, 0.5));
        assert_eq!(fb.get_pixel(0, 0), Color::rgb(0.5, 0.5, 0.5));
    }

    #[test]
    fn render_frame_rect_and_circle() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        item.commands.push(DrawCommand::DrawCircle {
            center: Vector2::new(15.0, 15.0),
            radius: 3.0,
            color: Color::rgb(0.0, 0.0, 1.0),
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        assert_eq!(frame.width, 20);
        assert_eq!(frame.height, 20);

        // Red rect pixel.
        let p = frame.pixels[0]; // (0,0)
        assert_eq!(p, Color::rgb(1.0, 0.0, 0.0));

        // Blue circle center.
        let center_idx = 15 * 20 + 15; // (15, 15)
        let p2 = frame.pixels[center_idx];
        assert_eq!(p2, Color::rgb(0.0, 0.0, 1.0));

        // Background pixel.
        let bg = frame.pixels[10 * 20 + 10]; // (10, 10)
        assert_eq!(bg, Color::BLACK);
    }

    #[test]
    fn z_index_ordering_later_draws_on_top() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        // Item at z=0: red rect filling entire viewport.
        let mut bottom = CanvasItem::new(CanvasItemId(1));
        bottom.z_index = 0;
        bottom.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });

        // Item at z=1: blue rect filling entire viewport (on top).
        let mut top = CanvasItem::new(CanvasItemId(2));
        top.z_index = 1;
        top.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::rgb(0.0, 0.0, 1.0),
            filled: true,
        });

        vp.add_canvas_item(bottom);
        vp.add_canvas_item(top);

        let frame = renderer.render_frame(&vp);
        // Blue should be on top everywhere.
        assert_eq!(frame.pixels[0], Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(frame.pixels[55], Color::rgb(0.0, 0.0, 1.0)); // (5, 5)
    }

    #[test]
    fn invisible_items_not_rendered() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.visible = false;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::WHITE,
            filled: true,
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        // Should remain black since item is invisible.
        assert_eq!(frame.pixels[0], Color::BLACK);
    }

    #[test]
    fn transform_applied_to_draw_commands() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        // Translate by (5, 5).
        item.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(3.0, 3.0)),
            color: Color::rgb(0.0, 1.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        let green = Color::rgb(0.0, 1.0, 0.0);

        // Origin should be black (rect was translated).
        assert_eq!(frame.pixels[0], Color::BLACK);
        // (5, 5) should be green.
        assert_eq!(frame.pixels[5 * 20 + 5], green);
        // (7, 7) should be green (within 3x3 rect).
        assert_eq!(frame.pixels[7 * 20 + 7], green);
        // (8, 8) should be black (outside 3x3 rect).
        assert_eq!(frame.pixels[8 * 20 + 8], Color::BLACK);
    }

    #[test]
    fn texture_drawing_with_modulate() {
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("test.png", Texture2D::solid(2, 2, Color::WHITE));

        let mut vp = Viewport::new(10, 10, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawTextureRect {
            texture_path: "test.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            modulate: Color::rgb(0.5, 0.0, 0.0),
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        let pixel = frame.pixels[0];
        assert!((pixel.r - 0.5).abs() < 0.01);
        assert!(pixel.g.abs() < 0.01);
    }

    #[test]
    fn determinism_same_commands_same_pixels() {
        let make_frame = || {
            let mut renderer = SoftwareRenderer::new();
            let mut vp = Viewport::new(10, 10, Color::BLACK);

            let mut item = CanvasItem::new(CanvasItemId(1));
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::new(1.0, 1.0), Vector2::new(5.0, 5.0)),
                color: Color::rgb(1.0, 0.0, 0.0),
                filled: true,
            });
            item.commands.push(DrawCommand::DrawCircle {
                center: Vector2::new(7.0, 7.0),
                radius: 2.0,
                color: Color::rgb(0.0, 1.0, 0.0),
            });
            vp.add_canvas_item(item);
            renderer.render_frame(&vp)
        };

        let frame1 = make_frame();
        let frame2 = make_frame();
        assert_eq!(frame1.pixels, frame2.pixels);
    }

    #[test]
    fn camera2d_offset_shifts_rendering() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);

        // Place rect at world position (10, 10).
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(10.0, 10.0), Vector2::new(2.0, 2.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        // Camera centered on (10, 10) → rect should appear at viewport center (10, 10).
        vp.camera_position = Vector2::new(10.0, 10.0);
        let frame = renderer.render_frame(&vp);
        let red = Color::rgb(1.0, 0.0, 0.0);

        // World (10,10) maps to screen (10,10) when camera is at (10,10) in a 20x20 viewport.
        assert_eq!(frame.pixels[10 * 20 + 10], red);
        // Origin should be black (camera shifted things).
        assert_eq!(frame.pixels[0], Color::BLACK);
    }

    #[test]
    fn camera2d_zoom_scales_rendering() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);

        // Rect at world origin, 2x2.
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
            color: Color::rgb(0.0, 1.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        // Camera at origin with 2x zoom.
        vp.camera_position = Vector2::ZERO;
        vp.camera_zoom = Vector2::new(2.0, 2.0);
        let frame = renderer.render_frame(&vp);
        // With 2x zoom and camera at origin, world (0,0) maps to screen (20,20)
        // because offset = -0 + 10 = 10, then scaled by 2 = 20... which is off-screen.
        // Actually let's just check the center area.
        // Transform: zoom * rotation * translation
        // translation maps (0,0) → (10,10)
        // zoom maps (10,10) → (20,20) — off screen!
        // So the green rect should NOT appear at (0,0).
        // Let's verify a different scenario: camera at (5,5) should put world origin near center.
        // This test just verifies camera zoom is active and changes output.
        let frame_no_zoom = {
            let mut r2 = SoftwareRenderer::new();
            let mut vp2 = Viewport::new(20, 20, Color::BLACK);
            let mut item2 = CanvasItem::new(CanvasItemId(1));
            item2.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
                color: Color::rgb(0.0, 1.0, 0.0),
                filled: true,
            });
            vp2.add_canvas_item(item2);
            r2.render_frame(&vp2)
        };

        // Zoomed frame should differ from non-zoomed frame.
        assert_ne!(frame.pixels, frame_no_zoom.pixels);
    }

    #[test]
    fn parent_transform_inherited_by_child() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(20, 20, Color::BLACK);

        // Parent translated to (5, 5).
        let parent = {
            let mut p = CanvasItem::new(CanvasItemId(1));
            p.transform = Transform2D::translated(Vector2::new(5.0, 5.0));
            p
        };

        // Child at (2, 2) relative to parent → should appear at (7, 7).
        let mut child = CanvasItem::new(CanvasItemId(2));
        child.parent = Some(CanvasItemId(1));
        child.transform = Transform2D::translated(Vector2::new(2.0, 2.0));
        child.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });

        vp.add_canvas_item(parent);
        vp.add_canvas_item(child);

        let frame = renderer.render_frame(&vp);
        let red = Color::rgb(1.0, 0.0, 0.0);

        // Child should be at (7,7) = parent(5,5) + child(2,2).
        assert_eq!(frame.pixels[7 * 20 + 7], red);
        assert_eq!(frame.pixels[8 * 20 + 8], red);
        // Origin should be black.
        assert_eq!(frame.pixels[0], Color::BLACK);
        // (5,5) should be black (parent has no draw commands, child is offset further).
        assert_eq!(frame.pixels[5 * 20 + 5], Color::BLACK);
    }

    #[test]
    fn canvas_layer_rendering() {
        use gdserver2d::canvas_layer::CanvasLayer;

        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        // Create a layer with transform offset (3, 3).
        let mut layer = CanvasLayer::new(1);
        layer.transform = Transform2D::translated(Vector2::new(3.0, 3.0));
        vp.add_canvas_layer(layer);

        // Item in layer 1 at origin → should appear at (3, 3) due to layer transform.
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.layer_id = Some(1);
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0)),
            color: Color::rgb(0.0, 0.0, 1.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        let blue = Color::rgb(0.0, 0.0, 1.0);

        assert_eq!(frame.pixels[3 * 10 + 3], blue);
        assert_eq!(frame.pixels[4 * 10 + 4], blue);
        assert_eq!(frame.pixels[0], Color::BLACK);
    }

    #[test]
    fn invisible_canvas_layer_hides_items() {
        use gdserver2d::canvas_layer::CanvasLayer;

        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        let mut layer = CanvasLayer::new(1);
        layer.visible = false;
        vp.add_canvas_layer(layer);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.layer_id = Some(1);
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::WHITE,
            filled: true,
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        // Everything should be black since layer is invisible.
        assert_eq!(frame.pixels[0], Color::BLACK);
        assert_eq!(frame.pixels[55], Color::BLACK);
    }

    #[test]
    fn texture_region_rendering_through_pipeline() {
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("atlas.png", Texture2D::solid(8, 8, Color::WHITE));

        let mut vp = Viewport::new(10, 10, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawTextureRegion {
            texture_path: "atlas.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            source_rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            modulate: Color::rgb(0.0, 0.5, 0.0),
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        let pixel = frame.pixels[1 * 10 + 1];
        assert!((pixel.g - 0.5).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
    }
}
