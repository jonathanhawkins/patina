//! pat-s3rrv.87 — `ShapeCast2D`, the swept-shape query node.
//!
//! ClassDB registered the class with properties and nothing else: there was no
//! driver anywhere, so a `ShapeCast2D` in a scene read `Nil` for every result.
//! This is the 2D twin of [`crate::shapecast3d`] and keeps its shape: sweep the
//! node's `shape` from its global transform along `target_position` in fixed
//! substeps, record the first overlapping step, and publish the results onto the
//! node.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/shape_cast_2d.cpp`,
//! which updates under `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`.
//!
//! **Result keys** match [`crate::raycast2d`] and [`crate::shapecast3d`]
//! (`_hit_colliding`, `_hit_count`, …) so the three query nodes stay readable
//! side by side.
//!
//! ⚠️ `collide_with_areas` is accepted but not honored, exactly as
//! `RayQuery2D::collide_with_areas` documents: 2D areas live in a separate
//! `AreaStore` that `PhysicsWorld2D` does not own, so a shape query can only
//! see bodies. `collide_with_bodies = false` therefore means "no hits at all".

use std::collections::HashSet;

use gdcore::math::Vector2;
use gdphysics2d::body::BodyId;
use gdphysics2d::shape::Shape2D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node2d;
use crate::physics_server::PhysicsServer;
use crate::scene_tree::SceneTree;

/// Godot's `ShapeCast2D.target_position` default (`shape_cast_2d.cpp`).
const DEFAULT_TARGET_POSITION: Vector2 = Vector2::new(0.0, 50.0);
/// Godot's default `collision_mask` is layer 1.
const DEFAULT_COLLISION_MASK: u32 = 1;
/// Godot's `ShapeCast2D.max_results` default.
const DEFAULT_MAX_RESULTS: usize = 32;
/// Substeps taken along the sweep.
///
/// The sweep is sampled rather than solved: the first step that overlaps gives
/// the unsafe fraction and the last clean step before it gives the safe
/// fraction, so the fractions are quantised to `1/SWEEP_STEPS`. Matches
/// [`crate::shapecast3d`], whose value this mirrors deliberately — the two nodes
/// should not disagree about how finely a sweep is resolved.
const SWEEP_STEPS: usize = 32;

/// One recorded overlap.
struct HitRecord {
    collider: Option<NodeId>,
    point: Vector2,
    normal: Vector2,
}

pub fn set_target_position(tree: &mut SceneTree, node_id: NodeId, target: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("target_position", Variant::Vector2(target));
    }
}

pub fn get_target_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("target_position") {
            Variant::Vector2(v) => v,
            _ => DEFAULT_TARGET_POSITION,
        })
        .unwrap_or(DEFAULT_TARGET_POSITION)
}

pub fn set_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("enabled", Variant::Bool(enabled));
    }
}

/// Godot's `enabled` defaults to **true**, so a node that never sets it still
/// casts.
pub fn is_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("enabled") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(false)
}

pub fn get_collision_mask(tree: &SceneTree, node_id: NodeId) -> u32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("collision_mask") {
            Variant::Int(i) => i as u32,
            _ => DEFAULT_COLLISION_MASK,
        })
        .unwrap_or(DEFAULT_COLLISION_MASK)
}

pub fn get_exclude_parent(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("exclude_parent") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

pub fn is_collide_with_bodies_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("collide_with_bodies") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

pub fn get_max_results(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property("max_results") {
            Variant::Int(i) if i > 0 => i as usize,
            _ => DEFAULT_MAX_RESULTS,
        })
        .unwrap_or(DEFAULT_MAX_RESULTS)
}

/// Adds a node to the exclusion set (Godot `add_exception`).
pub fn add_exception(tree: &mut SceneTree, node_id: NodeId, excluded: NodeId) {
    let mut ids = read_exceptions(tree, node_id);
    let raw = excluded.raw();
    if !ids.contains(&raw) {
        ids.push(raw);
    }
    write_exceptions(tree, node_id, &ids);
}

/// Clears the exclusion set (Godot `clear_exceptions`).
pub fn clear_exceptions(tree: &mut SceneTree, node_id: NodeId) {
    write_exceptions(tree, node_id, &[]);
}

