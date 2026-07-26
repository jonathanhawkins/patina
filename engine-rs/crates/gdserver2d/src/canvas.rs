//! Canvas item and drawing command abstractions.
//!
//! Provides the core data structures for 2D rendering: canvas items that hold
//! draw commands, and the draw commands themselves (rect, circle, line, texture).

use gdcore::math::{Color, Rect2, Transform2D, Vector2};

use crate::material::{CanvasItemMaterial, TextureFilter, TextureRepeat};
use crate::shader::ShaderMaterial;

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
    /// Draw a connected run of line segments (`CanvasItem.draw_polyline`).
    ///
    /// Unlike a chain of [`DrawLine`](Self::DrawLine)s this is rasterized as one
    /// stroke, so the joints between segments are filled instead of leaving a
    /// notch on every turn.
    DrawPolyline {
        /// Vertices in the item's local space.
        points: Vec<Vector2>,
        /// Stroke color.
        color: Color,
        /// Stroke width in pixels.
        width: f32,
        /// Whether the last point joins back to the first.
        closed: bool,
    },
    /// Draw a set of independent segments, each `points[2i] -> points[2i+1]`
    /// (`CanvasItem.draw_multiline`). Odd trailing points are ignored.
    DrawMultiline {
        /// Segment endpoints, in pairs.
        points: Vec<Vector2>,
        /// Stroke color.
        color: Color,
        /// Stroke width in pixels.
        width: f32,
    },
    /// Draw a dashed line from `from` to `to` (`CanvasItem.draw_dashed_line`).
    DrawDashedLine {
        /// Start point.
        from: Vector2,
        /// End point.
        to: Vector2,
        /// Stroke color.
        color: Color,
        /// Stroke width in pixels.
        width: f32,
        /// Length of each dash (and of each gap) in pixels.
        dash: f32,
    },
    /// Draw a circular arc as a stroked polyline (`CanvasItem.draw_arc`).
    ///
    /// Angles are in radians and follow Godot's convention: 0 points along +X
    /// and the sweep runs toward +Y.
    DrawArc {
        /// Arc centre in local space.
        center: Vector2,
        /// Arc radius.
        radius: f32,
        /// Start angle in radians.
        start_angle: f32,
        /// End angle in radians.
        end_angle: f32,
        /// How many segments approximate the arc.
        point_count: u32,
        /// Stroke color.
        color: Color,
        /// Stroke width in pixels.
        width: f32,
    },
    /// Fill a polygon (`CanvasItem.draw_polygon` / `draw_colored_polygon`).
    ///
    /// `colors` holds either one color for the whole polygon or one per vertex;
    /// the software rasterizer fills with the first, since it has no
    /// per-vertex interpolation.
    DrawPolygon {
        /// Vertices in the item's local space.
        points: Vec<Vector2>,
        /// Per-polygon or per-vertex colors.
        colors: Vec<Color>,
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
    /// Optional 2D shader material. When set with a `fragment()` shader, the
    /// item's textured draw commands run the shader per-pixel in the software
    /// render path — `COLOR` starts as the sampled texel × modulate, `UV` spans
    /// 0..1 across the destination rect, `TIME` is elapsed seconds (pat-u3l8c.45).
    pub material: Option<ShaderMaterial>,
    /// Optional clip rectangle in this item's local space — Godot's
    /// `RenderingServer::canvas_item_set_clip` / `Control.clip_contents`
    /// (pat-0q2kx.86).
    ///
    /// When set, the renderer scissors every one of this item's draw commands
    /// to the rect, so content larger than its container (a ScrollContainer's
    /// child, a clipped Panel) is cut off instead of overflowing. `None` means
    /// unclipped, which is every item's default.
    ///
    /// This is a per-item rect rather than a push/pop command pair because the
    /// renderer walks items flat and resolves each one's transform
    /// independently — there is no draw-order stack for a pop to unwind.
    pub clip_rect: Option<Rect2>,
    /// Optional [`CanvasItemMaterial`] controlling how this item composites
    /// (pat-0q2kx.131).
    ///
    /// Separate from [`material`](Self::material), which is a *shader*
    /// material: Godot's `CanvasItem.material` slot holds either, and they do
    /// different jobs — a shader rewrites the colour, this decides how the
    /// result lands on the framebuffer. `None` means plain `Mix`.
    pub canvas_material: Option<CanvasItemMaterial>,
    /// Resolved texture sampling filter for this item's textured draws.
    ///
    /// The scene bridge resolves `ParentNode` up the ancestor chain to the
    /// project default before setting it here, so the rasterizer never has to
    /// walk parents.
    pub texture_filter: TextureFilter,
    /// Resolved UV wrap mode, resolved the same way as
    /// [`texture_filter`](Self::texture_filter).
    pub texture_repeat: TextureRepeat,
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
            material: None,
            clip_rect: None,
            canvas_material: None,
            texture_filter: TextureFilter::default(),
            texture_repeat: TextureRepeat::default(),
        }
    }

    /// Clips this item's draw commands to `rect` (in the item's local space) —
    /// Godot's `canvas_item_set_clip` (pat-0q2kx.86).
    pub fn set_clip_rect(&mut self, rect: Rect2) {
        self.clip_rect = Some(rect);
    }

    /// Attaches a shader material so this item's textured draws run the
    /// material's `fragment()` shader per-pixel (pat-u3l8c.45).
    pub fn set_material(&mut self, material: ShaderMaterial) {
        self.material = Some(material);
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
