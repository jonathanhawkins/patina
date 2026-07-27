//! pat-0q2kx.190 — RemoteTransform2D, VisibleOnScreenNotifier2D and
//! TouchScreenButton actually do something at runtime.
//!
//! All three were registered in ClassDB with the right properties and signals
//! and were completely inert: `RemoteTransform2D`, `VisibleOnScreenNotifier2D`,
//! `VisibleOnScreenEnabler2D` and `TouchScreenButton` appeared **only** in the
//! class tables, the inspector schema and the default-stripping lists. Nothing
//! in `main_loop` or `scene_tree` mentioned any of them, so a scene containing
//! one behaved exactly as if the node were absent — the remote target never
//! moved, `screen_entered`/`screen_exited` never fired, and a touch button
//! swallowed nothing and pressed nothing.
//!
//! Godot: `upstream/godot/scene/2d/remote_transform_2d.cpp`,
//! `visible_on_screen_notifier_2d.cpp`, `touch_screen_button.cpp`.

use gdcore::math::{Rect2, Vector2};
use gdobject::signal::Connection;
use gdplatform::input::InputEvent;
use gdscene::behavioral_2d::{BUTTON_PRESSED_PROP, ON_SCREEN_PROP};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId, ProcessMode};
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const EPSILON: f32 = 1e-4;

/// Counts emissions of `signal` on `node` so the test observes the signal
/// reaching a listener, not merely that a connection exists.
fn count_signal(tree: &mut SceneTree, node: NodeId, signal: &str) -> Arc<AtomicUsize> {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let listener = node.object_id();
    tree.connect_signal(
        node,
        signal,
        Connection::with_callback(listener, "_on_signal", move |_args| {
            counter.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    hits
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test** — all three behaviors in one stepped tree, which is
/// the point: these are frame phases, not standalone functions.
#[test]
fn behavioral_2d_nodes_runtime_wiring() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // (a) A RemoteTransform2D and the sibling it drives.
    let remote_src = tree
        .add_child(root, Node::new("Remote", "RemoteTransform2D"))
        .unwrap();
    let follower = tree
        .add_child(root, Node::new("Follower", "Node2D"))
        .unwrap();
    tree.get_node_mut(remote_src).unwrap().set_property(
        "remote_path",
        Variant::NodePath(gdcore::node_path::NodePath::new("../Follower")),
    );
    node2d::set_position(&mut tree, remote_src, Vector2::new(120.0, 45.0));

    // (b) A notifier that starts inside the default 1152x648 viewport.
    let notifier = tree
        .add_child(root, Node::new("Notifier", "VisibleOnScreenNotifier2D"))
        .unwrap();
    node2d::set_position(&mut tree, notifier, Vector2::new(200.0, 200.0));
    let entered = count_signal(&mut tree, notifier, "screen_entered");
    let exited = count_signal(&mut tree, notifier, "screen_exited");

    // (c) A 64x64 touch button at the origin bound to the "jump" action.
    let button = tree
        .add_child(root, Node::new("Button", "TouchScreenButton"))
        .unwrap();
    {
        let n = tree.get_node_mut(button).unwrap();
        n.set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
        n.set_property("action", Variant::String("jump".to_string()));
    }
    let pressed_hits = count_signal(&mut tree, button, "pressed");
    let released_hits = count_signal(&mut tree, button, "released");

    let mut ml = MainLoop::new(tree);

    // -- (a) the sibling lands on the remote's global position ---------------
    ml.step(0.016);
    let pos = node2d::get_global_position(ml.tree(), follower);
    assert!(
        (pos - Vector2::new(120.0, 45.0)).length() < EPSILON,
        "the remote target must be moved to the RemoteTransform2D's global \
         position after one frame, got {pos:?}",
    );

    // -- (b) leaving the view emits screen_exited exactly once ---------------
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "a notifier spawned inside the view reports entering it",
    );
    assert_eq!(
        ml.tree()
            .get_node(notifier)
            .unwrap()
            .get_property(ON_SCREEN_PROP),
        Variant::Bool(true),
        "is_on_screen is true while inside the viewport rect",
    );

    // Well outside the 1152x648 viewport.
    node2d::set_position(ml.tree_mut(), notifier, Vector2::new(-5000.0, -5000.0));
    ml.step(0.016);

    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "leaving the view emits screen_exited exactly once",
    );
    assert_eq!(
        ml.tree()
            .get_node(notifier)
            .unwrap()
            .get_property(ON_SCREEN_PROP),
        Variant::Bool(false),
        "is_on_screen flips false once the rect leaves the view",
    );

    // Staying off-screen must not re-emit — `screen_exited` is an edge, and a
    // handler that spawns a death effect would fire it every frame otherwise.
    ml.step(0.016);
    ml.step(0.016);
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "screen_exited is emitted on the transition, not every frame after",
    );

    // -- (c) a touch inside the texture presses the button and its action ----
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(10.0, 10.0),
        pressed: true,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);

    assert_eq!(
        pressed_hits.load(Ordering::SeqCst),
        1,
        "a touch inside the texture bounds emits `pressed`",
    );
    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(true),
        "the button reports itself pressed",
    );
    assert!(
        ml.input_state().is_action_pressed("jump"),
        "the button injects its `action`, so Input sees it like a key binding",
    );

    // Releasing emits `released` and drops the action.
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(10.0, 10.0),
        pressed: false,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);

    assert_eq!(
        released_hits.load(Ordering::SeqCst),
        1,
        "lifting the finger emits `released`",
    );
    assert!(
        !ml.input_state().is_action_pressed("jump"),
        "the injected action is released with the button",
    );
}

