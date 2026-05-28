//! ShapeCast3D scene node helpers.
//!
//! A `ShapeCast3D` node sweeps a child `CollisionShape3D` from its global
//! origin along a `target_position` (in local space) and reports overlapping
//! bodies. This module mirrors Godot's `ShapeCast3D` node on top of
//! [`PhysicsShapeQuery3D`](gdphysics3d::query::PhysicsShapeQuery3D) and the
//! scene-tree [`PhysicsServer3D`](crate::physics_server_3d::PhysicsServer3D).
//!
//! State is stored as [`Variant`] properties on the node itself for
//! consistency with the other `*3d` helper modules.

use std::collections::HashSet;

use gdcore::math::Vector3;
use gdphysics3d::body::BodyId3D;
use gdphysics3d::query::PhysicsShapeQuery3D;
use gdphysics3d::shape::Shape3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::physics_server_3d::{shape_from_node_3d, PhysicsServer3D};
use crate::scene_tree::SceneTree;

const DEFAULT_TARGET_POSITION: Vector3 = Vector3::new(0.0, -1.0, 0.0);
const DEFAULT_COLLISION_MASK: u32 = 0xFFFFFFFF;
const DEFAULT_MAX_RESULTS: usize = 32;
// Number of substeps used to discretize the swept shape between origin and
// target. The first colliding step defines the unsafe fraction; the previous
// step defines the safe fraction.
const SWEEP_STEPS: usize = 32;

// ===========================================================================
// Configuration properties
// ===========================================================================

/// Sets the sweep endpoint in the node's local space.
pub fn set_target_position(tree: &mut SceneTree, node_id: NodeId, target: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("target_position", Variant::Vector3(target));
    }
}

/// Reads the sweep endpoint in the node's local space, defaulting to
/// `(0, -1, 0)`.
pub fn get_target_position(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("target_position") {
            Variant::Vector3(v) => v,
            _ => DEFAULT_TARGET_POSITION,
        })
        .unwrap_or(DEFAULT_TARGET_POSITION)
}

/// Sets whether the shape cast is active.
pub fn set_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("enabled", Variant::Bool(enabled));
    }
}

/// Reads whether the shape cast is enabled, defaulting to `true`.
pub fn is_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("enabled") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

/// Sets the collision layer mask filter for this shape cast.
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

/// Sets whether the parent node should be excluded from hits.
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

/// Sets whether the cast tests against solid bodies.
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

/// Sets whether the cast tests against areas.
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

/// Sets the maximum number of overlapping bodies to record per cast.
pub fn set_max_results(tree: &mut SceneTree, node_id: NodeId, max: usize) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("max_results", Variant::Int(max as i64));
    }
}

/// Reads the `max_results` cap, defaulting to 32.
pub fn get_max_results(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property("max_results") {
            Variant::Int(i) if i > 0 => i as usize,
            _ => DEFAULT_MAX_RESULTS,
        })
        .unwrap_or(DEFAULT_MAX_RESULTS)
}

/// Adds a physics body node to the per-shapecast exclusion set.
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

/// Clears all shape-cast exceptions.
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
// Shape extraction
// ===========================================================================

/// Returns the swept shape attached to the ShapeCast3D node, if any.
///
/// The shape is taken from the first child `CollisionShape3D` whose `shape`
/// property resolves to a recognized shape type. Returns `None` when no such
/// child exists.
fn extract_cast_shape(tree: &SceneTree, node_id: NodeId) -> Option<Shape3D> {
    let children = tree.get_node(node_id)?.children().to_vec();
    for child in children {
        if let Some(shape) = shape_from_node_3d(tree, child) {
            return Some(shape);
        }
    }
    None
}

// ===========================================================================
// Query execution
// ===========================================================================

