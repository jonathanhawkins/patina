//! 3D physics server that bridges scene tree nodes to `gdphysics3d`.
//!
//! The [`PhysicsServer3D`] maintains a [`PhysicsWorld3D`] and maps scene tree
//! nodes of type `RigidBody3D`, `StaticBody3D`, and `CharacterBody3D` to
//! physics bodies. Collision shapes are registered from child
//! `CollisionShape3D` nodes.
//!
//! The typical frame flow mirrors the 2D physics server:
//! 1. [`sync_to_physics`](PhysicsServer3D::sync_to_physics) — push node
//!    transforms into the physics world.
//! 2. [`step`](PhysicsServer3D::step) — advance the physics simulation by
//!    one fixed timestep.
//! 3. [`sync_from_physics`](PhysicsServer3D::sync_from_physics) — pull
//!    updated transforms back into the scene tree.

use std::collections::HashMap;

use gdcore::math::Vector3;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;

/// Maps scene node class names to 3D physics body types.
fn body_type_for_class_3d(class_name: &str) -> Option<BodyType3D> {
    match class_name {
        "RigidBody3D" => Some(BodyType3D::Rigid),
        "StaticBody3D" => Some(BodyType3D::Static),
        "CharacterBody3D" => Some(BodyType3D::Kinematic),
        _ => None,
    }
}

/// Reads a float property from a 3D node, defaulting to `default`.
fn float_prop_3d(tree: &SceneTree, id: NodeId, key: &str, default: f64) -> f64 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a `u32` bitmask property (`collision_layer` / `collision_mask`),
/// defaulting to `default`.
fn u32_prop_3d(tree: &SceneTree, id: NodeId, key: &str, default: u32) -> u32 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Int(i) => i as u32,
            Variant::Float(f) => f as u32,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a truthy property that Godot may expose as a bool or as an enum int