fn read_exceptions(tree: &SceneTree, node_id: NodeId) -> Vec<u64> {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_exceptions") {
            Variant::Array(items) => items
                .to_vec()
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

fn write_exceptions(tree: &mut SceneTree, node_id: NodeId, ids: &[u64]) {
    if let Some(node) = tree.get_node_mut(node_id) {
        let items: Vec<Variant> = ids.iter().map(|r| Variant::Int(*r as i64)).collect();
        node.set_property("_exceptions", Variant::Array(items.into()));
    }
}

/// The shape this node sweeps.
///
/// Godot's ShapeCast2D carries its `shape` as a property on the node itself
/// (unlike the 3D driver, which reads a child collider), and `packed_scene`
/// flattens that sub-resource onto the node as `__shape__*` — the same
/// companions `PhysicsServer` already builds bodies from, so the two agree on
/// how a `.tscn` shape is spelled.
fn extract_cast_shape(tree: &SceneTree, node_id: NodeId) -> Option<Shape2D> {
    crate::physics_server::shape_from_shape_props(tree, node_id)
}

/// Runs the sweep now and republishes the results (Godot
/// `force_shapecast_update`). Returns whether anything was hit.
pub fn force_shapecast_update(
    tree: &mut SceneTree,
    physics: &PhysicsServer,
    node_id: NodeId,
) -> bool {
    // Refuse any other class OUTRIGHT, before the clear below. The scripted
    // entry point is reachable by name from any node, and `_hit_*` are the same
    // keys `raycast2d` publishes — clearing here would wipe a RayCast2D's own
    // results if a script called `force_shapecast_update()` on one.
    if !tree
        .get_node(node_id)
        .is_some_and(|n| n.class_name() == "ShapeCast2D")
    {
        return false;
    }
    if !is_enabled(tree, node_id) {
        clear_hit(tree, node_id);
        return false;
    }
    // Areas are invisible to a 2D shape query (see the module docs), so with
    // bodies switched off there is nothing left to hit.
    if !is_collide_with_bodies_enabled(tree, node_id) {
        clear_hit(tree, node_id);
        return false;
    }
    let Some(shape) = extract_cast_shape(tree, node_id) else {
        clear_hit(tree, node_id);
        return false;
    };

    let global = node2d::get_global_transform(tree, node_id);
    let target_local = get_target_position(tree, node_id);
    let from = global.origin;
    // `target_position` is node-LOCAL, so it rotates and scales with the node.
    let to = global.xform(target_local);

    let mut exclude: HashSet<BodyId> = HashSet::new();
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
    let exclude: Vec<BodyId> = exclude.into_iter().collect();

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
        let pos = Vector2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        // Keep the node's rotation and scale; only the translation sweeps.
        let mut xform = global;
        xform.origin = pos;

        let results = physics.world().shape_cast(&shape, xform, &exclude, mask);
        if results.is_empty() {
            safe_fraction = t;
            continue;
        }

        unsafe_fraction = t;
        for r in results.into_iter().take(max_results) {
            hit_results.push(HitRecord {
                collider: physics.node_for_body(r.body_id),
                point: r.point,
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

fn clear_hit(tree: &mut SceneTree, node_id: NodeId) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_colliding", Variant::Bool(false));
        node.set_property("_hit_count", Variant::Int(0));
        node.set_property("_hit_colliders", Variant::Array(Default::default()));
        node.set_property("_hit_points", Variant::Array(Default::default()));
        node.set_property("_hit_normals", Variant::Array(Default::default()));
        node.set_property("_safe_fraction", Variant::Float(1.0));
        node.set_property("_unsafe_fraction", Variant::Float(1.0));
    }
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
        let points: Vec<Variant> = hits.iter().map(|h| Variant::Vector2(h.point)).collect();
        let normals: Vec<Variant> = hits.iter().map(|h| Variant::Vector2(h.normal)).collect();
        node.set_property("_hit_colliders", Variant::Array(colliders.into()));
        node.set_property("_hit_points", Variant::Array(points.into()));
        node.set_property("_hit_normals", Variant::Array(normals.into()));
        node.set_property("_safe_fraction", Variant::Float(safe_fraction as f64));
        node.set_property("_unsafe_fraction", Variant::Float(unsafe_fraction as f64));
    }
}

pub fn is_colliding(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| matches!(n.get_property("_hit_colliding"), Variant::Bool(true)))
        .unwrap_or(false)
}

pub fn get_collision_count(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_hit_count") {
            Variant::Int(i) if i > 0 => i as usize,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Reads one entry out of a published result array.
fn indexed(tree: &SceneTree, node_id: NodeId, key: &str, index: usize) -> Option<Variant> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Array(items) => items.get(index),
        _ => None,
    }
}

