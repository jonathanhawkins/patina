//! pat-0q2kx.149 — a `Label`'s `autowrap_mode`, `horizontal_alignment` and
//! `text_overrun_behavior` actually change what is drawn.
//!
//! `gdrender2d::text_layout` implemented word breaking, alignment offsets and
//! ellipsis trimming — with its own unit tests — but had **zero consumers**
//! anywhere in the workspace (`rg text_layout` outside the module matched only
//! its `pub mod` line). The Label draw path emitted ONE `DrawString` at the
//! rect's top-left, so a long string ran straight off the edge and every
//! alignment setting rendered identically. Tested code that nothing calls is
//! the exact shape this audit epic exists to find.
//!
//! These assert on the emitted draw items, which is what the renderer and the
//! editor viewport both consume.
//!
//! NOTE ON THE FILE NAME: kept short. The verifier truncates the test command
//! at 58 characters, so a long binary name silently becomes a missing target.

use gdcore::math::Vector2;
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdvariant::Variant;

const LONG_TEXT: &str = "the quick brown fox jumps over the lazy dog";

/// A root Control with one Label child placed at `(0,0)` sized `w x h`.
fn label_scene(w: f32, h: f32, props: &[(&str, Variant)]) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree.add_child(root, Node::new("Root", "Control")).unwrap();
    control::apply_anchor_preset(&mut tree, panel, AnchorPreset::TopLeft);
    control::set_offset_left(&mut tree, panel, 0.0);
    control::set_offset_top(&mut tree, panel, 0.0);
    control::set_offset_right(&mut tree, panel, w);
    control::set_offset_bottom(&mut tree, panel, h);

    let label = tree.add_child(panel, Node::new("L", "Label")).unwrap();
    control::apply_anchor_preset(&mut tree, label, AnchorPreset::TopLeft);
    control::set_offset_left(&mut tree, label, 0.0);
    control::set_offset_top(&mut tree, label, 0.0);
    control::set_offset_right(&mut tree, label, w);
    control::set_offset_bottom(&mut tree, label, h);
    for (k, v) in props {
        tree.get_node_mut(label).unwrap().set_property(k, v.clone());
    }
    (tree, panel)
}

/// Every text item the scene draws, as `(text, position)`.
fn text_items(tree: &mut SceneTree, root: NodeId, w: f32, h: f32) -> Vec<(String, Vector2)> {
    let theme = Theme::default_theme();
    control_draw_items(tree, root, &theme, Vector2::new(w, h))
        .into_iter()
        .filter_map(|item| match item {
            ControlDrawItem::Text { text, position, .. } => Some((text, position)),
            _ => None,
        })
        .collect()
}

/// `autowrap_mode = WORD` breaks a long string onto several lines inside the
/// rect, where wrapping off leaves exactly one.
#[test]
fn autowrap_word_breaks_into_multiple_lines() {
    // WORD = 2 in Godot's AutowrapMode.
    let (mut wrapped, root) = label_scene(
        100.0,
        80.0,
        &[
            ("text", Variant::String(LONG_TEXT.to_string())),
            ("autowrap_mode", Variant::Int(2)),
        ],
    );
    let lines = text_items(&mut wrapped, root, 100.0, 80.0);
    assert!(
        lines.len() > 1,
        "a 100px Label with autowrap WORD must draw more than one line, drew {}: {lines:?}",
        lines.len()
    );

    // Successive lines step downward — otherwise they would overdraw.
    for pair in lines.windows(2) {
        assert!(
            pair[1].1.y > pair[0].1.y,
            "each wrapped line must sit below the previous one: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }

    // The control: the same text with wrapping OFF is a single line.
    let (mut flat, flat_root) = label_scene(
        100.0,
        80.0,
        &[("text", Variant::String(LONG_TEXT.to_string()))],
    );
    let flat_lines = text_items(&mut flat, flat_root, 100.0, 80.0);
    assert_eq!(
        flat_lines.len(),
        1,
        "with autowrap off the Label must draw exactly one line, drew {flat_lines:?}"
    );

    // Wrapping must not lose words.
    let joined: String = lines
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        joined, LONG_TEXT,
        "wrapping must preserve every word in order"
    );
}

/// `text_overrun_behavior = ELLIPSIS` trims an unwrapped overflowing line and
/// marks it, instead of running off the edge.
#[test]
fn overrun_ellipsis_trims_a_single_line() {
    // ELLIPSIS = 3 in Godot's OverrunBehavior; autowrap stays off.
    let (mut tree, root) = label_scene(
        100.0,
        40.0,
        &[
            ("text", Variant::String(LONG_TEXT.to_string())),
            ("text_overrun_behavior", Variant::Int(3)),
        ],
    );
    let lines = text_items(&mut tree, root, 100.0, 40.0);
    assert_eq!(lines.len(), 1, "overrun trimming stays on one line");

    let drawn = &lines[0].0;
    assert!(
        drawn.ends_with("..."),
        "an overflowing line must end in an ellipsis, got {drawn:?}"
    );
    assert!(
        drawn.len() < LONG_TEXT.len(),
        "the trimmed line must be shorter than the full text, got {drawn:?}"
    );

    // Without the setting the full string is drawn untrimmed — proving the
    // property is what changed the output.
    let (mut plain, plain_root) = label_scene(
        100.0,
        40.0,
        &[("text", Variant::String(LONG_TEXT.to_string()))],
    );
    let plain_lines = text_items(&mut plain, plain_root, 100.0, 40.0);
    assert_eq!(plain_lines[0].0, LONG_TEXT);
}

/// `horizontal_alignment = CENTER` shifts the line right; RIGHT shifts it
/// further. LEFT stays flush.
#[test]
fn horizontal_alignment_shifts_the_line() {
    let short = "hi";
    let x_for = |align: Option<i64>| -> f32 {
        let mut props = vec![("text", Variant::String(short.to_string()))];
        if let Some(a) = align {
            props.push(("horizontal_alignment", Variant::Int(a)));
        }
        let (mut tree, root) = label_scene(100.0, 40.0, &props);
        let items = text_items(&mut tree, root, 100.0, 40.0);
        assert_eq!(items.len(), 1, "short text is one line");
        items[0].1.x
    };

    let left = x_for(None);
    let center = x_for(Some(1));
    let right = x_for(Some(2));

    assert!(
        center > left,
        "CENTER must shift the line right of LEFT ({center} vs {left})"
    );
    assert!(
        right > center,
        "RIGHT must shift further than CENTER ({right} vs {center})"
    );
    // CENTER is exactly halfway between LEFT and RIGHT: the right offset is the
    // full slack `max_width - line_width`, and the centre offset is half of it.
    // (Comparing against a hardcoded number instead would only hold for one
    // particular string width.)
    assert!(
        (center - (left + (right - left) / 2.0)).abs() < 0.01,
        "CENTER must be the midpoint of LEFT and RIGHT; left={left} center={center} right={right}"
    );
}

/// An empty Label draws nothing at all, rather than an empty string item.
#[test]
fn an_empty_label_draws_no_text() {
    let (mut tree, root) = label_scene(100.0, 40.0, &[("text", Variant::String(String::new()))]);
    assert!(text_items(&mut tree, root, 100.0, 40.0).is_empty());
}
