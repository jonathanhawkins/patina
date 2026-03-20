//! Canvas item and drawing command abstractions.
//!
//! Provides the core data structures for 2D rendering: canvas items that hold
//! draw commands, and the draw commands themselves (rect, circle, line, texture).

use gdcore::math::{Color, Rect2, Transform2D, Vector2};

/// Unique identifier for a canvas draw item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanvasItemId(pub u64);

/// A drawing command that can be queued on a [`CanvasItem`].
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    /// Draw a rectangle, optionally filled.
    DrawRect {
        rect: Rect2,
        color: Color,
        filled: bool,
    },
    /// Draw a circle at `center` with the given `radius`.
    DrawCircle {
        center: Vector2,
        radius: f32,
        color: Color,
    },
    /// Draw a line segment from `from` to `to`.
    DrawLine {
        from: Vector2,
        to: Vector2,
        color: Color,
        width: f32,
    },
    /// Draw a texture stretched into `rect`, tinted by `modulate`.
    DrawTextureRect {
        texture_path: String,
        rect: Rect2,
        modulate: Color,
    },
    /// Draw a sub-region of a texture atlas into `rect`, tinted by `modulate`.
    DrawTextureRegion {
        texture_path: String,
        rect: Rect2,
        source_rect: Rect2,
        modulate: Color,
    },
    /// Draw a text string at `position` with the given `color` and `font_size`.
    DrawString {
        /// The text to render.
        text: String,
        /// Top-left position for the text.
        position: Vector2,
        /// Text color.
        color: Color,
        /// Font size (used as scale multiplier over the built-in 5×7 font).
        font_size: u32,
    },
    /// Draw a nine-patch (9-slice) texture into `rect`.
    ///
    /// The texture is divided into 9 regions by `margin_left`, `margin_top`,
    /// `margin_right`, and `margin_bottom`. Corners are drawn at fixed size,
    /// edges are stretched along one axis, and the center is stretched in both.
    DrawNinePatch {
        /// Path to the source texture.
        texture_path: String,
        /// Destination rectangle.
        rect: Rect2,
        /// Left margin in pixels (corner/edge width).
        margin_left: f32,
        /// Top margin in pixels (corner/edge height).
        margin_top: f32,
        /// Right margin in pixels (corner/edge width).
        margin_right: f32,
        /// Bottom margin in pixels (corner/edge height).
        margin_bottom: f32,
        /// Whether to draw the center region.
        draw_center: bool,
        /// Tint color applied to all nine regions.
        modulate: Color,
    },
}

/// A canvas item that holds a transform, draw commands, and child references.
#[derive(Debug, Clone)]
pub struct CanvasItem {
    /// Unique identifier for this item.
    pub id: CanvasItemId,
    /// Local transform applied to all draw commands.
    pub transform: Transform2D,
    /// Draw order index; higher values render on top.
    pub z_index: i32,
    /// Whether the item is visible.
    pub visible: bool,
    /// Queued draw commands for this item.
    pub commands: Vec<DrawCommand>,
    /// Child canvas item IDs.
    pub children: Vec<CanvasItemId>,
    /// Parent canvas item ID (for transform inheritance).
    pub parent: Option<CanvasItemId>,
    /// Canvas layer this item belongs to.
    pub layer_id: Option<u64>,
}

impl CanvasItem {
    /// Creates a new canvas item with default settings.
    pub fn new(id: CanvasItemId) -> Self {
        Self {
            id,
            transform: Transform2D::IDENTITY,
            z_index: 0,
            visible: true,
            commands: Vec::new(),
            children: Vec::new(),
            parent: None,
            layer_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_item_creation() {
        let item = CanvasItem::new(CanvasItemId(1));
        assert_eq!(item.id, CanvasItemId(1));
        assert_eq!(item.z_index, 0);
        assert!(item.visible);
        assert!(item.commands.is_empty());
        assert!(item.children.is_empty());
        assert_eq!(item.transform, Transform2D::IDENTITY);
    }

    #[test]
    fn draw_command_queuing() {
        let mut item = CanvasItem::new(CanvasItemId(42));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::WHITE,
            filled: true,
        });
        item.commands.push(DrawCommand::DrawCircle {
            center: Vector2::new(5.0, 5.0),
            radius: 3.0,
            color: Color::rgb(1.0, 0.0, 0.0),
        });
        item.commands.push(DrawCommand::DrawLine {
            from: Vector2::ZERO,
            to: Vector2::new(10.0, 10.0),
            color: Color::BLACK,
            width: 1.0,
        });
        assert_eq!(item.commands.len(), 3);
    }

    #[test]
    fn nine_patch_draw_command() {
        let mut item = CanvasItem::new(CanvasItemId(10));
        item.commands.push(DrawCommand::DrawNinePatch {
            texture_path: "res://panel.png".to_string(),
            rect: Rect2::new(Vector2::ZERO, Vector2::new(100.0, 50.0)),
            margin_left: 8.0,
            margin_top: 8.0,
            margin_right: 8.0,
            margin_bottom: 8.0,
            draw_center: true,
            modulate: Color::WHITE,
        });
        assert_eq!(item.commands.len(), 1);
        match &item.commands[0] {
            DrawCommand::DrawNinePatch {
                margin_left,
                draw_center,
                ..
            } => {
                assert_eq!(*margin_left, 8.0);
                assert!(*draw_center);
            }
            _ => panic!("expected DrawNinePatch"),
        }
    }

    #[test]
    fn canvas_item_id_equality() {
        let a = CanvasItemId(1);
        let b = CanvasItemId(1);
        let c = CanvasItemId(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
