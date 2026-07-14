//! pat-u3l8c.70 — StyleBox resource family, framebuffer + parse tests.
//!
//! Before this bead NO StyleBox classes existed and the theme stored styleboxes
//! as bare `Variant::Color`. These tests drive a typed StyleBox through the
//! Control render pass into a framebuffer: a `StyleBoxFlat` with a border and
//! rounded corners renders a bordered panel whose corners are clipped away, and
//! a `StyleBoxTexture` renders its nine-patch texture. A `.tscn` StyleBox
//! sub-resource is parsed into a Theme.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{capture_frame, pixel_at};
use gdrender2d::texture::Texture2D;
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::stylebox::{StyleBox, StyleBoxFlat, StyleBoxTexture};
use gdscene::theme::Theme;
use gdserver2d::viewport::Viewport;

fn close(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() <= tol && (a.g - b.g).abs() <= tol && (a.b - b.b).abs() <= tol
}

fn panel_tree() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree.add_child(root, Node::new("Panel", "Panel")).unwrap();
    control::apply_anchor_preset(&mut tree, panel, AnchorPreset::FullRect);
    (tree, panel)
}

#[test]
fn styleboxflat_rounded_bordered_panel_renders() {
    let (mut tree, panel) = panel_tree();

    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Panel",
        "panel",
        StyleBox::Flat(StyleBoxFlat {
            bg_color: Color::new(0.2, 0.2, 0.2, 1.0),
            border_width: [4.0; 4],
            border_color: Color::WHITE,
            corner_radius: [12.0; 4],
            ..Default::default()
        }),
    );

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(100.0, 100.0));
    let fb = capture_frame(&mut renderer, &vp);

    // Center is the flat background.
    assert!(
        close(pixel_at(&fb, 50, 50), Color::new(0.2, 0.2, 0.2, 1.0), 0.03),
        "center should be bg color, got {:?}",
        pixel_at(&fb, 50, 50)
    );
    // Top and left edge centers show the white border frame.
    assert!(
        close(pixel_at(&fb, 50, 1), Color::WHITE, 0.03),
        "top border should be white, got {:?}",
        pixel_at(&fb, 50, 1)
    );
    assert!(
        close(pixel_at(&fb, 1, 50), Color::WHITE, 0.03),
        "left border should be white, got {:?}",
        pixel_at(&fb, 1, 50)
    );
    // The extreme top-left corner is rounded away → still the clear color.
    assert!(
        close(pixel_at(&fb, 0, 0), Color::BLACK, 0.03),
        "rounded corner should be clipped (clear), got {:?}",
        pixel_at(&fb, 0, 0)
    );
}

#[test]
fn styleboxtexture_nine_patch_renders() {
    let (mut tree, panel) = panel_tree();

    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Panel",
        "panel",
        StyleBox::Texture(StyleBoxTexture {
            texture_path: "panel.png".into(),
            texture_margin: [4.0; 4],
            ..Default::default()
        }),
    );

    let mut renderer = SoftwareRenderer::new();
    // 16x16 texture so the nine-patch center source region (16 - 4 - 4 = 8px)
    // is non-empty with 4px margins; an 8px texture would leave no center.
    renderer.register_texture("panel.png", Texture2D::solid(16, 16, Color::rgb(0.0, 0.0, 1.0)));
    let mut vp = Viewport::new(64, 64, Color::BLACK);
    render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(64.0, 64.0));
    let fb = capture_frame(&mut renderer, &vp);

    // The nine-patch (all-blue texture) fills the panel with blue.
    assert!(
        close(pixel_at(&fb, 32, 32), Color::rgb(0.0, 0.0, 1.0), 0.03),
        "nine-patch center should draw the texture, got {:?}",
        pixel_at(&fb, 32, 32)
    );
    assert!(
        close(pixel_at(&fb, 2, 2), Color::rgb(0.0, 0.0, 1.0), 0.03),
        "nine-patch corner should draw the texture, got {:?}",
        pixel_at(&fb, 2, 2)
    );
}

#[test]
fn theme_loads_styleboxes_from_tscn() {
    let tscn = r#"
[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="panel"]
bg_color = Color(0.1, 0.15, 0.2, 1)
border_width_left = 3
corner_radius_top_left = 5

[sub_resource type="StyleBoxLine" id="separator"]
color = Color(0.4, 0.4, 0.4, 1)
thickness = 2
vertical = true

[resource]
"#;
    let mut theme = Theme::new("parsed");
    theme.load_styleboxes_from_tscn("Panel", tscn);

    assert!(theme.has_stylebox("Panel", "panel"), "flat stylebox parsed");
    assert!(theme.has_stylebox("Panel", "separator"), "line stylebox parsed");
    match theme.get_stylebox("Panel", "panel") {
        Some(StyleBox::Flat(f)) => {
            assert!((f.bg_color.b - 0.2).abs() < 1e-5);
            assert!((f.border_width[0] - 3.0).abs() < 1e-5);
            assert!((f.corner_radius[0] - 5.0).abs() < 1e-5);
        }
        other => panic!("expected StyleBoxFlat, got {other:?}"),
    }

    // The parsed flat stylebox renders into a framebuffer via the render pass.
    let (mut tree, panel) = panel_tree();
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(40, 40, Color::BLACK);
    render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(40.0, 40.0));
    let fb: FrameBuffer = capture_frame(&mut renderer, &vp);
    assert!(
        close(pixel_at(&fb, 20, 20), Color::new(0.1, 0.15, 0.2, 1.0), 0.03),
        "parsed StyleBoxFlat bg should render, got {:?}",
        pixel_at(&fb, 20, 20)
    );
}
