//! pat-u3l8c.67 — GUI Control render pass, framebuffer app test.
//!
//! Before this bead NO Control drawing from the scene tree existed: the
//! viewport was a flat Vec of manual draw commands and
//! `resolve_control_layout`/`arrange_container` were called only from unit
//! tests. This test builds a real Control tree (Panel > ColorRect, plus Button,
//! Label, TextureRect), runs the render pass that walks the tree and resolves
//! anchors/offsets live, rasterizes the resulting draw commands with the
//! software renderer, and asserts that each control produced themed pixels in
//! the framebuffer.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{capture_frame, pixel_at};
use gdrender2d::texture::Texture2D;
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

fn add(tree: &mut SceneTree, parent: NodeId, name: &str, class: &str) -> NodeId {
    tree.add_child(parent, Node::new(name, class)).unwrap()
}

fn set_prop(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

/// Positions a control top-left with an explicit local rect.
fn place(tree: &mut SceneTree, id: NodeId, l: f32, t: f32, r: f32, b: f32) {
    control::apply_anchor_preset(tree, id, AnchorPreset::TopLeft);
    control::set_offset_left(tree, id, l);
    control::set_offset_top(tree, id, t);
    control::set_offset_right(tree, id, r);
    control::set_offset_bottom(tree, id, b);
}

fn close(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() <= tol && (a.g - b.g).abs() <= tol && (a.b - b.b).abs() <= tol
}

/// Returns true if any pixel in the (inclusive) rect matches `color`.
fn region_has(fb: &FrameBuffer, x0: u32, y0: u32, x1: u32, y1: u32, color: Color, tol: f32) -> bool {
    for y in y0..=y1 {
        for x in x0..=x1 {
            if close(pixel_at(fb, x, y), color, tol) {
                return true;
            }
        }
    }
    false
}

#[test]
fn control_tree_renders_themed_pixels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Root Control fills the 200x200 viewport.
    let ui = add(&mut tree, root, "UI", "Control");
    control::apply_anchor_preset(&mut tree, ui, AnchorPreset::FullRect);

    // Panel across the top half; ColorRect nested inside it.
    let panel = add(&mut tree, ui, "Panel", "Panel");
    place(&mut tree, panel, 0.0, 0.0, 200.0, 100.0);
    let cr = add(&mut tree, panel, "Rect", "ColorRect");
    place(&mut tree, cr, 10.0, 10.0, 60.0, 60.0); // local → global (10,10)-(60,60)
    set_prop(&mut tree, cr, "color", Variant::Color(Color::rgb(1.0, 0.0, 0.0)));

    // Button with centered text.
    let btn = add(&mut tree, ui, "Btn", "Button");
    place(&mut tree, btn, 10.0, 120.0, 110.0, 160.0);
    set_prop(&mut tree, btn, "text", Variant::String("OK".to_owned()));

    // Label with text.
    let lbl = add(&mut tree, ui, "Lbl", "Label");
    place(&mut tree, lbl, 10.0, 170.0, 110.0, 190.0);
    set_prop(&mut tree, lbl, "text", Variant::String("HI".to_owned()));

    // TextureRect referencing a registered texture.
    let tex = add(&mut tree, ui, "Tex", "TextureRect");
    place(&mut tree, tex, 120.0, 120.0, 180.0, 180.0);
    set_prop(&mut tree, tex, "texture", Variant::String("icon.png".to_owned()));

    // Render the tree into draw commands, then rasterize.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("icon.png", Texture2D::solid(4, 4, Color::rgb(0.0, 0.0, 1.0)));
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, ui, &theme, &mut vp, Vector2::new(200.0, 200.0));
    let fb = capture_frame(&mut renderer, &vp);

    let tol = 0.03;

    // Panel background (themed StyleBox flat = 0.15 gray), on the top half
    // where the ColorRect does not cover it.
    assert!(
        close(pixel_at(&fb, 150, 50), Color::new(0.15, 0.15, 0.15, 1.0), tol),
        "Panel StyleBox bg should fill its rect, got {:?}",
        pixel_at(&fb, 150, 50)
    );

    // ColorRect draws its property color (red), on top of the panel.
    assert!(
        close(pixel_at(&fb, 30, 30), Color::rgb(1.0, 0.0, 0.0), tol),
        "ColorRect should draw its color over the panel, got {:?}",
        pixel_at(&fb, 30, 30)
    );

    // Button background (themed normal StyleBox = 0.2 gray), away from centered text.
    assert!(
        close(pixel_at(&fb, 15, 155), Color::new(0.2, 0.2, 0.2, 1.0), tol),
        "Button StyleBox bg should fill its rect, got {:?}",
        pixel_at(&fb, 15, 155)
    );

    // Button text: themed font_color (~0.875 gray) appears somewhere in the button.
    assert!(
        region_has(&fb, 10, 120, 109, 159, Color::new(0.875, 0.875, 0.875, 1.0), tol),
        "Button should render themed font_color text pixels"
    );

    // Label text: themed font_color (white) appears somewhere in the label rect.
    assert!(
        region_has(&fb, 10, 170, 109, 189, Color::WHITE, tol),
        "Label should render themed font_color text pixels"
    );

    // TextureRect draws the registered texture (blue), tinted by white modulate.
    assert!(
        close(pixel_at(&fb, 150, 150), Color::rgb(0.0, 0.0, 1.0), tol),
        "TextureRect should draw its texture, got {:?}",
        pixel_at(&fb, 150, 150)
    );
}

#[test]
fn anchors_resolved_live_fullrect_fills_viewport() {
    // A single ColorRect with a FullRect preset must fill the whole viewport,
    // proving anchors/offsets are resolved during the render pass (not baked).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cr = add(&mut tree, root, "Full", "ColorRect");
    control::apply_anchor_preset(&mut tree, cr, AnchorPreset::FullRect);
    set_prop(&mut tree, cr, "color", Variant::Color(Color::rgb(0.0, 1.0, 0.0)));

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(48, 48, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, cr, &theme, &mut vp, Vector2::new(48.0, 48.0));
    let fb = capture_frame(&mut renderer, &vp);

    // Corners and center should all be green.
    for (x, y) in [(0, 0), (47, 0), (0, 47), (47, 47), (24, 24)] {
        assert!(
            close(pixel_at(&fb, x, y), Color::rgb(0.0, 1.0, 0.0), 0.03),
            "FullRect ColorRect should fill ({x},{y}), got {:?}",
            pixel_at(&fb, x, y)
        );
    }
}
