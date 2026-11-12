//! pat-s3rrv.129 — a Label honours mandatory line breaks.
//!
//! `layout_label_lines` ran `text.split_whitespace()` over the whole string and
//! rejoined the result with `' '`. `split_whitespace` treats `'\n'` as ordinary
//! whitespace, so **every explicit newline was destroyed**: a two-line Label
//! drew as one line, and with autowrap off every word landed on a single line no
//! matter how many the author typed.
//!
//! 🔑 Godot shapes mandatory breaks FIRST and wraps each paragraph
//! independently (`Label::_shape`, `scene/gui/label.cpp`).

use gdcore::math::Vector2;
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::Node;
use gdscene::theme::Theme;
use gdscene::SceneTree;
use gdvariant::Variant;

/// Builds `Root -> Label` with `text` and returns every Text item's
/// `(string, y)`, in emission order.
fn text_rows(text: &str) -> Vec<(String, f32)> {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Control"))
        .unwrap();
    let label = tree.add_child(root, Node::new("Label", "Label")).unwrap();
    tree.get_node_mut(label)
        .unwrap()
        .set_property("text", Variant::String(text.to_string()));

    control_draw_items(
        &mut tree,
        root,
        &Theme::default(),
        Vector2::new(400.0, 200.0),
    )
    .into_iter()
    .filter_map(|i| match i {
        ControlDrawItem::Text { text, position, .. } => Some((text, position.y)),
        _ => None,
    })
    .collect()
}

/// **The core of the bead.** Two lines of text draw as two items at different
/// heights.
#[test]
fn an_explicit_newline_produces_two_lines() {
    let rows = text_rows("Line1\nLine2");

    assert_eq!(
        rows.len(),
        2,
        "an explicit newline must produce two draw items, got {rows:?}"
    );
    assert_eq!(rows[0].0, "Line1");
    assert_eq!(rows[1].0, "Line2");
    assert!(
        rows[1].1 > rows[0].1,
        "the second line must sit below the first, got y {} then {}",
        rows[0].1,
        rows[1].1
    );
}

/// ⚠️ Before this change the two words were joined with a space onto one line.
/// Asserting only the item count would not catch a version that emitted two
/// items with the *same* text, so the strings are pinned too — and this pins
/// that they were not silently merged.
#[test]
fn the_newline_is_not_turned_into_a_space() {
    let rows = text_rows("Line1\nLine2");
    assert!(
        !rows.iter().any(|(t, _)| t.contains("Line1 Line2")),
        "the newline must not become a space: {rows:?}"
    );
}

/// 🔑 A blank line still OCCUPIES a row — `"A\n\nB"` puts B **two** rows down,
/// not one.
///
/// ⚠️ This is the discriminating case for the empty-paragraph branch. An
/// implementation that simply skips empty paragraphs emits the same two strings
/// at the same pitch as `"A\nB"` and passes every other test here, while
/// silently closing the gap the author typed.
#[test]
fn a_blank_line_still_occupies_a_row() {
    let single = text_rows("A\nB");
    let blank = text_rows("A\n\nB");

    assert_eq!(single.len(), 2);
    assert_eq!(blank.len(), 2, "the blank line draws nothing itself");

    let single_gap = single[1].1 - single[0].1;
    let blank_gap = blank[1].1 - blank[0].1;
    assert!(
        (blank_gap - single_gap * 2.0).abs() < 1e-3,
        "a blank line must double the gap: {single_gap} vs {blank_gap}"
    );
}

/// The control: ordinary single-line text is unchanged, so this cannot have
/// altered every Label in the project.
#[test]
fn text_without_newlines_still_draws_one_line() {
    let rows = text_rows("Just one line here");
    assert_eq!(rows.len(), 1, "got {rows:?}");
    assert_eq!(rows[0].0, "Just one line here");
}

/// Trailing and leading newlines are real rows too, so the visible text does not
/// jump to the top of the rect.
#[test]
fn a_leading_newline_pushes_the_text_down() {
    let plain = text_rows("A");
    let led = text_rows("\nA");

    assert_eq!(plain.len(), 1);
    assert_eq!(led.len(), 1, "the leading blank draws nothing itself");
    assert!(
        led[0].1 > plain[0].1,
        "a leading newline must push the text down a row, got {} vs {}",
        plain[0].1,
        led[0].1
    );
}
