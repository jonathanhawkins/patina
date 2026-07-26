//! pat-0q2kx.175 clause 2 — `Control` cursor shape reporting.
//!
//! Godot maps each Control's `mouse_default_cursor_shape` onto an OS/browser
//! cursor. Patina had neither the property (see the `control_aux_input` schema
//! test) nor any resolver, so nothing could tell a host shell what cursor to
//! draw.
//!
//! These live in an **integration** test rather than `control.rs`'s
//! `#[cfg(test)]` module on purpose: `control.rs` carries a **pre-existing**
//! duplicate `#[test]` attribute (a bare `#[test]`, a comment banner, then a
//! second `#[test] fn rtl_layout_mirrors_hbox_child_positions`). Two `#[test]`
//! attributes on one function do not compile, and that module is only built by
//! `--lib`, which is why every `--test` run in this crate stays green while
//! `-p gdscene --lib` does not. Not this bead's bug; reported separately.

use gdcore::math::Vector2;
use gdscene::control::{get_cursor_shape, CursorShape};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// A control's declared shape resolves through the property; anything else falls
/// back to `Arrow`, which is both enum-zero and the registered default.
#[test]
fn cursor_shape_resolves_from_the_control_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree.add_child(root, Node::new("Btn", "Button")).unwrap();

    // Unset: the registered default.
    assert_eq!(get_cursor_shape(&tree, button), CursorShape::Arrow);

    tree.get_node_mut(button)
        .unwrap()
        .set_property("mouse_default_cursor_shape", Variant::Int(2));
    assert_eq!(get_cursor_shape(&tree, button), CursorShape::PointingHand);

    // Out of range must not panic or wrap — it falls back to Arrow.
    tree.get_node_mut(button)
        .unwrap()
        .set_property("mouse_default_cursor_shape", Variant::Int(99));
    assert_eq!(get_cursor_shape(&tree, button), CursorShape::Arrow);

    // A non-integer value likewise resolves rather than failing.
    tree.get_node_mut(button)
        .unwrap()
        .set_property("mouse_default_cursor_shape", Variant::Nil);
    assert_eq!(get_cursor_shape(&tree, button), CursorShape::Arrow);
}

/// Every one of Godot's 17 shapes must round-trip through its integer constant,
/// so a `.tscn` value and the reported shape cannot drift.
#[test]
fn all_cursor_shapes_round_trip_through_godot_ints() {
    for i in 0..17i64 {
        let shape = CursorShape::from_godot_int(i);
        assert_eq!(
            shape.to_godot_int(),
            i,
            "shape {shape:?} must map back to {i}"
        );
        assert!(
            !shape.css_name().is_empty(),
            "{shape:?} needs a CSS cursor keyword for the web shell"
        );
    }
    // 17 is one past the end — the fallback, not a wrap to IBeam.
    assert_eq!(CursorShape::from_godot_int(17), CursorShape::Arrow);
    assert_eq!(CursorShape::from_godot_int(-1), CursorShape::Arrow);
}

/// **The wiring clause.** The shape must reach `FrameOutput`, or the host shell
/// has nothing to read and the resolver is a producer with no consumer.
///
/// With nothing hovered the frame reports `Arrow`, which is also what an
/// unhovered viewport shows.
#[test]
fn frame_output_reports_the_default_cursor_when_nothing_is_hovered() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Btn", "Button")).unwrap();

    let mut ml = MainLoop::new(tree);
    let out = ml.step(1.0 / 60.0);

    assert_eq!(
        out.cursor_shape,
        CursorShape::Arrow,
        "no hovered control means the default arrow"
    );
}

/// And the reported shape tracks the hovered control's property, so moving the
/// mouse onto a text field changes the cursor the shell draws.
#[test]
fn frame_output_reports_the_hovered_controls_shape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let field = tree
        .add_child(root, Node::new("Field", "LineEdit"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(field) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        n.set_property("size", Variant::Vector2(Vector2::new(100.0, 40.0)));
        // CURSOR_IBEAM — what a text field asks for.
        n.set_property("mouse_default_cursor_shape", Variant::Int(1));
    }

    // Hover it through the real GUI input path — `push_input` is what drives
    // `gui_input_stage`, which is where `hovered_control` is tracked.
    tree.push_input(&gdplatform::input::InputEvent::MouseMotion {
        position: Vector2::new(20.0, 20.0),
        relative: Vector2::ZERO,
    });
    assert_eq!(
        tree.hovered_control(),
        Some(field),
        "the motion must land on the field, or this test proves nothing"
    );

    let mut ml = MainLoop::new(tree);
    let out = ml.step(1.0 / 60.0);
    assert_eq!(
        out.cursor_shape,
        CursorShape::IBeam,
        "the hovered control's shape must reach FrameOutput"
    );
}
