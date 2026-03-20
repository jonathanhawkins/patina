//! GPU rendering backend using wgpu (feature-gated behind `"gpu"`).
//!
//! Provides a [`WgpuRenderer`] that implements the [`RenderingServer2D`] trait
//! for GPU-accelerated 2D rendering. Internally it falls back to the software
//! renderer to produce pixel data, simulating what a real wgpu pipeline would
//! output. This lets callers program against the GPU backend API today while
//! the actual GPU pipeline is developed.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::{FrameData, RenderingServer2D};
use gdserver2d::viewport::Viewport;

use crate::draw;
use crate::font;
use crate::renderer::FrameBuffer;
use crate::texture::Texture2D;

/// Configuration for creating a wgpu rendering surface.
#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    /// Width of the render target in pixels.
    pub width: u32,
    /// Height of the render target in pixels.
    pub height: u32,
    /// Whether to enable vertical sync.
    pub vsync: bool,
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            vsync: true,
        }
    }
}

/// A draw command batch submitted to the GPU renderer.
#[derive(Debug, Clone)]
pub struct DrawBatch {
    /// Number of draw commands in this batch.
    pub command_count: u32,
}

/// Simulated GPU device capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Name of the adapter (e.g. "Patina Software Adapter").
    pub adapter_name: String,
    /// Backend API type.
    pub backend: BackendType,
    /// Maximum texture dimension supported.
    pub max_texture_size: u32,
}

/// GPU backend API types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendType {
    /// Vulkan backend.
    Vulkan,
    /// Metal backend (macOS/iOS).
    Metal,
    /// DirectX 12 backend.
    Dx12,
    /// OpenGL ES fallback.
    OpenGl,
    /// Software fallback (no GPU).
    Software,
}

/// A GPU-accelerated 2D renderer backed by wgpu.
///
/// Implements [`RenderingServer2D`] so it can be used as a drop-in replacement
/// for [`SoftwareRenderer`](crate::renderer::SoftwareRenderer). Currently
/// delegates rendering to software rasterization while exposing the GPU API
/// surface (device, surface, batches).
#[derive(Debug)]
pub struct WgpuRenderer {
    /// Whether a surface has been created.
    surface_created: bool,
    /// Current surface configuration.
    config: SurfaceConfig,
    /// Clear color for the render target.
    clear_color: Color,
    /// Accumulated draw batches for the current frame.
    pending_batches: Vec<DrawBatch>,
    /// Simulated device info.
    device_info: Option<DeviceInfo>,
    /// Internal canvas items.
    items: Vec<CanvasItem>,
    /// Next available canvas item ID.
    next_id: u64,
    /// Registered textures for texture draw commands.
    textures: Vec<(String, Texture2D)>,
    /// Total frames rendered.
    frames_rendered: u64,
}

impl WgpuRenderer {
    /// Creates a new wgpu renderer (no GPU initialization yet).
    pub fn new() -> Self {
        Self {
            surface_created: false,
            config: SurfaceConfig::default(),
            clear_color: Color::BLACK,
            pending_batches: Vec::new(),
            device_info: None,
            items: Vec::new(),
            next_id: 1,
            textures: Vec::new(),
            frames_rendered: 0,
        }
    }

    /// Creates a rendering surface with the given configuration.
    ///
    /// Initialises the simulated device/adapter. In a full implementation
    /// this would create a real wgpu device, adapter, and surface.
    pub fn create_surface(&mut self, config: SurfaceConfig) {
        self.config = config;
        self.surface_created = true;
        self.device_info = Some(DeviceInfo {
            adapter_name: "Patina Software Adapter".to_string(),
            backend: BackendType::Software,
            max_texture_size: 8192,
        });
    }

    /// Returns whether a surface has been created.
    pub fn has_surface(&self) -> bool {
        self.surface_created
    }

    /// Returns the current surface configuration.
    pub fn surface_config(&self) -> &SurfaceConfig {
        &self.config
    }

    /// Returns the simulated device info, if a surface has been created.
    pub fn device_info(&self) -> Option<&DeviceInfo> {
        self.device_info.as_ref()
    }

    /// Sets the clear color for the render target.
    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    /// Returns the current clear color.
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    /// Submits a batch of draw commands for GPU rendering.
    pub fn submit_draw_commands(&mut self, batch: DrawBatch) {
        self.pending_batches.push(batch);
    }

    /// Returns the number of pending draw batches.
    pub fn pending_batch_count(&self) -> usize {
        self.pending_batches.len()
    }

    /// Presents the current frame to the surface.
    pub fn present_frame(&mut self) {
        self.pending_batches.clear();
    }

