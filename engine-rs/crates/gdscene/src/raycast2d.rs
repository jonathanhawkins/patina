//! RayCast2D scene node helpers.
//!
//! Mirrors [`raycast3d`](crate::raycast3d) for 2D: a `RayCast2D` casts from its
//! **global** origin toward `target_position` mapped through its **global**
//! transform each physics frame, filtered exactly as Godot filters
//! (`upstream/godot/scene/2d/physics/ray_cast_2d.cpp::_update_raycast_state`),
//! and caches the closest hit as node properties.
//!
//! Before pat-0q2kx.32 the 2D driver in `main_loop.rs` cast from the node's
//! LOCAL position along the raw (unrotated) `target_position` using the
//! unfiltered `world.raycast`, and published `is_colliding`/`collider` — while
//! the GDScript bridge reads `_hit_colliding`/`_hit_collider`/`_hit_point`/
//! `_hit_normal`. Both halves worked; they were simply wired to different
//! property names, so `$Ray.is_colliding()` was permanently `false`.
//!
//! - Configuration: `target_position`, `enabled`, `collision_mask`,
//!   `exclude_parent`, `collide_with_bodies`, `collide_with_areas`,
//!   `hit_from_inside`, `_exceptions`.
//! - Result cache: `_hit_colliding`, `_hit_collider`, `_hit_point`,
//!   `_hit_normal` (plus the legacy `is_colliding`/`collider` keys, which
//!   predate the `_hit_*` contract and still have readers).

use gdcore::math::Vector2;
use gdphysics2d::body::BodyId;
use gdphysics2d::world::RayQuery2D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node2d;
use crate::physics_server::PhysicsServer;
use crate::scene_tree::SceneTree;

/// Godot's `RayCast2D` defaults, from `doc/classes/RayCast2D.xml`. Note the
/// mask is **1**, not all-layers: a ray that ignored the mask would hit bodies
/// on layers the author deliberately excluded.
const DEFAULT_TARGET_POSITION: Vector2 = Vector2::new(0.0, 50.0);
const DEFAULT_COLLISION_MASK: u32 = 1;

// ===========================================================================
// Configuration properties
// ===========================================================================

/// Sets the ray endpoint in the node's local space.
pub fn set_target_position(tree: &mut SceneTree, node_id: NodeId, target: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("target_position", Variant::Vector2(target));
    }
}

/// Reads the ray endpoint in the node's local space.
pub fn get_target_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("target_position"))
    {
        Some(Variant::Vector2(v)) => v,
        _ => DEFAULT_TARGET_POSITION,
    }
}

/// Enables or disables per-frame updates for this ray.
pub fn set_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("enabled", Variant::Bool(enabled));
    }
}

/// Whether the ray updates each physics frame (Godot default: `true`).
pub fn is_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    !matches!(
        tree.get_node(node_id).map(|n| n.get_property("enabled")),
        Some(Variant::Bool(false))
    )
}

fn bool_prop_default_true(tree: &SceneTree, node_id: NodeId, key: &str) -> bool {
    !matches!(
        tree.get_node(node_id).map(|n| n.get_property(key)),
        Some(Variant::Bool(false))
    )
}

