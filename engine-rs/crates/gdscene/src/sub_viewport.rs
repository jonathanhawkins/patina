//! SubViewport node + ViewportTexture sampling path (pat-zx775).
//!
//! A [`SubViewport`] is a [`Node`](crate::node::Node) subclass that owns an
//! off-screen software color buffer (render target). A [`ViewportTexture`]
//! samples that buffer so UI / materials can display the rendered contents.
//!
//! This module provides the minimal runtime slice matching Godot 4 defaults:
//! * size default `512×512`
//! * `UpdateMode::WhenVisible` (2)
//! * `ClearMode::Always` (0)
//! * `transparent_bg` default `false`
//!
//! The software fill path rasterizes simple child content (solid ColorRect /
//! filled rect) into the SubViewport buffer so tests can assert nonzero pixels.

use gdcore::math::{Color, Vector2, Vector2i};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Enums (Godot SubViewport constants)
// ===========================================================================

/// SubViewport render-target update mode (`SubViewport.UpdateMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i64)]
pub enum UpdateMode {
    /// Do not update the render target.
    Disabled = 0,
    /// Update once, then switch to disabled.
    Once = 1,
    /// Update when the SubViewport is visible (Godot default).
    #[default]
    WhenVisible = 2,
    /// Update when the parent is visible.
    WhenParentVisible = 3,
    /// Always update every frame.
    Always = 4,
}

impl UpdateMode {
    /// Converts from the integer representation stored in Variant.
    pub fn from_i64(v: i64) -> Self {
        match v {
            0 => Self::Disabled,
            1 => Self::Once,
            3 => Self::WhenParentVisible,
            4 => Self::Always,
            _ => Self::WhenVisible,
        }
    }

    /// Returns the Godot integer representation.
    pub fn to_i64(self) -> i64 {
        self as i64
    }
}

/// SubViewport clear mode (`SubViewport.ClearMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i64)]
pub enum ClearMode {
    /// Clear the render target every frame (Godot default).
    #[default]
    Always = 0,
    /// Never clear automatically.
    Never = 1,
    /// Clear once, then switch to never.
    Once = 2,
}

impl ClearMode {
    /// Converts from the integer representation stored in Variant.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Never,
            2 => Self::Once,
            _ => Self::Always,
        }
    }

    /// Returns the Godot integer representation.
    pub fn to_i64(self) -> i64 {
        self as i64
    }
}

// ===========================================================================
// Color buffer (software render target)
// ===========================================================================

/// Software RGBA color buffer owned by a SubViewport.
#[derive(Debug, Clone)]
pub struct ColorBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major pixel data.
    pub pixels: Vec<Color>,
}

impl ColorBuffer {
    /// Allocates a buffer filled with `clear_color`.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            width: w,
            height: h,
            pixels: vec![clear_color; (w * h) as usize],
        }
    }

    /// Fills the entire buffer with `color`.
    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    /// Sets a pixel; no-op if out of bounds.
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

    /// Fills an axis-aligned rect (inclusive min, exclusive max) with `color`.
    pub fn fill_rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: Color) {
        let x0 = x0.min(self.width);
        let y0 = y0.min(self.height);
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// Returns `true` if any pixel has a nonzero **RGB** component.
    ///
    /// Alpha alone does not count — opaque black (`Color::BLACK`) is treated
    /// as empty clear so rendered ColorRect / mesh content is distinguishable.
    pub fn has_nonzero_pixels(&self) -> bool {
        self.pixels
            .iter()
            .any(|c| c.r > 0.0 || c.g > 0.0 || c.b > 0.0)
    }

    /// Counts pixels where any RGB channel is above `eps`.
    pub fn count_nonzero_pixels(&self, eps: f32) -> usize {
        self.pixels
            .iter()
            .filter(|c| c.r > eps || c.g > eps || c.b > eps)
            .count()
    }
}

// ===========================================================================
// SubViewport runtime state
// ===========================================================================

