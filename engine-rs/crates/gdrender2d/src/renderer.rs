//! Software renderer implementing [`RenderingServer2D`].
//!
//! Provides a fully CPU-based 2D renderer that rasterizes draw commands
//! into a [`FrameBuffer`].

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::{FrameData, RenderingServer2D};
use gdserver2d::shader::{parse_fragment_body, FragmentInstruction, ShaderMaterial};
use gdserver2d::viewport::Viewport;
use gdvariant::variant::Variant;

use crate::draw;
use crate::font;
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

    /// Alpha-blends a pixel on top of the existing pixel at `(x, y)`.
    ///
    /// Uses standard "source over" compositing: `out = src * src.a + dst * (1 - src.a)`.
    /// No-op if out of bounds.
    pub fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) as usize;
            let dst = self.pixels[idx];
            let a = color.a;
            let inv_a = 1.0 - a;
            self.pixels[idx] = Color::new(
                color.r * a + dst.r * inv_a,
                color.g * a + dst.g * inv_a,
                color.b * a + dst.b * inv_a,
                (a + dst.a * inv_a).min(1.0),
            );
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

    /// Encodes the framebuffer as an uncompressed 32-bit BMP image.
    pub fn to_bmp(&self) -> Vec<u8> {
        crate::export::encode_bmp(self)
    }

    /// Encodes the framebuffer as a PNG image.
    pub fn to_png(&self) -> Vec<u8> {
        crate::export::encode_png(self)
    }

    /// Encodes the framebuffer as a binary PPM (P6) image.
    pub fn to_ppm(&self) -> Vec<u8> {
        crate::export::encode_ppm(self)
    }

    /// Saves the framebuffer as a BMP file.
    pub fn save_bmp(&self, path: &str) -> std::io::Result<()> {
        crate::export::save_bmp(self, path)
    }

    /// Saves the framebuffer as a PNG file.
    pub fn save_png(&self, path: &str) -> std::io::Result<()> {
        crate::export::save_png(self, path)
    }

    /// Saves the framebuffer as a binary PPM file.
    pub fn save_ppm(&self, path: &str) -> std::io::Result<()> {
        crate::export::save_ppm(self, path)
    }
}

/// A blendable raster target that projected-line overlays (the editor's 3D grid,
/// gizmos, and selection outlines) can draw onto.
///
/// Implemented for both the float [`FrameBuffer`] (the software render path) and
/// the byte-native [`Bgra8Image`] (the GPU render path), so the overlay drawing
/// code is written once and composites onto whichever target the caller holds.
/// The GPU path uses `Bgra8Image` to avoid a per-pixel float round-trip
/// (`bytes → Color → bytes`) that dominates the frame at full resolution.
pub trait Raster {
    /// Target width in pixels.
    fn width(&self) -> u32;
    /// Target height in pixels.
    fn height(&self) -> u32;
    /// Alpha-blends `color` over the pixel at (`x`, `y`) with coverage `a`
    /// (clamped to `0..=1`). Out-of-bounds coordinates are a no-op; the result
    /// stays fully opaque, matching the editor overlay compositing.
    fn blend(&mut self, x: i32, y: i32, color: Color, a: f32);
}

impl Raster for FrameBuffer {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn blend(&mut self, x: i32, y: i32, color: Color, a: f32) {
        if a <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = a.min(1.0);
        let idx = (y as u32 * self.width + x as u32) as usize;
        let d = self.pixels[idx];
        self.pixels[idx] = Color {
            r: d.r * (1.0 - a) + color.r * a,
            g: d.g * (1.0 - a) + color.g * a,
            b: d.b * (1.0 - a) + color.b * a,
            a: 1.0,
        };
    }
}

