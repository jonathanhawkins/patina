//! pat-0q2kx.198 — `VisibleOnScreenNotifier3D` / `VisibleOnScreenEnabler3D`
//! frustum culling actually runs.
//!
//! Every ingredient existed and nothing connected them: `Frustum::intersects_aabb`
//! and `get_camera_frustum` were written and tested in `camera3d.rs`, the
//! notifier was a bare ClassDB entry with **no `aabb` and no signals**, and
//! `VisibleOnScreenEnabler3D` was not registered anywhere at all. `main_loop`
//! never mentioned either class, so a notifier in a played scene could not be
//! authored, could not be connected to, and never fired.
//!
//! Godot: `upstream/godot/scene/3d/visible_on_screen_notifier_3d.cpp`.

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId, ProcessMode};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdscene::visibility_3d::ON_SCREEN_PROP;
use gdvariant::Variant;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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

/// A scene with a `current` Camera3D at the origin. Patina's Camera3D looks
/// down **-Z**, so -Z is in front of it and +Z is behind.
fn scene_with_camera() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    tree.get_node_mut(cam)
        .unwrap()
        .set_property("current", Variant::Bool(true));
    (tree, root)
}

fn on_screen(ml: &MainLoop, id: NodeId) -> bool {
    matches!(
        ml.tree().get_node(id).unwrap().get_property(ON_SCREEN_PROP),
        Variant::Bool(true)
    )
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** Moving a notifier across the camera's frustum
/// boundary fires `screen_entered` then `screen_exited` exactly once each.
#[test]
fn visible_on_screen_notifier3d_signals() {
    let (mut tree, root) = scene_with_camera();

    let notifier = tree
        .add_child(root, Node::new("Notif", "VisibleOnScreenNotifier3D"))
        .unwrap();
    // Start behind the camera — off screen.
    node3d::set_position(&mut tree, notifier, Vector3::new(0.0, 0.0, 20.0));

    let entered = count_signal(&mut tree, notifier, "screen_entered");
    let exited = count_signal(&mut tree, notifier, "screen_exited");

    let mut ml = MainLoop::new(tree);

    ml.step(0.016);
    assert!(!on_screen(&ml, notifier), "starts behind the camera");
    assert_eq!(
        entered.load(Ordering::SeqCst),
        0,
        "nothing entered the view yet",
    );

    // -- move in front of the camera: enters -------------------------------
    node3d::set_position(ml.tree_mut(), notifier, Vector3::new(0.0, 0.0, -10.0));
    ml.step(0.016);

    assert!(on_screen(&ml, notifier), "now in front of the camera");
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "crossing into the frustum emits screen_entered exactly once",
    );

    // Staying inside must not re-emit — `screen_entered` is an edge.
    ml.step(0.016);
    ml.step(0.016);
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "screen_entered fires on the transition, not every frame",
    );
    assert_eq!(exited.load(Ordering::SeqCst), 0, "it has not left yet");

    // -- move back behind the camera: exits ---------------------------------
    node3d::set_position(ml.tree_mut(), notifier, Vector3::new(0.0, 0.0, 20.0));
    ml.step(0.016);

    assert!(!on_screen(&ml, notifier), "behind the camera again");
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "leaving the frustum emits screen_exited exactly once",
    );

    ml.step(0.016);
    assert_eq!(
        exited.load(Ordering::SeqCst),
        1,
        "screen_exited does not re-fire while it stays outside",
    );
    assert_eq!(
        entered.load(Ordering::SeqCst),
        1,
        "…and entering did not fire again either",
    );
}

// ===========================================================================
// AABB handling
// ===========================================================================