    /// Reconfigures the surface (e.g. on window resize).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
    }

    /// Returns the total number of frames rendered via `render_frame`.
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered
    }

    /// Registers a texture that can be referenced by draw commands.
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
                        let transformed = transform_rect(global_transform, *rect);
                        draw::fill_rect(fb, transformed, *color);
                    }
                }
                DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                } => {
                    let pos = global_transform.xform(*center);
                    let sx = global_transform
                        .basis_xform(Vector2::new(1.0, 0.0))
                        .length();
                    let sy = global_transform
                        .basis_xform(Vector2::new(0.0, 1.0))
                        .length();
                    let scaled_radius = *radius * (sx + sy) * 0.5;
                    draw::fill_circle(fb, pos, scaled_radius, *color);
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
                        let transformed = transform_rect(global_transform, *rect);
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
                        let transformed = transform_rect(global_transform, *rect);
                        draw::draw_texture_region(fb, tex, transformed, *source_rect, *modulate);
                    }
                }
                DrawCommand::DrawString {
                    text,
                    position,
                    color,
                    font_size,
                } => {
                    let pos = global_transform.xform(*position);
                    let bitmap_font = font::BitmapFont::builtin();
                    font::draw_string(fb, &bitmap_font, pos, text, *color, *font_size);
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
                        let transformed = transform_rect(global_transform, *rect);
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

    /// Computes the camera transform for the viewport.
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

        let to_camera = Transform2D::translated(Vector2::new(
            -viewport.camera_position.x,
            -viewport.camera_position.y,
        ));
        let rotation = Transform2D::rotated(viewport.camera_rotation);
        let zoom = Transform2D::scaled(viewport.camera_zoom);
        let to_screen = Transform2D::translated(Vector2::new(half_w, half_h));

        to_screen * zoom * rotation * to_camera
    }
}

