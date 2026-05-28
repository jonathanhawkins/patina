//! RayCast3D scene node helpers.
//!
//! A `RayCast3D` node casts a ray from its global origin toward a local
//! `target_position` each physics frame and exposes the closest hit via
//! property accessors. This module mirrors Godot's `RayCast3D` node on top
//! of [`PhysicsRayQuery3D`](gdphysics3d::query::PhysicsRayQuery3D) and the
//! scene-tree [`PhysicsServer3D`](crate::physics_server_3d::PhysicsServer3D).
//!
//! State is stored as [`Variant`] properties on the node itself so the
//! module stays consistent with the other `*3d` helper modules:
//!
//! - Configuration: `target_position`, `enabled`, `collision_mask`,
//!   `exclude_parent`, `collide_with_bodies`, `collide_with_areas`.
//! - Result cache: `_hit_colliding`, `_hit_collider`, `_hit_point`,
//!   `_hit_normal`.

use std::collections::HashSet;

use gdcore::math::Vector3;
use gdphysics3d::body::BodyId3D;
use gdphysics3d::query::PhysicsRayQuery3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::physics_server_3d::PhysicsServer3D;
use crate::scene_tree::SceneTree;

const DEFAULT_TARGET_POSITION: Vector3 = Vector3::new(0.0, -1.0, 0.0);
const DEFAULT_COLLISION_MASK: u32 = 0xFFFFFFFF;

// ===========================================================================
// Configuration properties
// ===========================================================================

/// Sets the ray endpoint in the node's local space.
pub fn set_target_position(tree: &mut SceneTree, node_id: NodeId, target: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("target_position", Variant::Vector3(target));
    }
}

/// Reads the ray endpoint in the node's local space, defaulting to `(0, -1, 0)`.
pub fn get_target_position(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("target_position") {
            Variant::Vector3(v) => v,
            _ => DEFAULT_TARGET_POSITION,
        })
        .unwrap_or(DEFAULT_TARGET_POSITION)
}

/// Sets whether the raycast is active. Disabled ray casts still cache the
/// last result but `force_raycast_update` is a no-op.
pub fn set_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("enabled", Variant::Bool(enabled));
    }
}

/// Reads whether the ray cast is enabled, defaulting to `true`.
pub fn is_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("enabled") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

/// Sets the collision layer mask filter for this ray cast.
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

/// Sets whether the parent node should be excluded from hits (if the parent
/// is a physics body).
pub fn set_exclude_parent(tree: &mut SceneTree, node_id: NodeId, exclude: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("exclude_parent", Variant::Bool(exclude));
    }
}

/// Reads the `exclude_parent` flag, defaulting to `true` to match Godot.
pub fn get_exclude_parent(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("exclude_parent") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

/// Sets whether the ray tests against solid bodies.
pub fn set_collide_with_bodies(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("collide_with_bodies", Variant::Bool(enabled));
    }
}

/// Reads the `collide_with_bodies` flag, defaulting to `true`.
pub fn is_collide_with_bodies_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("collide_with_bodies") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

/// Sets whether the ray tests against areas.
pub fn set_collide_with_areas(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("collide_with_areas", Variant::Bool(enabled));
    }
}

/// Reads the `collide_with_areas` flag, defaulting to `false`.
pub fn is_collide_with_areas_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("collide_with_areas") {
            Variant::Bool(b) => b,
            _ => false,
        })
        .unwrap_or(false)
}

/// Adds a physics body node to the per-raycast exclusion set.
pub fn add_exception(tree: &mut SceneTree, node_id: NodeId, excluded: NodeId) {
    let mut list = read_exceptions(tree, node_id);
    if !list.contains(&excluded.raw()) {
        list.push(excluded.raw());
        write_exceptions(tree, node_id, &list);
    }
}

/// Removes a previously added exception.
pub fn remove_exception(tree: &mut SceneTree, node_id: NodeId, excluded: NodeId) {
    let mut list = read_exceptions(tree, node_id);
    list.retain(|&raw| raw != excluded.raw());
    write_exceptions(tree, node_id, &list);
}

/// Clears all ray-cast exceptions.
pub fn clear_exceptions(tree: &mut SceneTree, node_id: NodeId) {
    write_exceptions(tree, node_id, &[]);
}

