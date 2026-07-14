//! Control-tree render pass (pat-u3l8c.67).
//!
//! Bridges the Control layout functions in [`crate::control`] (previously
//! exercised only from tests) to gdserver2d's draw-command `Viewport`.
//! [`render_control_tree`] walks a [`SceneTree`]'s Control subtree, resolves
//! each control's rect **live** from its anchors/offsets against the parent
//! rect, looks up themed colors and StyleBoxes, and emits [`DrawCommand`]s so a
//! 2D renderer produces themed pixels for `Button`/`Label`/`Panel`/`ColorRect`/
//! `TextureRect`.
//!
//! Positions accumulate down the tree into global (viewport) space; each
//! control becomes one [`CanvasItem`] added in tree pre-order so children paint
//! over their parents (painter's algorithm), matching Godot's draw order.

use gdcore::math::{Color, Rect2, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

use crate::control;
use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use crate::theme::{Theme, ThemePropertyType};

/// Nominal glyph metrics of the built-in 5×7 bitmap font used by the software
/// renderer's `DrawString`. Used to place/centre control text.
const GLYPH_W: f32 = 5.0;
const GLYPH_H: f32 = 7.0;
const GLYPH_GAP: f32 = 1.0;

/// Control classes handled by the render pass. Used as a fallback membership
/// test in case the ClassDB inheritance chain is unavailable for `is_class`.
const CONTROL_CLASSES: &[&str] = &[
    "Control",
    "Panel",
    "PanelContainer",
    "Button",
    "Label",
    "ColorRect",
    "TextureRect",
];

/// Renders the Control subtree rooted at `root` into `viewport`.
///
/// `viewport_size` is the rect the root control anchors against (typically the
/// framebuffer size). Each control's layout is resolved live from its anchors
/// and offsets; global positions accumulate down the tree. Non-control nodes
/// are transparent to layout — their children inherit the same parent rect.
pub fn render_control_tree(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &Theme,
    viewport: &mut Viewport,
    viewport_size: Vector2,
) {
    let mut order: u64 = 0;
    render_node(
        tree,
        root,
        theme,
        viewport,
        Vector2::ZERO,
        viewport_size,
        &mut order,
    );
}

fn is_control(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("Control") || CONTROL_CLASSES.contains(&n.class_name()))
        .unwrap_or(false)
}

fn class_of(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| n.class_name().to_string())
        .unwrap_or_default()
}

fn children_of(tree: &SceneTree, node_id: NodeId) -> Vec<NodeId> {
    tree.get_node(node_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default()
}

fn render_node(
    tree: &mut SceneTree,
    node_id: NodeId,
    theme: &Theme,
    viewport: &mut Viewport,
    parent_global: Vector2,
    parent_size: Vector2,
    order: &mut u64,
) {
    if is_control(tree, node_id) {
        // Resolve this control's rect from anchors/offsets against the parent
        // rect. resolve_control_layout stores a parent-relative position.
        control::resolve_control_layout(tree, node_id, parent_size);
        let local_pos = control::get_position(tree, node_id);
        let size = control::get_size(tree, node_id);
        let global_pos = parent_global + local_pos;
        let rect = Rect2::new(global_pos, size);

        let seq = *order;
        *order += 1;
        let mut item = CanvasItem::new(CanvasItemId(seq + 1));
        // Pre-order sequence as z so children (added later) paint on top.
        item.z_index = seq as i32;
        draw_control(tree, node_id, theme, rect, &mut item.commands);
        if !item.commands.is_empty() {
            viewport.add_canvas_item(item);
        }

        // Children anchor against this control's rect in its global space.
        for child in children_of(tree, node_id) {
            render_node(tree, child, theme, viewport, global_pos, size, order);
        }
    } else {
        // Non-control node: pass the parent rect straight through to children.
        for child in children_of(tree, node_id) {
            render_node(tree, child, theme, viewport, parent_global, parent_size, order);
        }
    }
}

/// Reads a `Color`-typed node property, if present.
fn color_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<Color> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Color(c) => Some(c),
        _ => None,
    }
}

/// Reads a `String`-typed node property, if present.
fn string_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<String> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::String(s) => Some(s),
        _ => None,
    }
}

/// Resolves a StyleBox theme item to its background color. Default styleboxes
/// are stored as `Variant::Color` (their flat background). Falls back to
/// `fallback` when the item is missing or a different Variant type.
fn stylebox_bg(theme: &Theme, control_type: &str, property: &str, fallback: Color) -> Color {
    match theme.get_item(ThemePropertyType::Stylebox, control_type, property) {
        Some(Variant::Color(c)) => *c,
        _ => fallback,
    }
}

/// Maps a theme font size (in points) to the built-in 5×7 font's integer pixel
/// scale so glyph height ≈ the requested size.
fn text_scale(font_size: i64) -> u32 {
    (font_size.max(1) as u32 / GLYPH_H as u32).max(1)
}

/// Approximate pixel width of `text` at the given scale, using the nominal
/// 5×7 monospace metrics.
fn text_width(text: &str, scale: u32) -> f32 {
    let n = text.chars().count() as f32;
    if n <= 0.0 {
        return 0.0;
    }
    (n * GLYPH_W + (n - 1.0) * GLYPH_GAP) * scale as f32
}

