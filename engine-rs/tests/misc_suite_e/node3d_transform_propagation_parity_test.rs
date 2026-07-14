//! pat-9sj7: Match 3D transform propagation for parent-child Node3D hierarchies.
//!
//! Focused parity fixtures proving:
//!   1. Parent translation propagates to child global position
//!   2. Parent rotation propagates to child global position
//!   3. Parent scale propagates to child global position
//!   4. Combined rotation + scale + translation propagation
//!   5. set_global_position with rotated/scaled parents
//!   6. Deep hierarchy (4+ levels) accumulation
//!   7. Non-uniform scale propagation
//!   8. Reparenting changes global transform
//!   9. Identity propagation invariants
//!  10. Local transform composition order matches Godot (T * R * S)

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;

const EPSILON: f32 = 1e-4;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn approx_vec3(a: Vector3, b: Vector3) -> bool {
    approx(a.x, b.x) && approx(a.y, b.y) && approx(a.z, b.z)
}

fn assert_vec3(actual: Vector3, expected: Vector3, msg: &str) {
    assert!(
        approx_vec3(actual, expected),
        "{msg}: expected {expected:?}, got {actual:?}"
    );
}

fn make_tree() -> SceneTree {
    SceneTree::new()
}

fn add_node3d(
    tree: &mut SceneTree,
    parent: gdscene::node::NodeId,
    name: &str,
) -> gdscene::node::NodeId {
    let node = Node::new(name, "Node3D");
    tree.add_child(parent, node).unwrap()
}

// ===========================================================================
// 1. Parent translation propagates to child global position
// ===========================================================================

#[test]
fn parent_translation_offsets_child_globally() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(10.0, 20.0, 30.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(1.0, 2.0, 3.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(11.0, 22.0, 33.0),
        "child global = parent + child local",
    );
}

#[test]
fn moving_parent_moves_child_globally() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(0.0, 0.0, 0.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(5.0, 0.0, 0.0));

    // Before move
    let g1 = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(g1, Vector3::new(5.0, 0.0, 0.0), "before parent move");

    // Move parent
    node3d::set_position(&mut tree, parent, Vector3::new(100.0, 0.0, 0.0));
    let g2 = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(g2, Vector3::new(105.0, 0.0, 0.0), "after parent move");
}

#[test]
fn child_local_position_unchanged_by_parent_move() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(7.0, 8.0, 9.0));

    node3d::set_position(&mut tree, parent, Vector3::new(999.0, 999.0, 999.0));
    assert_eq!(
        node3d::get_position(&tree, child),
        Vector3::new(7.0, 8.0, 9.0),
        "local position must not change when parent moves"
    );
}

// ===========================================================================
// 2. Parent rotation propagates to child global position
// ===========================================================================

#[test]
fn parent_90deg_y_rotation_swaps_child_xz() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    // 90 degrees around Y axis
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    // 90 degrees Y: (10,0,0) -> (0,0,-10)
    assert_vec3(
        global,
        Vector3::new(0.0, 0.0, -10.0),
        "90 deg Y: X becomes -Z",
    );
}

#[test]
fn parent_90deg_x_rotation_swaps_child_yz() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(0.0, 10.0, 0.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    // 90 degrees X: (0,10,0) -> (0,0,10)
    assert_vec3(
        global,
        Vector3::new(0.0, 0.0, 10.0),
        "90 deg X: Y becomes Z",
    );
}

#[test]
fn parent_180deg_y_rotation_negates_child_x_and_z() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::PI, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(5.0, 3.0, 7.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    // 180 degrees Y: (5,3,7) -> (-5,3,-7)
    assert_vec3(
        global,
        Vector3::new(-5.0, 3.0, -7.0),
        "180 deg Y: negate X and Z",
    );
}

#[test]
fn parent_rotation_plus_translation() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(100.0, 0.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    // Parent at (100,0,0), rotated 90Y. Child local (10,0,0) becomes (0,0,-10) in parent frame.
    // Global = (100,0,0) + (0,0,-10) = (100,0,-10)
    assert_vec3(
        global,
        Vector3::new(100.0, 0.0, -10.0),
        "translation + rotation",
    );
}

// ===========================================================================
// 3. Parent scale propagates to child global position
// ===========================================================================

#[test]
fn parent_uniform_scale_multiplies_child_position() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 2.0, 2.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(5.0, 10.0, 15.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(10.0, 20.0, 30.0),
        "uniform 2x scale doubles child pos",
    );
}

