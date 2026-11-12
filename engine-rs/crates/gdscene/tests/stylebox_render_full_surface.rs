//! pat-0q2kx.148 — the StyleBox surface reaches rendered pixels and layout.
//!
//! The bead's three acceptance clauses:
//!   1. a `StyleBoxFlat` with `corner_radius_top_left=12` and the other three 0
//!      renders **one** rounded corner and three square ones;
//!   2. a `StyleBoxEmpty` with `content_margin_left=8` shifts a panel child's
//!      layout by 8px;
//!   3. a `StyleBoxTexture` with `axis_stretch_horizontal=Tile` **repeats** the
//!      texture instead of stretching it.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{capture_frame, pixel_at};
use gdrender2d::texture::Texture2D;
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::stylebox::{AxisStretchMode, StyleBox, StyleBoxEmpty, StyleBoxFlat, StyleBoxTexture};
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

/// **Clause 1.** Per-corner radii are independent.
///
/// This is the regression that matters: the old `radius()` folded all four
/// corners to their `max`, so this box rendered with *four* 12px roundings. A
/// test that only checked the rounded corner would have passed against that bug
/// — the three square corners are the assertion that catches it.
#[test]
fn a_single_corner_radius_rounds_only_that_corner() {
    let (mut tree, panel) = control_tree("Panel");

    let bg = Color::new(0.2, 0.5, 0.9, 1.0);
    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Panel",
        "panel",
        StyleBox::Flat(StyleBoxFlat {
            bg_color: bg,
            corner_radius: [12.0, 0.0, 0.0, 0.0],
            ..Default::default()
        }),
    );

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    render_control_tree(
        &mut tree,
        panel,
        &theme,
        &mut vp,
        Vector2::new(100.0, 100.0),
    );
    let fb = capture_frame(&mut renderer, &vp);

    assert!(
        close(pixel_at(&fb, 50, 50), bg, 0.03),
        "the interior must still be filled, got {:?}",
        pixel_at(&fb, 50, 50)
    );
    assert!(
        close(pixel_at(&fb, 0, 0), Color::BLACK, 0.03),
        "top-left has radius 12 and must be rounded away, got {:?}",
        pixel_at(&fb, 0, 0)
    );
    for (x, y, name) in [
        (99, 0, "top-right"),
        (99, 99, "bottom-right"),
        (0, 99, "bottom-left"),
    ] {
        assert!(
            close(pixel_at(&fb, x, y), bg, 0.03),
            "{name} has radius 0 and must stay square; folding the four radii to \
             their max would round it. got {:?}",
            pixel_at(&fb, x, y)
        );
    }
}

/// **Clause 2.** `StyleBoxEmpty`'s content margins move a child.
///
/// `StyleBoxEmpty` draws nothing at all, so padding is its entire reason to
/// exist — and its margins were parsed and stored but read by nothing until this
/// bead. The child's *position* is the only observable, which is why this clause
/// asserts layout rather than pixels.
#[test]
fn empty_stylebox_content_margin_shifts_the_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree
        .add_child(root, Node::new("PanelContainer", "PanelContainer"))
        .unwrap();
    control::apply_anchor_preset(&mut tree, panel, AnchorPreset::FullRect);
    let child = tree.add_child(panel, Node::new("Panel", "Child")).unwrap();

    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "PanelContainer",
        "panel",
        StyleBox::Empty(StyleBoxEmpty {
            content_margin: [8.0, 0.0, 0.0, 0.0],
        }),
    );

    let mut vp = Viewport::new(100, 100, Color::BLACK);
    render_control_tree(
        &mut tree,
        panel,
        &theme,
        &mut vp,
        Vector2::new(100.0, 100.0),
    );

    let pos = control::get_position(&tree, child);
    let size = control::get_size(&tree, child);
    assert_eq!(
        pos.x, 8.0,
        "content_margin_left=8 must inset the child by 8px, got {pos:?}"
    );
    assert_eq!(pos.y, 0.0, "only the left margin was set");
    assert_eq!(
        size.x, 92.0,
        "the child loses exactly the inset width, not the full margin on both \
         sides; got {size:?}"
    );
    assert_eq!(size.y, 100.0, "height is untouched by a left margin");
}

/// Margins wider than the container collapse the child rather than inverting it.
/// A negative size would make hit-testing select the outside of the panel.
#[test]
fn oversized_margins_collapse_rather_than_invert() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree
        .add_child(root, Node::new("PanelContainer", "PanelContainer"))
        .unwrap();
    let child = tree.add_child(panel, Node::new("Panel", "Child")).unwrap();

    control::arrange_panel_container(
        &mut tree,
        panel,
        Vector2::new(20.0, 20.0),
        [30.0, 30.0, 30.0, 30.0],
    );

    let size = control::get_size(&tree, child);
    assert!(
        size.x >= 0.0 && size.y >= 0.0,
        "an over-inset child must collapse to zero, never invert; got {size:?}"
    );
}

/// **Clause 3.** `Tile` repeats the source instead of stretching it.
///
/// ⚠️ The texture is **striped, not solid**. Tiling a solid colour is pixel-wise
/// identical to stretching it, so a solid-texture test would pass against a
/// renderer that ignores the mode entirely — it would assert nothing at all.
/// Counting stripe transitions across the row is what distinguishes the two.
#[test]
fn tile_axis_stretch_repeats_the_texture() {
    let (mut tree, panel) = control_tree("Panel");

    let mut theme = Theme::new("t");
    theme.set_stylebox(
        "Panel",
        "panel",
        StyleBox::Texture(StyleBoxTexture {
            texture_path: "striped.png".into(),
            texture_margin: [0.0; 4],
            axis_h: AxisStretchMode::Tile,
            ..Default::default()
        }),
    );

    // 8x8, left half red / right half green: one transition per repetition.
    let mut tex = Texture2D::solid(8, 8, Color::rgb(1.0, 0.0, 0.0));
    for y in 0..8 {
        for x in 4..8 {
            tex.pixels[(y * 8 + x) as usize] = Color::rgb(0.0, 1.0, 0.0);
        }
    }

    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("striped.png", tex);
    let mut vp = Viewport::new(64, 16, Color::BLACK);
    render_control_tree(&mut tree, panel, &theme, &mut vp, Vector2::new(64.0, 16.0));
    let fb = capture_frame(&mut renderer, &vp);

    // Walk one row and count red->green / green->red flips.
    let mut transitions = 0;
    let mut prev_is_red = pixel_at(&fb, 0, 8).r > pixel_at(&fb, 0, 8).g;
    for x in 1..64 {
        let p = pixel_at(&fb, x, 8);
        let is_red = p.r > p.g;
        if is_red != prev_is_red {
            transitions += 1;
        }
        prev_is_red = is_red;
    }
    assert!(
        transitions > 1,
        "Tile must repeat the 8px source across 64px — a stretch yields exactly \
         one red->green transition, tiling yields several. got {transitions}"
    );
}

/// The bead's named acceptance, running all three clauses together.
#[test]
fn stylebox_render_full_surface() {
    a_single_corner_radius_rounds_only_that_corner();
    empty_stylebox_content_margin_shifts_the_child();
    tile_axis_stretch_repeats_the_texture();
}
