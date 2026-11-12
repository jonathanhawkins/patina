//! pat-z35r2.17 — a per-node `theme_override_colors/font_color` must be applied
//! when a Control is drawn (not just resolvable via the theme API).
//!
//! `pat-nzgdu.89` added the override *resolution* helpers
//! (`control::add_theme_color_override` / `resolve_theme_color`), and
//! `pat-u3l8c.70` wired StyleBox draw, but `draw_control` still read font colors
//! straight from the Theme, so a local override never reached the framebuffer.
//! This drives the real render pass and inspects the emitted `DrawString` to
//! prove the override color wins during draw for both `Button` and `Label`.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

fn close(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() <= tol && (a.g - b.g).abs() <= tol && (a.b - b.b).abs() <= tol
}

/// Builds a single `Control` of `class` with `text`, anchored to fill its parent.
fn control_with_text(class: &str, text: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new(class, class)).unwrap();
    control::apply_anchor_preset(&mut tree, node, AnchorPreset::FullRect);
    if let Some(n) = tree.get_node_mut(node) {
        n.set_property("text", Variant::String(text.to_string()));
    }
    (tree, node)
}

/// The color of the first `DrawString` emitted into the viewport's base layer.
fn drawn_text_color(vp: &Viewport) -> Option<Color> {
    for item in vp.get_unlayered_items() {
        for cmd in &item.commands {
            if let DrawCommand::DrawString { color, .. } = cmd {
                return Some(*color);
            }
        }
    }
    None
}

fn render_text_color(class: &str, override_color: Option<Color>) -> Color {
    // Empty theme: without an override the class font_color falls back to the
    // built-in default, which is deliberately different from our override.
    let theme = Theme::new("t");
    let (mut tree, node) = control_with_text(class, "Hi");
    if let Some(c) = override_color {
        control::add_theme_color_override(&mut tree, node, "font_color", c);
    }
    let mut vp = Viewport::new(80, 40, Color::BLACK);
    render_control_tree(&mut tree, node, &theme, &mut vp, Vector2::new(80.0, 40.0));
    drawn_text_color(&vp).expect("control text must emit a DrawString")
}

#[test]
fn theme_override_color_applied_on_control_draw() {
    let override_col = Color::new(1.0, 0.0, 0.0, 1.0); // red

    for class in ["Button", "Label"] {
        // Baseline (no override) uses the theme/default color…
        let base = render_text_color(class, None);
        // …and a per-node override wins during draw.
        let drawn = render_text_color(class, Some(override_col));

        assert!(
            close(drawn, override_col, 0.001),
            "{class}: theme_override_colors/font_color must be applied during draw, got {drawn:?}"
        );
        assert!(
            !close(drawn, base, 0.001),
            "{class}: overridden color must differ from the un-overridden default ({base:?})"
        );
    }
}
