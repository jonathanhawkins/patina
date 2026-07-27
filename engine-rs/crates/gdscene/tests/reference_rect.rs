//! pat-0q2kx.245 — `ReferenceRect` draws only a coloured border around its rect.
//!
//! Godot ground truth: `scene/gui/reference_rect.cpp:37-44` —
//! `if (is_editor_hint() || !editor_only) draw_rect(Rect2(Point2(), get_size()),
//! border_color, false, border_width)`. Note the `false`: it is an **outline**,
//! never a fill, which is the whole point of the class.
//!
//! ⚠️ Godot's defaults are `border_color = Color(1, 0, 0)` (red, not white),
//! `border_width = 1.0`, and `editor_only = **true**` (`reference_rect.h:38-40`).
//! `editor_only` defaulting to on is why the acceptance has to set it false: an
//! unconfigured ReferenceRect is invisible at runtime *by design*.
//!
//! This drives the real `render_control_tree` pass and asserts on emitted
//! `DrawCommand`s — the same commands a viewport rasterises.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Renders a lone `ReferenceRect` carrying `props`, and returns its draw commands.
fn draw(props: &[(&str, Variant)]) -> Vec<DrawCommand> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("Ref", "ReferenceRect");
    for (k, v) in props {
        n.set_property(k, v.clone());
    }
    let id: NodeId = tree.add_child(root, n).unwrap();
    control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, id, 40.0);
    control::set_offset_bottom(&mut tree, id, 20.0);

    let mut vp = Viewport::new(64, 32, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(64.0, 32.0));
    vp.get_sorted_items()
        .first()
        .map(|it| it.commands.clone())
        .unwrap_or_default()
}

/// The unfilled rects among `cmds`, as `(color, size)`.
fn outlines(cmds: &[DrawCommand]) -> Vec<(Color, Vector2)> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect,
                color,
                filled: false,
            } => Some((*color, rect.size)),
            _ => None,
        })
        .collect()
}

/// **The acceptance test.** ClassDB knows the class, and a runtime-visible
/// ReferenceRect emits a border-coloured outline.
#[test]
fn reference_rect_draws_border() {
    gdobject::class_db::register_2d_classes();

    // -- ClassDB clause -----------------------------------------------------
    assert!(
        gdobject::class_db::class_exists("ReferenceRect"),
        "ReferenceRect must be registered"
    );
    assert!(
        gdobject::class_db::is_parent_class("ReferenceRect", "Control"),
        "ReferenceRect must inherit Control"
    );

    // -- Draw clause --------------------------------------------------------
    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    let cmds = draw(&[
        ("editor_only", Variant::Bool(false)),
        ("border_width", Variant::Float(1.0)),
        ("border_color", Variant::Color(green)),
    ]);
    let outs = outlines(&cmds);
    assert_eq!(
        outs.len(),
        1,
        "border_width 1 must emit exactly one outline, got {outs:?}"
    );
    let (color, size) = outs[0];
    assert!(
        (color.g - 1.0).abs() < 1e-4 && color.r.abs() < 1e-4,
        "the outline must use border_color, got {color:?}"
    );
    assert!(
        (size.x - 40.0).abs() < 1.0 && (size.y - 20.0).abs() < 1.0,
        "the outline must span the control's rect, got {size:?}"
    );

    // Nothing may be FILLED — a ReferenceRect that filled its rect would hide
    // whatever it is meant to be visualising.
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawRect { filled: true, .. })),
        "ReferenceRect must never fill its rect: {cmds:?}"
    );
}

/// `editor_only` is Godot's default (**true**), and this is the runtime pass —
/// so an unconfigured ReferenceRect draws nothing. Without this control, an
/// implementation that ignored `editor_only` entirely would still pass the
/// acceptance above.
#[test]
fn editor_only_hides_it_at_runtime() {
    gdobject::class_db::register_2d_classes();

    // Explicitly true.
    let cmds = draw(&[
        ("editor_only", Variant::Bool(true)),
        ("border_width", Variant::Float(2.0)),
    ]);
    assert!(
        outlines(&cmds).is_empty(),
        "editor_only = true must draw nothing at runtime, got {cmds:?}"
    );

    // And by default — `editor_only` unset must behave as true, not as false.
    let cmds = draw(&[("border_width", Variant::Float(2.0))]);
    assert!(
        outlines(&cmds).is_empty(),
        "editor_only defaults to TRUE, so an unconfigured ReferenceRect is \
         invisible at runtime, got {cmds:?}"
    );
}

/// A wider border emits more concentric outlines, and a zero width emits none.
#[test]
fn border_width_scales_and_zero_draws_nothing() {
    gdobject::class_db::register_2d_classes();

    let cmds = draw(&[
        ("editor_only", Variant::Bool(false)),
        ("border_width", Variant::Float(3.0)),
    ]);
    assert_eq!(
        outlines(&cmds).len(),
        3,
        "border_width 3 must be 3px thick, got {cmds:?}"
    );

    let cmds = draw(&[
        ("editor_only", Variant::Bool(false)),
        ("border_width", Variant::Float(0.0)),
    ]);
    assert!(
        outlines(&cmds).is_empty(),
        "border_width 0 must draw nothing, got {cmds:?}"
    );
}
