//! pat-s3rrv.38 (slice 1): the theme a Control actually draws with.
//!
//! Godot resolves each theme item through the node's own theme, then each
//! ancestor's nearest-first, then the project/ThemeDB default.
//! `control::resolve_theme_item` implemented exactly that — and had ZERO
//! production callers, so a `theme` assigned in a `.tscn`, a theme on a parent,
//! and `gui/theme/custom` all changed nothing on screen.
//!
//! `control_render::effective_theme` flattens that chain into one `Theme` so the
//! ~19 existing lookups and 15 `stylebox_bg` call sites can keep working
//! unchanged. These pin the precedence the flattening must produce.

use std::collections::HashMap;

use gdcore::math::Color;
use gdscene::control_render::effective_theme;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;

/// A theme whose `Button/font_color` is `c`.
fn theme_with(name: &str, c: Color) -> Theme {
    let mut t = Theme::new(name);
    t.set_color("Button", "font_color", c);
    t
}

const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// Builds root → mid → leaf and returns their ids.
fn three_deep() -> (SceneTree, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let a = tree.add_child(root_id, Node::new("A", "Panel")).unwrap();
    let b = tree.add_child(a, Node::new("B", "Panel")).unwrap();
    let c = tree.add_child(b, Node::new("C", "Button")).unwrap();
    (tree, a, b, c)
}

fn button_color(t: &Theme) -> Option<Color> {
    t.get_color("Button", "font_color")
}

/// With no node themes at all, the default answers — the behaviour that must not
/// regress for every scene that has no authored theme.
#[test]
fn the_default_answers_when_no_node_has_a_theme() {
    let (tree, _a, _b, c) = three_deep();
    let default = theme_with("default", RED);
    let got = effective_theme(&tree, c, &HashMap::new(), &default);
    assert_eq!(button_color(&got), Some(RED));
}

/// A theme on the node itself beats the default.
#[test]
fn a_node_theme_beats_the_default() {
    let (tree, _a, _b, c) = three_deep();
    let default = theme_with("default", RED);
    let mut themes = HashMap::new();
    themes.insert(c, theme_with("node", GREEN));

    assert_eq!(
        button_color(&effective_theme(&tree, c, &themes, &default)),
        Some(GREEN)
    );
}

/// **The case the bead names.** A theme on a GRANDPARENT still reaches the
/// Button — this is what the old flat lookup could not do at all.
#[test]
fn a_grandparent_theme_reaches_the_control() {
    let (tree, a, _b, c) = three_deep();
    let default = theme_with("default", RED);
    let mut themes = HashMap::new();
    themes.insert(a, theme_with("grandparent", GREEN));

    assert_eq!(
        button_color(&effective_theme(&tree, c, &themes, &default)),
        Some(GREEN),
        "a theme two levels up must reach the Button"
    );
}

/// The NEAREST ancestor wins over a farther one. This is the assertion that
/// distinguishes a correct walk from one that merges in the wrong order — both
/// would pass the grandparent test above.
#[test]
fn the_nearest_ancestor_wins() {
    let (tree, a, b, c) = three_deep();
    let default = theme_with("default", RED);
    let mut themes = HashMap::new();
    themes.insert(a, theme_with("far", GREEN));
    themes.insert(b, theme_with("near", BLUE));

    assert_eq!(
        button_color(&effective_theme(&tree, c, &themes, &default)),
        Some(BLUE),
        "the nearer ancestor's theme must win over the farther one"
    );
}

/// The node's own theme beats every ancestor.
#[test]
fn the_nodes_own_theme_beats_every_ancestor() {
    let (tree, a, b, c) = three_deep();
    let default = theme_with("default", RED);
    let mut themes = HashMap::new();
    themes.insert(a, theme_with("far", GREEN));
    themes.insert(b, theme_with("near", BLUE));
    themes.insert(c, theme_with("own", RED));

    assert_eq!(
        button_color(&effective_theme(&tree, c, &themes, &default)),
        Some(RED)
    );
}

/// An ancestor theme that is SILENT about an item does not shadow the default's
/// value for it. Godot stops at the first theme that HAS the item, not the first
/// node that has a theme.
#[test]
fn a_silent_ancestor_theme_does_not_shadow_the_default() {
    let (tree, a, _b, c) = three_deep();
    let mut default = Theme::new("default");
    default.set_color("Button", "font_color", RED);
    default.set_color("Label", "font_color", GREEN);

    // The ancestor speaks only about Button, saying nothing about Label.
    let mut themes = HashMap::new();
    themes.insert(a, theme_with("ancestor", BLUE));

    let got = effective_theme(&tree, c, &themes, &default);
    assert_eq!(
        button_color(&got),
        Some(BLUE),
        "ancestor wins where it speaks"
    );
    assert_eq!(
        got.get_color("Label", "font_color"),
        Some(GREEN),
        "…and the default still answers for an item the ancestor never mentions"
    );
}
