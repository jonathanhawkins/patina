//! pat-nzgdu.197 — Separator / HSeparator / VSeparator.
//!
//! Both acceptance clauses, driven through the real pipeline: `render_control_tree`
//! for the draw command, and `separator_minimum_size` for the sizing. The ClassDB
//! clause is gdobject's `separator_classdb_test`.
//!
//! Sizing verified against Godot's `scene/gui/separator.cpp:35-43`; the theme
//! values against `scene/theme/default_theme.cpp:734-739, 1063-1069`.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::{render_control_tree, separator_minimum_size};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
// `Viewport` lives in gdserver2d::viewport, NOT ::canvas (which owns
// CanvasItem/DrawCommand).
use gdserver2d::viewport::Viewport;

/// A separator laid out at 100x10 in a fresh tree.
fn separator(class: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Sep", class))
        .expect("separator added");
    control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, id, 100.0);
    control::set_offset_bottom(&mut tree, id, 10.0);
    (tree, id)
}

fn draw(tree: &mut SceneTree, id: NodeId) -> Vec<DrawCommand> {
    let theme = Theme::default_theme();
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    render_control_tree(tree, id, &theme, &mut vp, Vector2::new(200.0, 200.0));
    vp.get_sorted_items()
        .first()
        .map(|i| i.commands.clone())
        .unwrap_or_default()
}

/// The acceptance's second clause: an HSeparator emits a draw command for its
/// line.
#[test]
fn separator_controls_test() {
    let (mut tree, id) = separator("HSeparator");
    let cmds = draw(&mut tree, id);
    assert!(
        !cmds.is_empty(),
        "an HSeparator must emit a draw command for its line"
    );

    // A VSeparator draws too — the orientation is the class, so both must find
    // their own stylebox.
    let (mut tree, id) = separator("VSeparator");
    assert!(
        !draw(&mut tree, id).is_empty(),
        "a VSeparator must emit a draw command for its line"
    );
}

/// ⚠️ The acceptance's first clause — and the trap in it.
///
/// Godot's base is **(3, 3), not zero** (`separator.cpp:36`): a separator always
/// reserves 3px on its long axis so it never collapses in a container. Only the
/// cross axis takes `separation`. `(0, 4)` and `(4, 4)` are both wrong and both
/// look plausible.
#[test]
fn minimum_size_takes_separation_on_the_cross_axis_only() {
    let theme = Theme::default_theme();

    // The acceptance: "an HSeparator reports a minimum height equal to the
    // theme separation constant".
    let h = separator_minimum_size(&theme, "HSeparator");
    assert_eq!(
        h.y, 4.0,
        "HSeparator's minimum HEIGHT is the theme separation (4)"
    );
    assert_eq!(
        h.x, 3.0,
        "...and its width is Godot's 3px floor, NOT 0 and NOT the separation"
    );

    // VSeparator is the transpose: separation is its WIDTH.
    let v = separator_minimum_size(&theme, "VSeparator");
    assert_eq!(v.x, 4.0, "VSeparator's minimum WIDTH is the separation");
    assert_eq!(v.y, 3.0, "...and its height is the 3px floor");

    // The two are genuinely transposed, not identical — that is the whole
    // difference between the classes.
    assert_ne!(h, v, "H and V separators must not size the same");
}

/// The size follows the THEME, not a hardcoded 4 — otherwise a restyled theme
/// would silently keep the default spacing.
#[test]
fn minimum_size_follows_the_theme_constant() {
    let mut theme = Theme::default_theme();
    theme.set_constant("HSeparator", "separation", 12);

    let h = separator_minimum_size(&theme, "HSeparator");
    assert_eq!(h.y, 12.0, "a restyled separation must change the height");
    assert_eq!(h.x, 3.0, "the 3px floor is not themed");

    // Untouched VSeparator keeps the default — the constants are per-class.
    assert_eq!(separator_minimum_size(&theme, "VSeparator").x, 4.0);
}

/// ⚠️ The default theme really does carry Godot's values. Without these the
/// separator would draw nothing and size to the fallback, and the failure would
/// look like a layout bug rather than a missing theme entry.
#[test]
fn default_theme_carries_godots_separator_entries() {
    let theme = Theme::default_theme();

    for class in ["HSeparator", "VSeparator"] {
        assert_eq!(
            theme.get_constant(class, "separation"),
            Some(4),
            "{class} separation is 4 in Godot's default theme"
        );
        assert!(
            theme.get_stylebox(class, "separator").is_some(),
            "{class} must have a `separator` stylebox to draw"
        );
    }
}

/// The line is drawn along the separator's long axis. An HSeparator in a wide,
/// short rect must produce a WIDE mark, not a tall one — the classic way to get
/// the orientation backwards and still "emit a draw command".
#[test]
fn the_drawn_line_runs_along_the_separators_long_axis() {
    let (mut tree, id) = separator("HSeparator");
    let cmds = draw(&mut tree, id);

    // Whatever primitive the stylebox chooses, its extent must be wider than it
    // is tall for a horizontal separator in a 100x10 rect.
    let wide = cmds.iter().any(|c| match c {
        DrawCommand::DrawRect { rect, .. } => rect.size.x > rect.size.y,
        DrawCommand::DrawLine { from, to, .. } => (to.x - from.x).abs() > (to.y - from.y).abs(),
        _ => false,
    });
    assert!(
        wide,
        "an HSeparator's line must run horizontally; got {cmds:?}"
    );
}