fn bool_prop_default_false(tree: &SceneTree, node_id: NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(node_id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// The ray's collision mask (Godot default: `1`).
pub fn get_collision_mask(tree: &SceneTree, node_id: NodeId) -> u32 {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("collision_mask"))
    {
        Some(Variant::Int(i)) => i as u32,
        _ => DEFAULT_COLLISION_MASK,
    }
}

/// Whether the ray skips its parent `CollisionObject2D` (Godot default: `true`).
pub fn get_exclude_parent(tree: &SceneTree, node_id: NodeId) -> bool {
    bool_prop_default_true(tree, node_id, "exclude_parent")
}

/// Explicit exceptions added via `add_exception`, stored as raw object ids.
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

// ===========================================================================
// The per-frame query
// ===========================================================================

/// Re-runs this ray's query against the current physics world and republishes
/// its result properties. Returns whether the ray is now colliding.
///
/// Called every physics frame by `MainLoop::process_raycasts`, and directly by
/// a scripted `force_raycast_update()` so a script that moves a node and then
/// queries it in the same frame sees the post-move answer.
pub fn force_raycast_update(
    tree: &mut SceneTree,
    physics: &PhysicsServer,
    node_id: NodeId,
) -> bool {
    if !is_enabled(tree, node_id) {
        clear_hit(tree, node_id);
        return false;
    }

    let collide_bodies = bool_prop_default_true(tree, node_id, "collide_with_bodies");
    let collide_areas = bool_prop_default_false(tree, node_id, "collide_with_areas");
    if !collide_bodies && !collide_areas {
        clear_hit(tree, node_id);
        return false;
    }

    // Godot: `from = global_position`, `to = global_transform * target_position`
    // — so the ray inherits the parent chain's rotation and scale, not just its
    // offset. Passing the target through `xform` (rather than rotating a
    // direction) is what makes a RayCast2D under a rotated body point where the
    // editor draws it.
    let global = node2d::get_global_transform(tree, node_id);
    let from = global.origin;
    let to = global.xform(get_target_position(tree, node_id));

    let mut query = RayQuery2D::new(from, to);
    query.collision_mask = get_collision_mask(tree, node_id);
    query.collide_with_bodies = collide_bodies;
    query.collide_with_areas = collide_areas;
    query.hit_from_inside = bool_prop_default_false(tree, node_id, "hit_from_inside");

    // Exclusions: explicit exceptions, plus the parent body unless the author
    // turned that off. Without this a ray parented to a CharacterBody2D
    // immediately hits its own body and reports distance 0 forever.
    let mut exclude: Vec<BodyId> = Vec::new();
    for raw in read_exceptions(tree, node_id) {
        let excl_node = NodeId::from_object_id(gdcore::ObjectId::from_raw(raw));
        if let Some(body_id) = physics.body_for_node(excl_node) {
            if !exclude.contains(&body_id) {
                exclude.push(body_id);
            }
        }
    }
    if get_exclude_parent(tree, node_id) {
        if let Some(parent_id) = tree.get_node(node_id).and_then(|n| n.parent()) {
            if let Some(body_id) = physics.body_for_node(parent_id) {
                if !exclude.contains(&body_id) {
                    exclude.push(body_id);
                }
            }
        }
    }
    query.exclude = exclude;

    match physics.world().intersect_ray(&query) {
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
    point: Vector2,
    normal: Vector2,
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
        node.set_property("_hit_point", Variant::Vector2(point));
        node.set_property("_hit_normal", Variant::Vector2(normal));

        // Legacy keys, written before the `_hit_*` contract existed. Kept in
        // sync rather than dropped — other readers still use them.
        node.set_property("is_colliding", Variant::Bool(true));
        let collider_val = match collider {
            Some(cid) => Variant::Int(cid.raw() as i64),
            None => Variant::Nil,
        };
        node.set_property("collider", collider_val);
    }
}

fn clear_hit(tree: &mut SceneTree, node_id: NodeId) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("_hit_colliding", Variant::Bool(false));
        node.set_property("_hit_collider", Variant::Nil);
        node.set_property("_hit_point", Variant::Vector2(Vector2::ZERO));
        node.set_property("_hit_normal", Variant::Vector2(Vector2::ZERO));

        node.set_property("is_colliding", Variant::Bool(false));
        node.set_property("collider", Variant::Nil);
    }
}

// ===========================================================================
// Result accessors
// ===========================================================================

/// Returns `true` if the last update found a hit.
pub fn is_colliding(tree: &SceneTree, node_id: NodeId) -> bool {
    matches!(
        tree.get_node(node_id)
            .map(|n| n.get_property("_hit_colliding")),
        Some(Variant::Bool(true))
    )
}

/// The node the ray hit, if any.
pub fn get_collider(tree: &SceneTree, node_id: NodeId) -> Option<NodeId> {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("_hit_collider"))
    {
        Some(Variant::ObjectId(oid)) => Some(NodeId::from_object_id(oid)),
        _ => None,
    }
}

