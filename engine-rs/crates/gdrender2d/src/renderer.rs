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

use crate::canvas_material::{self, BlendMode, TextureFilter, TextureRepeat};
use crate::draw;
use crate::font;
use crate::polygon_raster;
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
    /// Optional scissor rectangle in framebuffer pixel space (pat-0q2kx.86).
    ///
    /// While set, every write through [`set_pixel`](Self::set_pixel),
    /// [`blend_pixel`](Self::blend_pixel) and the [`Raster`] impl is discarded
    /// outside the rect. The renderer sets it per canvas item from that item's
    /// [`clip_rect`](gdserver2d::canvas::CanvasItem::clip_rect), which is what
    /// makes `Control.clip_contents` (and therefore ScrollContainer) cut
    /// content off instead of letting it overflow.
    ///
    /// Clipping lives here rather than in each `draw::` routine because these
    /// three writes are the only paths to a pixel — one check covers rects,
    /// circles, lines, textures, nine-patches and text alike.
    pub scissor: Option<Rect2>,
    /// Blend equation for every write through [`blend_pixel`](Self::blend_pixel)
    /// and the [`Raster`] impl (pat-0q2kx.131).
    ///
    /// Set per canvas item from that item's `CanvasItemMaterial`, for the same
    /// reason `scissor` lives here: these writes are the only paths to a pixel,
    /// so one dispatch point covers rects, circles, lines, textures,
    /// nine-patches, polygons and text alike. Threading a mode parameter
    /// through every `draw::` routine instead would touch dozens of signatures
    /// and still miss whichever one was added next.
    pub blend_mode: BlendMode,
    /// Resolved texture sampling filter for the item currently being drawn
    /// (pat-0q2kx.131). Carried here, like `blend_mode` and `scissor`, so the
    /// `draw::` texture routines pick it up without seven signature changes.
    pub texture_filter: TextureFilter,
    /// Resolved UV wrap mode for the item currently being drawn.
    pub texture_repeat: TextureRepeat,
}

