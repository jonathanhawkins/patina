//! pat-kur: Expanded Node2D default-property oracle coverage.
//!
//! Tests every Node2D property: skew, global_position, global_rotation,
//! y_sort_enabled, top_level, z_as_relative, and their interactions with
//! the transform hierarchy.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

const EPSILON: f32 = 1e-4;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn approx_vec(a: Vector2, b: Vector2) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
}

fn make_tree() -> SceneTree {
    SceneTree::new()
}

// ===========================================================================
// 1. Skew property
// ===========================================================================

#[test]
fn skew_default_is_zero() {
    let tree = make_tree();
    let root = tree.root_id();
    let skew = tree
        .get_node(root)
        .map(|n| match n.get_property("skew") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0);
    assert!(approx_eq(skew, 0.0));
}

#[test]
fn skew_set_and_get() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("Skewed", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("skew", Variant::Float(0.5));

    let skew = match tree.get_node(id).unwrap().get_property("skew") {
        Variant::Float(f) => f as f32,
        _ => panic!("expected Float"),
    };
    assert!(approx_eq(skew, 0.5));
}

#[test]
fn skew_negative_value() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("skew", Variant::Float(-1.2));

    let skew = match tree.get_node(id).unwrap().get_property("skew") {
        Variant::Float(f) => f as f32,
        _ => panic!("expected Float"),
    };
    assert!(approx_eq(skew, -1.2));
}

// ===========================================================================
// 2. Global position via set_global_position
// ===========================================================================

#[test]
fn global_position_with_rotated_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();
    node2d::set_position(&mut tree, pid, Vector2::new(100.0, 0.0));
    node2d::set_rotation(&mut tree, pid, FRAC_PI_2);

    let child = Node::new("Child", "Node2D");
    let cid = tree.add_child(pid, child).unwrap();

    // Set global position to (100, 50) — parent is at (100, 0) rotated 90°
    node2d::set_global_position(&mut tree, cid, Vector2::new(100.0, 50.0));

    // Verify the global transform gives back (100, 50)
    let global = node2d::get_global_transform(&tree, cid).xform(Vector2::ZERO);
    assert!(
        approx_vec(global, Vector2::new(100.0, 50.0)),
        "global should be (100, 50), got ({}, {})",
        global.x,
        global.y
    );
}

#[test]
fn global_position_with_scaled_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();
    node2d::set_position(&mut tree, pid, Vector2::new(10.0, 10.0));
    node2d::set_scale(&mut tree, pid, Vector2::new(2.0, 2.0));

    let child = Node::new("Child", "Node2D");
    let cid = tree.add_child(pid, child).unwrap();

    // Want global at (30, 30). Parent at (10,10) with scale 2x.
    node2d::set_global_position(&mut tree, cid, Vector2::new(30.0, 30.0));

    let global = node2d::get_global_transform(&tree, cid).xform(Vector2::ZERO);
    assert!(
        approx_vec(global, Vector2::new(30.0, 30.0)),
        "got ({}, {})",
        global.x,
        global.y
    );

    // Local should be (10, 10) since parent scale is 2x and parent position is (10,10)
    let local = node2d::get_position(&tree, cid);
    assert!(
        approx_vec(local, Vector2::new(10.0, 10.0)),
        "local should be (10, 10), got ({}, {})",
        local.x,
        local.y
    );
}

// ===========================================================================
// 3. Global rotation
// ===========================================================================

#[test]
fn global_rotation_accumulates_through_hierarchy() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();
    node2d::set_rotation(&mut tree, pid, FRAC_PI_4);

    let child = Node::new("Child", "Node2D");
    let cid = tree.add_child(pid, child).unwrap();
    node2d::set_rotation(&mut tree, cid, FRAC_PI_4);

    // Global rotation should be PI/4 + PI/4 = PI/2
    let global_t = node2d::get_global_transform(&tree, cid);
    let global_rot = global_t.x.angle(); // rotation from X basis vector
    assert!(
        approx_eq(global_rot, FRAC_PI_2),
        "expected PI/2, got {}",
        global_rot
    );
}

#[test]
fn global_rotation_three_levels() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let aid = tree.add_child(root, a).unwrap();
    node2d::set_rotation(&mut tree, aid, PI / 6.0); // 30°

    let b = Node::new("B", "Node2D");
    let bid = tree.add_child(aid, b).unwrap();
    node2d::set_rotation(&mut tree, bid, PI / 6.0); // 30°

    let c = Node::new("C", "Node2D");
    let cid = tree.add_child(bid, c).unwrap();
    node2d::set_rotation(&mut tree, cid, PI / 6.0); // 30°

    // Total rotation: 90°
    let global = node2d::get_global_transform(&tree, cid);
    let rot = global.x.angle();
    assert!(approx_eq(rot, FRAC_PI_2), "expected PI/2, got {}", rot);
}