/// The world-space contact point of the last hit.
pub fn get_collision_point(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    match tree.get_node(node_id).map(|n| n.get_property("_hit_point")) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

/// The surface normal at the last hit.
pub fn get_collision_normal(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    match tree
        .get_node(node_id)
        .map(|n| n.get_property("_hit_normal"))
    {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::physics_server::PhysicsServer;
    use crate::scene_tree::SceneTree;

    /// The bead's geometry: a `RayCast2D` under a `CharacterBody2D` at (100,0)
    /// rotated a quarter turn, aiming at local +X. Its global direction is
    /// therefore +Y, so the wall it must hit sits *below* it — somewhere the
    /// old local-space cast could never reach.
    fn rotated_ray_scene() -> (SceneTree, PhysicsServer, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut player = Node::new("Player", "CharacterBody2D");
        player.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
        player.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_2));
        let player_id = tree.add_child(root, player).unwrap();
        let mut pshape = Node::new("Shape", "CollisionShape2D");
        pshape.set_property("radius", Variant::Float(10.0));
        tree.add_child(player_id, pshape).unwrap();

        // Ray at the body's origin aiming along the body's local +X.
        let mut ray = Node::new("Ray", "RayCast2D");
        ray.set_property("position", Variant::Vector2(Vector2::ZERO));
        ray.set_property("target_position", Variant::Vector2(Vector2::new(50.0, 0.0)));
        let ray_id = tree.add_child(player_id, ray).unwrap();

        // Wall spanning y ∈ [35,45] — under the ray's rotated global direction.
        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(100.0, 40.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut wshape = Node::new("Shape", "CollisionShape2D");
        wshape.set_property("size", Variant::Vector2(Vector2::new(200.0, 10.0)));
        tree.add_child(wall_id, wshape).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        (tree, server, ray_id, wall_id, player_id)
    }

    /// ⚠️ The core regression: origin and direction both come from the node's
    /// GLOBAL transform. Casting from the local position along the raw
    /// `target_position` sends this ray from (0,0) toward +X, which misses
    /// everything in the scene.
    #[test]
    fn raycast2d_uses_global_transform_not_local() {
        let (mut tree, server, ray_id, wall_id, player_id) = rotated_ray_scene();

        assert!(
            force_raycast_update(&mut tree, &server, ray_id),
            "the rotated ray must reach the wall at its global aim"
        );
        assert!(is_colliding(&tree, ray_id));

        let collider = get_collider(&tree, ray_id);
        assert_eq!(
            collider,
            Some(wall_id),
            "the wall must be reported as the collider"
        );
        assert_ne!(
            collider,
            Some(player_id),
            "the ray's own parent body must never be the collider"
        );

        // Entry face of a wall spanning y ∈ [35,45], crossed travelling +Y.
        let point = get_collision_point(&tree, ray_id);
        assert!(
            (point.x - 100.0).abs() < 0.01 && (point.y - 35.0).abs() < 0.01,
            "expected the wall's top face at (100,35), got {point:?}"
        );
        let normal = get_collision_normal(&tree, ray_id);
        assert!(
            normal.y < -0.9 && normal.x.abs() < 0.1,
            "the surface normal must face back along the ray, got {normal:?}"
        );
    }

    /// The `_hit_*` keys are the ones the GDScript bridge reads; the legacy
    /// `is_colliding`/`collider` keys still have readers and stay in sync.
    #[test]
    fn raycast2d_publishes_hit_keys_the_script_bridge_reads() {
        let (mut tree, server, ray_id, wall_id, _player) = rotated_ray_scene();
        force_raycast_update(&mut tree, &server, ray_id);

        let node = tree.get_node(ray_id).expect("ray node");
        assert!(
            matches!(node.get_property("_hit_colliding"), Variant::Bool(true)),
            "`_hit_colliding` is what `$Ray.is_colliding()` reads"
        );
        assert!(
            matches!(node.get_property("_hit_collider"), Variant::ObjectId(_)),
            "`get_collider()` expects an ObjectId handle, not a raw Int"
        );
        assert!(matches!(
            node.get_property("_hit_point"),
            Variant::Vector2(_)
        ));
        assert!(matches!(
            node.get_property("_hit_normal"),
            Variant::Vector2(_)
        ));

        // Legacy keys kept in sync.
        assert!(matches!(
            node.get_property("is_colliding"),
            Variant::Bool(true)
        ));
        match node.get_property("collider") {
            Variant::Int(raw) => assert_eq!(raw as u64, wall_id.raw()),
            other => panic!("legacy `collider` should stay an Int, got {other:?}"),
        }
    }

    /// A mask that shares no bit with the body's layer must yield no hit —
    /// the unfiltered `world.raycast` this driver used to call ignored masks
    /// entirely, so mask-filtered rays hit things they were told to skip.
    #[test]
    fn raycast2d_mask_mismatch_reports_no_hit() {
        let (mut tree, server, ray_id, _wall, _player) = rotated_ray_scene();

        // The wall is on layer 1 (Godot's default); ask only for layer 2.
        if let Some(node) = tree.get_node_mut(ray_id) {
            node.set_property("collision_mask", Variant::Int(2));
        }
        assert!(
            !force_raycast_update(&mut tree, &server, ray_id),
            "a layer-2 mask must not see a layer-1 wall"
        );
        assert!(!is_colliding(&tree, ray_id));
        assert_eq!(get_collider(&tree, ray_id), None);

        // Restoring the default mask brings the hit back, proving the miss came
        // from the filter rather than from the geometry.
        if let Some(node) = tree.get_node_mut(ray_id) {
            node.set_property("collision_mask", Variant::Int(1));
        }
        assert!(force_raycast_update(&mut tree, &server, ray_id));
    }

    /// `exclude_parent` (Godot default true) keeps a ray from hitting the body
    /// it is attached to. Unrotated so the only variable is the exclusion.
    #[test]
    fn raycast2d_exclude_parent_skips_own_body() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Parent body spanning x ∈ [-10,10].
        let mut player = Node::new("Player", "CharacterBody2D");
        player.set_property("position", Variant::Vector2(Vector2::ZERO));
        let player_id = tree.add_child(root, player).unwrap();
        let mut pshape = Node::new("Shape", "CollisionShape2D");
        pshape.set_property("size", Variant::Vector2(Vector2::new(20.0, 20.0)));
        tree.add_child(player_id, pshape).unwrap();

        // Ray starts OUTSIDE the parent at x=-30 and aims through it at a wall
        // spanning x ∈ [45,55]; without the exclusion it stops on its own body.
        let mut ray = Node::new("Ray", "RayCast2D");
        ray.set_property("position", Variant::Vector2(Vector2::new(-30.0, 0.0)));
        ray.set_property(
            "target_position",
            Variant::Vector2(Vector2::new(100.0, 0.0)),
        );
        let ray_id = tree.add_child(player_id, ray).unwrap();

        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut wshape = Node::new("Shape", "CollisionShape2D");
        wshape.set_property("size", Variant::Vector2(Vector2::new(10.0, 200.0)));
        tree.add_child(wall_id, wshape).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // Default: parent excluded, so the ray reaches past it to the wall.
        assert!(force_raycast_update(&mut tree, &server, ray_id));
        assert_eq!(
            get_collider(&tree, ray_id),
            Some(wall_id),
            "with exclude_parent the ray must pass through its own body"
        );

        // Opting out reports the parent body instead — the exclusion is real,
        // not an accident of the parent being out of range.
        if let Some(node) = tree.get_node_mut(ray_id) {
            node.set_property("exclude_parent", Variant::Bool(false));
        }
        assert!(force_raycast_update(&mut tree, &server, ray_id));
        assert_eq!(
            get_collider(&tree, ray_id),
            Some(player_id),
            "without exclude_parent the nearest hit is the parent body"
        );
    }

    /// A disabled ray reports no collision rather than a stale result.
    #[test]
    fn raycast2d_disabled_clears_previous_hit() {
        let (mut tree, server, ray_id, _wall, _player) = rotated_ray_scene();
        assert!(force_raycast_update(&mut tree, &server, ray_id));

        set_enabled(&mut tree, ray_id, false);
        assert!(!force_raycast_update(&mut tree, &server, ray_id));
        assert!(
            !is_colliding(&tree, ray_id),
            "a disabled ray must clear its cached hit, not keep reporting it"
        );
    }
}