/// Runtime state for a SubViewport node: properties + owned color buffer.
#[derive(Debug, Clone)]
pub struct SubViewport {
    /// Pixel size of the render target (Godot default 512×512).
    pub size: Vector2i,
    /// When the target is redrawn.
    pub update_mode: UpdateMode,
    /// When the target is cleared.
    pub clear_mode: ClearMode,
    /// Whether the clear color is transparent.
    pub transparent_bg: bool,
    /// Optional clear color override (defaults from `transparent_bg`).
    pub clear_color: Color,
    /// Software framebuffer.
    buffer: ColorBuffer,
    /// Scene-tree node this state is attached to (if any).
    pub node_id: Option<NodeId>,
}

impl Default for SubViewport {
    fn default() -> Self {
        Self::new(Vector2i::new(512, 512))
    }
}

impl SubViewport {
    /// Creates a SubViewport with Godot-default modes and the given size.
    pub fn new(size: Vector2i) -> Self {
        let w = size.x.max(1) as u32;
        let h = size.y.max(1) as u32;
        let clear = Color::BLACK;
        Self {
            size: Vector2i::new(w as i32, h as i32),
            update_mode: UpdateMode::WhenVisible,
            clear_mode: ClearMode::Always,
            transparent_bg: false,
            clear_color: clear,
            buffer: ColorBuffer::new(w, h, clear),
            node_id: None,
        }
    }

    /// Creates a SubViewport with Godot defaults (512×512, WHEN_VISIBLE, ALWAYS clear).
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Returns the owned color buffer.
    pub fn buffer(&self) -> &ColorBuffer {
        &self.buffer
    }

    /// Returns a mutable reference to the color buffer.
    pub fn buffer_mut(&mut self) -> &mut ColorBuffer {
        &mut self.buffer
    }

    /// Resizes the render target, reallocating the color buffer.
    pub fn set_size(&mut self, size: Vector2i) {
        let w = size.x.max(1) as u32;
        let h = size.y.max(1) as u32;
        self.size = Vector2i::new(w as i32, h as i32);
        let clear = self.resolve_clear_color();
        self.buffer = ColorBuffer::new(w, h, clear);
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.buffer.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.buffer.height
    }

    /// Resolves the clear color from `transparent_bg` / override.
    pub fn resolve_clear_color(&self) -> Color {
        if self.transparent_bg {
            Color::TRANSPARENT
        } else {
            self.clear_color
        }
    }

    /// Clears the buffer according to [`ClearMode`].
    ///
    /// Returns `true` if a clear was performed.
    pub fn apply_clear(&mut self) -> bool {
        match self.clear_mode {
            ClearMode::Always => {
                let c = self.resolve_clear_color();
                self.buffer.clear(c);
                true
            }
            ClearMode::Once => {
                let c = self.resolve_clear_color();
                self.buffer.clear(c);
                self.clear_mode = ClearMode::Never;
                true
            }
            ClearMode::Never => false,
        }
    }

    /// Fills the entire buffer with a solid color (software child fill helper).
    pub fn fill_solid(&mut self, color: Color) {
        self.buffer.clear(color);
    }