// ===========================================================================
// RemoteTransform2D
// ===========================================================================

/// The three `update_*` toggles are independent — Godot lets you push position
/// without dragging rotation and scale along, which is the whole reason a
/// camera rig uses this node.
#[test]
fn remote_transform_update_flags_are_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let src = tree
        .add_child(root, Node::new("Remote", "RemoteTransform2D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node2D")).unwrap();

    tree.get_node_mut(src).unwrap().set_property(
        "remote_path",
        Variant::NodePath(gdcore::node_path::NodePath::new("../T")),
    );
    // Position only.
    tree.get_node_mut(src)
        .unwrap()
        .set_property("update_rotation", Variant::Bool(false));
    tree.get_node_mut(src)
        .unwrap()
        .set_property("update_scale", Variant::Bool(false));

    node2d::set_position(&mut tree, src, Vector2::new(7.0, 8.0));
    node2d::set_rotation(&mut tree, src, 1.25);
    node2d::set_scale(&mut tree, src, Vector2::new(3.0, 3.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert!(
        (node2d::get_position(ml.tree(), target) - Vector2::new(7.0, 8.0)).length() < EPSILON,
        "position is pushed",
    );
    assert!(
        node2d::get_rotation(ml.tree(), target).abs() < EPSILON,
        "rotation is NOT pushed when update_rotation is false",
    );
    assert!(
        (node2d::get_scale(ml.tree(), target) - Vector2::new(1.0, 1.0)).length() < EPSILON,
        "scale is NOT pushed when update_scale is false",
    );
}

/// With `use_global_coordinates` off, Godot copies the *local* transform —
/// so a source nested under a moved parent hands over its local offset, not
/// where it ended up on screen.
#[test]
fn remote_transform_local_mode_copies_local_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let holder = tree.add_child(root, Node::new("Holder", "Node2D")).unwrap();
    node2d::set_position(&mut tree, holder, Vector2::new(1000.0, 0.0));

    let src = tree
        .add_child(holder, Node::new("Remote", "RemoteTransform2D"))
        .unwrap();
    let target = tree.add_child(root, Node::new("T", "Node2D")).unwrap();
    {
        let n = tree.get_node_mut(src).unwrap();
        n.set_property(
            "remote_path",
            Variant::NodePath(gdcore::node_path::NodePath::new("/root/T")),
        );
        n.set_property("use_global_coordinates", Variant::Bool(false));
    }
    node2d::set_position(&mut tree, src, Vector2::new(5.0, 5.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    let pos = node2d::get_global_position(ml.tree(), target);
    assert!(
        (pos - Vector2::new(5.0, 5.0)).length() < EPSILON,
        "local mode copies the local offset (5,5), not the global (1005,5); got {pos:?}",
    );
}

/// A parent's transform must not be double-counted: pushing a global position
/// onto a target that is itself nested has to subtract the parent's transform,
/// or the target overshoots by the parent's offset every frame.
#[test]
fn remote_transform_global_push_accounts_for_target_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let src = tree
        .add_child(root, Node::new("Remote", "RemoteTransform2D"))
        .unwrap();
    node2d::set_position(&mut tree, src, Vector2::new(100.0, 100.0));

    let holder = tree.add_child(root, Node::new("Holder", "Node2D")).unwrap();
    node2d::set_position(&mut tree, holder, Vector2::new(40.0, 0.0));
    let target = tree.add_child(holder, Node::new("T", "Node2D")).unwrap();

    tree.get_node_mut(src).unwrap().set_property(
        "remote_path",
        Variant::NodePath(gdcore::node_path::NodePath::new("/root/Holder/T")),
    );

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    ml.step(0.016);

    let global = node2d::get_global_position(ml.tree(), target);
    assert!(
        (global - Vector2::new(100.0, 100.0)).length() < EPSILON,
        "the nested target must land on the source's GLOBAL position; got {global:?}",
    );
    let local = node2d::get_position(ml.tree(), target);
    assert!(
        (local - Vector2::new(60.0, 100.0)).length() < EPSILON,
        "its local position compensates for the holder's offset; got {local:?}",
    );
}

/// A remote_path pointing at the node itself would feed its own transform back
/// each frame. It must be ignored rather than looped.
#[test]
fn remote_transform_ignores_self_target() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let src = tree
        .add_child(root, Node::new("Remote", "RemoteTransform2D"))
        .unwrap();
    tree.get_node_mut(src).unwrap().set_property(
        "remote_path",
        Variant::NodePath(gdcore::node_path::NodePath::new(".")),
    );
    node2d::set_position(&mut tree, src, Vector2::new(3.0, 4.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    ml.step(0.016);

    assert!(
        (node2d::get_position(ml.tree(), src) - Vector2::new(3.0, 4.0)).length() < EPSILON,
        "a self-referential remote_path leaves the node where it was",
    );
}

// ===========================================================================
// VisibleOnScreenNotifier2D / Enabler2D
// ===========================================================================

/// The notifier tests its `rect`, not its origin: a node whose origin is off
/// the edge but whose rect still overlaps is on screen.
#[test]
fn notifier_tests_its_rect_not_its_origin() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("N", "VisibleOnScreenNotifier2D"))
        .unwrap();
    // A wide rect extending right from the origin.
    tree.get_node_mut(n).unwrap().set_property(
        "rect",
        Variant::Rect2(Rect2::new(Vector2::ZERO, Vector2::new(400.0, 20.0))),
    );
    // Origin is off the left edge, but the rect reaches x = 300 — on screen.
    node2d::set_position(&mut tree, n, Vector2::new(-100.0, 100.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert_eq!(
        ml.tree().get_node(n).unwrap().get_property(ON_SCREEN_PROP),
        Variant::Bool(true),
        "the rect still overlaps the viewport even though the origin does not",
    );
}

/// A camera scrolls the visible rect, so the same world position leaves the
/// view when the camera pans away — this is what makes the notifier useful.
#[test]
fn notifier_follows_the_current_camera() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = tree.add_child(root, Node::new("Cam", "Camera2D")).unwrap();
    tree.get_node_mut(cam)
        .unwrap()
        .set_property("current", Variant::Bool(true));

    let n = tree
        .add_child(root, Node::new("N", "VisibleOnScreenNotifier2D"))
        .unwrap();
    node2d::set_position(&mut tree, n, Vector2::new(0.0, 0.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    // The camera sits at the origin, so its visible rect is centered there and
    // a notifier at the origin is inside it.
    assert_eq!(
        ml.tree().get_node(n).unwrap().get_property(ON_SCREEN_PROP),
        Variant::Bool(true),
        "the notifier is at the camera's center",
    );

    // Pan the camera far away; the notifier has not moved but is now off view.
    node2d::set_position(ml.tree_mut(), cam, Vector2::new(9000.0, 9000.0));
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(n).unwrap().get_property(ON_SCREEN_PROP),
        Variant::Bool(false),
        "panning the camera away takes the notifier off screen",
    );
}

/// The Enabler subclass disables its target's processing off screen and
/// restores it on screen — the behavior the node exists for.
#[test]
fn enabler_toggles_its_target_process_mode() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let holder = tree.add_child(root, Node::new("Enemy", "Node2D")).unwrap();
    // Default enable_node_path is "..", i.e. the parent.
    let enabler = tree
        .add_child(holder, Node::new("Enabler", "VisibleOnScreenEnabler2D"))
        .unwrap();
    node2d::set_position(&mut tree, holder, Vector2::new(300.0, 300.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Inherit,
        "an on-screen target processes normally",
    );

    node2d::set_position(ml.tree_mut(), holder, Vector2::new(-9000.0, -9000.0));
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Disabled,
        "an off-screen target stops processing",
    );

    // Coming back into view re-enables it.
    node2d::set_position(ml.tree_mut(), holder, Vector2::new(300.0, 300.0));
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Inherit,
        "returning to view restores processing",
    );
    assert_eq!(
        ml.tree()
            .get_node(enabler)
            .unwrap()
            .get_property(ON_SCREEN_PROP),
        Variant::Bool(true),
        "…and the enabler reports itself on screen",
    );
}

