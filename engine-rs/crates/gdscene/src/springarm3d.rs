//! SpringArm3D scene node helpers.
//!
//! A `SpringArm3D` node extends along its local `-Z` axis by `spring_length`
//! and casts a ray toward that tip each process call. If the ray hits
//! something, the arm retracts to the hit distance minus a configurable
//! `margin`, keeping a third-person camera from clipping into walls.
//!
//! This module mirrors Godot's `SpringArm3D` on top of
//! [`PhysicsRayQuery3D`](gdphysics3d::query::PhysicsRayQuery3D) and the
//! scene-tree [`PhysicsServer3D`](crate::physics_server_3d::PhysicsServer3D).
//!
//! State is stored as [`Variant`] properties on the node itself so the module
//! stays consistent with the other `*3d` helper modules:
//!
//! - Configuration: `spring_length`, `margin`, `collision_mask`.
//! - Result cache: `_hit_length`, `_hit_colliding`, `_hit_collider`,
//!   `_hit_point`, `_hit_normal`.

use std::collections::HashSet;

use gdcore::math::Vector3;
use gdphysics3d::body::BodyId3D;
use gdphysics3d::query::PhysicsRayQuery3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::physics_server_3d::PhysicsServer3D;
use crate::scene_tree::SceneTree;

const DEFAULT_SPRING_LENGTH: f32 = 1.0;
const DEFAULT_MARGIN: f32 = 0.01;
const DEFAULT_COLLISION_MASK: u32 = 0xFFFFFFFF;

// ===========================================================================
// Configuration properties
// ===========================================================================

/// Sets the maximum arm length along the node's local `-Z` axis.
pub fn set_spring_length(tree: &mut SceneTree, node_id: NodeId, length: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("spring_length", Variant::Float(length.max(0.0) as f64));
    }
}

/// Reads the maximum arm length, defaulting to `1.0`.
pub fn get_spring_length(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("spring_length") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => DEFAULT_SPRING_LENGTH,
        })
        .unwrap_or(DEFAULT_SPRING_LENGTH)
}

/// Sets the safety margin subtracted from the hit distance when the arm
/// retracts.
pub fn set_margin(tree: &mut SceneTree, node_id: NodeId, margin: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("margin", Variant::Float(margin.max(0.0) as f64));
    }
}

/// Reads the margin subtracted on hit, defaulting to `0.01`.
pub fn get_margin(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("margin") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => DEFAULT_MARGIN,
        })
        .unwrap_or(DEFAULT_MARGIN)
}

/// Sets the collision layer mask filter for the arm.
pub fn set_collision_mask(tree: &mut SceneTree, node_id: NodeId, mask: u32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("collision_mask", Variant::Int(mask as i64));
    }
}

/// Reads the collision layer mask filter, defaulting to `0xFFFFFFFF`.
pub fn get_collision_mask(tree: &SceneTree, node_id: NodeId) -> u32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("collision_mask") {
            Variant::Int(i) => i as u32,
            Variant::Float(f) => f as u32,
            _ => DEFAULT_COLLISION_MASK,
        })
        .unwrap_or(DEFAULT_COLLISION_MASK)
}

/// Adds a physics body node to the arm's exclusion set.
pub fn add_excluded_object(tree: &mut SceneTree, node_id: NodeId, excluded: NodeId) {
    let mut list = read_exclusions(tree, node_id);
    if !list.contains(&excluded.raw()) {
        list.push(excluded.raw());
        write_exclusions(tree, node_id, &list);
    }
}

/// Removes a previously added exclusion.
pub fn remove_excluded_object(tree: &mut SceneTree, node_id: NodeId, excluded: NodeId) {
    let mut list = read_exclusions(tree, node_id);
    list.retain(|&raw| raw != excluded.raw());
    write_exclusions(tree, node_id, &list);
}

/// Clears all arm exclusions.
pub fn clear_excluded_objects(tree: &mut SceneTree, node_id: NodeId) {
    write_exclusions(tree, node_id, &[]);
}