/// (e.g. `continuous_cd`'s CCD mode, where any nonzero mode enables CCD).
fn truthy_prop_3d(tree: &SceneTree, id: NodeId, key: &str, default: bool) -> bool {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Bool(b) => b,
            Variant::Int(i) => i != 0,
            Variant::Float(f) => f != 0.0,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a `PackedVector3Array` (or an `Array` of `Vector3`) property from a
/// node, returning its points. Returns an empty vec when the property is
/// absent or not a vector array — used to populate convex-hull / trimesh
/// geometry from `CollisionShape3D` sub-resources (pat-u3l8c.56).
fn vec3_array_prop_3d(node: &crate::node::Node, key: &str) -> Vec<Vector3> {
    match node.get_property(key) {
        Variant::PackedVector3Array(pts) => pts,
        Variant::Array(items) => items
            .into_iter()
            .filter_map(|v| match v {
                Variant::Vector3(p) => Some(p),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extracts a 3D collision shape from a `CollisionShape3D` node's properties.
pub(crate) fn shape_from_node_3d(tree: &SceneTree, node_id: NodeId) -> Option<Shape3D> {
    let node = tree.get_node(node_id)?;
    if node.class_name() != "CollisionShape3D" {
        return None;
    }

    // If the shape is disabled, skip it.
    if let Variant::Bool(true) = node.get_property("disabled") {
        return None;
    }

    match node.get_property("shape") {
        Variant::String(s) => match s.as_str() {
            "SphereShape3D" => {
                let radius = float_prop_3d(tree, node_id, "radius", 0.5) as f32;
                Some(Shape3D::Sphere { radius })
            }
            "BoxShape3D" => {
                let size = match node.get_property("size") {
                    Variant::Vector3(v) => v,
                    _ => Vector3::new(1.0, 1.0, 1.0),
                };
                Some(Shape3D::BoxShape {
                    half_extents: Vector3::new(size.x / 2.0, size.y / 2.0, size.z / 2.0),
                })
            }
            "CapsuleShape3D" => {
                let radius = float_prop_3d(tree, node_id, "radius", 0.5) as f32;
                let height = float_prop_3d(tree, node_id, "height", 1.0) as f32;
                Some(Shape3D::CapsuleShape { radius, height })
            }
            "CylinderShape3D" => {
                let radius = float_prop_3d(tree, node_id, "radius", 0.5) as f32;
                let height = float_prop_3d(tree, node_id, "height", 1.0) as f32;
                Some(Shape3D::CylinderShape { radius, height })
            }
            "ConvexPolygonShape3D" => Some(Shape3D::ConvexPolygonShape {
                points: vec3_array_prop_3d(node, "points"),
            }),
            "ConcavePolygonShape3D" => Some(Shape3D::ConcavePolygonShape {
                // Godot stores the triangle soup in `data` (every 3 vertices =
                // one triangle); accept `faces` as an alias.
                faces: {
                    let data = vec3_array_prop_3d(node, "data");
                    if data.is_empty() {
                        vec3_array_prop_3d(node, "faces")
                    } else {
                        data
                    }
                },
            }),
            "WorldBoundaryShape3D" => Some(Shape3D::WorldBoundaryShape {
                normal: Vector3::new(0.0, 1.0, 0.0),
                distance: 0.0,
            }),
            _ => None,
        },
        _ => {
            // Fallback: check for "radius" (sphere) or "size" (box) properties.
            let radius_prop = node.get_property("radius");
            match &radius_prop {
                Variant::Float(r) => Some(Shape3D::Sphere { radius: *r as f32 }),
                _ => None,
            }
        }
    }
}

/// Bridges scene tree 3D physics nodes to a [`PhysicsWorld3D`].
///
/// Tracks which scene nodes have been registered as physics bodies and
/// provides the sync-step-sync frame flow for 3D physics simulation.
pub struct PhysicsServer3D {
    world: PhysicsWorld3D,
    node_to_body: HashMap<NodeId, BodyId3D>,
    gravity: Vector3,
}

impl PhysicsServer3D {
    /// Creates a new 3D physics server with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }

    /// Creates a new 3D physics server with custom gravity.
    pub fn with_gravity(gravity: Vector3) -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            gravity,
        }
    }

    /// Returns the gravity vector.
    pub fn gravity(&self) -> Vector3 {
        self.gravity
    }

    /// Sets the global 3D gravity vector (m/s²).
    ///
    /// Mirrors Godot's `physics/3d/default_gravity` × `default_gravity_vector`
    /// (default direction `(0, -1, 0)`). Boot code calls this when a project
    /// overrides the default gravity. Propagates into the underlying
    /// [`PhysicsWorld3D`] so integration actually honors the new value.
    pub fn set_gravity(&mut self, gravity: Vector3) {
        self.gravity = gravity;
        self.world.gravity = gravity;
    }

    /// Returns the number of tracked physics bodies.
    pub fn body_count(&self) -> usize {
        self.node_to_body.len()
    }

    /// Returns the underlying physics world (read-only).
    pub fn world(&self) -> &PhysicsWorld3D {
        &self.world
    }

    /// Returns the physics body id registered for `node_id`, if any.
    pub fn body_for_node(&self, node_id: NodeId) -> Option<BodyId3D> {
        self.node_to_body.get(&node_id).copied()
    }

    /// Returns the scene node that was registered for `body_id`, if any.
    pub fn node_for_body(&self, body_id: BodyId3D) -> Option<NodeId> {
        self.node_to_body
            .iter()
            .find(|&(_, &b)| b == body_id)
            .map(|(&n, _)| n)
    }

    /// Syncs scene tree 3D body nodes into the physics world.
    ///
    /// Registers new bodies for untracked nodes and updates positions for
    /// existing tracked bodies.
    pub fn sync_to_physics(&mut self, tree: &SceneTree) {
        let all_nodes = tree.all_nodes_in_tree_order();
        let mut current_body_nodes: HashMap<NodeId, ()> = HashMap::new();

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                let class = node.class_name();
                if let Some(body_type) = body_type_for_class_3d(class) {
                    current_body_nodes.insert(nid, ());

                    if !self.node_to_body.contains_key(&nid) {
                        // Register a new body.
                        let placeholder_id = BodyId3D(0);

                        let position = node3d::get_global_transform(tree, nid).origin;
                        let mass = float_prop_3d(tree, nid, "mass", 1.0) as f32;

                        // Look for a CollisionShape3D child.
                        let shape = tree
                            .get_node(nid)
                            .map(|n| n.children().to_vec())
                            .unwrap_or_default()
                            .iter()
                            .find_map(|&child_id| shape_from_node_3d(tree, child_id))
                            .unwrap_or(Shape3D::Sphere { radius: 0.5 });

                        let mut body =
                            PhysicsBody3D::new(placeholder_id, body_type, position, shape, mass);

                        // Initial orientation from the node transform.
                        body.rotation =
                            gdcore::math3d::Quaternion::from_euler(node3d::get_rotation(tree, nid));

                        // Read the physics material / body properties from the
                        // node so authored values drive the simulation.
                        body.collision_layer = u32_prop_3d(tree, nid, "collision_layer", 1);
                        body.collision_mask = u32_prop_3d(tree, nid, "collision_mask", 1);
                        body.gravity_scale = float_prop_3d(tree, nid, "gravity_scale", 1.0) as f32;
                        body.linear_damp = float_prop_3d(tree, nid, "linear_damp", 0.0) as f32;
                        body.angular_damp = float_prop_3d(tree, nid, "angular_damp", 0.0) as f32;
                        body.friction = float_prop_3d(tree, nid, "friction", 0.5) as f32;
                        body.bounce = float_prop_3d(tree, nid, "bounce", 0.0) as f32;
                        body.continuous_cd = truthy_prop_3d(tree, nid, "continuous_cd", false);
                        body.can_sleep = truthy_prop_3d(tree, nid, "can_sleep", true);

                        let actual_id = self.world.add_body(body);
                        self.node_to_body.insert(nid, actual_id);
                    } else if body_type != BodyType3D::Rigid {
                        // Static/kinematic bodies are driven by the scene tree:
                        // push their current node transform into the world so a
                        // moving platform (animated StaticBody/CharacterBody)
                        // pushes rigid bodies correctly.
                        if let Some(&body_id) = self.node_to_body.get(&nid) {
                            let position = node3d::get_global_transform(tree, nid).origin;
                            let rotation = gdcore::math3d::Quaternion::from_euler(
                                node3d::get_rotation(tree, nid),
                            );
                            if let Some(b) = self.world.get_body_mut(body_id) {
                                b.position = position;
                                b.rotation = rotation;
                            }
                        }
                    }
                }
            }
        }

        // Remove bodies for nodes that no longer exist.
        let stale: Vec<NodeId> = self
            .node_to_body
            .keys()
            .filter(|nid| !current_body_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(body_id) = self.node_to_body.remove(&nid) {
                // Free the simulated body so a removed node stops simulating and
                // does not leak in the physics world.
                self.world.remove_body(body_id);
            }
        }
    }

    /// Steps the 3D physics simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.world.step(dt);
    }

    /// Syncs updated physics transforms back into the scene tree.
    ///
    /// Only rigid bodies have their positions updated (static/kinematic
    /// bodies are driven by the scene tree, not by physics).
    pub fn sync_from_physics(&self, tree: &mut SceneTree) {
        for (&nid, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                if body.body_type == BodyType3D::Rigid {
                    node3d::set_position(tree, nid, body.position);
                    // Pull the simulated orientation back so tumbling/rolling
                    // rigid bodies rotate their scene nodes too.
                    node3d::set_rotation(tree, nid, body.rotation.to_euler());
                }
            }
        }
    }
}

