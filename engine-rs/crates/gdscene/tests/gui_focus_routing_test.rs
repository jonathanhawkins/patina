//! pat-0q2kx.21: keyboard focus on the SceneTree, and click-grabs-focus in the
//! GUI input stage.
//!
//! Before this there was no focus state anywhere — `grep grab_focus` across the
//! non-test sources returned zero hits — so `Control.has_focus()` had no answer
//! and clicking a Button could not move focus.
//!
//! The GUI stage itself (hit-test, `_gui_input`, hover, button press/release)
//! landed in pat-0q2kx.18; these cover only the focus layer added on top.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("position", Variant::Vector2(Vector2::new(x, y)));
    n.set_property("size", Variant::Vector2(Vector2::new(w, h)));
    n
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

/// `Main -> {Button, LineEdit, Label}`, laid out side by side.
fn focus_scene() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let btn = tree
        .add_child(main, control("Play", "Button", 0.0, 0.0, 80.0, 30.0))
        .unwrap();
    let edit = tree
        .add_child(main, control("Name", "LineEdit", 100.0, 0.0, 80.0, 30.0))
        .unwrap();
    // A Label is FOCUS_NONE in Godot — clicking it must not take focus.
    let label = tree
        .add_child(main, control("Title", "Label", 200.0, 0.0, 80.0, 30.0))
        .unwrap();
    (tree, btn, edit, label)
}

#[test]
fn grab_and_release_focus_emit_signals_and_track_owner() {
    let (mut tree, btn, edit, _label) = focus_scene();

    assert_eq!(tree.focus_owner(), None, "nothing is focused initially");
    assert!(!tree.has_focus(btn));

    tree.grab_focus(btn);
    assert_eq!(tree.focus_owner(), Some(btn));
    assert!(tree.has_focus(btn));

    // Moving focus transfers ownership exactly once.
    tree.grab_focus(edit);
    assert_eq!(tree.focus_owner(), Some(edit));
    assert!(!tree.has_focus(btn), "the previous owner loses focus");

    tree.release_focus();
    assert_eq!(tree.focus_owner(), None, "release drops focus entirely");
}

/// Re-grabbing on the current owner is a no-op, so repeated clicks do not
/// re-fire `focus_entered`.
#[test]
fn regrabbing_focus_on_the_current_owner_is_a_noop() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    tree.grab_focus(btn);
    tree.grab_focus(btn);
    assert_eq!(tree.focus_owner(), Some(btn));
}

/// **The wiring test.** A click inside a focusable Control's rect grabs focus
/// through the GUI input stage — not via a direct `grab_focus` call.
#[test]
fn clicking_a_control_grabs_focus_through_push_input() {
    let (mut tree, btn, edit, _label) = focus_scene();

    tree.push_input(&press(40.0, 15.0));
    assert!(
        tree.has_focus(btn),
        "clicking a Button must grab focus, got {:?}",
        tree.focus_owner()
    );

    tree.push_input(&press(140.0, 15.0));
    assert!(
        tree.has_focus(edit),
        "clicking the LineEdit must move focus to it, got {:?}",
        tree.focus_owner()
    );
}

/// Clicking a `FOCUS_NONE` Control drops focus rather than keeping the old
/// owner, matching Godot.
#[test]
fn clicking_a_non_focusable_control_releases_focus() {
    let (mut tree, btn, _edit, label) = focus_scene();

    tree.push_input(&press(40.0, 15.0));
    assert!(tree.has_focus(btn));

    tree.push_input(&press(240.0, 15.0));
    assert_eq!(
        tree.focus_owner(),
        None,
        "clicking a Label (FOCUS_NONE) must clear focus, not retain the Button"
    );
    assert!(!tree.has_focus(label));
}

/// An explicit `focus_mode = 0` overrides the class-based default, so a Button
/// authored as FOCUS_NONE does not take focus.
#[test]
fn explicit_focus_mode_none_overrides_the_class_default() {
    let (mut tree, btn, _edit, _label) = focus_scene();
    tree.get_node_mut(btn)
        .unwrap()
        .set_property("focus_mode", Variant::Int(0));

    tree.push_input(&press(40.0, 15.0));
    assert_eq!(
        tree.focus_owner(),
        None,
        "an explicit focus_mode = FOCUS_NONE must beat the class default"
    );
}