fn read_exceptions(tree: &SceneTree, node_id: NodeId) -> Vec<u64> {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_exceptions") {
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

fn write_exceptions(tree: &mut SceneTree, node_id: NodeId, raws: &[u64]) {
    let arr: Vec<Variant> = raws.iter().map(|&r| Variant::Int(r as i64)).collect();
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_exceptions", Variant::Array(arr));
    }
}

// ===========================================================================
// Query execution
// ===========================================================================

/// Casts the ray immediately against the given [`PhysicsServer3D`] and caches
/// the result on the node. This is the scene-side analogue of Godot's
/// `RayCast3D.force_raycast_update`.
///
/// Returns `true` if the ray hit a body this call.
pub fn force_raycast_update(
    tree: &mut SceneTree,
    physics: &PhysicsServer3D,
    node_id: NodeId,
) -> bool {
    if !is_enabled(tree, node_id) {
        clear_hit(tree, node_id);
        return false;
    }

    let collide_bodies = is_collide_with_bodies_enabled(tree, node_id);
    let collide_areas = is_collide_with_areas_enabled(tree, node_id);
    if !collide_bodies && !collide_areas {
        clear_hit(tree, node_id);
        return false;
    }

    let global = node3d::get_global_transform(tree, node_id);
    let target_local = get_target_position(tree, node_id);
    let from = global.origin;
    let to = global.xform(target_local);

    let mut query = PhysicsRayQuery3D::new(from, to);
    query.collision_mask = get_collision_mask(tree, node_id);
    query.collide_with_bodies = collide_bodies;
    query.collide_with_areas = collide_areas;

    // Build exclusion set: explicit exceptions plus (optionally) the parent body.
    let mut exclude: HashSet<BodyId3D> = HashSet::new();
    for raw in read_exceptions(tree, node_id) {
        let excl_node = NodeId::from_object_id(gdcore::ObjectId::from_raw(raw));
        if let Some(body_id) = physics.body_for_node(excl_node) {
            exclude.insert(body_id);
        }
    }
    if get_exclude_parent(tree, node_id) {
        if let Some(parent_id) = tree.get_node(node_id).and_then(|n| n.parent()) {
            if let Some(body_id) = physics.body_for_node(parent_id) {
                exclude.insert(body_id);
            }
        }
    }
    query.exclude = exclude;

    match query.intersect(physics.world().bodies()) {
        Some(hit) => {
            let collider_node = physics.node_for_body(hit.body_id);
            store_hit(tree, node_id, collider_node, hit.point, hit.normal);
            true
        }
        None => {
            clear_hit(tree, node_id);
            false
        }
    }
}

fn store_hit(
    tree: &mut SceneTree,
    node_id: NodeId,
    collider: Option<NodeId>,
    point: Vector3,
    normal: Vector3,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
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

fn clear_hit(tree: &mut SceneTree, node_id: NodeId) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_colliding", Variant::Bool(false));
        node.set_property("_hit_collider", Variant::Nil);
        node.set_property("_hit_point", Variant::Vector3(Vector3::ZERO));
        node.set_property("_hit_normal", Variant::Vector3(Vector3::ZERO));
    }
}

// ===========================================================================
// Result accessors
// ===========================================================================