impl Default for PhysicsServer3D {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PhysicsServer3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsServer3D")
            .field("body_count", &self.node_to_body.len())
            .field("gravity", &self.gravity)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    #[test]
    fn empty_scene_no_bodies() {
        let tree = SceneTree::new();
        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 0);
    }

    #[test]
    fn registers_rigid_body() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Ball", "RigidBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 10.0, 0.0));

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }

    #[test]
    fn registers_static_body() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Floor", "StaticBody3D");
        tree.add_child(root, body).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }

    #[test]
    fn registers_character_body() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Player", "CharacterBody3D");
        tree.add_child(root, body).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }

    #[test]
    fn ignores_non_physics_nodes() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        tree.add_child(root, Node::new("Mesh", "MeshInstance3D"))
            .unwrap();
        tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 0);
    }

    #[test]
    fn step_advances_simulation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Ball", "RigidBody3D");
        let body_nid = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_nid, Vector3::new(0.0, 10.0, 0.0));

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        // Step a few frames.
        for _ in 0..10 {
            server.step(1.0 / 60.0);
        }

        server.sync_from_physics(&mut tree);

        // Rigid body should have fallen due to gravity.
        let pos = node3d::get_position(&tree, body_nid);
        assert!(
            pos.y < 10.0,
            "rigid body should fall due to gravity, got y={}",
            pos.y,
        );
    }

    #[test]
    fn removed_node_untracked() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Ball", "RigidBody3D");
        let body_nid = tree.add_child(root, body).unwrap();
        node3d::set_position(&mut tree, body_nid, Vector3::new(0.0, 5.0, 0.0));

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);

        tree.remove_node(body_nid).unwrap();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 0);
    }

    #[test]
    fn collision_shape_sphere_from_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Ball", "RigidBody3D");
        let body_nid = tree.add_child(root, body).unwrap();

        let mut shape_node = Node::new("Shape", "CollisionShape3D");
        shape_node.set_property("shape", Variant::String("SphereShape3D".to_owned()));
        shape_node.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_nid, shape_node).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }

    #[test]
    fn collision_shape_box_from_child() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Crate", "RigidBody3D");
        let body_nid = tree.add_child(root, body).unwrap();

        let mut shape_node = Node::new("Shape", "CollisionShape3D");
        shape_node.set_property("shape", Variant::String("BoxShape3D".to_owned()));
        shape_node.set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
        tree.add_child(body_nid, shape_node).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }

    #[test]
    fn convex_shape_populates_points_from_resource() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut shape_node = Node::new("Shape", "CollisionShape3D");
        shape_node.set_property("shape", Variant::String("ConvexPolygonShape3D".to_owned()));
        shape_node.set_property(
            "points",
            Variant::PackedVector3Array(vec![
                Vector3::new(-1.0, -1.0, -1.0),
                Vector3::new(1.0, -1.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ]),
        );
        let nid = tree.add_child(root, shape_node).unwrap();

        match shape_from_node_3d(&tree, nid) {
            Some(Shape3D::ConvexPolygonShape { points }) => {
                assert_eq!(points.len(), 3, "convex hull points must be populated");
            }
            other => panic!("expected populated ConvexPolygonShape, got {other:?}"),
        }
    }

    #[test]
    fn concave_shape_populates_faces_from_data() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut shape_node = Node::new("Shape", "CollisionShape3D");
        shape_node.set_property("shape", Variant::String("ConcavePolygonShape3D".to_owned()));
        shape_node.set_property(
            "data",
            Variant::PackedVector3Array(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ]),
        );
        let nid = tree.add_child(root, shape_node).unwrap();

        match shape_from_node_3d(&tree, nid) {
            Some(Shape3D::ConcavePolygonShape { faces }) => {
                assert_eq!(
                    faces.len(),
                    3,
                    "trimesh faces must be populated from `data`"
                );
            }
            other => panic!("expected populated ConcavePolygonShape, got {other:?}"),
        }
    }

    #[test]
    fn convex_shape_empty_without_points() {
        // Missing `points` yields empty geometry rather than a panic.
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut shape_node = Node::new("Shape", "CollisionShape3D");
        shape_node.set_property("shape", Variant::String("ConvexPolygonShape3D".to_owned()));
        let nid = tree.add_child(root, shape_node).unwrap();

        match shape_from_node_3d(&tree, nid) {
            Some(Shape3D::ConvexPolygonShape { points }) => assert!(points.is_empty()),
            other => panic!("expected ConvexPolygonShape, got {other:?}"),
        }
    }

    #[test]
    fn default_gravity() {
        let server = PhysicsServer3D::new();
        let g = server.gravity();
        assert!((g.x).abs() < f32::EPSILON);
        assert!((g.y - (-9.8)).abs() < 0.01);
        assert!((g.z).abs() < f32::EPSILON);
    }

    #[test]
    fn custom_gravity() {
        let server = PhysicsServer3D::with_gravity(Vector3::new(0.0, -20.0, 0.0));
        assert!((server.gravity().y - (-20.0)).abs() < 0.01);
    }

    // -- Server sync fixes (pat-u3l8c.59) ---------------------------------

    #[test]
    fn removed_node_frees_world_body() {
        // Previously remove_body was a no-op that leaked the simulated body.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Ball", "RigidBody3D");
        let body_nid = tree.add_child(root, body).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.world().body_count(), 1);

        tree.remove_node(body_nid).unwrap();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 0);
        assert_eq!(
            server.world().body_count(),
            0,
            "removed node must stop simulating (no leaked world body)"
        );
    }

    #[test]
    fn rotation_syncs_back_to_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body_nid = tree
            .add_child(root, Node::new("Spinner", "RigidBody3D"))
            .unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);

        // Give the simulated body a spin, integrate, and pull it back.
        let bid = server.body_for_node(body_nid).unwrap();
        server
            .world
            .get_body_mut(bid)
            .unwrap()
            .set_angular_velocity(Vector3::new(0.0, 0.0, 5.0));
        for _ in 0..10 {
            server.step(1.0 / 60.0);
        }
        server.sync_from_physics(&mut tree);

        let rot = node3d::get_rotation(&tree, body_nid);
        assert!(
            rot.length() > 1e-3,
            "spinning body should update the node rotation, rot={rot:?}"
        );
    }

    #[test]
    fn reads_physics_props_from_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Ball", "RigidBody3D");
        node.set_property("mass", Variant::Float(4.0));
        node.set_property("collision_mask", Variant::Int(0b0110));
        node.set_property("gravity_scale", Variant::Float(2.5));
        node.set_property("linear_damp", Variant::Float(0.3));
        node.set_property("friction", Variant::Float(0.8));
        node.set_property("bounce", Variant::Float(0.6));
        node.set_property("continuous_cd", Variant::Bool(true));
        node.set_property("can_sleep", Variant::Bool(false));
        let nid = tree.add_child(root, node).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        let bid = server.body_for_node(nid).unwrap();
        let body = server.world().get_body(bid).unwrap();

        assert!((body.mass - 4.0).abs() < 1e-5);
        assert_eq!(body.collision_mask, 0b0110);
        assert!((body.gravity_scale - 2.5).abs() < 1e-5);
        assert!((body.linear_damp - 0.3).abs() < 1e-5);
        assert!((body.friction - 0.8).abs() < 1e-5);
        assert!((body.bounce - 0.6).abs() < 1e-5);
        assert!(body.continuous_cd);
        assert!(!body.can_sleep);
    }

    #[test]
    fn idempotent_sync() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let body = Node::new("Ball", "RigidBody3D");
        tree.add_child(root, body).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);

        // Second sync should not duplicate the body.
        server.sync_to_physics(&tree);
        assert_eq!(server.body_count(), 1);
    }
}
