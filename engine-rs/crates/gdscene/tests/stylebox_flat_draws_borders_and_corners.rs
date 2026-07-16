//! pat-nzgdu.90 — acceptance: StyleBox resources render bordered/rounded/
//! nine-patch backgrounds through the Control render pass, and the StyleBox
//! class family is registered in ClassDB.
//!
//! The StyleBox types + drawing + theme consumption already exist (pat-u3l8c.70);
//! this test pins the bead's named acceptance contract:
//!   - a `Panel` with a `StyleBoxFlat` (bg_color, border_width>0, corner_radius>0)
//!     renders the fill color inside, a distinct border color on the edges, and
//!     rounded (non-square) corners;
//!   - a `Button` with a `StyleBoxTexture` renders its nine-patch texture;
//!   - `StyleBoxFlat`/`StyleBoxTexture`/… resolve through ClassDB to the
//!     `StyleBox -> Resource` parent chain (pat-nzgdu.90 class registration).

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
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

fn control_tree(class: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree.add_child(root, Node::new(class, class)).unwrap();
    control::apply_anchor_preset(&mut tree, node, AnchorPreset::FullRect);
    (tree, node)
}

/// Acceptance (a): a Panel with a StyleBoxFlat renders the fill inside, a
/// distinct border color on the edges, and rounded (clipped) corners.
#[test]
fn stylebox_flat_draws_borders_and_corners() {
    let (mut tree, panel) = control_tree("Panel");

    let bg = Color::new(0.15, 0.35, 0.6, 1.0);
    let border = Color::new(0.95, 0.85, 0.1, 1.0);
    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Panel",
        "panel",
        StyleBox::Flat(StyleBoxFlat {
            bg_color: bg,
            border_width: [5.0; 4],
            border_color: border,
            corner_radius: [14.0; 4],
            ..Default::default()
        }),
    );

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(100.0, 100.0));
    let fb = capture_frame(&mut renderer, &vp);

    // Fill color shows through the interior.
    assert!(
        close(pixel_at(&fb, 50, 50), bg, 0.03),
        "center should be the StyleBoxFlat fill color, got {:?}",
        pixel_at(&fb, 50, 50)
    );
    // A distinct border color shows on the top and left edge centers.
    assert!(
        close(pixel_at(&fb, 50, 1), border, 0.03),
        "top edge should show the border color, got {:?}",
        pixel_at(&fb, 50, 1)
    );
    assert!(
        close(pixel_at(&fb, 1, 50), border, 0.03),
        "left edge should show the border color, got {:?}",
        pixel_at(&fb, 1, 50)
    );
    // Border and fill are visibly different (not a plain square fill).
    assert!(
        !close(pixel_at(&fb, 50, 1), bg, 0.03),
        "border must be distinct from the fill color"
    );
    // The extreme corner is rounded away → still the clear color (non-square).
    assert!(
        close(pixel_at(&fb, 0, 0), Color::BLACK, 0.03),
        "rounded corner should be clipped to the clear color, got {:?}",
        pixel_at(&fb, 0, 0)
    );
}

/// Acceptance (b): a Button with a StyleBoxTexture renders its nine-patch.
#[test]
fn stylebox_texture_button_nine_patch() {
    let (mut tree, button) = control_tree("Button");

    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Button",
        "normal",
        StyleBox::Texture(StyleBoxTexture {
            texture_path: "btn.png".into(),
            texture_margin: [4.0; 4],
            ..Default::default()
        }),
    );

    let mut renderer = SoftwareRenderer::new();
    // 16x16 texture so the nine-patch center source region (16-4-4=8px) is
    // non-empty with 4px margins.
    renderer.register_texture("btn.png", Texture2D::solid(16, 16, Color::rgb(0.0, 0.8, 0.2)));
    let mut vp = Viewport::new(64, 64, Color::BLACK);
    render_control_tree(&mut tree, button, &theme, &mut vp, Vector2::new(64.0, 64.0));
    let fb = capture_frame(&mut renderer, &vp);

    assert!(
        close(pixel_at(&fb, 32, 32), Color::rgb(0.0, 0.8, 0.2), 0.03),
        "nine-patch center should draw the texture, got {:?}",
        pixel_at(&fb, 32, 32)
    );
    assert!(
        close(pixel_at(&fb, 2, 2), Color::rgb(0.0, 0.8, 0.2), 0.03),
        "nine-patch corner region should draw the texture, got {:?}",
        pixel_at(&fb, 2, 2)
    );
}

/// pat-nzgdu.90 class registration: the StyleBox family resolves through ClassDB
/// to the `StyleBox -> Resource` parent chain, so `[sub_resource type="…"]`
/// theme entries classify correctly.
#[test]
fn stylebox_classes_registered_in_classdb() {
    gdobject::register_2d_classes();
    assert!(
        gdobject::is_parent_class("StyleBoxFlat", "StyleBox"),
        "StyleBoxFlat should inherit StyleBox"
    );
    assert!(
        gdobject::is_parent_class("StyleBoxFlat", "Resource"),
        "StyleBoxFlat should inherit Resource through StyleBox"
    );
    assert!(
        gdobject::is_parent_class("StyleBoxTexture", "Resource"),
        "StyleBoxTexture should inherit Resource"
    );
    assert!(
        gdobject::is_parent_class("StyleBoxLine", "StyleBox"),
        "StyleBoxLine should inherit StyleBox"
    );
    assert!(
        gdobject::is_parent_class("StyleBoxEmpty", "StyleBox"),
        "StyleBoxEmpty should inherit StyleBox"
    );
}