/// A byte-native BGRA raster (top-down, 4 bytes/pixel in `B, G, R, A` order) — the
/// exact format the GPU editor path reads back and the BMP stream wants.
///
/// Compositing overlays and emitting BMP directly on these bytes avoids the two
/// per-pixel float loops (`bytes → Color` on readback and `Color → bytes` on
/// encode) that a [`FrameBuffer`] would require — a large per-frame win at full
/// resolution, where those loops otherwise dominate.
#[derive(Debug, Clone)]
pub struct Bgra8Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major, top-down BGRA bytes; length is `width * height * 4`.
    pub data: Vec<u8>,
}

impl Bgra8Image {
    /// Wraps raw top-down BGRA bytes. `data` must be exactly `width*height*4` long.
    pub fn from_bgra(width: u32, height: u32, data: Vec<u8>) -> Self {
        debug_assert_eq!(
            data.len(),
            (width * height * 4) as usize,
            "BGRA buffer size mismatch"
        );
        Self {
            width,
            height,
            data,
        }
    }
}

impl Raster for Bgra8Image {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn blend(&mut self, x: i32, y: i32, color: Color, a: f32) {
        if a <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = a.min(1.0);
        let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
        // Stored order is B, G, R, A (matching the GPU Bgra8Unorm readback + BMP).
        let sb = color.b.clamp(0.0, 1.0) * 255.0;
        let sg = color.g.clamp(0.0, 1.0) * 255.0;
        let sr = color.r.clamp(0.0, 1.0) * 255.0;
        self.data[idx] = (self.data[idx] as f32 * (1.0 - a) + sb * a) as u8;
        self.data[idx + 1] = (self.data[idx + 1] as f32 * (1.0 - a) + sg * a) as u8;
        self.data[idx + 2] = (self.data[idx + 2] as f32 * (1.0 - a) + sr * a) as u8;
        self.data[idx + 3] = 255;
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
    /// Active 2D lights applied after rasterization (pat-z35r2.1).
    lights: Vec<crate::light2d::Light2D>,
    /// Occluders used for light shadows.
    occluders: Vec<crate::light2d::LightOccluder2D>,
    /// Ambient contribution before lights.
    ambient: crate::light2d::CanvasAmbient,
    /// Item mask for which lights apply in the post-shade pass.
    light_item_mask: u32,
    /// When true, `apply_lights_to_framebuffer` runs each frame.
    lighting_enabled: bool,
    /// Optional outline font. When set, `DrawString` renders real TTF glyphs
    /// (kerned, anti-aliased) instead of the built-in 5×7 bitmap (pat-u3l8c.69).
    ttf_font: Option<crate::ttf_font::TtfFont>,
    /// Glyph atlas cache for TTF rendering (interior-mutable so rasterization
    /// can cache glyphs from the `&self` rasterize pass).
    glyph_atlas: std::cell::RefCell<crate::ttf_font::GlyphAtlas>,
}

impl SoftwareRenderer {
    /// Creates a new software renderer.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            textures: Vec::new(),
            lights: Vec::new(),
            occluders: Vec::new(),
            ambient: crate::light2d::CanvasAmbient::default(),
            light_item_mask: 1,
            lighting_enabled: false,
            ttf_font: None,
            glyph_atlas: std::cell::RefCell::new(crate::ttf_font::GlyphAtlas::new(512, 512)),
        }
    }

    /// Sets the outline font used to render `DrawString` text. Once set, Label /
    /// Button text renders as real TTF glyphs at `font_size × 7` px (matching
    /// the bitmap fallback's visual size).
    pub fn set_font(&mut self, font: crate::ttf_font::TtfFont) {
        self.ttf_font = Some(font);
    }

    /// Whether an outline (TTF) font is active for text rendering.
    pub fn has_ttf_font(&self) -> bool {
        self.ttf_font.is_some()
    }

    /// Loads the engine's built-in default UI font so `DrawString` (Label /
    /// Button text) renders TTF glyphs. Call once at app startup after `new()`.
    pub fn use_default_font(&mut self) {
        self.set_font(crate::ttf_font::TtfFont::builtin());
    }

    /// Replace the light list used by the frame post-shade pass.
    pub fn set_lights(&mut self, lights: Vec<crate::light2d::Light2D>) {
        self.lighting_enabled = !lights.is_empty() || !self.occluders.is_empty();
        self.lights = lights;
        if !self.lights.is_empty() {
            self.lighting_enabled = true;
        }
    }

    /// Replace occluders used for shadow rays.
    pub fn set_occluders(&mut self, occluders: Vec<crate::light2d::LightOccluder2D>) {
        self.occluders = occluders;
        if !self.lights.is_empty() || !self.occluders.is_empty() {
            self.lighting_enabled = true;
        }
    }

    /// Enable/disable the lighting post-pass explicitly.
    pub fn set_lighting_enabled(&mut self, enabled: bool) {
        self.lighting_enabled = enabled;
    }

    /// Returns true when lighting post-pass is active.
    pub fn lighting_enabled(&self) -> bool {
        self.lighting_enabled
    }

    /// Set ambient light for the shade pass.
    pub fn set_ambient(&mut self, ambient: crate::light2d::CanvasAmbient) {
        self.ambient = ambient;
    }

    /// Registers a texture that can be referenced by `DrawTextureRect` commands.
    pub fn register_texture(&mut self, path: &str, texture: Texture2D) {
        self.textures.push((path.to_string(), texture));
    }

    /// Attaches a shader material to a server-owned canvas item (created via
    /// [`RenderingServer2D::create_canvas_item`]) so its axis-aligned textured
    /// draws run the material's `fragment()` shader per-pixel (pat-u3l8c.45).
    pub fn canvas_item_set_material(&mut self, id: CanvasItemId, material: ShaderMaterial) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.material = Some(material);
        }
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
        let axis_aligned = Self::transform_is_axis_aligned(global_transform);

        // pat-u3l8c.45: precompute this item's canvas_item fragment shader (if
        // any) once per item. `uniforms` merges the shader's declared defaults
        // with the material's runtime parameter overrides. Textured draws below
        // run the shader per-pixel via `draw::draw_texture_rect_shaded`.
        let shader_program: Option<(Vec<FragmentInstruction>, HashMap<String, Variant>)> = item
            .material
            .as_ref()
            .and_then(|m| m.shader.as_ref().map(|sh| (m, sh)))
            .map(|(m, sh)| {
                let instructions = parse_fragment_body(&sh.source_code);
                let mut uniforms: HashMap<String, Variant> = HashMap::new();
                for u in &sh.uniforms {
                    uniforms.insert(u.name.clone(), u.default_value.clone());
                }
                for (k, v) in &m.parameters {
                    uniforms.insert(k.clone(), v.clone());
                }
                (instructions, uniforms)
            });

        for cmd in &item.commands {
            match cmd {
                DrawCommand::DrawRect {
                    rect,
                    color,
                    filled,
                } => {
                    if *filled {
                        if axis_aligned {
                            let transformed = Self::transform_rect(global_transform, *rect);
                            draw::fill_rect_blended(fb, transformed, *color);
                        } else {
                            draw::fill_rect_transformed(fb, global_transform, *rect, *color);
                        }
                    } else {
                        let transformed = Self::transform_rect(global_transform, *rect);
                        draw::draw_rect_outline_blended(fb, transformed, *color, 1.0);
                    }
                }
                DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                } => {
                    let pos = global_transform.xform(*center);
                    // Scale radius by the average of the basis scale factors.
                    let sx = global_transform
                        .basis_xform(Vector2::new(1.0, 0.0))
                        .length();
                    let sy = global_transform
                        .basis_xform(Vector2::new(0.0, 1.0))
                        .length();
                    let scaled_radius = *radius * (sx + sy) * 0.5;
                    draw::fill_circle_blended(fb, pos, scaled_radius, *color);
                }
                DrawCommand::DrawLine {
                    from,
                    to,
                    color,
                    width,
                } => {
                    let f = global_transform.xform(*from);
                    let t = global_transform.xform(*to);
                    draw::draw_line_blended(fb, f, t, *color, *width);
                }
                DrawCommand::DrawTextureRect {
                    texture_path,
                    rect,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        if axis_aligned {
                            let transformed = Self::transform_rect(global_transform, *rect);
                            if let Some((instructions, uniforms)) = shader_program.as_ref() {
                                // pat-u3l8c.45: run the item's fragment shader
                                // per-pixel over the sprite's destination rect.
                                draw::draw_texture_rect_shaded(
                                    fb,
                                    tex,
                                    transformed,
                                    *modulate,
                                    instructions,
                                    uniforms,
                                    0.0,
                                );
                            } else {
                                draw::draw_texture_rect_blended(fb, tex, transformed, *modulate);
                            }
                        } else {
                            draw::draw_texture_rect_transformed(
                                fb,
                                tex,
                                global_transform,
                                *rect,
                                None,
                                *modulate,
                            );
                        }
                    }
                }
                DrawCommand::DrawTextureRegion {
                    texture_path,
                    rect,
                    source_rect,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        draw::draw_texture_rect_transformed(
                            fb,
                            tex,
                            global_transform,
                            *rect,
                            Some(*source_rect),
                            *modulate,
                        );
                    }
                }
                DrawCommand::DrawString {
                    text,
                    position,
                    color,
                    font_size,
                } => {
                    let pos = global_transform.xform(*position);
                    if let Some(ttf) = &self.ttf_font {
                        // `font_size` is the bitmap scale; the bitmap cell is 7px
                        // tall, so `scale × 7` gives the matching TTF pixel size.
                        let px = (*font_size as f32) * 7.0;
                        let mut atlas = self.glyph_atlas.borrow_mut();
                        crate::ttf_font::draw_ttf_text(fb, &mut atlas, ttf, text, pos, px, *color);
                    } else {
                        let bitmap_font = font::BitmapFont::builtin();
                        font::draw_string(fb, &bitmap_font, pos, text, *color, *font_size);
                    }
                }
                DrawCommand::DrawNinePatch {
                    texture_path,
                    rect,
                    margin_left,
                    margin_top,
                    margin_right,
                    margin_bottom,
                    draw_center,
                    modulate,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        let transformed = Self::transform_rect(global_transform, *rect);
                        draw::draw_nine_patch(
                            fb,
                            tex,
                            transformed,
                            *margin_left,
                            *margin_top,
                            *margin_right,
                            *margin_bottom,
                            *draw_center,
                            *modulate,
                        );
                    }
                }
            }
        }
    }

    /// Returns `true` when the transform has no rotation or skew (basis is
    /// axis-aligned, possibly scaled/flipped), so rects can use the fast
    /// AABB draw path without distortion (pat-u3l8c.24).
    fn transform_is_axis_aligned(xform: Transform2D) -> bool {
        xform.x.y.abs() < 1e-6 && xform.y.x.abs() < 1e-6
    }

    /// Transforms an axis-aligned rect through a 2D transform, returning the AABB.
    ///
    /// All four corners are transformed, then the axis-aligned bounding box is
    /// computed. Only valid as a draw rect for axis-aligned transforms —
    /// rotated/skewed items must use the `*_transformed` draw paths (see
    /// [`Self::transform_is_axis_aligned`]).
    fn transform_rect(xform: Transform2D, rect: Rect2) -> Rect2 {
        let p0 = xform.xform(rect.position);
        let p1 = xform.xform(Vector2::new(rect.position.x + rect.size.x, rect.position.y));
        let p2 = xform.xform(Vector2::new(rect.position.x, rect.position.y + rect.size.y));
        let p3 = xform.xform(Vector2::new(
            rect.position.x + rect.size.x,
            rect.position.y + rect.size.y,
        ));
        let min_x = p0.x.min(p1.x).min(p2.x).min(p3.x);
        let min_y = p0.y.min(p1.y).min(p2.y).min(p3.y);
        let max_x = p0.x.max(p1.x).max(p2.x).max(p3.x);
        let max_y = p0.y.max(p1.y).max(p2.y).max(p3.y);
        Rect2::new(
            Vector2::new(min_x, min_y),
            Vector2::new(max_x - min_x, max_y - min_y),
        )
    }

    /// Computes the camera transform to apply to draw commands.
    ///
    /// Follows the Godot convention:
    ///   screen = viewport_center + zoom * rotation * (world - camera_position)
    ///
    /// Decomposed as a right-to-left transform chain:
    ///   translate(viewport_center) * scale(zoom) * rotate(angle) * translate(-camera_pos)
    ///
    /// When all camera properties are at defaults, returns identity.
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

        // Step 1: translate world so camera_position is at the origin.
        let to_camera = Transform2D::translated(Vector2::new(
            -viewport.camera_position.x,
            -viewport.camera_position.y,
        ));
        // Step 2: rotate around the origin.
        let rotation = Transform2D::rotated(viewport.camera_rotation);
        // Step 3: apply zoom (scale around the origin).
        let zoom = Transform2D::scaled(viewport.camera_zoom);
        // Step 4: translate so the origin maps to viewport center.
        let to_screen = Transform2D::translated(Vector2::new(half_w, half_h));

        // Right-to-left: first to_camera, then rotation, then zoom, then to_screen.
        to_screen * zoom * rotation * to_camera
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

        // Render server-owned items (created via create_canvas_item).
        // This matches Godot's RenderingServer where items created on the
        // server are rendered alongside viewport items.
        for item in &self.items {
            self.rasterize_item(&mut fb, item, camera_xform);
        }

        // Canvas light post-pass (pat-z35r2.1): Point/Directional lights + occluders.
        if self.lighting_enabled && !self.lights.is_empty() {
            crate::light2d::apply_lights_to_framebuffer(
                &mut fb,
                &self.lights,
                &self.occluders,
                self.ambient,
                self.light_item_mask,
            );
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

    /// A rotated DrawRect must rasterize as the rotated shape, not its AABB
    /// (pat-u3l8c.24 regression).
    #[test]
    fn rotated_rect_renders_rotated_not_aabb() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(64, 64, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        // Thin horizontal bar in local space, centered on the item origin.
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-16.0, -2.0), Vector2::new(32.0, 4.0)),
            color: Color::WHITE,
            filled: true,
        });
        let angle = std::f32::consts::FRAC_PI_4;
        let (s, c) = angle.sin_cos();
        item.transform = Transform2D {
            x: Vector2::new(c, s),
            y: Vector2::new(-s, c),
            origin: Vector2::new(32.0, 32.0),
        };
        vp.add_canvas_item(item);
        let frame = renderer.render_frame(&vp);

        // On the rotated bar's diagonal axis.
        assert_eq!(
            frame.pixels[42 * 64 + 42],
            Color::WHITE,
            "diagonal point should lie on the rotated bar",
        );
        // Inside the bar's AABB but off the rotated bar: stays background.
        assert_eq!(
            frame.pixels[24 * 64 + 40],
            Color::BLACK,
            "point inside the AABB but off the rotated bar must not be filled",
        );
    }

    /// A semi-transparent rect must composite over the background rather
    /// than overwrite it (pat-u3l8c.24 regression).
    #[test]
    fn semi_transparent_rect_blends_over_background() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(8, 8, Color::rgb(1.0, 0.0, 0.0));
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(8.0, 8.0)),
            color: Color::new(0.0, 0.0, 1.0, 0.5),
            filled: true,
        });
        vp.add_canvas_item(item);
        let frame = renderer.render_frame(&vp);

        let px = frame.pixels[4 * 8 + 4];
        assert!(
            px.r > 0.3 && px.b > 0.3,
            "50% blue over red should mix red and blue, got {px:?}",
        );
    }

    #[test]
    fn render_frame_applies_point_light_shade() {
        use crate::light2d::Light2D;
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(32, 32, Color::WHITE);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(32.0, 32.0)),
            color: Color::WHITE,
            filled: true,
        });
        vp.add_canvas_item(item);
        renderer.set_lights(vec![Light2D::point(
            Vector2::new(4.0, 4.0),
            Color::WHITE,
            2.0,
            14.0,
        )]);
        renderer.set_ambient(crate::light2d::CanvasAmbient {
            color: Color::new(0.05, 0.05, 0.05, 1.0),
        });
        let frame = renderer.render_frame(&vp);
        let near = frame.pixels[(4 * 32 + 4) as usize];
        let far = frame.pixels[(30 * 32 + 30) as usize];
        assert!(
            near.r > far.r,
            "SoftwareRenderer light pass: near {} must exceed far {}",
            near.r,
            far.r
        );
        assert!(renderer.lighting_enabled());
    }

    #[test]
    fn use_default_font_enables_ttf() {
        let mut r = SoftwareRenderer::new();
        assert!(!r.has_ttf_font());
        r.use_default_font();
        assert!(r.has_ttf_font());
    }

    #[test]
    fn draw_string_renders_ttf_when_font_set() {
        const FONT: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_assets/Cantarell-Regular.ttf"
        ));

        fn render_label(renderer: &mut SoftwareRenderer) -> FrameData {
            let mut vp = Viewport::new(120, 40, Color::BLACK);
            let mut item = CanvasItem::new(CanvasItemId(1));
            item.commands.push(DrawCommand::DrawString {
                text: "Ag".to_string(),
                position: Vector2::new(4.0, 4.0),
                color: Color::WHITE,
                font_size: 3,
            });
            vp.add_canvas_item(item);
            renderer.render_frame(&vp)
        }

        // Bitmap fallback renders binary (fully on/off) coverage.
        let mut bitmap = SoftwareRenderer::new();
        assert!(!bitmap.has_ttf_font());
        let bmp = render_label(&mut bitmap);
        assert!(bmp.pixels.iter().any(|c| c.r > 0.5), "bitmap text drawn");

        // TTF path renders real, anti-aliased glyph outlines.
        let ttf = crate::ttf_font::TtfFont::from_bytes(FONT.to_vec()).unwrap();
        let mut outline = SoftwareRenderer::new();
        outline.set_font(ttf);
        assert!(outline.has_ttf_font());
        let out = render_label(&mut outline);
        assert!(out.pixels.iter().any(|c| c.r > 0.5), "TTF text drawn");
        // Anti-aliased edges produce partially-covered (gray) pixels that the
        // 1-bit bitmap font never produces.
        let ttf_partial = out
            .pixels
            .iter()
            .filter(|c| c.r > 0.15 && c.r < 0.85)
            .count();
        assert!(ttf_partial > 0, "TTF glyphs should be anti-aliased");
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
    fn nine_patch_rendering_through_pipeline() {
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture("panel.png", Texture2D::solid(6, 6, Color::WHITE));

        let mut vp = Viewport::new(20, 20, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawNinePatch {
            texture_path: "panel.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(20.0, 20.0)),
            margin_left: 2.0,
            margin_top: 2.0,
            margin_right: 2.0,
            margin_bottom: 2.0,
            draw_center: true,
            modulate: Color::rgb(0.0, 1.0, 0.0),
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        // Corner pixel should be green-tinted white
        let pixel = frame.pixels[0];
        assert!((pixel.g - 1.0).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
        // Center pixel too
        let center = frame.pixels[10 * 20 + 10];
        assert!((center.g - 1.0).abs() < 0.01);
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