    /// Rasterizes a solid axis-aligned rect into the buffer (ColorRect stand-in).
    pub fn draw_color_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        self.buffer
            .fill_rect(x, y, x.saturating_add(w), y.saturating_add(h), color);
    }

    /// Software path: clear (per clear_mode) then fill a full-viewport ColorRect.
    ///
    /// Returns a snapshot of the color buffer after the update.
    pub fn render_color_rect_child(&mut self, color: Color) -> ColorBuffer {
        self.apply_clear();
        self.draw_color_rect(0, 0, self.width(), self.height(), color);
        self.buffer.clone()
    }

    /// Software path: clear then draw a MeshInstance-style solid marker
    /// (a filled rect representing projected mesh coverage in the slice).
    pub fn render_mesh_marker(&mut self, color: Color) -> ColorBuffer {
        self.apply_clear();
        // Centered half-size marker so edges still show clear color when non-transparent.
        let w = self.width();
        let h = self.height();
        let x0 = w / 4;
        let y0 = h / 4;
        self.draw_color_rect(x0, y0, w / 2, h / 2, color);
        self.buffer.clone()
    }

    /// Syncs typed fields from a scene-tree node's property bag.
    pub fn sync_from_node(&mut self, tree: &SceneTree, node_id: NodeId) {
        self.node_id = Some(node_id);
        if let Some(node) = tree.get_node(node_id) {
            if let Variant::Vector2(v) = node.get_property("size") {
                let nx = v.x.round() as i32;
                let ny = v.y.round() as i32;
                if nx != self.size.x || ny != self.size.y {
                    self.set_size(Vector2i::new(nx, ny));
                }
            }
            if let Variant::Int(i) = node.get_property("render_target_update_mode") {
                self.update_mode = UpdateMode::from_i64(i);
            }
            if let Variant::Int(i) = node.get_property("render_target_clear_mode") {
                self.clear_mode = ClearMode::from_i64(i);
            }
            if let Variant::Bool(b) = node.get_property("transparent_bg") {
                self.transparent_bg = b
            }
        }
    }

    /// Writes typed fields into a scene-tree node's property bag.
    pub fn apply_to_node(&self, tree: &mut SceneTree, node_id: NodeId) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(
                "size",
                Variant::Vector2(Vector2::new(self.size.x as f32, self.size.y as f32)),
            );
            node.set_property(
                "render_target_update_mode",
                Variant::Int(self.update_mode.to_i64()),
            );
            node.set_property(
                "render_target_clear_mode",
                Variant::Int(self.clear_mode.to_i64()),
            );
            node.set_property("transparent_bg", Variant::Bool(self.transparent_bg));
        }
    }
}

// ===========================================================================
// ViewportTexture
// ===========================================================================

/// Texture resource that samples a SubViewport color buffer.
///
/// In Godot this is a `Texture2D` subclass pointing at a viewport via
/// `viewport_path`. Here it holds either a live pixel snapshot or a linked
/// SubViewport id for re-sampling.
#[derive(Debug, Clone)]
pub struct ViewportTexture {
    /// Path string of the linked SubViewport (Godot `viewport_path`).
    pub viewport_path: String,
    /// Linked SubViewport node id when known.
    pub viewport_node_id: Option<NodeId>,
    /// Cached sampled pixels (width × height).
    sampled: Option<ColorBuffer>,
}

impl Default for ViewportTexture {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportTexture {
    /// Creates an unbound ViewportTexture.
    pub fn new() -> Self {
        Self {
            viewport_path: String::new(),
            viewport_node_id: None,
            sampled: None,
        }
    }

    /// Binds this texture to a SubViewport by node id and path label.
    pub fn bind_subviewport(&mut self, node_id: NodeId, path: impl Into<String>) {
        self.viewport_node_id = Some(node_id);
        self.viewport_path = path.into();
    }

    /// Samples (copies) the color buffer from `subviewport`.
    pub fn sample(&mut self, subviewport: &SubViewport) {
        self.sampled = Some(subviewport.buffer().clone());
    }

    /// Returns sampled width, or 0 if unbound / empty.
    pub fn width(&self) -> u32 {
        self.sampled.as_ref().map(|b| b.width).unwrap_or(0)
    }

    /// Returns sampled height, or 0 if unbound / empty.
    pub fn height(&self) -> u32 {
        self.sampled.as_ref().map(|b| b.height).unwrap_or(0)
    }

    /// Returns a reference to the sampled buffer, if any.
    pub fn image(&self) -> Option<&ColorBuffer> {
        self.sampled.as_ref()
    }

    /// Pixel at `(x, y)` from the last sample.
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        self.sampled.as_ref().map(|b| b.get_pixel(x, y))
    }

    /// `true` if the last sample contains any nonzero pixel.
    pub fn has_nonzero_pixels(&self) -> bool {
        self.sampled
            .as_ref()
            .map(|b| b.has_nonzero_pixels())
            .unwrap_or(false)
    }
}

// ===========================================================================
// Scene-tree property helpers
// ===========================================================================

/// Applies Godot SubViewport defaults onto a node property bag.
pub fn apply_subviewport_defaults(tree: &mut SceneTree, node_id: NodeId) {
    SubViewport::with_defaults().apply_to_node(tree, node_id);
}