// ===========================================================================
// TouchScreenButton
// ===========================================================================

/// A touch outside the texture bounds must not press the button — otherwise
/// every tap anywhere on screen would trigger every touch control.
#[test]
fn touch_outside_the_bounds_does_not_press() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree
        .add_child(root, Node::new("B", "TouchScreenButton"))
        .unwrap();
    {
        let n = tree.get_node_mut(button).unwrap();
        n.set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
        n.set_property("action", Variant::String("fire".to_string()));
    }
    let hits = count_signal(&mut tree, button, "pressed");

    let mut ml = MainLoop::new(tree);
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        // Well past the 64x64 texture.
        position: Vector2::new(500.0, 500.0),
        pressed: true,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);

    assert_eq!(hits.load(Ordering::SeqCst), 0, "a miss presses nothing");
    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(false),
        "the button stays unpressed",
    );
    assert!(
        !ml.input_state().is_action_pressed("fire"),
        "and no action is injected",
    );
}

/// The hit test runs in the button's local space, so a moved button is hit
/// where it appears — not where it was authored.
#[test]
fn touch_hit_test_follows_the_button_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree
        .add_child(root, Node::new("B", "TouchScreenButton"))
        .unwrap();
    tree.get_node_mut(button)
        .unwrap()
        .set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
    node2d::set_position(&mut tree, button, Vector2::new(400.0, 300.0));

    let mut ml = MainLoop::new(tree);

    // The authored origin is now empty space.
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(10.0, 10.0),
        pressed: true,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);
    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(false),
        "the old location no longer hits",
    );

    // Its new location does.
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(410.0, 310.0),
        pressed: true,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);
    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(true),
        "the moved button is hit at its new position",
    );
}