fn read_exclusions(tree: &SceneTree, node_id: NodeId) -> Vec<u64> {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_exclusions") {
            Variant::Array(items) => items
                .iter()
                .filter_map(|v| match v {
                    Variant::Int(i) => Some(*i as u64),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn write_exclusions(tree: &mut SceneTree, node_id: NodeId, raws: &[u64]) {
    let arr: Vec<Variant> = raws.iter().map(|&r| Variant::Int(r as i64)).collect();
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_exclusions", Variant::Array(arr));
    }
}

// ===========================================================================
// Query execution
// ===========================================================================

/// Casts the spring arm against the given [`PhysicsServer3D`] and caches the
/// resulting hit length on the node. This is the scene-side analogue of
/// Godot's `SpringArm3D._process`, which runs the same logic each frame.
///
/// Returns `true` if the arm hit a body this call.
pub fn process(tree: &mut SceneTree, physics: &PhysicsServer3D, node_id: NodeId) -> bool {
    let spring_length = get_spring_length(tree, node_id);
    if spring_length <= 0.0 {
        store_no_hit(tree, node_id, 0.0);
        return false;
    }

    let global = node3d::get_global_transform(tree, node_id);
    let local_tip = Vector3::new(0.0, 0.0, -spring_length);
    let from = global.origin;
    let to = global.xform(local_tip);

    let mut query = PhysicsRayQuery3D::new(from, to);
    query.collision_mask = get_collision_mask(tree, node_id);
    query.collide_with_bodies = true;
    query.collide_with_areas = false;

    // Exclude the parent body if one exists — you don't want the body the
    // arm is attached to to block its own camera.
    let mut exclude: HashSet<BodyId3D> = HashSet::new();
    for raw in read_exclusions(tree, node_id) {
        let excl_node = NodeId::from_object_id(gdcore::ObjectId::from_raw(raw));
        if let Some(body_id) = physics.body_for_node(excl_node) {
            exclude.insert(body_id);
        }
    }
    if let Some(parent_id) = tree.get_node(node_id).and_then(|n| n.parent()) {
        if let Some(body_id) = physics.body_for_node(parent_id) {
            exclude.insert(body_id);
        }
    }
    query.exclude = exclude;

    match query.intersect(physics.world().bodies()) {
        Some(hit) => {
            let distance = (hit.point - from).length();
            let margin = get_margin(tree, node_id);
            let retracted = (distance - margin).clamp(0.0, spring_length);
            let collider_node = physics.node_for_body(hit.body_id);
            store_hit(tree, node_id, retracted, collider_node, hit.point, hit.normal);
            true
        }
        None => {
            store_no_hit(tree, node_id, spring_length);
            false
        }
    }
}

fn store_hit(
    tree: &mut SceneTree,
    node_id: NodeId,
    length: f32,
    collider: Option<NodeId>,
    point: Vector3,
    normal: Vector3,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_length", Variant::Float(length as f64));
        node.set_property("_hit_colliding", Variant::Bool(true));
        match collider {
            Some(nid) => {
                node.set_property("_hit_collider", Variant::ObjectId(nid.object_id()));
            }
            None => {
                node.set_property("_hit_collider", Variant::Nil);
            }
        }
        node.set_property("_hit_point", Variant::Vector3(point));
        node.set_property("_hit_normal", Variant::Vector3(normal));
    }
}

fn store_no_hit(tree: &mut SceneTree, node_id: NodeId, length: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_length", Variant::Float(length as f64));
        node.set_property("_hit_colliding", Variant::Bool(false));
        node.set_property("_hit_collider", Variant::Nil);
        node.set_property("_hit_point", Variant::Vector3(Vector3::ZERO));
        node.set_property("_hit_normal", Variant::Vector3(Vector3::ZERO));
    }
}

// ===========================================================================
// Result accessors
// ===========================================================================

/// Returns the current arm length after the most recent `process` call. If
/// nothing was hit, this equals `spring_length`; otherwise it's the clamped
/// hit distance minus `margin`.
pub fn get_hit_length(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_length") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => get_spring_length(tree, node_id),
        })
        .unwrap_or_else(|| get_spring_length(tree, node_id))
}

/// Returns the tip of the arm in world space for the most recent `process`
/// call: global origin plus `get_hit_length * -Z` in the node's basis.
pub fn get_tip_position(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    let global = node3d::get_global_transform(tree, node_id);
    let length = get_hit_length(tree, node_id);
    global.xform(Vector3::new(0.0, 0.0, -length))
}

/// Returns `true` if the most recent `process` call found a hit.
pub fn is_colliding(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_colliding") {
            Variant::Bool(b) => b,
            _ => false,
        })
        .unwrap_or(false)
}

/// Returns the scene node that was hit, if any.
pub fn get_collider(tree: &SceneTree, node_id: NodeId) -> Option<NodeId> {
    tree.get_node(node_id).and_then(|n| match n.get_property("_hit_collider") {
        Variant::ObjectId(id) => Some(NodeId::from_object_id(id)),
        _ => None,
    })
}