/// Sweeps the cast shape against the given [`PhysicsServer3D`] and caches the
/// result on the node. This is the scene-side analogue of Godot's
/// `ShapeCast3D.force_shapecast_update`.
///
/// Returns `true` if the swept shape collided with at least one body.
pub fn force_shapecast_update(
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

    let shape = match extract_cast_shape(tree, node_id) {
        Some(s) => s,
        None => {
            clear_hit(tree, node_id);
            return false;
        }
    };

    let global = node3d::get_global_transform(tree, node_id);
    let target_local = get_target_position(tree, node_id);
    let from = global.origin;
    let to = global.xform(target_local);

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

    let mask = get_collision_mask(tree, node_id);
    let max_results = get_max_results(tree, node_id);

    // Walk the sweep in fixed substeps. The first step that produces a hit is
    // the unsafe fraction; the previous clean step is the safe fraction. The
    // overlap snapshot at the unsafe step becomes the result list.
    let mut safe_fraction: f32 = 1.0;
    let mut unsafe_fraction: f32 = 1.0;
    let mut hit_results: Vec<HitRecord> = Vec::new();

    for step in 0..=SWEEP_STEPS {
        let t = step as f32 / SWEEP_STEPS as f32;
        let pos = from.lerp(to, t);

        let mut query = PhysicsShapeQuery3D::new(shape.clone(), pos);
        query.collision_mask = mask;
        query.exclude = exclude.clone();
        query.collide_with_bodies = collide_bodies;
        query.collide_with_areas = collide_areas;
        query.max_results = max_results;

        let results = query.intersect(physics.world().bodies());
        if results.is_empty() {
            safe_fraction = t;
            continue;
        }

        unsafe_fraction = t;
        for r in results {
            let collider = physics.node_for_body(r.body_id);
            // The query position at the unsafe step is where the swept shape
            // first overlaps the body — record it as the contact point.
            hit_results.push(HitRecord {
                collider,
                point: pos,
                normal: r.normal,
            });
        }
        break;
    }

    if hit_results.is_empty() {
        clear_hit(tree, node_id);
        false
    } else {
        store_hit(tree, node_id, &hit_results, safe_fraction, unsafe_fraction);
        true
    }
}

#[derive(Clone, Copy)]
struct HitRecord {
    collider: Option<NodeId>,
    point: Vector3,
    normal: Vector3,
}

fn store_hit(
    tree: &mut SceneTree,
    node_id: NodeId,
    hits: &[HitRecord],
    safe_fraction: f32,
    unsafe_fraction: f32,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_colliding", Variant::Bool(true));
        node.set_property("_hit_count", Variant::Int(hits.len() as i64));
        let colliders: Vec<Variant> = hits
            .iter()
            .map(|h| match h.collider {
                Some(nid) => Variant::ObjectId(nid.object_id()),
                None => Variant::Nil,
            })
            .collect();
        let points: Vec<Variant> = hits.iter().map(|h| Variant::Vector3(h.point)).collect();
        let normals: Vec<Variant> = hits.iter().map(|h| Variant::Vector3(h.normal)).collect();
        node.set_property("_hit_colliders", Variant::Array(colliders));
        node.set_property("_hit_points", Variant::Array(points));
        node.set_property("_hit_normals", Variant::Array(normals));
        node.set_property("_safe_fraction", Variant::Float(safe_fraction as f64));
        node.set_property("_unsafe_fraction", Variant::Float(unsafe_fraction as f64));
    }
}

fn clear_hit(tree: &mut SceneTree, node_id: NodeId) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_colliding", Variant::Bool(false));
        node.set_property("_hit_count", Variant::Int(0));
        node.set_property("_hit_colliders", Variant::Array(Vec::new()));
        node.set_property("_hit_points", Variant::Array(Vec::new()));
        node.set_property("_hit_normals", Variant::Array(Vec::new()));
        node.set_property("_safe_fraction", Variant::Float(1.0));
        node.set_property("_unsafe_fraction", Variant::Float(1.0));
    }
}

// ===========================================================================
// Result accessors
// ===========================================================================

/// Returns `true` if the last `force_shapecast_update` produced any hits.
pub fn is_colliding(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_colliding") {
            Variant::Bool(b) => b,
            _ => false,
        })
        .unwrap_or(false)
}