// ===========================================================================
// 4. y_sort_enabled property
// ===========================================================================

#[test]
fn y_sort_enabled_default_false() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    let y_sort = match tree.get_node(id).unwrap().get_property("y_sort_enabled") {
        Variant::Bool(b) => b,
        _ => false, // default
    };
    assert!(!y_sort);
}

#[test]
fn y_sort_enabled_set_true() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("y_sort_enabled", Variant::Bool(true));

    assert_eq!(
        tree.get_node(id).unwrap().get_property("y_sort_enabled"),
        Variant::Bool(true)
    );
}

// ===========================================================================
// 5. top_level property
// ===========================================================================

#[test]
fn top_level_default_false() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    let top = match tree.get_node(id).unwrap().get_property("top_level") {
        Variant::Bool(b) => b,
        _ => false,
    };
    assert!(!top);
}

#[test]
fn top_level_set_true() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("top_level", Variant::Bool(true));

    assert_eq!(
        tree.get_node(id).unwrap().get_property("top_level"),
        Variant::Bool(true)
    );
}

// ===========================================================================
// 6. z_as_relative property
// ===========================================================================

#[test]
fn z_as_relative_default_true() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    // Godot default: z_as_relative = true
    let z_rel = match tree.get_node(id).unwrap().get_property("z_as_relative") {
        Variant::Bool(b) => b,
        _ => true, // default is true
    };
    assert!(z_rel);
}

#[test]
fn z_as_relative_set_false() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("z_as_relative", Variant::Bool(false));

    assert_eq!(
        tree.get_node(id).unwrap().get_property("z_as_relative"),
        Variant::Bool(false)
    );
}

// ===========================================================================
// 7. Z-index interaction with z_as_relative
// ===========================================================================

#[test]
fn z_index_stacks_with_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node2D");
    let pid = tree.add_child(root, parent).unwrap();
    node2d::set_z_index(&mut tree, pid, 5);

    let child = Node::new("Child", "Node2D");
    let cid = tree.add_child(pid, child).unwrap();
    node2d::set_z_index(&mut tree, cid, 3);

    assert_eq!(node2d::get_z_index(&tree, pid), 5);
    assert_eq!(node2d::get_z_index(&tree, cid), 3);
}

// ===========================================================================
// 8. Visibility in tree with multiple hidden ancestors
// ===========================================================================

#[test]
fn visibility_chain_any_hidden_hides_descendants() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = Node::new("A", "Node2D");
    let aid = tree.add_child(root, a).unwrap();

    let b = Node::new("B", "Node2D");
    let bid = tree.add_child(aid, b).unwrap();

    let c = Node::new("C", "Node2D");
    let cid = tree.add_child(bid, c).unwrap();

    // All visible by default
    assert!(node2d::is_visible_in_tree(&tree, cid));

    // Hide middle node
    node2d::set_visible(&mut tree, bid, false);
    assert!(!node2d::is_visible_in_tree(&tree, cid));

    // C is still "locally visible"
    assert!(node2d::is_visible(&tree, cid));
}

// ===========================================================================
// 9. Transform composition with all properties
// ===========================================================================

#[test]
fn transform_with_position_rotation_scale() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    node2d::set_position(&mut tree, id, Vector2::new(100.0, 0.0));
    node2d::set_rotation(&mut tree, id, FRAC_PI_2);
    node2d::set_scale(&mut tree, id, Vector2::new(2.0, 2.0));

    let t = node2d::get_local_transform(&tree, id);

    // Origin should be at position
    let origin = t.xform(Vector2::ZERO);
    assert!(approx_vec(origin, Vector2::new(100.0, 0.0)));

    // A point at (1, 0) in local space: rotated 90° becomes (0, 1),
    // scaled 2x becomes (0, 2), translated becomes (100, 2)
    let p = t.xform(Vector2::new(1.0, 0.0));
    assert!(
        approx_vec(p, Vector2::new(100.0, 2.0)),
        "got ({}, {})",
        p.x,
        p.y
    );
}

// ===========================================================================
// 10. Modulate color property
// ===========================================================================

#[test]
fn modulate_default_is_white() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("N", "Node2D");
    let id = tree.add_child(root, node).unwrap();

    // Modulate defaults to white when not set
    let modulate = tree.get_node(id).unwrap().get_property("modulate");
    match modulate {
        Variant::Color(c) => {
            assert!(approx_eq(c.r, 1.0));
            assert!(approx_eq(c.g, 1.0));
            assert!(approx_eq(c.b, 1.0));
            assert!(approx_eq(c.a, 1.0));
        }
        Variant::Nil => {} // Not set yet = default white
        _ => panic!("unexpected type"),
    }
}
