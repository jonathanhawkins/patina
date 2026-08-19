//! pat-s3rrv.74: RichTextLabel lines and paragraph alignment.
//!
//! Godot ground truth: `[center]`, `[right]`, `[left]` and `[fill]` each call
//! `push_paragraph` (`rich_text_label.cpp:5675-5688`), so they start a new
//! PARAGRAPH — `a[center]mid[/center]b` is three paragraphs, not one line with a
//! centred middle.
//!
//! ⚠️ Two gaps closed here, and the second is the reason the first was worth
//! doing properly:
//!
//! 1. The draw arm never broke lines at all. It advanced `x` and never `y`, so a
//!    label containing `\n` drew every line on top of the first and the newline
//!    reached the bitmap font as an unrenderable glyph.
//! 2. The arm and the `[url]` hit-test each advanced `x` themselves. That agreed
//!    only by accident on one line, and would have diverged the instant lines
//!    existed — a link on line 2 clickable on line 1. Both now read ONE layout
//!    (`richtext_placements`), so the last test here is the one that matters:
//!    it asserts a link is clickable on the row it is painted on.

use gdcore::math::{Color, Rect2, Vector2};
use gdscene::control::AnchorPreset;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

const W: f32 = 300.0;
const H: f32 = 80.0;

/// Line advance: `(GLYPH_H + GLYPH_GAP) * scale`, with scale = font_size / 7.
const LINE_H: f32 = (7.0 + 1.0) * 2.0;

fn label(bbcode: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_enabled", Variant::Bool(true));
    n.set_property("bbcode_text", Variant::String(bbcode.to_owned()));
    let id = tree.add_child(root, n).unwrap();
    // ⚠️ Anchors + offsets, NOT `set_size`. The render walk runs
    // `resolve_control_layout`, which RECOMPUTES `size` from the anchor/offset
    // pair — a fixture that only writes `size` is silently reset to 0x0 the
    // moment it is drawn, and every position asserted below would be measured
    // against a zero-width rect (centring in nothing lands at x = 0, which
    // looks like a plain off-by-something rather than a dead fixture).
    gdscene::control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    gdscene::control::set_offset_right(&mut tree, id, W);
    gdscene::control::set_offset_bottom(&mut tree, id, H);
    (tree, id)
}

/// Every drawn string with its position.
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

// ---------------------------------------------------------------------------

/// A literal `\n` starts a new line.
#[test]
fn a_newline_starts_a_new_line() {
    let (mut tree, id) = label("top\nbottom");
    let items = drawn(&mut tree, id);

    let top = find(&items, "top").1;
    let bottom = find(&items, "bottom").1;

    assert_eq!(top.x, bottom.x, "both lines start at the same left edge");
    assert!(
        (bottom.y - top.y - LINE_H).abs() < 1e-3,
        "the second line must sit one line-height below the first — {} vs {} \
         (expected a gap of {LINE_H}). Before this the arm never advanced y and \
         drew them on top of each other",
        top.y,
        bottom.y,
    );
    assert!(
        !items.iter().any(|(t, _)| t.contains('\n')),
        "the newline is layout, not a glyph — it must not reach the font: {items:?}"
    );
}

/// `[center]` centres its paragraph; `[right]` pushes it to the right edge.
///
/// ⚠️ Asserted against the run's own measured width rather than a literal pixel,
/// so the test states the RELATIONSHIP (centred within the rect) rather than
/// re-encoding the font metrics.
#[test]
fn center_and_right_align_their_paragraphs() {
    let (mut tree, id) = label("[center]mid[/center]");
    let items = drawn(&mut tree, id);
    let (_, pos) = find(&items, "mid");
    // "mid" is 3 glyphs: (3*5 + 2*1) * 2 = 34 wide.
    let w = 34.0;
    assert!(
        ((pos.x + w * 0.5) - W * 0.5).abs() < 1e-3,
        "the run's centre must land on the rect's centre ({}), got centre {}",
        W * 0.5,
        pos.x + w * 0.5,
    );

    let (mut tree, id) = label("[right]end[/right]");
    let items = drawn(&mut tree, id);
    let (_, pos) = find(&items, "end");
    assert!(
        ((pos.x + w) - W).abs() < 1e-3,
        "a right-aligned run must end at the rect's right edge ({W}), got {}",
        pos.x + w,
    );
}