/// Returns `true` if the last `force_raycast_update` found a hit.
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

    fn make_raycast_scene() -> (SceneTree, PhysicsServer3D, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Target body: RigidBody3D at (0, 0, 10) with a sphere collision shape.
        let body = Node::new("Target", "RigidBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 0.0, 10.0));
        tree.get_node_mut(body_id)
            .unwrap()
            .set_property("mass", Variant::Float(1.0));
        let mut shape = Node::new("Shape", "CollisionShape3D");
        shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(body_id, shape).unwrap();

        // RayCast3D at the origin pointing toward the body.
        let ray = Node::new("Ray", "RayCast3D");
        let ray_id = tree.add_child(root, ray).unwrap();
        node3d::set_position(&mut tree, ray_id, Vector3::ZERO);
        set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 20.0));

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);
        (tree, physics, ray_id, body_id)
    }

    #[test]
    fn raycast_hits_body() {
        let (mut tree, physics, ray_id, body_id) = make_raycast_scene();
        let hit = force_raycast_update(&mut tree, &physics, ray_id);
        assert!(hit, "expected the ray to hit the target sphere");
        assert!(is_colliding(&tree, ray_id));
        assert_eq!(get_collider(&tree, ray_id), Some(body_id));
        let point = get_collision_point(&tree, ray_id);
        assert!((point.z - 9.0).abs() < 0.01, "hit point z = {}", point.z);
        let normal = get_collision_normal(&tree, ray_id);
        assert!(normal.z < 0.0, "normal should point back along -Z");
    }

    #[test]
    fn raycast_respects_disabled() {
        let (mut tree, physics, ray_id, _body_id) = make_raycast_scene();
        set_enabled(&mut tree, ray_id, false);
        let hit = force_raycast_update(&mut tree, &physics, ray_id);
        assert!(!hit);
        assert!(!is_colliding(&tree, ray_id));
    }

    #[test]
    fn raycast_target_direction_misses() {
        let (mut tree, physics, ray_id, _body_id) = make_raycast_scene();
        // Point sideways — should miss the target at (0, 0, 10).
        set_target_position(&mut tree, ray_id, Vector3::new(20.0, 0.0, 0.0));
        let hit = force_raycast_update(&mut tree, &physics, ray_id);
        assert!(!hit);
        assert!(!is_colliding(&tree, ray_id));
    }

    #[test]
    fn raycast_collision_mask_filters() {
        let (mut tree, mut physics, ray_id, body_id) = make_raycast_scene();
        // Move the body to layer 0b0010 and set the mask to 0b0001 — miss.
        tree.get_node_mut(body_id)
            .unwrap()
            .set_property("collision_layer", Variant::Int(0b0010));
        // Re-register bodies with the new layer by creating a fresh server.
        physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);
        set_collision_mask(&mut tree, ray_id, 0b0001);
        assert!(!force_raycast_update(&mut tree, &physics, ray_id));

        // Now match the layer — hit again.
        set_collision_mask(&mut tree, ray_id, 0b0010);
        assert!(force_raycast_update(&mut tree, &physics, ray_id));
    }

    #[test]
    fn raycast_excludes_parent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Parent static body wrapping the ray cast.
        let parent = Node::new("Parent", "StaticBody3D");
        let parent_id = tree.add_child(root, parent).unwrap();
        node3d::set_position(&mut tree, parent_id, Vector3::ZERO);
        let mut parent_shape = Node::new("ParentShape", "CollisionShape3D");
        parent_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        parent_shape.set_property("radius", Variant::Float(2.0));
        tree.add_child(parent_id, parent_shape).unwrap();

        // Ray cast as child of the static body. Offset it to -Z so its global
        // origin lies OUTSIDE the parent sphere (ray_sphere rejects hits when
        // the origin is inside the shape).
        let ray = Node::new("Ray", "RayCast3D");
        let ray_id = tree.add_child(parent_id, ray).unwrap();
        node3d::set_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, -5.0));
        set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 25.0));

        // Target body further along the ray.
        let target = Node::new("Target", "StaticBody3D");
        let target_id = tree.add_child(root, target).unwrap();
        node3d::set_position(&mut tree, target_id, Vector3::new(0.0, 0.0, 10.0));
        let mut target_shape = Node::new("TargetShape", "CollisionShape3D");
        target_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        target_shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(target_id, target_shape).unwrap();

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);

        // exclude_parent is true by default → the parent body is skipped and
        // the ray should reach the target at z = 10.
        let hit = force_raycast_update(&mut tree, &physics, ray_id);
        assert!(hit, "ray should skip parent and hit the target");
        assert_eq!(get_collider(&tree, ray_id), Some(target_id));

        // Turn exclude_parent off → the parent sphere now intercepts the ray
        // before it reaches the target, so the collider becomes the parent.
        set_exclude_parent(&mut tree, ray_id, false);
        assert!(force_raycast_update(&mut tree, &physics, ray_id));
        assert_eq!(get_collider(&tree, ray_id), Some(parent_id));
    }

    #[test]
    fn raycast_add_and_clear_exception() {
        let (mut tree, physics, ray_id, body_id) = make_raycast_scene();
        add_exception(&mut tree, ray_id, body_id);
        assert!(!force_raycast_update(&mut tree, &physics, ray_id));
        clear_exceptions(&mut tree, ray_id);
        assert!(force_raycast_update(&mut tree, &physics, ray_id));
    }
}
