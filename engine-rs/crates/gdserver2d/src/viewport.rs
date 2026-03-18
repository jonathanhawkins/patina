//! Viewport management and coordinate mapping.
//!
//! A viewport owns a collection of canvas items and canvas layers, and provides
//! z-index-sorted iteration for the rendering pipeline.

use gdcore::math::{Color, Vector2};

use crate::canvas::{CanvasItem, CanvasItemId};
use crate::canvas_layer::CanvasLayer;

/// A rendering viewport that holds canvas items and their draw order.
#[derive(Debug, Clone)]
pub struct Viewport {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Color used to clear the framebuffer before rendering.
    pub clear_color: Color,
    /// Canvas items owned by this viewport.
    canvas_items: Vec<CanvasItem>,
    /// Canvas layers for grouping items.
    canvas_layers: Vec<CanvasLayer>,
    /// Camera position in world space.
    pub camera_position: Vector2,
    /// Camera zoom factor (1.0 = no zoom).
    pub camera_zoom: Vector2,
    /// Camera rotation in radians.
    pub camera_rotation: f32,
}

impl Viewport {
    /// Creates a new viewport with the given dimensions and clear color.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        Self {
            width,
            height,
            clear_color,
            canvas_items: Vec::new(),
            canvas_layers: Vec::new(),
            camera_position: Vector2::ZERO,
            camera_zoom: Vector2::ONE,
            camera_rotation: 0.0,
        }
    }

    /// Adds a canvas item to the viewport.
    pub fn add_canvas_item(&mut self, item: CanvasItem) {
        self.canvas_items.push(item);
    }

    /// Removes a canvas item by ID. Returns `true` if it was found and removed.
    pub fn remove_canvas_item(&mut self, id: CanvasItemId) -> bool {
        let len_before = self.canvas_items.len();
        self.canvas_items.retain(|item| item.id != id);
        self.canvas_items.len() < len_before
    }

    /// Returns a mutable reference to a canvas item by ID.
    pub fn get_canvas_item_mut(&mut self, id: CanvasItemId) -> Option<&mut CanvasItem> {
        self.canvas_items.iter_mut().find(|item| item.id == id)
    }

    /// Returns a reference to a canvas item by ID.
    pub fn get_canvas_item(&self, id: CanvasItemId) -> Option<&CanvasItem> {
        self.canvas_items.iter().find(|item| item.id == id)
    }

    /// Returns canvas items sorted by z_index (ascending) for rendering.
    pub fn get_sorted_items(&self) -> Vec<&CanvasItem> {
        let mut items: Vec<&CanvasItem> = self.canvas_items.iter().collect();
        items.sort_by_key(|item| item.z_index);
        items
    }

    /// Adds a canvas layer to the viewport.
    pub fn add_canvas_layer(&mut self, layer: CanvasLayer) {
        self.canvas_layers.push(layer);
    }

    /// Removes a canvas layer by ID. Returns `true` if found and removed.
    pub fn remove_canvas_layer(&mut self, layer_id: u64) -> bool {
        let len_before = self.canvas_layers.len();
        self.canvas_layers.retain(|l| l.layer_id != layer_id);
        self.canvas_layers.len() < len_before
    }

    /// Returns a mutable reference to a canvas layer by ID.
    pub fn get_canvas_layer_mut(&mut self, layer_id: u64) -> Option<&mut CanvasLayer> {
        self.canvas_layers
            .iter_mut()
            .find(|l| l.layer_id == layer_id)
    }

    /// Returns canvas layers sorted by z_order (ascending) for rendering.
    pub fn get_sorted_layers(&self) -> Vec<&CanvasLayer> {
        let mut layers: Vec<&CanvasLayer> = self.canvas_layers.iter().collect();
        layers.sort_by_key(|l| l.z_order);
        layers
    }

    /// Returns canvas items belonging to a specific layer, sorted by z_index.
    pub fn get_items_for_layer(&self, layer_id: u64) -> Vec<&CanvasItem> {
        let mut items: Vec<&CanvasItem> = self
            .canvas_items
            .iter()
            .filter(|item| item.layer_id == Some(layer_id))
            .collect();
        items.sort_by_key(|item| item.z_index);
        items
    }

    /// Returns canvas items that don't belong to any layer, sorted by z_index.
    pub fn get_unlayered_items(&self) -> Vec<&CanvasItem> {
        let mut items: Vec<&CanvasItem> = self
            .canvas_items
            .iter()
            .filter(|item| item.layer_id.is_none())
            .collect();
        items.sort_by_key(|item| item.z_index);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::CanvasItem;

    #[test]
    fn viewport_add_remove() {
        let mut vp = Viewport::new(800, 600, Color::BLACK);
        vp.add_canvas_item(CanvasItem::new(CanvasItemId(1)));
        vp.add_canvas_item(CanvasItem::new(CanvasItemId(2)));
        assert_eq!(vp.get_sorted_items().len(), 2);

        assert!(vp.remove_canvas_item(CanvasItemId(1)));
        assert_eq!(vp.get_sorted_items().len(), 1);
        assert_eq!(vp.get_sorted_items()[0].id, CanvasItemId(2));

        // Removing non-existent item returns false.
        assert!(!vp.remove_canvas_item(CanvasItemId(99)));
    }

    #[test]
    fn viewport_z_index_sorting() {
        let mut vp = Viewport::new(100, 100, Color::BLACK);

        let mut a = CanvasItem::new(CanvasItemId(1));
        a.z_index = 10;
        let mut b = CanvasItem::new(CanvasItemId(2));
        b.z_index = -5;
        let mut c = CanvasItem::new(CanvasItemId(3));
        c.z_index = 0;

        vp.add_canvas_item(a);
        vp.add_canvas_item(b);
        vp.add_canvas_item(c);

        let sorted = vp.get_sorted_items();
        assert_eq!(sorted[0].id, CanvasItemId(2)); // z=-5
        assert_eq!(sorted[1].id, CanvasItemId(3)); // z=0
        assert_eq!(sorted[2].id, CanvasItemId(1)); // z=10
    }
}