/// A paragraph tag is a LINE break, not an inline attribute.
///
/// `a[center]mid[/center]b` is three paragraphs in Godot. Treating the tags as
/// inline styling would leave one line and centre the whole thing the moment a
/// single word was tagged.
#[test]
fn a_paragraph_tag_breaks_the_line() {
    let (mut tree, id) = label("a[center]mid[/center]b");
    let items = drawn(&mut tree, id);

    let a = find(&items, "a").1;
    let mid = find(&items, "mid").1;
    let b = find(&items, "b").1;

    assert!(mid.y > a.y, "the centred paragraph starts a new line");
    assert!(b.y > mid.y, "and the text after it starts another");
    assert!(
        (b.y - a.y - 2.0 * LINE_H).abs() < 1e-3,
        "three paragraphs means two line advances between the first and last, \
         got {} and {}",
        a.y,
        b.y
    );
}

/// A label that OPENS with a paragraph tag has no blank line above it.
#[test]
fn a_leading_paragraph_tag_does_not_leave_a_blank_first_line() {
    let (mut tree, id) = label("[center]only[/center]");
    let items = drawn(&mut tree, id);
    let pos = find(&items, "only").1;

    assert!(
        pos.y.abs() < 1e-3,
        "the text belongs on the first line, got y = {}",
        pos.y
    );
}

/// 🔑 A link on the second line is clickable on the second line.
///
/// This is what the shared layout buys. The draw arm and the hit-test used to
/// advance `x` independently — which agreed by accident while everything sat on
/// one line, and would have put every multi-line link's hit region on row 0.
/// Both now read the same placements, so the region is asserted to sit exactly
/// where the run is painted.
#[test]
fn a_link_on_the_second_line_is_hit_tested_on_the_second_line() {
    let (mut tree, id) = label("first line\n[url=go]second[/url]");

    let regions = gdscene::control_render::richtext_meta_regions(
        &tree,
        id,
        gdscene::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(W, H)),
    );
    assert_eq!(regions.len(), 1, "one link, got {regions:?}");
    let (rect, meta) = regions[0].clone();
    assert_eq!(meta, "go");

    let items = drawn(&mut tree, id);
    let painted = find(&items, "second").1;

    assert!(
        (rect.position.y - painted.y).abs() < 1e-3,
        "the hit region must sit on the row the run is PAINTED on — region y {} \
         vs painted y {}. A region left on row 0 is the exact bug a shared \
         layout prevents",
        rect.position.y,
        painted.y,
    );
    assert!(
        (rect.position.x - painted.x).abs() < 1e-3,
        "and at the same column: region x {} vs painted x {}",
        rect.position.x,
        painted.x,
    );
    assert!(
        rect.position.y > 0.0,
        "sanity: the second line is not at the top, else this proves nothing"
    );
}

// ---------------------------------------------------------------------------
// fit_content
// ---------------------------------------------------------------------------

/// Renders once, which is what applies `fit_content`.
fn render(tree: &mut SceneTree, id: NodeId) {
    let mut vp = Viewport::new(W as u32, H as u32, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(tree, id, &theme, &mut vp, Vector2::new(W, H));
}

/// ⚠️ `fit_content` was registered and **unread** — the inspector showed it, the
/// value round-tripped through `.tscn`, and nothing consulted it. Godot: "the
/// label's minimum size will be automatically updated to fit its content".
///
/// The default is `false`, so the first half of this test is what stops the
/// feature from silently resizing every label that never asked for it.
#[test]
fn fit_content_drives_min_size_only_when_asked() {
    // Opted out (the default): min_size is left alone.
    let (mut tree, id) = label("ab\ncdef");
    render(&mut tree, id);
    assert_eq!(
        gdscene::control::get_min_size(&tree, id),
        Vector2::ZERO,
        "a label that has not opted in must not be resized"
    );

    // Opted in: min_size follows the content.
    let (mut tree, id) = label("ab\ncdef");
    tree.get_node_mut(id)
        .unwrap()
        .set_property("fit_content", Variant::Bool(true));
    render(&mut tree, id);

    // Glyphs are 5 wide with a 1px gap at scale 2: "ab" = 22, "cdef" = 46.
    let min = gdscene::control::get_min_size(&tree, id);
    assert!(
        (min.x - 46.0).abs() < 1e-3,
        "width follows the WIDEST line (cdef = 46), not the first or the sum — \
         got {}",
        min.x
    );
    assert!(
        (min.y - 2.0 * LINE_H).abs() < 1e-3,
        "height follows the LINE COUNT (2 lines), got {}",
        min.y
    );
}

/// The measured width is the widest line, which only a fixture whose FIRST line
/// is not the widest can distinguish.
///
/// With "cdef" first, a `min_size` taken from line 0 — or from the last line —
/// would coincidentally match, and the test would pass while measuring the
/// wrong thing.
#[test]
fn fit_content_measures_the_widest_line_not_the_first() {
    let (mut tree, id) = label("cdef\nab");
    tree.get_node_mut(id)
        .unwrap()
        .set_property("fit_content", Variant::Bool(true));
    render(&mut tree, id);

    assert!(
        (gdscene::control::get_min_size(&tree, id).x - 46.0).abs() < 1e-3,
        "the widest line is now the FIRST one; the answer must not change"
    );
}

// ---------------------------------------------------------------------------
// [bgcolor] / [fgcolor]
// ---------------------------------------------------------------------------

/// Filled rects the label paints, as `(rect, colour)`.
fn drawn_rects(tree: &mut SceneTree, id: NodeId) -> Vec<(Rect2, Color)> {
    let mut vp = Viewport::new(W as u32, H as u32, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(tree, id, &theme, &mut vp, Vector2::new(W, H));
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect,
                color,
                filled: true,
            } => Some((rect, color)),
            _ => None,
        })
        .collect()
}

