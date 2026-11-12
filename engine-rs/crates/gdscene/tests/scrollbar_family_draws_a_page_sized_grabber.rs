//! pat-s3rrv.70 — the ScrollBar half of the Range family draws.
//!
//! `HScrollBar` / `VScrollBar` were registered in ClassDB and present in both
//! `CONTROL_HIT_CLASSES` and `FOCUSABLE_BY_DEFAULT`, so a `.tscn` scrollbar was
//! clickable and tab-focusable — and **drew nothing at all**. They were absent
//! from `control_render.rs`'s `CONTROL_CLASSES`, the membership list that gates
//! the draw arm, while `theme.rs` had shipped their `scroll` stylebox since
//! pat-nzgdu with no consumer.
//!
//! ⚠️ The detail that separates a scrollbar from a slider: its grabber is sized
//! by `page`, not fixed. That length is the whole readable signal — it tells the
//! user how much of the content is visible. Drawing a slider-style fixed handle
//! would look completely plausible and convey nothing.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::range_controls::{scrollbar_grabber_rect, SliderAxis, GRABBER_EXTENT};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

fn control_node(class: &str, w: f32, h: f32, props: &[(&str, Variant)]) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("C", class);
    for (k, v) in props {
        n.set_property(k, v.clone());
    }
    let id = tree.add_child(root, n).unwrap();
    control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, id, w);
    control::set_offset_bottom(&mut tree, id, h);
    (tree, id)
}

fn draw(class: &str, w: f32, h: f32, props: &[(&str, Variant)]) -> Vec<DrawCommand> {
    let (mut tree, id) = control_node(class, w, h, props);
    let mut vp = Viewport::new(256, 128, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(256.0, 128.0));
    vp.get_sorted_items()
        .first()
        .map(|it| it.commands.clone())
        .unwrap_or_default()
}

fn filled_rects(cmds: &[DrawCommand]) -> Vec<(Vector2, Vector2)> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect, filled: true, ..
            } => Some((rect.position, rect.size)),
            _ => None,
        })
        .collect()
}

/// **The acceptance test**: a scrollbar emits a track and a grabber.
#[test]
fn scrollbar_family_draws_a_page_sized_grabber() {
    for class in ["HScrollBar", "VScrollBar"] {
        let cmds = draw(
            class,
            100.0,
            100.0,
            &[
                ("min_value", Variant::Float(0.0)),
                ("max_value", Variant::Float(100.0)),
                ("page", Variant::Float(25.0)),
                ("value", Variant::Float(0.0)),
            ],
        );
        let rects = filled_rects(&cmds);
        assert!(
            rects.len() >= 2,
            "{class} must draw a track AND a grabber; got {} filled rect(s). \
             Zero means the class is still missing from CONTROL_CLASSES and the \
             draw arm was never reached",
            rects.len(),
        );
    }
}

/// The grabber's length tracks `page` (pat-s3rrv.70).
///
/// This is the assertion that separates a real scrollbar from a slider wearing
/// a scrollbar's name. A quarter-page bar shows a quarter-length handle.
#[test]
fn the_grabber_length_reflects_the_page() {
    // Geometry is asserted through the shared helper, which is what the draw arm
    // calls — so this cannot pass while the rendered handle disagrees.
    let quarter = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        0.0,
        25.0,
        0.0,
        100.0,
    );
    let half = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        0.0,
        50.0,
        0.0,
        100.0,
    );
    assert!(
        half.size.x > quarter.size.x,
        "a larger page must give a longer grabber: page 50 -> {}, page 25 -> {}",
        half.size.x,
        quarter.size.x,
    );

    // Godot: grabber = page/range * area + grabber_min, area = extent - grabber_min.
    let area = 100.0 - GRABBER_EXTENT;
    let expected = 0.25 * area + GRABBER_EXTENT;
    assert!(
        (quarter.size.x - expected).abs() < 1e-3,
        "expected {expected}, got {}",
        quarter.size.x,
    );

    // ⚠️ With no page at all the grabber collapses to the minimum rather than
    // vanishing — Godot's behaviour, and what keeps a scrollbar authored
    // without a page still usable.
    let none = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        0.0,
        0.0,
        0.0,
        100.0,
    );
    assert!(
        (none.size.x - GRABBER_EXTENT).abs() < 1e-3,
        "a page-less scrollbar keeps a minimum grabber, got {}",
        none.size.x,
    );
}

/// The grabber travels with the value, and a full page cannot move
/// (pat-s3rrv.70).
#[test]
fn the_grabber_travels_and_a_full_page_cannot() {
    let at_start = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        0.0,
        25.0,
        0.0,
        100.0,
    );
    let at_end = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        1.0,
        25.0,
        0.0,
        100.0,
    );
    assert!(
        at_end.position.x > at_start.position.x,
        "ratio 1.0 must sit further along than ratio 0.0",
    );
    assert!(
        at_end.position.x + at_end.size.x <= 100.0 + 1e-3,
        "the grabber must not run past the end of the track: {:?}",
        at_end,
    );

    // A bar showing the whole content has nowhere to scroll.
    let full = scrollbar_grabber_rect(
        Vector2::new(100.0, 12.0),
        SliderAxis::Horizontal,
        1.0,
        100.0,
        0.0,
        100.0,
    );
    assert!(
        (full.size.x - 100.0).abs() < 1e-3 && full.position.x.abs() < 1e-3,
        "a full-page grabber fills the track and cannot travel: {full:?}",
    );
}

/// A vertical scrollbar puts ratio 0 at the BOTTOM (pat-s3rrv.70).
///
/// The same inversion the VSlider carries. Reusing the horizontal mapping gives
/// a bar that works perfectly and runs backwards, which "the value moved" style
/// assertions do not catch.
#[test]
fn a_vertical_scrollbar_is_inverted_like_a_vslider() {
    let low = scrollbar_grabber_rect(
        Vector2::new(12.0, 100.0),
        SliderAxis::Vertical,
        0.0,
        25.0,
        0.0,
        100.0,
    );
    let high = scrollbar_grabber_rect(
        Vector2::new(12.0, 100.0),
        SliderAxis::Vertical,
        1.0,
        25.0,
        0.0,
        100.0,
    );
    assert!(
        low.position.y > high.position.y,
        "ratio 0 sits BELOW ratio 1 on a vertical bar: low {:?}, high {:?}",
        low.position,
        high.position,
    );
    assert!(
        high.position.y.abs() < 1e-3,
        "ratio 1 reaches the top, got {}",
        high.position.y,
    );
}