/// The notifier tests its **`aabb`**, not its origin: a box whose origin is
/// outside the frustum but which still overlaps it counts as on screen.
#[test]
fn notifier_tests_its_aabb_not_its_origin() {
    let (mut tree, root) = scene_with_camera();
    let n = tree
        .add_child(root, Node::new("N", "VisibleOnScreenNotifier3D"))
        .unwrap();

    // A box stretching far along -Z from an origin that sits behind the camera.
    tree.get_node_mut(n).unwrap().set_property(
        "aabb",
        Variant::Aabb(Aabb::new(
            Vector3::new(-1.0, -1.0, -40.0),
            Vector3::new(2.0, 2.0, 45.0),
        )),
    );
    node3d::set_position(&mut tree, n, Vector3::new(0.0, 0.0, 5.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert!(
        on_screen(&ml, n),
        "the aabb reaches in front of the camera even though the origin does not",
    );
}

/// The aabb travels with the node's transform, so a moved notifier is tested
/// where it actually is.
#[test]
fn notifier_aabb_follows_the_node_transform() {
    let (mut tree, root) = scene_with_camera();
    let n = tree
        .add_child(root, Node::new("N", "VisibleOnScreenNotifier3D"))
        .unwrap();
    node3d::set_position(&mut tree, n, Vector3::new(0.0, 0.0, -10.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    assert!(on_screen(&ml, n), "in front to begin with");

    // Far off to the side, well outside any sane horizontal FOV.
    node3d::set_position(ml.tree_mut(), n, Vector3::new(10_000.0, 0.0, -10.0));
    ml.step(0.016);
    assert!(!on_screen(&ml, n), "moved outside the frustum sideways");
}

/// Without a current camera nothing is rendered, so nothing is on screen — and
/// crucially no `screen_entered` storm fires on the first frame of a scene that
/// has not set a camera up yet.
#[test]
fn no_current_camera_means_nothing_is_on_screen() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("N", "VisibleOnScreenNotifier3D"))
        .unwrap();
    let entered = count_signal(&mut tree, n, "screen_entered");

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    ml.step(0.016);

    assert!(!on_screen(&ml, n), "no camera, nothing visible");
    assert_eq!(
        entered.load(Ordering::SeqCst),
        0,
        "a cameraless scene must not emit screen_entered",
    );
}

// ===========================================================================
// VisibleOnScreenEnabler3D
// ===========================================================================

/// The Enabler subclass disables its target's processing off screen and
/// restores it on screen — the behavior the node exists for.
#[test]
fn enabler3d_toggles_its_target_process_mode() {
    let (mut tree, root) = scene_with_camera();

    let holder = tree.add_child(root, Node::new("Enemy", "Node3D")).unwrap();
    // Default enable_node_path is "..", i.e. the parent.
    let enabler = tree
        .add_child(holder, Node::new("Enabler", "VisibleOnScreenEnabler3D"))
        .unwrap();

    // Behind the camera to start with.
    node3d::set_position(&mut tree, holder, Vector3::new(0.0, 0.0, 20.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Disabled,
        "an off-screen target stops processing",
    );

    // Bring it in front of the camera.
    node3d::set_position(ml.tree_mut(), holder, Vector3::new(0.0, 0.0, -10.0));
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Inherit,
        "coming into view restores processing",
    );
    assert!(
        on_screen(&ml, enabler),
        "…and the enabler reports itself on screen",
    );

    // And back out again.
    node3d::set_position(ml.tree_mut(), holder, Vector3::new(0.0, 0.0, 20.0));
    ml.step(0.016);
    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Disabled,
        "leaving the view disables it again",
    );
}

/// `enable_mode = ENABLE_MODE_ALWAYS` (1) restores the target to `Always`
/// rather than `Inherit`, so it keeps running even while the tree is paused.
#[test]
fn enabler3d_honors_enable_mode() {
    let (mut tree, root) = scene_with_camera();
    let holder = tree.add_child(root, Node::new("Enemy", "Node3D")).unwrap();
    let enabler = tree
        .add_child(holder, Node::new("Enabler", "VisibleOnScreenEnabler3D"))
        .unwrap();
    tree.get_node_mut(enabler)
        .unwrap()
        .set_property("enable_mode", Variant::Int(1));
    node3d::set_position(&mut tree, holder, Vector3::new(0.0, 0.0, -10.0));

    let mut ml = MainLoop::new(tree);
    ml.step(0.016);

    assert_eq!(
        ml.tree().get_node(holder).unwrap().process_mode(),
        ProcessMode::Always,
        "ENABLE_MODE_ALWAYS is honored, not flattened to Inherit",
    );
}

// ===========================================================================
// ClassDB registration
// ===========================================================================

/// Both classes must carry their authored surface, or a `.tscn` has nowhere to
/// put `aabb` / `enable_node_path` and a script has no signal to connect to.
#[test]
fn visibility_3d_classes_register_their_surface() {
    gdobject::class_db::register_2d_classes();

    let notifier = gdobject::class_db::get_class_info("VisibleOnScreenNotifier3D")
        .expect("VisibleOnScreenNotifier3D must be registered");
    let props: Vec<&str> = notifier
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        props.contains(&"aabb"),
        "the notifier must register `aabb`; has {props:?}",
    );
    let signals: Vec<&str> = notifier.signals.iter().map(|s| s.name.as_str()).collect();
    assert!(
        signals.contains(&"screen_entered") && signals.contains(&"screen_exited"),
        "both screen signals must be registered; has {signals:?}",
    );
    assert_eq!(
        notifier.parent_class, "VisualInstance3D",
        "Godot derives the notifier from VisualInstance3D, not Node3D",
    );

    let enabler = gdobject::class_db::get_class_info("VisibleOnScreenEnabler3D")
        .expect("VisibleOnScreenEnabler3D must be registered at all");
    let eprops: Vec<&str> = enabler.properties.iter().map(|p| p.name.as_str()).collect();
    for expected in ["enable_mode", "enable_node_path"] {
        assert!(
            eprops.contains(&expected),
            "the enabler must register `{expected}`; has {eprops:?}",
        );
    }
    assert_eq!(
        enabler.parent_class, "VisibleOnScreenNotifier3D",
        "the enabler is a notifier subclass, so it inherits aabb and the signals",
    );
}