impl FrameBuffer {
    /// Creates a new framebuffer filled with `clear_color`.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![clear_color; (width * height) as usize],
            scissor: None,
            blend_mode: BlendMode::Mix,
            texture_filter: TextureFilter::Nearest,
            texture_repeat: TextureRepeat::Disabled,
        }
    }

    /// Wraps existing pixel data (e.g. a decoded golden PNG or a rendered
    /// frame) with the same default drawing state as [`new`](Self::new).
    ///
    /// Prefer this over a struct literal: the drawing-state fields keep
    /// growing, and every literal breaks when they do.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<Color>) -> Self {
        debug_assert_eq!(pixels.len(), (width * height) as usize);
        Self {
            width,
            height,
            pixels,
            scissor: None,
            blend_mode: BlendMode::Mix,
            texture_filter: TextureFilter::Nearest,
            texture_repeat: TextureRepeat::Disabled,
        }
    }

    /// Clears the entire framebuffer to the given color.
    ///
    /// Ignores the scissor — clearing is a whole-frame operation, as it is in
    /// Godot where the clear happens before any item is drawn.
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    /// Whether `(x, y)` survives the current scissor. Always `true` when no
    /// scissor is set.
    ///
    /// The test is half-open on the far edges, matching `Rect2::has_point`, so
    /// a clip rect of exactly the control's size keeps the pixel column at
    /// `position.x + size.x` outside.
    fn in_scissor(&self, x: u32, y: u32) -> bool {
        match self.scissor {
            None => true,
            Some(r) => {
                let (px, py) = (x as f32, y as f32);
                px >= r.position.x
                    && py >= r.position.y
                    && px < r.position.x + r.size.x
                    && py < r.position.y + r.size.y
            }
        }
    }

    /// Sets a pixel at `(x, y)`. No-op if out of bounds or scissored away.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height && self.in_scissor(x, y) {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    /// Alpha-blends a pixel on top of the existing pixel at `(x, y)`.
    ///
    /// Uses standard "source over" compositing: `out = src * src.a + dst * (1 - src.a)`.
    /// No-op if out of bounds or scissored away.
    pub fn blend_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height && self.in_scissor(x, y) {
            let idx = (y * self.width + x) as usize;
            let dst = self.pixels[idx];
            // `Mix` here is exactly the source-over this used to inline; any
            // other mode is the item's CanvasItemMaterial talking.
            self.pixels[idx] = canvas_material::blend(color, dst, self.blend_mode);
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
        // The anti-aliased path (text, thick lines) reaches pixels through here
        // rather than set_pixel/blend_pixel, so it needs the same scissor test —
        // otherwise clipped text would still show its edges (pat-0q2kx.86).
        if !self.in_scissor(x as u32, y as u32) {
            return;
        }
        let a = a.min(1.0);
        let idx = (y as u32 * self.width + x as u32) as usize;
        let d = self.pixels[idx];
        // Coverage multiplies into the source alpha, then the item's blend mode
        // decides the equation — so an anti-aliased edge on an ADD-blended item
        // adds partially rather than mixing.
        let src = Color::new(color.r, color.g, color.b, color.a * a);
        let out = canvas_material::blend(src, d, self.blend_mode);
        self.pixels[idx] = Color { a: 1.0, ..out };
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

        // pat-0q2kx.86: honor the item's clip rect for the whole of its command
        // list. The rect is stored in the item's local space, so it goes through
        // the same transform as the commands it bounds. Items are rasterized
        // flat (see `render_frame`), so the previous scissor is restored on the
        // way out rather than relying on a draw-order stack.
        // pat-0q2kx.131: the item's CanvasItemMaterial blend mode applies to
        // every command it emits, and is restored on the way out for the same
        // reason the scissor is — items are rasterized flat, so there is no
        // draw-order stack to unwind.
        let prev_blend = fb.blend_mode;
        fb.blend_mode = item
            .canvas_material
            .map(|m| m.blend_mode)
            .unwrap_or(BlendMode::Mix);
        let prev_filter = fb.texture_filter;
        let prev_repeat = fb.texture_repeat;
        // The bridge already resolved PARENT_NODE up the ancestor chain, so
        // these are concrete sampling modes by the time they arrive.
        fb.texture_filter = item.texture_filter;
        fb.texture_repeat = item.texture_repeat;

        let prev_scissor = fb.scissor;
        if let Some(clip) = item.clip_rect {
            let clip = Self::transform_rect(global_transform, clip);
            fb.scissor = Some(match prev_scissor {
                Some(outer) => Self::rect_intersection(outer, clip),
                None => clip,
            });
        }

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
                // The stroke/fill family (pat-0q2kx.129). `polygon_raster` had
                // the scanline fill and the capped/jointed stroke written and
                // unit-tested but no caller anywhere in the workspace; these
                // arms are what connect it to the frame. Points are transformed
                // into pixel space first, so the item's rotation and scale
                // apply to the geometry itself rather than to a bounding box.
                DrawCommand::DrawPolyline {
                    points,
                    color,
                    width,
                    closed,
                } => {
                    let pts = Self::transform_points(global_transform, points);
                    polygon_raster::draw_polyline(
                        fb,
                        &pts,
                        Self::scaled_width(global_transform, *width),
                        *color,
                        *closed,
                        polygon_raster::LineCap::None,
                        // Round joints: a sharp joint leaves a wedge missing on
                        // every corner, which is exactly what makes a chain of
                        // `draw_line` calls look broken.
                        polygon_raster::LineJoint::Round,
                    );
                }
                DrawCommand::DrawMultiline {
                    points,
                    color,
                    width,
                } => {
                    let w = Self::scaled_width(global_transform, *width);
                    for pair in points.chunks_exact(2) {
                        let seg = [
                            global_transform.xform(pair[0]),
                            global_transform.xform(pair[1]),
                        ];
                        polygon_raster::draw_polyline(
                            fb,
                            &seg,
                            w,
                            *color,
                            false,
                            polygon_raster::LineCap::None,
                            polygon_raster::LineJoint::Sharp,
                        );
                    }
                }
                DrawCommand::DrawDashedLine {
                    from,
                    to,
                    color,
                    width,
                    dash,
                } => {
                    let f = global_transform.xform(*from);
                    let t = global_transform.xform(*to);
                    let span = t - f;
                    let len = span.length();
                    let step = Self::scaled_width(global_transform, *dash).max(0.1);
                    if len > 1e-6 {
                        let dir = span * (1.0 / len);
                        let w = Self::scaled_width(global_transform, *width);
                        // Walk the line laying down `dash` on, `dash` off.
                        let mut travelled = 0.0_f32;
                        while travelled < len {
                            let end = (travelled + step).min(len);
                            let seg = [f + dir * travelled, f + dir * end];
                            polygon_raster::draw_polyline(
                                fb,
                                &seg,
                                w,
                                *color,
                                false,
                                polygon_raster::LineCap::None,
                                polygon_raster::LineJoint::Sharp,
                            );
                            travelled = end + step;
                        }
                    }
                }
                DrawCommand::DrawArc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    point_count,
                    color,
                    width,
                } => {
                    let steps = (*point_count).max(2);
                    let sweep = *end_angle - *start_angle;
                    let pts: Vec<Vector2> = (0..steps)
                        .map(|i| {
                            let t = i as f32 / (steps - 1) as f32;
                            let a = *start_angle + sweep * t;
                            global_transform.xform(Vector2::new(
                                center.x + radius * a.cos(),
                                center.y + radius * a.sin(),
                            ))
                        })
                        .collect();
                    polygon_raster::draw_polyline(
                        fb,
                        &pts,
                        Self::scaled_width(global_transform, *width),
                        *color,
                        false,
                        polygon_raster::LineCap::None,
                        polygon_raster::LineJoint::Round,
                    );
                }
                DrawCommand::DrawPolygon { points, colors } => {
                    let pts = Self::transform_points(global_transform, points);
                    // Godot accepts one color for the whole polygon or one per
                    // vertex; the software rasterizer has no per-vertex
                    // interpolation, so the first color wins.
                    let color = colors.first().copied().unwrap_or(Color::WHITE);
                    polygon_raster::fill_polygon(fb, &pts, color);
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
                        // pat-s3rrv.37: `font_size` is now the Godot PIXEL size,
                        // used directly. It used to carry the quantised 5x7
                        // bitmap scale, which this reconstructed as `scale * 7`
                        // — so a Label authored at 16 and one at 20 both drew at
                        // 14px, and the size layout measured never matched the
                        // size actually drawn.
                        let px = *font_size as f32;
                        let mut atlas = self.glyph_atlas.borrow_mut();
                        crate::ttf_font::draw_ttf_text(fb, &mut atlas, ttf, text, pos, px, *color);
                    } else {
                        // The bitmap fallback still wants an integer cell scale,
                        // so the pixel size is converted back here rather than at
                        // the source (7px cell, minimum 1).
                        let bitmap_font = font::BitmapFont::builtin();
                        let cell_scale = (*font_size / 7).max(1);
                        font::draw_string(fb, &bitmap_font, pos, text, *color, cell_scale);
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
                    axis_h,
                    axis_v,
                    modulate,
                    region,
                } => {
                    if let Some(tex) = self.find_texture(texture_path) {
                        // `region_rect` confines sampling to a sub-rect of the
                        // source (pat-s3rrv.167). A zero-size region yields
                        // `None`, and the whole texture is used byte-for-byte as
                        // before this field existed.
                        let cropped = draw::crop_to_region(tex, *region);
                        let tex = cropped.as_ref().unwrap_or(tex);
                        let transformed = Self::transform_rect(global_transform, *rect);
                        draw::draw_nine_patch_axes(
                            fb,
                            tex,
                            transformed,
                            *margin_left,
                            *margin_top,
                            *margin_right,
                            *margin_bottom,
                            *draw_center,
                            *axis_h,
                            *axis_v,
                            *modulate,
                        );
                    }
                }
            }
        }

        fb.scissor = prev_scissor;
        fb.blend_mode = prev_blend;
        fb.texture_filter = prev_filter;
        fb.texture_repeat = prev_repeat;
    }

    /// The overlap of two rects, or an empty rect at `a`'s origin when they do
    /// not overlap — a nested clip can only ever shrink the visible area, never
    /// reveal what an ancestor already cut off (pat-0q2kx.86).
    fn rect_intersection(a: Rect2, b: Rect2) -> Rect2 {
        let x0 = a.position.x.max(b.position.x);
        let y0 = a.position.y.max(b.position.y);
        let x1 = (a.position.x + a.size.x).min(b.position.x + b.size.x);
        let y1 = (a.position.y + a.size.y).min(b.position.y + b.size.y);
        Rect2::new(
            Vector2::new(x0, y0),
            Vector2::new((x1 - x0).max(0.0), (y1 - y0).max(0.0)),
        )
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
    /// Transforms a run of local-space points into pixel space.
    fn transform_points(xform: Transform2D, points: &[Vector2]) -> Vec<Vector2> {
        points.iter().map(|p| xform.xform(*p)).collect()
    }

    /// Scales a stroke width by the item's transform, using the mean of the
    /// two basis scale factors — the same convention `DrawCircle` uses for its
    /// radius, so a scaled item's strokes and circles stay consistent.
    fn scaled_width(xform: Transform2D, width: f32) -> f32 {
        let sx = xform.basis_xform(Vector2::new(1.0, 0.0)).length();
        let sy = xform.basis_xform(Vector2::new(0.0, 1.0)).length();
        // Godot treats a non-positive width as "thinnest visible line".
        width.max(1.0) * (sx + sy) * 0.5
    }

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
            axis_h: gdserver2d::canvas::NinePatchAxis::Stretch,
            axis_v: gdserver2d::canvas::NinePatchAxis::Stretch,
            modulate: Color::rgb(0.0, 1.0, 0.0),
            region: gdcore::math::Rect2::new(
                gdcore::math::Vector2::ZERO,
                gdcore::math::Vector2::ZERO,
            ),
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

    // =======================================================================
    // Scissor / clip_rect (pat-0q2kx.86)
    //
    // `Control.clip_contents` — and therefore ScrollContainer — reaches pixels
    // through exactly one mechanism: `CanvasItem::clip_rect` becomes
    // `FrameBuffer::scissor` for the span of that item's command list. The
    // ScrollContainer end-to-end assertions live in gdscene
    // (`control_render_test::scroll_container_clips_and_scrolls`), which proves
    // the whole chain but cannot say *where* a regression is. These pin the
    // rasterizer's half of it directly.
    // =======================================================================

    /// The scissor gates the two primitive writes every `draw::` routine funnels
    /// through, so one check covers rects, circles, lines, textures and text.
    #[test]
    fn scissor_discards_writes_outside_the_rect() {
        let mut fb = FrameBuffer::new(8, 8, Color::BLACK);
        fb.scissor = Some(Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(4.0, 4.0)));

        fb.set_pixel(3, 3, Color::WHITE);
        fb.set_pixel(0, 0, Color::WHITE);
        fb.blend_pixel(4, 4, Color::WHITE);
        fb.blend_pixel(7, 7, Color::WHITE);

        assert_eq!(
            fb.get_pixel(3, 3),
            Color::WHITE,
            "a set_pixel inside the scissor must land"
        );
        // Asserted as "no longer the background" rather than an exact colour:
        // what is under test is the clip, not the blend equation.
        assert_ne!(
            fb.get_pixel(4, 4),
            Color::BLACK,
            "a blend_pixel inside the scissor must land"
        );
        assert_eq!(
            fb.get_pixel(0, 0),
            Color::BLACK,
            "set_pixel outside the scissor must be discarded, not clamped"
        );
        assert_eq!(
            fb.get_pixel(7, 7),
            Color::BLACK,
            "blend_pixel outside the scissor must be discarded too — blending is \
             the path alpha content takes, and skipping it there would let a \
             translucent overflow through"
        );
    }

    /// Half-open on the far edges, matching `Rect2::has_point`. This is what
    /// makes a clip rect of exactly the control's size keep the column at
    /// `position.x + size.x` outside instead of painting one pixel of overflow.
    #[test]
    fn scissor_edges_are_half_open() {
        let mut fb = FrameBuffer::new(8, 8, Color::BLACK);
        fb.scissor = Some(Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(4.0, 4.0)));

        fb.set_pixel(2, 2, Color::WHITE); // near edge: inside
        fb.set_pixel(5, 5, Color::WHITE); // last pixel inside
        fb.set_pixel(6, 6, Color::WHITE); // far edge: outside
        fb.set_pixel(1, 1, Color::WHITE); // just before the near edge

        assert_eq!(fb.get_pixel(2, 2), Color::WHITE, "the near edge is inside");
        assert_eq!(
            fb.get_pixel(5, 5),
            Color::WHITE,
            "position + size - 1 is the last pixel inside"
        );
        assert_eq!(
            fb.get_pixel(6, 6),
            Color::BLACK,
            "position + size is outside — half-open, or every clipped control \
             would bleed a pixel"
        );
        assert_eq!(fb.get_pixel(1, 1), Color::BLACK, "before the near edge");
    }

    /// No scissor is every item's default, and must not clip anything.
    #[test]
    fn no_scissor_writes_everywhere() {
        let mut fb = FrameBuffer::new(4, 4, Color::BLACK);
        assert!(fb.scissor.is_none(), "unclipped is the default");
        fb.set_pixel(0, 0, Color::WHITE);
        fb.set_pixel(3, 3, Color::WHITE);
        assert_eq!(fb.get_pixel(0, 0), Color::WHITE);
        assert_eq!(fb.get_pixel(3, 3), Color::WHITE);
    }

    /// End-to-end through `render_frame`: an item whose command overflows its
    /// `clip_rect` is cut off at the rect. This is the ScrollContainer case with
    /// the scene layer removed — an oversized child inside a smaller viewport.
    #[test]
    fn canvas_item_clip_rect_cuts_off_an_oversized_command() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(16, 16, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        // A 16x16 fill inside an 8x8 clip: the bottom-right quadrant survives
        // nothing, the top-left quadrant survives whole.
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
            color: Color::WHITE,
            filled: true,
        });
        item.set_clip_rect(Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)));
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        assert_eq!(
            frame.pixels[2 * 16 + 2],
            Color::WHITE,
            "inside the clip rect the command draws normally"
        );
        assert_eq!(
            frame.pixels[12 * 16 + 12],
            Color::BLACK,
            "past the clip rect the same command must not reach the framebuffer"
        );
    }

    /// The clip rect is in the item's LOCAL space, so it rides the item's
    /// transform along with the commands it bounds. A clip that was applied in
    /// framebuffer space would cut the wrong region for any translated item —
    /// which is every scrolled control, since scrolling *is* a translation.
    #[test]
    fn clip_rect_follows_the_items_transform() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(16, 16, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
            color: Color::WHITE,
            filled: true,
        });
        // Same local 8x8 clip as above, but the item is shifted 8px right/down,
        // so the surviving band is the BOTTOM-RIGHT quadrant this time.
        item.set_clip_rect(Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)));
        item.transform = Transform2D::translated(Vector2::new(8.0, 8.0));
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        assert_eq!(
            frame.pixels[12 * 16 + 12],
            Color::WHITE,
            "the clip moved with the item, so this quadrant is now inside it"
        );
        assert_eq!(
            frame.pixels[2 * 16 + 2],
            Color::BLACK,
            "and the quadrant the untransformed clip covered is now outside — \
             proving the rect is transformed, not applied in screen space"
        );
    }

    /// Items rasterize flat (no draw-order stack), so the scissor has to be
    /// restored when an item's command list ends. Without that, the first
    /// clipped control in a scene would silently clip everything drawn after it.
    #[test]
    fn clip_rect_does_not_leak_into_the_next_item() {
        let mut renderer = SoftwareRenderer::new();
        let mut vp = Viewport::new(16, 16, Color::BLACK);

        let mut clipped = CanvasItem::new(CanvasItemId(1));
        clipped.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        clipped.set_clip_rect(Rect2::new(Vector2::ZERO, Vector2::new(8.0, 8.0)));
        clipped.z_index = 0;
        vp.add_canvas_item(clipped);

        // Drawn after, unclipped, and covering the region the first item's clip
        // excluded.
        let mut unclipped = CanvasItem::new(CanvasItemId(2));
        unclipped.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(8.0, 8.0), Vector2::new(8.0, 8.0)),
            color: Color::rgb(0.0, 1.0, 0.0),
            filled: true,
        });
        unclipped.z_index = 1;
        vp.add_canvas_item(unclipped);

        let frame = renderer.render_frame(&vp);
        assert_eq!(
            frame.pixels[12 * 16 + 12],
            Color::rgb(0.0, 1.0, 0.0),
            "the second item is unclipped and must paint in full — a leaked \
             scissor would have discarded this pixel"
        );
        assert_eq!(
            frame.pixels[2 * 16 + 2],
            Color::rgb(1.0, 0.0, 0.0),
            "and the first item still drew inside its own clip"
        );
    }
}