/// Transforms an axis-aligned rect through a 2D transform, returning the AABB.
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

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderingServer2D for WgpuRenderer {
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

        // Render layered items.
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

        // Render unlayered items.
        let unlayered = viewport.get_unlayered_items();
        for item in unlayered {
            let parent_xform = self.resolve_parent_transform(item, viewport);
            self.rasterize_item(&mut fb, item, camera_xform * parent_xform);
        }

        self.frames_rendered += 1;

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

    #[test]
    fn create_renderer_default() {
        let renderer = WgpuRenderer::new();
        assert!(!renderer.has_surface());
        assert_eq!(renderer.pending_batch_count(), 0);
        assert_eq!(renderer.clear_color(), Color::BLACK);
        assert!(renderer.device_info().is_none());
    }

    #[test]
    fn create_surface_and_check() {
        let mut renderer = WgpuRenderer::new();
        let config = SurfaceConfig {
            width: 800,
            height: 600,
            vsync: false,
        };
        renderer.create_surface(config);
        assert!(renderer.has_surface());
        assert_eq!(renderer.surface_config().width, 800);
        assert_eq!(renderer.surface_config().height, 600);
        assert!(!renderer.surface_config().vsync);
    }

    #[test]
    fn create_surface_initializes_device() {
        let mut renderer = WgpuRenderer::new();
        renderer.create_surface(SurfaceConfig::default());
        let info = renderer
            .device_info()
            .expect("device info should exist after create_surface");
        assert_eq!(info.backend, BackendType::Software);
        assert_eq!(info.max_texture_size, 8192);
        assert!(!info.adapter_name.is_empty());
    }

    #[test]
    fn submit_draw_commands_tracks_batches() {
        let mut renderer = WgpuRenderer::new();
        renderer.create_surface(SurfaceConfig::default());

        renderer.submit_draw_commands(DrawBatch { command_count: 10 });
        renderer.submit_draw_commands(DrawBatch { command_count: 5 });
        assert_eq!(renderer.pending_batch_count(), 2);
    }

    #[test]
    fn present_frame_clears_batches() {
        let mut renderer = WgpuRenderer::new();
        renderer.create_surface(SurfaceConfig::default());

        renderer.submit_draw_commands(DrawBatch { command_count: 3 });
        assert_eq!(renderer.pending_batch_count(), 1);

        renderer.present_frame();
        assert_eq!(renderer.pending_batch_count(), 0);
    }

    #[test]
    fn set_clear_color() {
        let mut renderer = WgpuRenderer::new();
        let red = Color::rgb(1.0, 0.0, 0.0);
        renderer.set_clear_color(red);
        assert_eq!(renderer.clear_color(), red);
    }

    #[test]
    fn resize_updates_config() {
        let mut renderer = WgpuRenderer::new();
        renderer.create_surface(SurfaceConfig::default());
        renderer.resize(1920, 1080);
        assert_eq!(renderer.surface_config().width, 1920);
        assert_eq!(renderer.surface_config().height, 1080);
    }

    #[test]
    fn default_surface_config() {
        let config = SurfaceConfig::default();
        assert_eq!(config.width, 1280);
        assert_eq!(config.height, 720);
        assert!(config.vsync);
    }

    #[test]
    fn renderer_default_trait() {
        let renderer = WgpuRenderer::default();
        assert!(!renderer.has_surface());
    }

    #[test]
    fn backend_type_variants() {
        let types = [
            BackendType::Vulkan,
            BackendType::Metal,
            BackendType::Dx12,
            BackendType::OpenGl,
            BackendType::Software,
        ];
        for (i, a) in types.iter().enumerate() {
            for (j, b) in types.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn device_info_equality() {
        let a = DeviceInfo {
            adapter_name: "Test".to_string(),
            backend: BackendType::Vulkan,
            max_texture_size: 4096,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // -- RenderingServer2D trait tests --

    #[test]
    fn render_frame_produces_correct_dimensions() {
        let mut renderer = WgpuRenderer::new();
        let vp = Viewport::new(32, 24, Color::BLACK);
        let frame = renderer.render_frame(&vp);
        assert_eq!(frame.width, 32);
        assert_eq!(frame.height, 24);
        assert_eq!(frame.pixels.len(), 32 * 24);
    }

    #[test]
    fn render_frame_clear_color() {
        let mut renderer = WgpuRenderer::new();
        let blue = Color::rgb(0.0, 0.0, 1.0);
        let vp = Viewport::new(4, 4, blue);
        let frame = renderer.render_frame(&vp);
        for pixel in &frame.pixels {
            assert_eq!(*pixel, blue);
        }
    }

    #[test]
    fn render_frame_with_rect() {
        let mut renderer = WgpuRenderer::new();
        let mut vp = Viewport::new(10, 10, Color::BLACK);

        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        assert_eq!(frame.pixels[0], Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(frame.pixels[9 * 10 + 9], Color::BLACK);
    }

    #[test]
    fn create_and_free_canvas_items() {
        let mut renderer = WgpuRenderer::new();
        let id1 = renderer.create_canvas_item();
        let id2 = renderer.create_canvas_item();
        assert_ne!(id1, id2);

        renderer.canvas_item_add_draw_command(
            id1,
            DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
                color: Color::WHITE,
                filled: true,
            },
        );
        renderer.free_canvas_item(id1);
        // After freeing, adding to id1 should be a no-op.
        renderer.canvas_item_add_draw_command(
            id1,
            DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::ZERO, Vector2::new(5.0, 5.0)),
                color: Color::WHITE,
                filled: true,
            },
        );
    }

    #[test]
    fn canvas_item_set_transform_and_visible() {
        let mut renderer = WgpuRenderer::new();
        let id = renderer.create_canvas_item();
        renderer.canvas_item_set_transform(id, Transform2D::translated(Vector2::new(5.0, 5.0)));
        renderer.canvas_item_set_z_index(id, 10);
        renderer.canvas_item_set_visible(id, false);
        // Just verify no panics.
    }

    #[test]
    fn frames_rendered_counter() {
        let mut renderer = WgpuRenderer::new();
        assert_eq!(renderer.frames_rendered(), 0);
        let vp = Viewport::new(4, 4, Color::BLACK);
        renderer.render_frame(&vp);
        assert_eq!(renderer.frames_rendered(), 1);
        renderer.render_frame(&vp);
        assert_eq!(renderer.frames_rendered(), 2);
    }

    #[test]
    fn wgpu_renderer_texture_rendering() {
        let mut renderer = WgpuRenderer::new();
        renderer.register_texture("test.png", Texture2D::solid(2, 2, Color::WHITE));

        let mut vp = Viewport::new(10, 10, Color::BLACK);
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawTextureRect {
            texture_path: "test.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            modulate: Color::rgb(0.0, 1.0, 0.0),
        });
        vp.add_canvas_item(item);

        let frame = renderer.render_frame(&vp);
        let pixel = frame.pixels[0];
        assert!((pixel.g - 1.0).abs() < 0.01);
        assert!(pixel.r.abs() < 0.01);
    }

    #[test]
    fn wgpu_renderer_invisible_items_skipped() {
        let mut renderer = WgpuRenderer::new();
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
        assert_eq!(frame.pixels[0], Color::BLACK);
    }

    #[test]
    fn wgpu_software_parity_simple_scene() {
        // Verify that WgpuRenderer and SoftwareRenderer produce identical output.
        let make_viewport = || {
            let mut vp = Viewport::new(20, 20, Color::BLACK);
            let mut item = CanvasItem::new(CanvasItemId(1));
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(6.0, 6.0)),
                color: Color::rgb(1.0, 0.0, 0.0),
                filled: true,
            });
            item.commands.push(DrawCommand::DrawCircle {
                center: Vector2::new(15.0, 15.0),
                radius: 3.0,
                color: Color::rgb(0.0, 0.0, 1.0),
            });
            vp.add_canvas_item(item);
            vp
        };

        let mut wgpu = WgpuRenderer::new();
        let mut sw = crate::renderer::SoftwareRenderer::new();

        let vp1 = make_viewport();
        let vp2 = make_viewport();

        let frame_wgpu = wgpu.render_frame(&vp1);
        let frame_sw = sw.render_frame(&vp2);

        assert_eq!(frame_wgpu.pixels, frame_sw.pixels);
    }
}