/// Reads size from a SubViewport node, defaulting to 512×512.
pub fn get_size(tree: &SceneTree, node_id: NodeId) -> Vector2i {
    tree.get_node(node_id)
        .map(|n| match n.get_property("size") {
            Variant::Vector2(v) => Vector2i::new(v.x.round() as i32, v.y.round() as i32),
            _ => Vector2i::new(512, 512),
        })
        .unwrap_or(Vector2i::new(512, 512))
}

/// Sets size on a SubViewport node.
pub fn set_size(tree: &mut SceneTree, node_id: NodeId, size: Vector2i) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            "size",
            Variant::Vector2(Vector2::new(size.x as f32, size.y as f32)),
        );
    }
}

/// Reads `render_target_update_mode`, defaulting to [`UpdateMode::WhenVisible`].
pub fn get_update_mode(tree: &SceneTree, node_id: NodeId) -> UpdateMode {
    tree.get_node(node_id)
        .map(|n| match n.get_property("render_target_update_mode") {
            Variant::Int(i) => UpdateMode::from_i64(i),
            _ => UpdateMode::WhenVisible,
        })
        .unwrap_or(UpdateMode::WhenVisible)
}

/// Sets `render_target_update_mode`.
pub fn set_update_mode(tree: &mut SceneTree, node_id: NodeId, mode: UpdateMode) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("render_target_update_mode", Variant::Int(mode.to_i64()));
    }
}

/// Reads `render_target_clear_mode`, defaulting to [`ClearMode::Always`].
pub fn get_clear_mode(tree: &SceneTree, node_id: NodeId) -> ClearMode {
    tree.get_node(node_id)
        .map(|n| match n.get_property("render_target_clear_mode") {
            Variant::Int(i) => ClearMode::from_i64(i),
            _ => ClearMode::Always,
        })
        .unwrap_or(ClearMode::Always)
}

/// Sets `render_target_clear_mode`.
pub fn set_clear_mode(tree: &mut SceneTree, node_id: NodeId, mode: ClearMode) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("render_target_clear_mode", Variant::Int(mode.to_i64()));
    }
}

/// Reads `transparent_bg`, defaulting to `false`.
pub fn get_transparent_bg(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| matches!(n.get_property("transparent_bg"), Variant::Bool(true)))
        .unwrap_or(false)
}

/// Sets `transparent_bg`.
pub fn set_transparent_bg(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("transparent_bg", Variant::Bool(enabled));
    }
}

