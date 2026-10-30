//! pat-nzgdu.198 — `SyntaxHighlighter`/`CodeHighlighter` are ClassDB resources,
//! and a `CodeHighlighter` assigned to a `TextEdit` colours a configured keyword
//! span in the control's emitted draw commands.
//!
//! Before this bead the only `SyntaxHighlighter` was an editor-internal unit
//! struct unrelated to ClassDB, and a runtime `TextEdit` drew nothing at all —
//! it had no case in `draw_control`. This drives the real render pass and reads
//! the emitted `DrawString` commands, proving the keyword colour reaches the
//! framebuffer path (not just that the resource is introspectable).

use gdcore::math::{Color, Vector2};
use gdobject::{class_exists, is_parent_class, register_editor_classes};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::{Dictionary, Variant};

fn approx(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.01
        && (a.g - b.g).abs() < 0.01
        && (a.b - b.b).abs() < 0.01
        && (a.a - b.a).abs() < 0.01
}

/// Every `DrawString` (text, color) emitted into the viewport, in draw order.
fn drawn_strings(vp: &Viewport) -> Vec<(String, Color)> {
    let mut out = Vec::new();
    for item in vp.get_unlayered_items() {
        for cmd in &item.commands {
            if let DrawCommand::DrawString { text, color, .. } = cmd {
                out.push((text.clone(), *color));
            }
        }
    }
    out
}

fn text_edit_with(text: &str, keyword_colors: Option<Dictionary>) -> (SceneTree, Viewport) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("TextEdit", "TextEdit"))
        .unwrap();
    control::apply_anchor_preset(&mut tree, node, AnchorPreset::FullRect);
    {
        let n = tree.get_node_mut(node).unwrap();
        n.set_property("text", Variant::String(text.to_owned()));
        if let Some(kw) = keyword_colors {
            n.set_property(
                "syntax_highlighter/keyword_colors",
                Variant::Dictionary(kw.into()),
            );
        }
    }

    let mut vp = Viewport::new(128, 32, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, node, &theme, &mut vp, Vector2::new(128.0, 32.0));
    (tree, vp)
}

/// The acceptance's registration clause.
#[test]
fn syntax_highlighter_resource_test() {
    register_editor_classes();

    assert!(
        class_exists("SyntaxHighlighter"),
        "SyntaxHighlighter registered"
    );
    assert!(
        class_exists("CodeHighlighter"),
        "CodeHighlighter registered"
    );
    assert!(is_parent_class("CodeHighlighter", "SyntaxHighlighter"));
    assert!(is_parent_class("CodeHighlighter", "Resource"));

    // The acceptance's wiring clause: keyword `func` -> blue on a TextEdit
    // holding `func foo()` colours the `func` span blue in the draw commands.
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    let mut kw = Dictionary::new();
    kw.insert(Variant::String("func".into()), Variant::Color(blue));

    let (_tree, vp) = text_edit_with("func foo()", Some(kw));
    let strings = drawn_strings(&vp);

    let func_span = strings
        .iter()
        .find(|(t, _)| t == "func")
        .unwrap_or_else(|| panic!("expected a 'func' DrawString; got {strings:?}"));
    assert!(
        approx(func_span.1, blue),
        "the 'func' keyword span must be blue; got {:?}",
        func_span.1
    );

    // ...and only the keyword: the rest of the line is not blue.
    assert!(
        !strings.iter().any(|(t, c)| t != "func" && approx(*c, blue)),
        "only the keyword should be blue; got {strings:?}"
    );
}

/// Without a highlighter, the whole line draws as a single (non-blue) span —
/// TextEdit now draws its text at all, where before it had no draw case.
#[test]
fn textedit_without_highlighter_draws_the_whole_line_uncolored() {
    let (_tree, vp) = text_edit_with("func foo()", None);
    let strings = drawn_strings(&vp);

    assert!(
        strings.iter().any(|(t, _)| t == "func foo()"),
        "the whole line should be one span; got {strings:?}"
    );
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    assert!(
        !strings.iter().any(|(_, c)| approx(*c, blue)),
        "nothing is blue without a highlighter; got {strings:?}"
    );
}

/// A keyword only colours whole-word matches: `function` is not `func`.
#[test]
fn keyword_does_not_color_a_longer_identifier() {
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    let mut kw = Dictionary::new();
    kw.insert(Variant::String("func".into()), Variant::Color(blue));

    let (_tree, vp) = text_edit_with("function", Some(kw));
    let strings = drawn_strings(&vp);

    assert!(
        !strings.iter().any(|(_, c)| approx(*c, blue)),
        "'function' contains 'func' but is a different word; got {strings:?}"
    );
    assert!(
        strings.iter().any(|(t, _)| t == "function"),
        "the identifier still draws; got {strings:?}"
    );
}
