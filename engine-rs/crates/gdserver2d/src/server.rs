//! 2D rendering server trait definitions and dispatch.
//!
//! Defines the abstract [`RenderingServer2D`] trait that all rendering
//! backends must implement.

use gdcore::math::Transform2D;

use crate::canvas::{CanvasItemId, DrawCommand};
use crate::viewport::Viewport;

/// Frame data produced by a rendering pass.
#[derive(Debug, Clone)]
pub struct FrameData {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw pixel data as a flat array of RGBA `f32` colors.
    pub pixels: Vec<gdcore::math::Color>,
}

/// Abstract rendering server for 2D content.
///
/// Implementations manage canvas items and produce rendered frames.
pub trait RenderingServer2D {
    /// Creates a new canvas item and returns its unique ID.
    fn create_canvas_item(&mut self) -> CanvasItemId;

    /// Frees a canvas item by ID.
    fn free_canvas_item(&mut self, id: CanvasItemId);

    /// Appends a draw command to a canvas item.
    fn canvas_item_add_draw_command(&mut self, id: CanvasItemId, command: DrawCommand);

    /// Sets the transform for a canvas item.
    fn canvas_item_set_transform(&mut self, id: CanvasItemId, transform: Transform2D);

    /// Sets the z-index for a canvas item.
    fn canvas_item_set_z_index(&mut self, id: CanvasItemId, z_index: i32);

    /// Sets visibility for a canvas item.
    fn canvas_item_set_visible(&mut self, id: CanvasItemId, visible: bool);

    /// Renders a frame for the given viewport and returns the resulting pixel data.
    fn render_frame(&mut self, viewport: &Viewport) -> FrameData;
}