/// Returns the number of bodies overlapping at the unsafe step.
pub fn get_collision_count(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_count") {
            Variant::Int(i) if i > 0 => i as usize,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Returns the collider scene node at index `i` in the hit list.
pub fn get_collider(tree: &SceneTree, node_id: NodeId, index: usize) -> Option<NodeId> {
    let arr = match tree.get_node(node_id)?.get_property("_hit_colliders") {
        Variant::Array(a) => a,
        _ => return None,
    };
    match arr.get(index)? {
        Variant::ObjectId(id) => Some(NodeId::from_object_id(*id)),
        _ => None,
    }
}

/// Returns the contact point at index `i` in the hit list (world space).
pub fn get_collision_point(tree: &SceneTree, node_id: NodeId, index: usize) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_points") {
            Variant::Array(arr) => match arr.get(index) {
                Some(Variant::Vector3(v)) => *v,
                _ => Vector3::ZERO,
            },
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Returns the contact normal at index `i` in the hit list (world space).
pub fn get_collision_normal(tree: &SceneTree, node_id: NodeId, index: usize) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_normals") {
            Variant::Array(arr) => match arr.get(index) {
                Some(Variant::Vector3(v)) => *v,
                _ => Vector3::ZERO,
            },
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Returns the largest sweep fraction (0..=1) that did NOT collide with
/// anything. Mirrors `ShapeCast3D.get_closest_collision_safe_fraction`.
pub fn get_closest_collision_safe_fraction(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_safe_fraction") {
            Variant::Float(f) => f as f32,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// Returns the smallest sweep fraction (0..=1) that DID collide. Mirrors
/// `ShapeCast3D.get_closest_collision_unsafe_fraction`.
pub fn get_closest_collision_unsafe_fraction(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_unsafe_fraction") {
            Variant::Float(f) => f as f32,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn make_shapecast_scene() -> (SceneTree, PhysicsServer3D, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Target body: StaticBody3D at (0, 0, 10) with a unit sphere.
        let body = Node::new("Target", "StaticBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 0.0, 10.0));
        let mut shape = Node::new("Shape", "CollisionShape3D");
        shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(body_id, shape).unwrap();

        // ShapeCast3D at the origin sweeping a small sphere toward the body.
        let cast = Node::new("Cast", "ShapeCast3D");
        let cast_id = tree.add_child(root, cast).unwrap();
        node3d::set_position(&mut tree, cast_id, Vector3::ZERO);
        set_target_position(&mut tree, cast_id, Vector3::new(0.0, 0.0, 20.0));
        let mut cast_shape = Node::new("CastShape", "CollisionShape3D");
        cast_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        cast_shape.set_property("radius", Variant::Float(0.5));
        tree.add_child(cast_id, cast_shape).unwrap();

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);
        (tree, physics, cast_id, body_id)
    }

    #[test]
    fn shapecast_hits_body() {
        let (mut tree, physics, cast_id, body_id) = make_shapecast_scene();
        let hit = force_shapecast_update(&mut tree, &physics, cast_id);
        assert!(hit, "swept sphere should reach the target sphere");
        assert!(is_colliding(&tree, cast_id));
        assert_eq!(get_collision_count(&tree, cast_id), 1);
        assert_eq!(get_collider(&tree, cast_id, 0), Some(body_id));

        // The unsafe fraction must be strictly between 0 and 1: at t=0 the
        // swept sphere starts away from the body, and at some midway point it
        // first overlaps the body's bounding sphere.
        let unsafe_t = get_closest_collision_unsafe_fraction(&tree, cast_id);
        let safe_t = get_closest_collision_safe_fraction(&tree, cast_id);
        assert!(unsafe_t > 0.0 && unsafe_t <= 1.0, "unsafe fraction = {unsafe_t}");
        assert!(safe_t < unsafe_t, "safe ({safe_t}) must precede unsafe ({unsafe_t})");
    }

    #[test]
    fn shapecast_respects_disabled() {
        let (mut tree, physics, cast_id, _body_id) = make_shapecast_scene();
        set_enabled(&mut tree, cast_id, false);
        let hit = force_shapecast_update(&mut tree, &physics, cast_id);
        assert!(!hit);
        assert!(!is_colliding(&tree, cast_id));
    }

    #[test]
    fn shapecast_misses_when_target_sideways() {
        let (mut tree, physics, cast_id, _body_id) = make_shapecast_scene();
        set_target_position(&mut tree, cast_id, Vector3::new(20.0, 0.0, 0.0));
        let hit = force_shapecast_update(&mut tree, &physics, cast_id);
        assert!(!hit);
        assert!(!is_colliding(&tree, cast_id));
        // Nothing was hit, so the safe fraction stays pinned at the end of the
        // sweep.
        assert_eq!(get_closest_collision_safe_fraction(&tree, cast_id), 1.0);
    }

    #[test]
    fn shapecast_collision_mask_filters() {
        let (mut tree, mut physics, cast_id, body_id) = make_shapecast_scene();
        tree.get_node_mut(body_id)
            .unwrap()
            .set_property("collision_layer", Variant::Int(0b0010));
        physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);

        // Mask doesn't include the target's layer → no hit.
        set_collision_mask(&mut tree, cast_id, 0b0001);
        assert!(!force_shapecast_update(&mut tree, &physics, cast_id));

        // Mask matches → hit.
        set_collision_mask(&mut tree, cast_id, 0b0010);
        assert!(force_shapecast_update(&mut tree, &physics, cast_id));
    }

    #[test]
    fn shapecast_excludes_parent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Parent static body wraps the cast at the origin.
        let parent = Node::new("Parent", "StaticBody3D");
        let parent_id = tree.add_child(root, parent).unwrap();
        node3d::set_position(&mut tree, parent_id, Vector3::ZERO);
        let mut parent_shape = Node::new("ParentShape", "CollisionShape3D");
        parent_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        parent_shape.set_property("radius", Variant::Float(2.0));
        tree.add_child(parent_id, parent_shape).unwrap();

        // ShapeCast3D as child of the parent body, offset to clear the parent.
        let cast = Node::new("Cast", "ShapeCast3D");
        let cast_id = tree.add_child(parent_id, cast).unwrap();
        node3d::set_position(&mut tree, cast_id, Vector3::new(0.0, 0.0, -5.0));
        set_target_position(&mut tree, cast_id, Vector3::new(0.0, 0.0, 25.0));
        let mut cast_shape = Node::new("CastShape", "CollisionShape3D");
        cast_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        cast_shape.set_property("radius", Variant::Float(0.25));
        tree.add_child(cast_id, cast_shape).unwrap();

        // Distinct target body further along the sweep.
        let target = Node::new("Target", "StaticBody3D");
        let target_id = tree.add_child(root, target).unwrap();
        node3d::set_position(&mut tree, target_id, Vector3::new(0.0, 0.0, 10.0));
        let mut target_shape = Node::new("TargetShape", "CollisionShape3D");
        target_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        target_shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(target_id, target_shape).unwrap();

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);

        // exclude_parent=true → parent body is skipped, target reached.
        let hit = force_shapecast_update(&mut tree, &physics, cast_id);
        assert!(hit, "shapecast should skip parent and reach the target");
        assert_eq!(get_collider(&tree, cast_id, 0), Some(target_id));

        // exclude_parent=false → parent intercepts before target.
        set_exclude_parent(&mut tree, cast_id, false);
        assert!(force_shapecast_update(&mut tree, &physics, cast_id));
        assert_eq!(get_collider(&tree, cast_id, 0), Some(parent_id));
    }

    #[test]
    fn shapecast_add_and_clear_exception() {
        let (mut tree, physics, cast_id, body_id) = make_shapecast_scene();
        add_exception(&mut tree, cast_id, body_id);
        assert!(!force_shapecast_update(&mut tree, &physics, cast_id));
        clear_exceptions(&mut tree, cast_id);
        assert!(force_shapecast_update(&mut tree, &physics, cast_id));
    }

    #[test]
    fn shapecast_without_shape_returns_no_hit() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Target sphere.
        let body = Node::new("Target", "StaticBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 0.0, 5.0));
        let mut body_shape = Node::new("Shape", "CollisionShape3D");
        body_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        body_shape.set_property("radius", Variant::Float(1.0));
        tree.add_child(body_id, body_shape).unwrap();

        // ShapeCast3D with no child CollisionShape3D — must not panic and
        // must report no hit.
        let cast = Node::new("Cast", "ShapeCast3D");
        let cast_id = tree.add_child(root, cast).unwrap();
        set_target_position(&mut tree, cast_id, Vector3::new(0.0, 0.0, 10.0));

        let mut physics = PhysicsServer3D::new();
        physics.sync_to_physics(&tree);

        assert!(!force_shapecast_update(&mut tree, &physics, cast_id));
        assert!(!is_colliding(&tree, cast_id));
    }
}
