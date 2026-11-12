//! pat-0q2kx.181 — a rotated Control is hit where it is DRAWN, not where its
//! unrotated rect used to be.
//!
//! Godot composes a Control's transform as
//! `translate(position) * translate(pivot) * rotate(rotation) * scale * translate(-pivot)`
//! and `Control::has_point` maps the query through the inverse
//! (`upstream/godot/scene/gui/control.cpp`). Tweens animate Control rotation and
//! scale routinely, so this is not an exotic path.
//!
//! Patina compared the click against the axis-aligned rect only, so a
//! 45°-rotated panel still swallowed clicks in its old corners and missed them
//! along its new diagonal.

use gdcore::math::Vector2;
use gdscene::control;
use gdscene::control::control_at_point;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A 100×100 ColorRect at (0,0), rotated 45° about its centre.
fn rotated_rect() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut rect = Node::new("Rot", "ColorRect");
    // Godot stores `rotation` in radians; pivot at the centre so the square
    // spins in place rather than swinging around its top-left corner.
    rect.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_4));
    rect.set_property("pivot_offset", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let id = tree.add_child(root, rect).unwrap();
    control::set_position(&mut tree, id, Vector2::new(0.0, 0.0));
    control::set_size(&mut tree, id, Vector2::new(100.0, 100.0));
    (tree, id)
}

/// **The acceptance test** (hit-test half).
#[test]
fn control_rotation_pivot_render_hit() {
    let (tree, rect) = rotated_rect();

    // The centre is inside the shape under any rotation — a sanity anchor that
    // proves the control is still hit-testable at all.
    assert_eq!(
        control_at_point(&tree, Vector2::new(50.0, 50.0)),
        Some(rect),
        "the pivot centre stays inside the rotated square",
    );

    // A 45° square about its centre has its corners at the edge midpoints of the
    // old AABB, and its diagonal reaches ~70.7 from the centre. So (5,5) — deep
    // inside the ORIGINAL top-left corner — is now OUTSIDE the rotated shape.
    // This is the assertion the old axis-aligned test could never make.
    assert_ne!(
        control_at_point(&tree, Vector2::new(5.0, 5.0)),
        Some(rect),
        "a click in the old AABB corner must NOT hit the rotated square",
    );

    // …while a point along the new diagonal — (50, 5), the top vertex region of
    // the rotated square — IS inside it, and was inside the AABB too.
    assert_eq!(
        control_at_point(&tree, Vector2::new(50.0, 8.0)),
        Some(rect),
        "a click inside the rotated shape must hit it",
    );
}

/// Scale is inverted too, and a degenerate zero scale must not swallow clicks.
#[test]
fn scale_is_inverted_and_zero_scale_is_safe() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rect = Node::new("Scaled", "ColorRect");
    // 2× scale about the origin: the control covers twice its rect on screen,
    // so a click at (150,50) maps back to local (75,25) — inside 0..100.
    rect.set_property("scale", Variant::Vector2(Vector2::new(2.0, 2.0)));
    let id = tree.add_child(root, rect).unwrap();
    control::set_position(&mut tree, id, Vector2::new(0.0, 0.0));
    control::set_size(&mut tree, id, Vector2::new(100.0, 100.0));

    assert_eq!(
        control_at_point(&tree, Vector2::new(150.0, 50.0)),
        Some(id),
        "a 2x-scaled control is hit across its scaled extent",
    );

    // A zero scale is degenerate: dividing by it yields NaN, and every later
    // comparison would be false — silently swallowing the click. It falls back
    // to unit scale instead.
    let mut tree2 = SceneTree::new();
    let root2 = tree2.root_id();
    let mut degenerate = Node::new("Zero", "ColorRect");
    degenerate.set_property("scale", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let z = tree2.add_child(root2, degenerate).unwrap();
    control::set_position(&mut tree2, z, Vector2::new(0.0, 0.0));
    control::set_size(&mut tree2, z, Vector2::new(100.0, 100.0));

    assert_eq!(
        control_at_point(&tree2, Vector2::new(50.0, 50.0)),
        Some(z),
        "a zero scale must not make the control unhittable",
    );
}

/// An untransformed control keeps exact integer behaviour — the fast path must
/// not route the common case through float trig.
#[test]
fn untransformed_controls_are_unaffected() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let plain = tree
        .add_child(root, Node::new("Plain", "ColorRect"))
        .unwrap();
    control::set_position(&mut tree, plain, Vector2::new(10.0, 10.0));
    control::set_size(&mut tree, plain, Vector2::new(80.0, 40.0));

    assert_eq!(
        control_at_point(&tree, Vector2::new(50.0, 30.0)),
        Some(plain),
        "an untransformed control still hit-tests by its plain rect",
    );
    assert_ne!(
        control_at_point(&tree, Vector2::new(95.0, 30.0)),
        Some(plain),
        "…and still misses outside it",
    );
}