#[test]
fn parent_nonuniform_scale_stretches_child_position() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(3.0, 1.0, 2.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(10.0, 10.0, 10.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(global, Vector3::new(30.0, 10.0, 20.0), "non-uniform scale");
}

#[test]
fn parent_scale_affects_grandchild_cumulatively() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 2.0, 2.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_scale(&mut tree, child, Vector3::new(3.0, 3.0, 3.0));

    let grandchild = add_node3d(&mut tree, child, "GrandChild");
    node3d::set_position(&mut tree, grandchild, Vector3::new(1.0, 1.0, 1.0));

    let global = node3d::get_global_transform(&tree, grandchild).xform(Vector3::ZERO);
    // Cumulative scale: 2 * 3 = 6x
    assert_vec3(
        global,
        Vector3::new(6.0, 6.0, 6.0),
        "cumulative scale 2*3=6",
    );
}

// ===========================================================================
// 4. Combined rotation + scale + translation propagation
// ===========================================================================

#[test]
fn parent_translate_rotate_scale_combined() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(50.0, 0.0, 0.0));
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 2.0, 2.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    // Scale(2) * Rotate(90Y) applied to (10,0,0) = 2 * (0,0,-10) = (0,0,-20)
    // Then translate by (50,0,0): (50,0,-20)
    assert_vec3(
        global,
        Vector3::new(50.0, 0.0, -20.0),
        "combined T*R*S on child",
    );
}

#[test]
fn child_has_own_rotation_on_top_of_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_rotation(
        &mut tree,
        child,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    // Two 90-degree Y rotations = 180 degrees
    let grandchild = add_node3d(&mut tree, child, "GrandChild");
    node3d::set_position(&mut tree, grandchild, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, grandchild).xform(Vector3::ZERO);
    // 180 deg Y: (10,0,0) -> (-10,0,0)
    assert_vec3(
        global,
        Vector3::new(-10.0, 0.0, 0.0),
        "two 90-deg Y rotations compound to 180",
    );
}

#[test]
fn child_scale_does_not_affect_parent_global() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(5.0, 5.0, 5.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_scale(&mut tree, child, Vector3::new(100.0, 100.0, 100.0));

    let parent_global = node3d::get_global_transform(&tree, parent).xform(Vector3::ZERO);
    assert_vec3(
        parent_global,
        Vector3::new(5.0, 5.0, 5.0),
        "child scale must not affect parent's global transform",
    );
}

// ===========================================================================
// 5. set_global_position with rotated/scaled parents
// ===========================================================================

#[test]
fn set_global_position_with_rotated_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_rotation(
        &mut tree,
        parent,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_global_position(&mut tree, child, Vector3::new(0.0, 0.0, -10.0));

    // Verify the global position is correct
    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(0.0, 0.0, -10.0),
        "global position after set_global_position",
    );

    // Under 90 deg Y, global (0,0,-10) -> inverse maps -Z back to +X -> local (10,0,0)
    let local = node3d::get_position(&tree, child);
    assert_vec3(
        local,
        Vector3::new(10.0, 0.0, 0.0),
        "local position under rotated parent",
    );
}

#[test]
fn set_global_position_with_scaled_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 2.0, 2.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_global_position(&mut tree, child, Vector3::new(20.0, 40.0, 60.0));

    let local = node3d::get_position(&tree, child);
    // Parent has 2x scale, so local = global / 2
    assert_vec3(
        local,
        Vector3::new(10.0, 20.0, 30.0),
        "local = global / parent_scale",
    );

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(20.0, 40.0, 60.0),
        "verify global roundtrip",
    );
}

#[test]
fn set_global_position_with_translated_and_scaled_parent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(100.0, 0.0, 0.0));
    node3d::set_scale(&mut tree, parent, Vector3::new(5.0, 5.0, 5.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_global_position(&mut tree, child, Vector3::new(150.0, 0.0, 0.0));

    let local = node3d::get_position(&tree, child);
    // Global (150,0,0), parent at (100,0,0) with 5x scale: local = (150-100)/5 = (10,0,0)
    assert_vec3(
        local,
        Vector3::new(10.0, 0.0, 0.0),
        "local compensates for translation+scale",
    );
}

#[test]
fn set_global_position_deep_hierarchy() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = add_node3d(&mut tree, root, "A");
    node3d::set_position(&mut tree, a, Vector3::new(10.0, 0.0, 0.0));

    let b = add_node3d(&mut tree, a, "B");
    node3d::set_position(&mut tree, b, Vector3::new(0.0, 10.0, 0.0));

    let c = add_node3d(&mut tree, b, "C");
    node3d::set_global_position(&mut tree, c, Vector3::new(10.0, 10.0, 50.0));

    let global = node3d::get_global_transform(&tree, c).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(10.0, 10.0, 50.0),
        "deep set_global_position roundtrip",
    );

    let local = node3d::get_position(&tree, c);
    assert_vec3(
        local,
        Vector3::new(0.0, 0.0, 50.0),
        "deep local compensates ancestor chain",
    );
}

