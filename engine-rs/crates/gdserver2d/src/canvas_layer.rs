//! Canvas layer system for grouping and ordering canvas items.
//!
//! A [`CanvasLayer`] groups canvas items that share a common transform and
//! z-order layer. Layers are rendered in ascending `z_order`.

use gdcore::math::Transform2D;

/// A canvas layer that groups items under a shared transform and z-order.
#[derive(Debug, Clone)]
pub struct CanvasLayer {
    /// Unique identifier for this layer.
    pub layer_id: u64,
    /// Render order — layers with lower z_order are drawn first.
    pub z_order: i32,
    /// Transform applied to all items in this layer.
    pub transform: Transform2D,
    /// Whether this layer is visible.
    pub visible: bool,
}

impl CanvasLayer {
    /// Creates a new canvas layer with default settings.
    pub fn new(layer_id: u64) -> Self {
        Self {
            layer_id,
            z_order: 0,
            transform: Transform2D::IDENTITY,
            visible: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;

    #[test]
    fn canvas_layer_creation() {
        let layer = CanvasLayer::new(1);
        assert_eq!(layer.layer_id, 1);
        assert_eq!(layer.z_order, 0);
        assert_eq!(layer.transform, Transform2D::IDENTITY);
        assert!(layer.visible);
    }

    #[test]
    fn canvas_layer_custom_z_order() {
        let mut layer = CanvasLayer::new(2);
        layer.z_order = 10;
        assert_eq!(layer.z_order, 10);
    }

    #[test]
    fn canvas_layer_with_transform() {
        let mut layer = CanvasLayer::new(3);
        layer.transform = Transform2D::translated(Vector2::new(100.0, 200.0));
        let p = layer.transform.xform(Vector2::ZERO);
        assert!((p.x - 100.0).abs() < 1e-6);
        assert!((p.y - 200.0).abs() < 1e-6);
    }
}