fn draw_control(
    tree: &SceneTree,
    node_id: NodeId,
    theme: &Theme,
    rect: Rect2,
    out: &mut Vec<DrawCommand>,
) {
    match class_of(tree, node_id).as_str() {
        "ColorRect" => {
            let color = color_prop(tree, node_id, "color").unwrap_or(Color::WHITE);
            out.push(DrawCommand::DrawRect {
                rect,
                color,
                filled: true,
            });
        }
        "Panel" | "PanelContainer" => {
            // Prefer a typed StyleBox (pat-u3l8c.70); fall back to the legacy
            // Variant::Color background.
            if let Some(sb) = theme.get_stylebox("Panel", "panel") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(theme, "Panel", "panel", Color::new(0.15, 0.15, 0.15, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }
        }
        "Button" => {
            if let Some(sb) = theme.get_stylebox("Button", "normal") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(theme, "Button", "normal", Color::new(0.2, 0.2, 0.2, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                let font_color = theme
                    .get_color("Button", "font_color")
                    .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
                let font_size = theme.get_font_size("Button", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                let tw = text_width(&text, scale);
                let th = GLYPH_H * scale as f32;
                // Centre the label within the button rect.
                let position = Vector2::new(
                    rect.position.x + (rect.size.x - tw) * 0.5,
                    rect.position.y + (rect.size.y - th) * 0.5,
                );
                out.push(DrawCommand::DrawString {
                    text,
                    position,
                    color: font_color,
                    font_size: scale,
                });
            }
        }
        "Label" => {
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                let font_color = theme.get_color("Label", "font_color").unwrap_or(Color::WHITE);
                let font_size = theme.get_font_size("Label", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                out.push(DrawCommand::DrawString {
                    text,
                    position: rect.position,
                    color: font_color,
                    font_size: scale,
                });
            }
        }
        "TextureRect" => {
            if let Some(path) = string_prop(tree, node_id, "texture") {
                if !path.is_empty() {
                    out.push(DrawCommand::DrawTextureRect {
                        texture_path: path,
                        rect,
                        modulate: Color::WHITE,
                    });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::AnchorPreset;
    use crate::node::Node;

    fn approx(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
    }

    #[test]
    fn text_scale_maps_point_size_to_font_scale() {
        assert_eq!(text_scale(16), 2); // 16 / 7 = 2
        assert_eq!(text_scale(7), 1);
        assert_eq!(text_scale(0), 1); // never zero
    }

    #[test]
    fn colorrect_emits_filled_rect_with_property_color() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("Rect", "ColorRect");
        n.set_property("color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(64, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(64.0, 64.0));

        let items = vp.get_sorted_items();
        assert_eq!(items.len(), 1);
        match &items[0].commands[0] {
            DrawCommand::DrawRect { color, filled, .. } => {
                assert!(filled);
                assert!(approx(*color, Color::new(1.0, 0.0, 0.0, 1.0)));
            }
            other => panic!("expected DrawRect, got {other:?}"),
        }
    }

    #[test]
    fn panel_uses_theme_stylebox_bg() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("P", "Panel")).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(32, 32, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(32.0, 32.0));

        match &vp.get_sorted_items()[0].commands[0] {
            DrawCommand::DrawRect { color, .. } => {
                assert!(approx(*color, Color::new(0.15, 0.15, 0.15, 1.0)));
            }
            other => panic!("expected DrawRect, got {other:?}"),
        }
    }

    #[test]
    fn button_emits_bg_and_centered_text() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("B", "Button");
        n.set_property("text", Variant::String("OK".to_owned()));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        control::set_offset_right(&mut tree, id, 100.0);
        control::set_offset_bottom(&mut tree, id, 40.0);

        let mut vp = Viewport::new(128, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(128.0, 64.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        assert_eq!(cmds.len(), 2, "button = bg rect + text");
        assert!(matches!(cmds[0], DrawCommand::DrawRect { filled: true, .. }));
        match &cmds[1] {
            DrawCommand::DrawString { text, position, .. } => {
                assert_eq!(text, "OK");
                assert!(position.x > 0.0 && position.x < 100.0);
                assert!(position.y > 0.0 && position.y < 40.0);
            }
            other => panic!("expected DrawString, got {other:?}"),
        }
    }

    #[test]
    fn nested_controls_accumulate_global_position() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let panel = tree.add_child(root, Node::new("Panel", "Panel")).unwrap();
        control::apply_anchor_preset(&mut tree, panel, AnchorPreset::TopLeft);
        control::set_offset_left(&mut tree, panel, 10.0);
        control::set_offset_top(&mut tree, panel, 10.0);
        control::set_offset_right(&mut tree, panel, 210.0);
        control::set_offset_bottom(&mut tree, panel, 210.0);

        let btn = tree.add_child(panel, Node::new("Btn", "Button")).unwrap();
        control::apply_anchor_preset(&mut tree, btn, AnchorPreset::TopLeft);
        control::set_offset_left(&mut tree, btn, 20.0);
        control::set_offset_top(&mut tree, btn, 20.0);
        control::set_offset_right(&mut tree, btn, 120.0);
        control::set_offset_bottom(&mut tree, btn, 60.0);

        let mut vp = Viewport::new(256, 256, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(256.0, 256.0));

        let items = vp.get_sorted_items();
        assert_eq!(items.len(), 2, "panel + button");
        // items sorted by z_index; panel (seq 0) first, button (seq 1) second.
        match &items[1].commands[0] {
            DrawCommand::DrawRect { rect, .. } => {
                assert!((rect.position.x - 30.0).abs() < 0.01, "global x=30");
                assert!((rect.position.y - 30.0).abs() < 0.01, "global y=30");
            }
            other => panic!("expected button DrawRect, got {other:?}"),
        }
    }
}
