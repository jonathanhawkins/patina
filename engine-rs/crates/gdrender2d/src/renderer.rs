//! Software renderer implementing [`RenderingServer2D`].
//!
//! Provides a fully CPU-based 2D renderer that rasterizes draw commands
//! into a [`FrameBuffer`].

use gdcore::math::{Color, Rect2, Transform2D};
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
    fn rasterize_item(&self, fb: &mut FrameBuffer, item: &CanvasItem) {
        if !item.visible {
            return;
        }

        for cmd in &item.commands {
            match cmd {
                DrawCommand::DrawRect {
                    rect,
                    color,
                    filled,
                } => {
                    if *filled {
                        // Apply transform to the rect position.
                        let pos = item.transform.xform(rect.position);
                        let transformed = Rect2::new(pos, rect.size);
                        draw::fill_rect(fb, transformed, *color);
                    }
                    // Non-filled rects are not yet implemented (outline).
                }
                DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                } => {
                    let pos = item.transform.xform(*center);
                    draw::fill_circle(fb, pos, *radius, *color);
                }
                DrawCommand::DrawLine {
                    from,
                    to,
                    color,
                    width,
                } => {
                    let f = item.transform.xform(*from);
                    let t = item.transform.xform(*to);
                    draw::draw_line(fb, f, t, *color, *width);
                }
                DrawCommand::DrawTextureRect {
                    texture_path,
                    rect,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        let pos = item.transform.xform(rect.position);
                        let transformed = Rect2::new(pos, rect.size);
                        draw::draw_texture_rect(fb, tex, transformed, *modulate);
                    }
                }
            }
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

        // Use viewport's sorted items for rendering.
        let sorted = viewport.get_sorted_items();
        for item in sorted {
            self.rasterize_item(&mut fb, item);
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
}