/// Releasing off the button still releases it. A finger dragged off the edge
/// before lifting must not leave the button — and its action — stuck down.
#[test]
fn release_away_from_the_button_still_releases_it() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree
        .add_child(root, Node::new("B", "TouchScreenButton"))
        .unwrap();
    {
        let n = tree.get_node_mut(button).unwrap();
        n.set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
        n.set_property("action", Variant::String("hold".to_string()));
    }
    let released = count_signal(&mut tree, button, "released");

    let mut ml = MainLoop::new(tree);
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(10.0, 10.0),
        pressed: true,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);
    assert!(ml.input_state().is_action_pressed("hold"), "pressed first");

    // Lift far away from the button.
    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(900.0, 600.0),
        pressed: false,
        double_tap: false,
        canceled: false,
    });
    ml.step(0.016);

    assert_eq!(
        released.load(Ordering::SeqCst),
        1,
        "the button releases even though the lift was outside it",
    );
    assert!(
        !ml.input_state().is_action_pressed("hold"),
        "the action must not stay stuck down",
    );
}

/// A drag across a button only presses it when `passby_press` is on. Without
/// the flag, sliding a finger over a control must not fire it.
#[test]
fn passby_press_gates_drag_activation() {
    fn button_scene(passby: bool) -> (MainLoop, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let button = tree
            .add_child(root, Node::new("B", "TouchScreenButton"))
            .unwrap();
        let n = tree.get_node_mut(button).unwrap();
        n.set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
        n.set_property("passby_press", Variant::Bool(passby));
        (MainLoop::new(tree), button)
    }

    let drag = InputEvent::ScreenDrag {
        index: 0,
        position: Vector2::new(20.0, 20.0),
        relative: Vector2::new(5.0, 0.0),
        velocity: Vector2::ZERO,
        pressure: 0.0,
        tilt: Vector2::ZERO,
    };

    let (mut plain, plain_btn) = button_scene(false);
    plain.push_event(drag.clone());
    plain.step(0.016);
    assert_eq!(
        plain
            .tree()
            .get_node(plain_btn)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(false),
        "a drag over a normal button does not press it",
    );

    let (mut passby, passby_btn) = button_scene(true);
    passby.push_event(drag);
    passby.step(0.016);
    assert_eq!(
        passby
            .tree()
            .get_node(passby_btn)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(true),
        "passby_press presses when a drag crosses the button",
    );

    // A bare mouse move is NOT a drag: it fires with no button held, so a
    // passby button must ignore it or hovering the cursor would press every
    // control it passed over.
    let (mut hover, hover_btn) = button_scene(true);
    hover.push_event(InputEvent::MouseMotion {
        position: Vector2::new(20.0, 20.0),
        relative: Vector2::new(5.0, 0.0),
    });
    hover.step(0.016);
    assert_eq!(
        hover
            .tree()
            .get_node(hover_btn)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(false),
        "hovering with no button held must not press a passby button",
    );
}

/// A right-click must not fire a touch button — only the left button stands in
/// for a finger.
#[test]
fn right_click_does_not_press_the_button() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree
        .add_child(root, Node::new("B", "TouchScreenButton"))
        .unwrap();
    tree.get_node_mut(button)
        .unwrap()
        .set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));

    let mut ml = MainLoop::new(tree);
    ml.push_event(InputEvent::MouseButton {
        button: gdplatform::input::MouseButton::Right,
        position: Vector2::new(10.0, 10.0),
        pressed: true,
    });
    ml.step(0.016);

    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(false),
        "a right-click inside the bounds leaves the button alone",
    );

    // …but a left click at the same point does press it, proving the miss above
    // is the button filter and not a broken hit test.
    ml.push_event(InputEvent::MouseButton {
        button: gdplatform::input::MouseButton::Left,
        position: Vector2::new(10.0, 10.0),
        pressed: true,
    });
    ml.step(0.016);
    assert_eq!(
        ml.tree()
            .get_node(button)
            .unwrap()
            .get_property(BUTTON_PRESSED_PROP),
        Variant::Bool(true),
        "a left click at the same point does press it",
    );
}