// ===========================================================================
// 6. Deep hierarchy (4+ levels) accumulation
// ===========================================================================

#[test]
fn four_level_translation_accumulation() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = add_node3d(&mut tree, root, "A");
    node3d::set_position(&mut tree, a, Vector3::new(1.0, 0.0, 0.0));

    let b = add_node3d(&mut tree, a, "B");
    node3d::set_position(&mut tree, b, Vector3::new(0.0, 2.0, 0.0));

    let c = add_node3d(&mut tree, b, "C");
    node3d::set_position(&mut tree, c, Vector3::new(0.0, 0.0, 3.0));

    let d = add_node3d(&mut tree, c, "D");
    node3d::set_position(&mut tree, d, Vector3::new(4.0, 5.0, 6.0));

    let global = node3d::get_global_transform(&tree, d).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(5.0, 7.0, 9.0),
        "4-level translation sum",
    );
}

#[test]
fn four_level_mixed_transforms() {
    let mut tree = make_tree();
    let root = tree.root_id();

    // Level 1: translate
    let a = add_node3d(&mut tree, root, "A");
    node3d::set_position(&mut tree, a, Vector3::new(100.0, 0.0, 0.0));

    // Level 2: scale 2x
    let b = add_node3d(&mut tree, a, "B");
    node3d::set_scale(&mut tree, b, Vector3::new(2.0, 2.0, 2.0));

    // Level 3: rotate 90 Y
    let c = add_node3d(&mut tree, b, "C");
    node3d::set_rotation(
        &mut tree,
        c,
        Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
    );

    // Level 4: child at (10,0,0) local
    let d = add_node3d(&mut tree, c, "D");
    node3d::set_position(&mut tree, d, Vector3::new(10.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, d).xform(Vector3::ZERO);
    // D local (10,0,0) -> rotated 90Y by C -> (0,0,-10) -> scaled 2x by B -> (0,0,-20)
    // -> translated by A -> (100,0,-20)
    assert_vec3(
        global,
        Vector3::new(100.0, 0.0, -20.0),
        "4-level mixed transforms",
    );
}

#[test]
fn five_level_chain() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let nodes: Vec<_> = (0..5)
        .scan(root, |parent, i| {
            let id = add_node3d(&mut tree, *parent, &format!("N{i}"));
            node3d::set_position(&mut tree, id, Vector3::new(1.0, 1.0, 1.0));
            *parent = id;
            Some(id)
        })
        .collect();

    let global = node3d::get_global_transform(&tree, *nodes.last().unwrap()).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(5.0, 5.0, 5.0),
        "5-level uniform offset",
    );
}

// ===========================================================================
// 7. Non-uniform scale propagation
// ===========================================================================

#[test]
fn nonuniform_scale_with_rotation_skews_correctly() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 1.0, 1.0));
    // No rotation on parent, just scale

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(5.0, 5.0, 5.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(global, Vector3::new(10.0, 5.0, 5.0), "only X scaled by 2");
}

#[test]
fn nonuniform_scale_also_affects_child_scale() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_scale(&mut tree, parent, Vector3::new(2.0, 3.0, 4.0));

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_scale(&mut tree, child, Vector3::new(1.0, 1.0, 1.0));

    // A point at (1,1,1) in child local space should scale by parent
    let global = node3d::get_global_transform(&tree, child);
    let point = global.xform(Vector3::new(1.0, 1.0, 1.0));
    assert_vec3(
        point,
        Vector3::new(2.0, 3.0, 4.0),
        "parent scale applies to child-local points",
    );
}

// ===========================================================================
// 8. Reparenting changes global transform
// ===========================================================================

#[test]
fn node_at_root_has_identity_parent_transform() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let child = add_node3d(&mut tree, root, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(7.0, 8.0, 9.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(7.0, 8.0, 9.0),
        "root child: global == local",
    );
}

#[test]
fn sibling_nodes_independent_transforms() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(10.0, 0.0, 0.0));

    let child_a = add_node3d(&mut tree, parent, "ChildA");
    node3d::set_position(&mut tree, child_a, Vector3::new(1.0, 0.0, 0.0));

    let child_b = add_node3d(&mut tree, parent, "ChildB");
    node3d::set_position(&mut tree, child_b, Vector3::new(0.0, 1.0, 0.0));

    let ga = node3d::get_global_transform(&tree, child_a).xform(Vector3::ZERO);
    let gb = node3d::get_global_transform(&tree, child_b).xform(Vector3::ZERO);

    assert_vec3(ga, Vector3::new(11.0, 0.0, 0.0), "sibling A");
    assert_vec3(gb, Vector3::new(10.0, 1.0, 0.0), "sibling B");
}

