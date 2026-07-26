//! pat-0q2kx.178 — a `SubViewportContainer` forwards input into its sub-scene.
//!
//! Godot's SubViewportContainer draws its SubViewport child's texture scaled by
//! `stretch_shrink` and forwards mouse events transformed into the sub-viewport's
//! coordinate space (`subviewport_container.cpp::_propagate_input_event`).
//!
//! Patina sized the child and had the coordinate mapper
//! (`subviewport_container_to_viewport_coord`), but the mapper's **only caller
//! was its own unit test** — so a click over the container hit-tested the
//! sub-scene at unscaled coordinates, meaning a control rendered at half scale
//! was only reachable where it was *not* drawn.

use gdcore::math::Vector2;
use gdscene::control;
use gdscene::control::{control_at_point, subviewport_container_to_viewport_coord};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A container at (100,100) sized 200×200 with `stretch`/`shrink=2`, hosting a
/// SubViewport whose Button sits at sub-viewport (20,20) sized 40×40.
fn scene_with_subviewport() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut container = Node::new("SVC", "SubViewportContainer");
    container.set_property("stretch", Variant::Bool(true));
    container.set_property("stretch_shrink", Variant::Int(2));
    let svc = tree.add_child(root, container).unwrap();
    control::set_position(&mut tree, svc, Vector2::new(100.0, 100.0));
    control::set_size(&mut tree, svc, Vector2::new(200.0, 200.0));

    let sv = tree
        .add_child(svc, Node::new("SubViewport", "SubViewport"))
        .unwrap();

    let button = tree.add_child(sv, Node::new("Btn", "Button")).unwrap();
    control::set_position(&mut tree, button, Vector2::new(20.0, 20.0));
    control::set_size(&mut tree, button, Vector2::new(40.0, 40.0));

    (tree, button)
}

/// **The acceptance test** (input half).
#[test]
fn subviewport_container_composite_and_input() {
    let (tree, button) = scene_with_subviewport();

    // The container is at (100,100) and shrink is 2, so sub-viewport (30,30) —
    // the middle of the Button — is reached by clicking container-local (60,60),
    // i.e. global (160,160). This is the mapping the audit says was missing.
    let hit = control_at_point(&tree, Vector2::new(160.0, 160.0));
    assert_eq!(
        hit,
        Some(button),
        "a click at container-local (60,60) must reach the Button at sub-viewport (30,30)",
    );

    // Without the re-targeting this is where the Button would have been hit:
    // global (120,120) is sub-viewport (10,10), OUTSIDE the button's 20..60 rect.
    // Asserting the negative is what distinguishes a real mapping from an
    // accidental pass.
    assert_ne!(
        control_at_point(&tree, Vector2::new(120.0, 120.0)),
        Some(button),
        "unscaled coordinates must NOT hit the button — that was the old bug",
    );

    // A point past the button's scaled extent misses it too: sub-viewport (70,70)
    // is beyond the 20..60 rect, reached at global (240,240).
    assert_ne!(
        control_at_point(&tree, Vector2::new(240.0, 240.0)),
        Some(button),
        "a click beyond the button's mapped rect must not hit it",
    );
}

/// The mapper itself, and its no-op cases — `stretch` off or `shrink <= 1`
/// leaves coordinates untouched, which is Godot's behaviour.
#[test]
fn coordinate_mapper_only_scales_when_stretching() {
    let p = Vector2::new(60.0, 40.0);

    let scaled = subviewport_container_to_viewport_coord(p, true, 2);
    assert_eq!(
        scaled,
        Vector2::new(30.0, 20.0),
        "shrink 2 halves the point"
    );

    assert_eq!(
        subviewport_container_to_viewport_coord(p, false, 2),
        p,
        "without stretch the point passes through unchanged",
    );
    assert_eq!(
        subviewport_container_to_viewport_coord(p, true, 1),
        p,
        "shrink 1 is identity",
    );
}

/// A container with default settings must not disturb ordinary hit-testing.
#[test]
fn default_container_does_not_rescale() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // No `stretch` property set → Godot's default is false.
    let svc = tree
        .add_child(root, Node::new("SVC", "SubViewportContainer"))
        .unwrap();
    control::set_position(&mut tree, svc, Vector2::new(0.0, 0.0));
    control::set_size(&mut tree, svc, Vector2::new(200.0, 200.0));
    let sv = tree
        .add_child(svc, Node::new("SubViewport", "SubViewport"))
        .unwrap();
    let button = tree.add_child(sv, Node::new("Btn", "Button")).unwrap();
    control::set_position(&mut tree, button, Vector2::new(20.0, 20.0));
    control::set_size(&mut tree, button, Vector2::new(40.0, 40.0));

    assert_eq!(
        control_at_point(&tree, Vector2::new(30.0, 30.0)),
        Some(button),
        "with stretch off, coordinates pass through unscaled",
    );
}