/// Returns the hit point in world space (zero if no hit).
pub fn get_collision_point(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_point") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Returns the hit surface normal in world space (zero if no hit).
pub fn get_collision_normal(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_normal") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn make_arm_scene() -> (SceneTree, PhysicsServer3D, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Obstacle static body at (0, 0, -5) with a unit sphere.
        let body = Node::new("Wall", "StaticBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 0.0, -5.0));
        let mut shape = Node::new("Shape", "CollisionShape3D");
        shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(body_id, shape).unwrap();

        // SpringArm3D at the origin, arm extending along -Z by default.
        let arm = Node::new("Arm", "SpringArm3D");
        let arm_id = tree.add_child(root, arm).unwrap();
        node3d::set_position(&mut tree, arm_id, Vector3::ZERO);
        set_spring_length(&mut tree, arm_id, 10.0);
        set_margin(&mut tree, arm_id, 0.0);

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);
        (tree, physics, arm_id, body_id)
    }

    #[test]
    fn springarm_retracts_on_hit() {
        let (mut tree, physics, arm_id, body_id) = make_arm_scene();
        let hit = process(&mut tree, &physics, arm_id);
        assert!(hit, "arm should hit the wall");
        assert!(is_colliding(&tree, arm_id));
        assert_eq!(get_collider(&tree, arm_id), Some(body_id));
        // Ray from z=0 along -Z into a sphere centered at z=-5 with radius 1
        // enters at z=-4 → distance 4.
        let length = get_hit_length(&tree, arm_id);
        assert!(
            (length - 4.0).abs() < 1e-3,
            "hit length should be ~4.0, got {}",
            length
        );
    }

    #[test]
    fn springarm_margin_retracts_further() {
        let (mut tree, physics, arm_id, _body_id) = make_arm_scene();
        set_margin(&mut tree, arm_id, 0.25);
        assert!(process(&mut tree, &physics, arm_id));
        let length = get_hit_length(&tree, arm_id);
        assert!(
            (length - 3.75).abs() < 1e-3,
            "margin should subtract from hit length, got {}",
            length
        );
    }

    #[test]
    fn springarm_full_length_on_miss() {
        let (mut tree, physics, arm_id, body_id) = make_arm_scene();
        // Move the obstacle well out of the ray.
        node3d::set_position(&mut tree, body_id, Vector3::new(20.0, 0.0, 0.0));
        let physics2 = {
            let mut p = PhysicsServer3D::new();
            p.sync_to_physics(&tree);
            p
        };
        let _ = physics; // drop original
        assert!(!process(&mut tree, &physics2, arm_id));
        assert!(!is_colliding(&tree, arm_id));
        let length = get_hit_length(&tree, arm_id);
        assert!((length - 10.0).abs() < 1e-4, "length should be full spring length, got {}", length);
    }

    #[test]
    fn springarm_parent_body_is_excluded() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Parent body wrapping the arm's pivot.
        let parent = Node::new("Pivot", "StaticBody3D");
        let parent_id = tree.add_child(root, parent).unwrap();
        node3d::set_position(&mut tree, parent_id, Vector3::ZERO);
        let mut parent_shape = Node::new("PivotShape", "CollisionShape3D");
        parent_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        parent_shape.set_property("radius", Variant::Float(2.0));
        tree.add_child(parent_id, parent_shape).unwrap();

        // Offset the arm's pivot outside the parent sphere so ray_sphere
        // doesn't reject the parent hit for starting inside the shape. We're
        // testing the exclusion, not the ray_sphere edge case.
        let arm = Node::new("Arm", "SpringArm3D");
        let arm_id = tree.add_child(parent_id, arm).unwrap();
        node3d::set_position(&mut tree, arm_id, Vector3::new(5.0, 0.0, 0.0));
        set_spring_length(&mut tree, arm_id, 10.0);
        set_margin(&mut tree, arm_id, 0.0);

        // Target body further along -Z from the arm's global origin.
        let target = Node::new("Target", "StaticBody3D");
        let target_id = tree.add_child(root, target).unwrap();
        node3d::set_position(&mut tree, target_id, Vector3::new(5.0, 0.0, -5.0));
        let mut target_shape = Node::new("TargetShape", "CollisionShape3D");
        target_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        target_shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(target_id, target_shape).unwrap();

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);

        // The parent body must be auto-excluded; the arm should hit the
        // target instead and report the target distance (4 world units).
        assert!(process(&mut tree, &physics, arm_id));
        assert_eq!(get_collider(&tree, arm_id), Some(target_id));
        let length = get_hit_length(&tree, arm_id);
        assert!((length - 4.0).abs() < 1e-3, "expected ~4.0 got {}", length);
    }

    #[test]
    fn springarm_collision_mask_filters() {
        let (mut tree, _physics, arm_id, body_id) = make_arm_scene();
        // Move the obstacle to layer 0b0010 and set the mask to 0b0001 → miss.
        tree.get_node_mut(body_id)
            .unwrap()
            .set_property("collision_layer", Variant::Int(0b0010));
        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);
        set_collision_mask(&mut tree, arm_id, 0b0001);
        assert!(!process(&mut tree, &physics, arm_id));
        assert_eq!(get_hit_length(&tree, arm_id), 10.0);

        // Match the layer → hit again.
        set_collision_mask(&mut tree, arm_id, 0b0010);
        assert!(process(&mut tree, &physics, arm_id));
    }
}