/// Builds a [`SubViewport`] runtime from a scene node, allocates its buffer,
/// optionally software-fills with `fill_color`, and returns a sampled
/// [`ViewportTexture`].
///
/// When `fill_color` is `Some`, a full-rect ColorRect fill is performed after
/// clear (the minimal "child ColorRect renders into texture" path).
pub fn render_subviewport_to_texture(
    tree: &SceneTree,
    subviewport_id: NodeId,
    fill_color: Option<Color>,
) -> (SubViewport, ViewportTexture) {
    let mut sv = SubViewport::with_defaults();
    sv.sync_from_node(tree, subviewport_id);
    if let Some(color) = fill_color {
        let _ = sv.render_color_rect_child(color);
    } else if matches!(sv.clear_mode, ClearMode::Always | ClearMode::Once) {
        sv.apply_clear();
    }
    let mut tex = ViewportTexture::new();
    tex.bind_subviewport(subviewport_id, format!("{}", subviewport_id.raw()));
    tex.sample(&sv);
    (sv, tex)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    #[test]
    fn defaults_match_godot_slice() {
        let sv = SubViewport::with_defaults();
        assert_eq!(sv.size, Vector2i::new(512, 512));
        assert_eq!(sv.update_mode, UpdateMode::WhenVisible);
        assert_eq!(sv.clear_mode, ClearMode::Always);
        assert!(!sv.transparent_bg);
        assert_eq!(sv.width(), 512);
        assert_eq!(sv.height(), 512);
        assert_eq!(sv.buffer().pixels.len(), 512 * 512);
    }

    #[test]
    fn color_rect_fill_produces_nonzero_pixels() {
        let mut sv = SubViewport::new(Vector2i::new(32, 32));
        // Fresh buffer is black (opaque) — alpha is 1, RGB is 0.
        // Fill with red ColorRect so RGB is nonzero.
        let buf = sv.render_color_rect_child(Color::rgb(1.0, 0.0, 0.0));
        assert!(buf.has_nonzero_pixels());
        assert_eq!(buf.get_pixel(0, 0), Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(buf.count_nonzero_pixels(0.01), 32 * 32);
    }

    #[test]
    fn viewport_texture_samples_subviewport_buffer() {
        let mut sv = SubViewport::new(Vector2i::new(16, 8));
        sv.render_mesh_marker(Color::rgb(0.0, 1.0, 0.0));

        let mut tex = ViewportTexture::new();
        tex.sample(&sv);

        assert_eq!(tex.width(), 16);
        assert_eq!(tex.height(), 8);
        assert!(tex.has_nonzero_pixels());
        // Center of marker should be green.
        let c = tex.get_pixel(8, 4).unwrap();
        assert!(c.g > 0.9, "center should be green, got {c:?}");
        // Corner should still be clear color (black) after clear+marker.
        let corner = tex.get_pixel(0, 0).unwrap();
        assert!(
            corner.r < 0.01 && corner.g < 0.01 && corner.b < 0.01,
            "corner should be clear black, got {corner:?}"
        );
    }

    #[test]
    fn scene_tree_subviewport_render_to_texture() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sv_id = tree
            .add_child(root, Node::new("Offscreen", "SubViewport"))
            .unwrap();
        apply_subviewport_defaults(&mut tree, sv_id);
        set_size(&mut tree, sv_id, Vector2i::new(64, 48));

        assert_eq!(get_size(&tree, sv_id), Vector2i::new(64, 48));
        assert_eq!(get_update_mode(&tree, sv_id), UpdateMode::WhenVisible);
        assert_eq!(get_clear_mode(&tree, sv_id), ClearMode::Always);
        assert!(!get_transparent_bg(&tree, sv_id));

        // Child ColorRect-style content: solid blue fill into the target.
        let (_sv, tex) =
            render_subviewport_to_texture(&tree, sv_id, Some(Color::rgb(0.0, 0.0, 1.0)));
        assert_eq!(tex.width(), 64);
        assert_eq!(tex.height(), 48);
        assert!(
            tex.has_nonzero_pixels(),
            "ViewportTexture must sample nonzero ColorRect pixels"
        );
        let p = tex.get_pixel(10, 10).unwrap();
        assert!(p.b > 0.9 && p.r < 0.01, "expected blue, got {p:?}");
    }

    #[test]
    fn transparent_bg_clear_uses_alpha_zero() {
        let mut sv = SubViewport::new(Vector2i::new(4, 4));
        sv.transparent_bg = true;
        sv.apply_clear();
        let p = sv.buffer().get_pixel(0, 0);
        assert_eq!(p.a, 0.0);
        // Clear (transparent or opaque black) has zero RGB → has_nonzero is false.
        assert!(!sv.buffer().has_nonzero_pixels());
        sv.draw_color_rect(0, 0, 2, 2, Color::rgb(1.0, 1.0, 0.0));
        assert!(sv.buffer().has_nonzero_pixels());
    }

    #[test]
    fn default_clear_black_is_not_nonzero_rgb() {
        let sv = SubViewport::new(Vector2i::new(8, 8));
        assert!(!sv.buffer().has_nonzero_pixels());
    }

    #[test]
    fn update_and_clear_mode_roundtrip() {
        assert_eq!(UpdateMode::from_i64(2), UpdateMode::WhenVisible);
        assert_eq!(UpdateMode::from_i64(4).to_i64(), 4);
        assert_eq!(ClearMode::from_i64(0), ClearMode::Always);
        assert_eq!(ClearMode::Once.to_i64(), 2);

        let mut sv = SubViewport::new(Vector2i::new(2, 2));
        sv.clear_mode = ClearMode::Once;
        assert!(sv.apply_clear());
        assert_eq!(sv.clear_mode, ClearMode::Never);
        assert!(!sv.apply_clear());
    }
}
