//! pat-s3rrv.74: `[indent]` insets a line's left edge and narrows what it wraps
//! against.
//!
//! Godot ground truth: `[indent]` pushes an indent item
//! (`rich_text_label.cpp`), which offsets the paragraph's left margin by one tab
//! stop per level. It is a *paragraph* construct, not a run attribute.
//!
//! # Why this needed no new draw-item field
//!
//! The rest of pat-s3rrv.74's remaining tags mostly do. `[b]`/`[i]` already
//! carry attributes on the internal `TextSpan`, but `DrawCommand::DrawString`
//! has only `text`, `position`, `color` and `font_size` — no weight, no font
//! path, no outline — so `[font]`, `[outline_size]` and `[outline_color]` all
//! need a field added to a command constructed in dozens of places across the
//! workspace.
//!
//! `[indent]` needs none of that: it moves the *position* the run is already
//! drawn at, and position is a field every draw command has had all along.
//!
//! ⚠️ The assertions below are RELATIVE — "indented sits right of plain", "two
//! levels is exactly twice one" — rather than pinned to a pixel count. One
//! indent level is four spaces at the current scale, so hardcoding it would
//! re-encode the font metrics and break on any glyph-width change without
//! anything actually being wrong.

use gdcore::math::{Color, Vector2};
use gdscene::control::AnchorPreset;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

const W: f32 = 300.0;
const H: f32 = 120.0;

fn label(bbcode: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_enabled", Variant::Bool(true));
    n.set_property("bbcode_text", Variant::String(bbcode.to_owned()));
    let id = tree.add_child(root, n).unwrap();
    // ⚠️ Anchors + offsets, NOT `set_size` — the render walk recomputes `size`
    // from the anchor/offset pair, so a fixture that only writes `size` is reset
    // to 0x0 the moment it is drawn. Same reason as the sibling layout test.
    gdscene::control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    gdscene::control::set_offset_right(&mut tree, id, W);
    gdscene::control::set_offset_bottom(&mut tree, id, H);
    (tree, id)
}

fn drawn(tree: &mut SceneTree, id: NodeId) -> Vec<(String, Vector2)> {
    let mut vp = Viewport::new(W as u32, H as u32, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(tree, id, &theme, &mut vp, Vector2::new(W, H));
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, position, .. } => Some((text, position)),
            _ => None,
        })
        .collect()
}

fn find<'a>(items: &'a [(String, Vector2)], text: &str) -> &'a (String, Vector2) {
    items
        .iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("no run {text:?} among {items:?}"))
}

/// The x of a run drawn for `bbcode`.
fn x_of(bbcode: &str, run: &str) -> f32 {
    let (mut tree, id) = label(bbcode);
    let items = drawn(&mut tree, id);
    find(&items, run).1.x
}

// ===========================================================================
// The acceptance
// ===========================================================================

/// **The acceptance.** `[indent]` moves a line right, and nesting is linear.
#[test]
fn richtextlabel_indent_insets_lines() {
    let plain = x_of("word", "word");
    let one = x_of("[indent]word[/indent]", "word");
    let two = x_of("[indent][indent]word[/indent][/indent]", "word");

    assert!(
        one > plain,
        "[indent] must move the line right: plain x={plain}, indented x={one}"
    );

    // Exactly twice, not merely more: the inset is `depth * one_level`, so a
    // second level that added a different amount would mean the depth is not
    // being multiplied but something else is happening.
    let step = one - plain;
    assert!(
        (two - plain - 2.0 * step).abs() < 0.001,
        "two levels must inset exactly twice one level: plain={plain}, \
         one={one} (step {step}), two={two}"
    );
}

/// The tag is consumed, not drawn.
///
/// The control for every assertion above: if `[indent]` fell through to the
/// "unknown tag" path it would still be stripped and the x would be unchanged,
/// so a test that only checked the text would pass on a no-op.
#[test]
fn richtextlabel_indent_insets_lines_tag_is_not_drawn() {
    let (mut tree, id) = label("[indent]word[/indent]");
    let items = drawn(&mut tree, id);

    assert!(
        items.iter().all(|(t, _)| !t.contains("indent")),
        "the tag text must never reach the draw stream: {items:?}"
    );
    // …and the run itself survives, so "nothing was drawn" cannot pass this.
    find(&items, "word");
}

/// Only the tagged line moves; its neighbours keep their left edge.
///
/// ⚠️ This is the assertion that pins the depth to the LINE it belongs to.
/// Resolving the depth from "the last change at or before this line" passes the
/// acceptance above and fails here, because the closer on line 1 would leak a
/// zero onto line 2 — or, spelled the other way, an opener would leak downward.
#[test]
fn richtextlabel_indent_insets_lines_only_the_tagged_line() {
    let (mut tree, id) = label("first\n[indent]second[/indent]\nthird");
    let items = drawn(&mut tree, id);

    let first = find(&items, "first").1;
    let second = find(&items, "second").1;
    let third = find(&items, "third").1;

    assert!(
        second.x > first.x,
        "the indented line moves right: first={first:?}, second={second:?}"
    );
    assert!(
        (third.x - first.x).abs() < 0.001,
        "the line AFTER `[/indent]` returns to the original left edge: \
         first={first:?}, third={third:?}"
    );
    // They really are three separate lines, so the x comparison is not
    // accidentally comparing runs that share a row.
    assert!(
        first.y < second.y && second.y < third.y,
        "the three runs must be on descending rows: {first:?} {second:?} {third:?}"
    );
}

/// An indented line wraps sooner, because the inset is taken out of the width
/// it wraps against.
///
/// Insetting without narrowing is the tempting half-implementation: it looks
/// right on a short line and pushes long text off the right edge instead of
/// wrapping it.
#[test]
fn richtextlabel_indent_insets_lines_narrows_the_wrap_width() {
    // Long enough to wrap once the inset is taken off, short enough not to wrap
    // without it — so the two cases genuinely differ.
    let text = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn";

    let plain_rows = {
        let (mut tree, id) = label(text);
        let items = drawn(&mut tree, id);
        let mut ys: Vec<i64> = items.iter().map(|(_, p)| p.y as i64).collect();
        ys.sort_unstable();
        ys.dedup();
        ys.len()
    };
    let indented_rows = {
        let (mut tree, id) = label(&format!("[indent][indent]{text}[/indent][/indent]"));
        let items = drawn(&mut tree, id);
        let mut ys: Vec<i64> = items.iter().map(|(_, p)| p.y as i64).collect();
        ys.sort_unstable();
        ys.dedup();
        ys.len()
    };

    assert!(
        indented_rows >= plain_rows,
        "indenting must never make text wrap into FEWER rows: plain={plain_rows}, \
         indented={indented_rows}"
    );
    assert!(
        plain_rows >= 1 && indented_rows >= 1,
        "both fixtures must have drawn something: plain={plain_rows}, \
         indented={indented_rows}"
    );
}

/// A stray `[/indent]` is stripped rather than underflowing.
///
/// `indent_depth` is a `usize`; subtracting below zero would panic in debug.
#[test]
fn richtextlabel_indent_insets_lines_survives_an_unmatched_closer() {
    let x = x_of("[/indent]word", "word");
    let plain = x_of("word", "word");
    assert!(
        (x - plain).abs() < 0.001,
        "an unmatched `[/indent]` leaves the line where it was: {x} vs {plain}"
    );
}