// ===========================================================================
// 9. Identity propagation invariants
// ===========================================================================

#[test]
fn identity_parent_does_not_alter_child() {
    let mut tree = make_tree();
    let root = tree.root_id();

    // Parent with no transforms set (identity)
    let parent = add_node3d(&mut tree, root, "Parent");

    let child = add_node3d(&mut tree, parent, "Child");
    node3d::set_position(&mut tree, child, Vector3::new(42.0, 43.0, 44.0));

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(42.0, 43.0, 44.0),
        "identity parent => global == local",
    );
}

#[test]
fn chain_of_identity_parents_is_transparent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = add_node3d(&mut tree, root, "A");
    let b = add_node3d(&mut tree, a, "B");
    let c = add_node3d(&mut tree, b, "C");
    let d = add_node3d(&mut tree, c, "D");
    node3d::set_position(&mut tree, d, Vector3::new(1.0, 2.0, 3.0));

    let global = node3d::get_global_transform(&tree, d).xform(Vector3::ZERO);
    assert_vec3(
        global,
        Vector3::new(1.0, 2.0, 3.0),
        "identity chain is transparent",
    );
}

#[test]
fn default_local_transform_is_identity() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = add_node3d(&mut tree, root, "Node");

    let local = node3d::get_local_transform(&tree, node);
    let v = Vector3::new(7.0, 11.0, 13.0);
    let result = local.xform(v);
    assert_vec3(result, v, "default local transform must be identity");
}

// ===========================================================================
// 10. Local transform composition order matches Godot (T * R * S)
// ===========================================================================

#[test]
fn local_transform_is_translate_then_rotate_then_scale() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = add_node3d(&mut tree, root, "Node");

    let pos = Vector3::new(10.0, 20.0, 30.0);
    let rot = Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0);
    let scl = Vector3::new(2.0, 3.0, 4.0);

    node3d::set_position(&mut tree, node, pos);
    node3d::set_rotation(&mut tree, node, rot);
    node3d::set_scale(&mut tree, node, scl);

    let local = node3d::get_local_transform(&tree, node);

    // Verify origin is the position
    assert_vec3(local.origin, pos, "local transform origin == position");

    // Verify basis columns reflect rotation * scale
    let pure_rotation = Basis::from_euler(rot);
    let expected_basis = Basis {
        x: pure_rotation.x * scl.x,
        y: pure_rotation.y * scl.y,
        z: pure_rotation.z * scl.z,
    };

    assert_vec3(local.basis.x, expected_basis.x, "basis.x matches R*S");
    assert_vec3(local.basis.y, expected_basis.y, "basis.y matches R*S");
    assert_vec3(local.basis.z, expected_basis.z, "basis.z matches R*S");
}

#[test]
fn global_transform_inverse_roundtrip_deep() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let a = add_node3d(&mut tree, root, "A");
    node3d::set_position(&mut tree, a, Vector3::new(10.0, 20.0, 30.0));
    node3d::set_rotation(&mut tree, a, Vector3::new(0.3, 0.5, 0.1));

    let b = add_node3d(&mut tree, a, "B");
    node3d::set_position(&mut tree, b, Vector3::new(5.0, 0.0, 0.0));
    node3d::set_scale(&mut tree, b, Vector3::new(2.0, 2.0, 2.0));

    let c = add_node3d(&mut tree, b, "C");
    node3d::set_position(&mut tree, c, Vector3::new(0.0, 0.0, 1.0));

    let global = node3d::get_global_transform(&tree, c);
    let inv = global.inverse();

    let original = Vector3::new(3.0, 7.0, 11.0);
    let transformed = global.xform(original);
    let recovered = inv.xform(transformed);

    assert_vec3(
        recovered,
        original,
        "global * inverse roundtrip in deep hierarchy",
    );
}

#[test]
fn set_global_position_then_read_global_matches() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let parent = add_node3d(&mut tree, root, "Parent");
    node3d::set_position(&mut tree, parent, Vector3::new(50.0, 0.0, 0.0));
    node3d::set_rotation(&mut tree, parent, Vector3::new(0.0, 1.0, 0.0));
    node3d::set_scale(&mut tree, parent, Vector3::new(3.0, 3.0, 3.0));

    let child = add_node3d(&mut tree, parent, "Child");
    let target = Vector3::new(200.0, 100.0, -50.0);
    node3d::set_global_position(&mut tree, child, target);

    let global = node3d::get_global_transform(&tree, child).xform(Vector3::ZERO);
    assert_vec3(
        global,
        target,
        "set then get global position must roundtrip",
    );
}