pub fn get_collider(tree: &SceneTree, node_id: NodeId, index: usize) -> Option<NodeId> {
    match indexed(tree, node_id, "_hit_colliders", index) {
        Some(Variant::ObjectId(oid)) => Some(NodeId::from_object_id(oid)),
        _ => None,
    }
}

pub fn get_collision_point(tree: &SceneTree, node_id: NodeId, index: usize) -> Vector2 {
    match indexed(tree, node_id, "_hit_points", index) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

pub fn get_collision_normal(tree: &SceneTree, node_id: NodeId, index: usize) -> Vector2 {
    match indexed(tree, node_id, "_hit_normals", index) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

pub fn get_closest_collision_safe_fraction(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_safe_fraction") {
            Variant::Float(f) => f as f32,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

pub fn get_closest_collision_unsafe_fraction(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("_unsafe_fraction") {
            Variant::Float(f) => f as f32,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// Answers `ShapeCast2D`'s script method surface, or `None` for any other node.
///
/// This must run AHEAD of the generic node-method table in
/// `scripting::call_node_method`. That table is flat rather than
/// class-dispatched, and its `is_colliding` / `get_collider` /
/// `get_collision_point` arms read RayCast2D's **singular** `_hit_point` /
/// `_hit_collider` keys — which a shape cast never writes, because it publishes
/// indexed arrays. Without this handler a `ShapeCast2D` would quietly read a
/// raycast's empty results and report `false` and `Nil` forever.
///
/// Declining every non-ShapeCast2D node is what keeps that table reachable
/// unchanged, the same contract `navigation_agents::call_agent_method` follows.
///
/// `force_shapecast_update` is NOT handled here: it needs the physics server,
/// which only the script host can reach, so `scripting` owns that one arm.
pub fn call_shapecast_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if tree.get_node(node_id)?.class_name() != "ShapeCast2D" {
        return None;
    }
    // Every indexed getter takes an optional index, defaulting to the closest
    // hit — Godot's own signature is `get_collider(index)`, but scripts written
    // against a single expected hit call it bare.
    let index = match args.first() {
        Some(Variant::Int(v)) if *v >= 0 => *v as usize,
        _ => 0,
    };
    Some(match method {
        "is_colliding" => Variant::Bool(is_colliding(tree, node_id)),
        "get_collision_count" => Variant::Int(get_collision_count(tree, node_id) as i64),
        "get_collider" => match get_collider(tree, node_id, index) {
            Some(nid) => Variant::ObjectId(nid.object_id()),
            None => Variant::Nil,
        },
        "get_collision_point" => Variant::Vector2(get_collision_point(tree, node_id, index)),
        "get_collision_normal" => Variant::Vector2(get_collision_normal(tree, node_id, index)),
        "get_closest_collision_safe_fraction" => {
            Variant::Float(get_closest_collision_safe_fraction(tree, node_id) as f64)
        }
        "get_closest_collision_unsafe_fraction" => {
            Variant::Float(get_closest_collision_unsafe_fraction(tree, node_id) as f64)
        }
        "add_exception" => {
            if let Some(Variant::ObjectId(oid)) = args.first() {
                add_exception(tree, node_id, NodeId::from_object_id(*oid));
            }
            Variant::Nil
        }
        "clear_exceptions" => {
            clear_exceptions(tree, node_id);
            Variant::Nil
        }
        // Not a shape-cast method: let the generic table answer it.
        _ => return None,
    })
}