/// `[bgcolor]` fills behind its run — and only its run.
///
/// ⚠️ The x-extent assertion is the one that matters. A fill that covered the
/// whole control, or started at the label's left edge instead of the run's,
/// would still produce "a red rect" and satisfy any test that only checked the
/// colour. Here the fill must begin where the tagged run begins, which is past
/// the untagged text in front of it.
#[test]
fn bgcolor_fills_behind_its_own_run_only() {
    let (mut tree, id) = label("ab[bgcolor=#ff0000]cd[/bgcolor]");
    let rects = drawn_rects(&mut tree, id);

    let red = rects
        .iter()
        .find(|(_, c)| c.r > 0.9 && c.g < 0.1 && c.b < 0.1)
        .unwrap_or_else(|| panic!("no red fill among {rects:?}"));

    // "ab" is 22 wide at scale 2, plus the 2px inter-run gap.
    assert!(
        (red.0.position.x - 24.0).abs() < 1e-3,
        "the fill must start where the TAGGED run starts (x = 24), not at the \
         label's left edge — got {}",
        red.0.position.x
    );
    // "cd" is also 22 wide.
    assert!(
        (red.0.size.x - 22.0).abs() < 1e-3,
        "and span only that run (22 wide), not the whole line — got {}",
        red.0.size.x
    );
}

/// A label with no `[bgcolor]` paints no fill at all.
///
/// Without this, the test above passes for an implementation that fills behind
/// every run with a default colour.
#[test]
fn a_label_without_bgcolor_paints_no_fill() {
    let (mut tree, id) = label("plain text");
    assert!(
        drawn_rects(&mut tree, id).is_empty(),
        "no [bgcolor] means no filled rect: {:?}",
        drawn_rects(&mut tree, id)
    );
}

/// `[fgcolor]` sets the glyph colour, like `[color]`, and restores on close.
#[test]
fn fgcolor_sets_the_run_colour_and_restores() {
    let (mut tree, id) = label("a[fgcolor=#00ff00]b[/fgcolor]c");
    let mut vp = Viewport::new(W as u32, H as u32, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(
        &mut tree,
        id,
        &theme,
        &mut vp,
        Vector2::new(W, H),
    );

    let runs: Vec<(String, Color)> = vp
        .get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, color, .. } => Some((text, color)),
            _ => None,
        })
        .collect();

    let green = runs
        .iter()
        .find(|(t, _)| t == "b")
        .unwrap_or_else(|| panic!("no run 'b' among {runs:?}"));
    assert!(
        green.1.g > 0.9 && green.1.r < 0.1,
        "[fgcolor] must colour its run green, got {:?}",
        green.1
    );

    let after = runs
        .iter()
        .find(|(t, _)| t == "c")
        .unwrap_or_else(|| panic!("no run 'c' among {runs:?}"));
    assert!(
        after.1.g < 0.99 || after.1.r > 0.1,
        "the colour must be restored after [/fgcolor] — 'c' is still green, so \
         the closer did not pop: {:?}",
        after.1
    );
}

// ---------------------------------------------------------------------------
// Word wrapping (autowrap_mode)
// ---------------------------------------------------------------------------

/// A line wider than the rect breaks at a space.
///
/// Words are 34 wide with a 14px space, so six fit in 300 (274) and a seventh
/// would not (322) — the break point is derived, not guessed.
#[test]
fn a_line_wider_than_the_rect_wraps_at_a_space() {
    let (mut tree, id) = label("aaa bbb ccc ddd eee fff ggg hhh");
    let items = drawn(&mut tree, id);

    let first = find(&items, "aaa").1;
    let seventh = find(&items, "ggg").1;

    assert!(first.y.abs() < 1e-3, "the first word starts on line 0");
    assert!(
        (seventh.y - LINE_H).abs() < 1e-3,
        "the seventh word must fall to the second line (six fit in 300px), got \
         y = {}",
        seventh.y
    );
    assert!(
        seventh.x.abs() < 1e-3,
        "and restart at the left edge, got x = {}",
        seventh.x
    );
}

/// ⚠️ A paragraph AFTER a wrapped one is pushed down by the wrap.
///
/// Visual lines are counted across the whole label, not per paragraph. Using the
/// logical line index plus the wrap index would put this paragraph on top of the
/// previous one's second visual line — a collision that only appears once
/// something actually wraps, which is why it is asserted separately.
#[test]
fn a_wrapped_paragraph_pushes_the_next_one_down() {
    let (mut tree, id) = label("aaa bbb ccc ddd eee fff ggg\nzz");
    let items = drawn(&mut tree, id);

    let wrapped = find(&items, "ggg").1;
    let after = find(&items, "zz").1;

    assert!(
        (wrapped.y - LINE_H).abs() < 1e-3,
        "the overflowing word sits on visual line 1, got {}",
        wrapped.y
    );
    assert!(
        (after.y - 2.0 * LINE_H).abs() < 1e-3,
        "and the next PARAGRAPH must clear it entirely — expected y = {}, got \
         {}. Equal to the line above would mean the two are drawn on top of \
         each other",
        2.0 * LINE_H,
        after.y
    );
}

/// `autowrap_mode = 0` (OFF) restores the un-wrapped behaviour.
#[test]
fn autowrap_off_does_not_wrap() {
    let (mut tree, id) = label("aaa bbb ccc ddd eee fff ggg hhh");
    tree.get_node_mut(id)
        .unwrap()
        .set_property("autowrap_mode", Variant::Int(0));
    let items = drawn(&mut tree, id);

    // ⚠️ With wrapping off there is nothing to split: the text carries no tags,
    // so it stays ONE run rather than becoming per-word runs. Asserting on a
    // word here is what my first draft got wrong — the word path only exists on
    // an overflowing line.
    assert_eq!(
        items.len(),
        1,
        "wrapping off leaves the line as a single run, got {items:?}"
    );
    assert!(
        items[0].1.y.abs() < 1e-3,
        "and on line 0, however far it overflows — got y = {}",
        items[0].1.y
    );
    assert!(
        items[0].0.ends_with("hhh"),
        "with the whole text intact: {:?}",
        items[0].0
    );
}

/// ⚠️ A word is NOT broken at a run boundary.
///
/// `[color=…]he[/color]llo` is one word that happens to change colour halfway.
/// Splitting the line into words per-run would treat "he" and "llo" as separate
/// words and could wrap between them — hyphenating a word nothing asked to
/// break, and inserting a space where the text has none.
#[test]
fn a_word_is_not_broken_where_a_tag_splits_it() {
    // The long first word forces a wrap; "hello" must land whole on line 1.
    let (mut tree, id) = label("xxxxxxxxxxxxxxxxxxxxxxxx [color=#ff0000]he[/color]llo");
    let items = drawn(&mut tree, id);

    let he = find(&items, "he").1;
    let llo = find(&items, "llo").1;

    assert!(
        (he.y - llo.y).abs() < 1e-3,
        "both halves of the word belong on the same line — got {} and {}",
        he.y,
        llo.y
    );
    assert!(
        he.y > 0.0,
        "sanity: the word did wrap to a second line, else this proves nothing"
    );
    assert!(
        (llo.x - (he.x + 22.0 + 2.0)).abs() < 1e-3,
        "and they stay adjacent (one inter-glyph gap apart, no space): 'he' at \
         {}, 'llo' at {}",
        he.x,
        llo.x
    );
}
