//! Physics server that bridges scene tree nodes to `gdphysics2d`.
//!
//! The [`PhysicsServer`] maintains a [`PhysicsWorld2D`] and maps scene tree
//! nodes of type `RigidBody2D`, `StaticBody2D`, `CharacterBody2D`, and
//! `Area2D` to physics bodies and areas. Collision shapes are registered
//! from child `CollisionShape2D` nodes.
//!
//! The typical frame flow is:
//! 1. [`sync_to_physics`](PhysicsServer::sync_to_physics) — push node
//!    transforms into the physics world.
//! 2. [`step_physics`](PhysicsServer::step_physics) — advance the physics
//!    simulation by one fixed timestep.
//! 3. [`sync_from_physics`](PhysicsServer::sync_from_physics) — pull
//!    updated transforms back into the scene tree.

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdphysics2d::area2d::{Area2D, AreaId, AreaStore, OverlapEvent};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::{CollisionEvent, PhysicsWorld2D};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Maps scene node types to physics body types.
fn body_type_for_class(class_name: &str) -> Option<BodyType> {
    match class_name {
        "RigidBody2D" => Some(BodyType::Rigid),
        "StaticBody2D" => Some(BodyType::Static),
        "CharacterBody2D" => Some(BodyType::Kinematic),
        _ => None,
    }
}

/// Reads a float property from a node, defaulting to `default`.
fn float_prop(tree: &SceneTree, id: NodeId, key: &str, default: f64) -> f64 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a u32 property from a node, defaulting to `default`.
fn u32_prop(tree: &SceneTree, id: NodeId, key: &str, default: u32) -> u32 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Int(i) => i as u32,
            Variant::Float(f) => f as u32,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a Vector2 property from a node, defaulting to `default`.
fn vec2_prop(tree: &SceneTree, id: NodeId, key: &str, default: Vector2) -> Vector2 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Vector2(v) => v,
            _ => default,
        })
        .unwrap_or(default)
}

/// Extracts a collision shape from a `CollisionShape2D` node's properties.
fn shape_from_node(tree: &SceneTree, node_id: NodeId) -> Option<Shape2D> {
    let node = tree.get_node(node_id)?;
    if node.class_name() != "CollisionShape2D" {
        return None;
    }

    // Check the "shape" property for a shape type string, or try shape-specific properties.
    match node.get_property("shape") {
        Variant::String(s) => match s.as_str() {
            "CircleShape2D" => {
                let radius = float_prop(tree, node_id, "radius", 10.0) as f32;
                Some(Shape2D::Circle { radius })
            }
            "RectangleShape2D" => {
                let size = vec2_prop(tree, node_id, "size", Vector2::new(20.0, 20.0));
                Some(Shape2D::Rectangle {
                    half_extents: Vector2::new(size.x / 2.0, size.y / 2.0),
                })
            }
            _ => None,
        },
        _ => {
            // Fallback: check for "radius" (circle) or "size" (rect) properties directly.
            let radius_prop = node.get_property("radius");
            let size_prop = node.get_property("size");
            match (&radius_prop, &size_prop) {
                (Variant::Float(r), _) => Some(Shape2D::Circle { radius: *r as f32 }),
                (_, Variant::Vector2(sz)) => Some(Shape2D::Rectangle {
                    half_extents: Vector2::new(sz.x / 2.0, sz.y / 2.0),
                }),
                _ => None,
            }
        }
    }
}

/// Finds the first `CollisionShape2D` child of a node and returns its shape.
fn find_collision_shape(tree: &SceneTree, parent_id: NodeId) -> Option<Shape2D> {
    let children = tree.get_node(parent_id)?.children().to_vec();
    for child_id in children {
        if let Some(shape) = shape_from_node(tree, child_id) {
            return Some(shape);
        }
    }
    None
}

/// Scales a collision shape by the given scale vector.
///
/// For circles, the larger of `scale.x` and `scale.y` is used (matching Godot).
/// For rectangles, each axis is scaled independently.
fn scale_shape(shape: Shape2D, scale: Vector2) -> Shape2D {
    match shape {
        Shape2D::Circle { radius } => {
            let s = scale.x.abs().max(scale.y.abs());
            Shape2D::Circle { radius: radius * s }
        }
        Shape2D::Rectangle { half_extents } => Shape2D::Rectangle {
            half_extents: Vector2::new(
                half_extents.x * scale.x.abs(),
                half_extents.y * scale.y.abs(),
            ),
        },
        Shape2D::Segment { a, b } => Shape2D::Segment {
            a: Vector2::new(a.x * scale.x, a.y * scale.y),
            b: Vector2::new(b.x * scale.x, b.y * scale.y),
        },
        Shape2D::Capsule { radius, height } => {
            let s = scale.x.abs().max(scale.y.abs());
            Shape2D::Capsule {
                radius: radius * s,
                height: height * scale.y.abs(),
            }
        }
    }
}

/// A physics trace record for one body at one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsTraceEntry {
    /// The scene node name.
    pub name: String,
    /// Frame number.
    pub frame: u64,
    /// Position at this frame.
    pub position: Vector2,
    /// Linear velocity at this frame.
    pub velocity: Vector2,
}

/// Bridges scene tree nodes to the `gdphysics2d` simulation.
pub struct PhysicsServer {
    /// The 2D physics world.
    world: PhysicsWorld2D,
    /// The area overlap store.
    area_store: AreaStore,
    /// Maps scene node IDs to physics body IDs.
    node_to_body: HashMap<NodeId, BodyId>,
    /// Maps physics body IDs back to scene node IDs.
    body_to_node: HashMap<BodyId, NodeId>,
    /// Maps scene node IDs to area IDs.
    node_to_area: HashMap<NodeId, AreaId>,
    /// Maps area IDs back to scene node IDs.
    area_to_node: HashMap<AreaId, NodeId>,
    /// Collision events from the last physics step.
    last_collision_events: Vec<CollisionEvent>,
    /// Overlap events from the last physics step.
    last_overlap_events: Vec<OverlapEvent>,
    /// Physics trace log (frame-by-frame body positions).
    trace: Vec<PhysicsTraceEntry>,
    /// Whether tracing is enabled.
    tracing_enabled: bool,
    /// Current trace frame counter.
    trace_frame: u64,
}

impl std::fmt::Debug for PhysicsServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsServer")
            .field("bodies", &self.node_to_body.len())
            .field("areas", &self.node_to_area.len())
            .field("tracing", &self.tracing_enabled)
            .finish()
    }
}

impl PhysicsServer {
    /// Creates a new physics server with an empty world.
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld2D::new(),
            area_store: AreaStore::new(),
            node_to_body: HashMap::new(),
            body_to_node: HashMap::new(),
            node_to_area: HashMap::new(),
            area_to_node: HashMap::new(),
            last_collision_events: Vec::new(),
            last_overlap_events: Vec::new(),
            trace: Vec::new(),
            tracing_enabled: false,
            trace_frame: 0,
        }
    }

    /// Enables or disables physics tracing.
    pub fn set_tracing(&mut self, enabled: bool) {
        self.tracing_enabled = enabled;
    }

    /// Returns whether tracing is enabled.
    pub fn is_tracing(&self) -> bool {
        self.tracing_enabled
    }

    /// Returns the collected physics trace.
    pub fn trace(&self) -> &[PhysicsTraceEntry] {
        &self.trace
    }

    /// Clears the physics trace.
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// Returns the number of registered physics bodies.
    pub fn body_count(&self) -> usize {
        self.node_to_body.len()
    }

    /// Returns the number of registered areas.
    pub fn area_count(&self) -> usize {
        self.node_to_area.len()
    }

    /// Returns the collision events from the last physics step.
    pub fn last_collision_events(&self) -> &[CollisionEvent] {
        &self.last_collision_events
    }

    /// Returns the overlap events from the last physics step.
    pub fn last_overlap_events(&self) -> &[OverlapEvent] {
        &self.last_overlap_events
    }

    /// Returns a reference to the underlying physics world.
    pub fn world(&self) -> &PhysicsWorld2D {
        &self.world
    }

    /// Returns the body ID for a scene node, if registered.
    pub fn body_for_node(&self, node_id: NodeId) -> Option<BodyId> {
        self.node_to_body.get(&node_id).copied()
    }

    /// Scans the scene tree and registers any physics body or area nodes
    /// that are not yet tracked.
    pub fn register_bodies(&mut self, tree: &SceneTree) {
        let all_ids = tree.all_nodes_in_tree_order();
        for &node_id in &all_ids {
            let node = match tree.get_node(node_id) {
                Some(n) => n,
                None => continue,
            };
            let class = node.class_name().to_owned();

            // Register physics bodies (RigidBody2D, StaticBody2D, CharacterBody2D).
            if let Some(body_type) = body_type_for_class(&class) {
                if self.node_to_body.contains_key(&node_id) {
                    continue;
                }
                let pos = vec2_prop(tree, node_id, "position", Vector2::ZERO);
                let base_shape =
                    find_collision_shape(tree, node_id).unwrap_or(Shape2D::Circle { radius: 16.0 });
                let node_scale = vec2_prop(tree, node_id, "scale", Vector2::new(1.0, 1.0));
                let shape = scale_shape(base_shape, node_scale);

                let mass = float_prop(tree, node_id, "mass", 1.0) as f32;
                let mut body = PhysicsBody2D::new(BodyId(0), body_type, pos, shape, mass);
                body.collision_layer = u32_prop(tree, node_id, "collision_layer", 1);
                body.collision_mask = u32_prop(tree, node_id, "collision_mask", 1);
                body.bounce = float_prop(tree, node_id, "bounce", 0.0) as f32;
                body.friction = float_prop(tree, node_id, "friction", 0.5) as f32;
                body.linear_velocity = vec2_prop(tree, node_id, "linear_velocity", Vector2::ZERO);
                body.rotation = float_prop(tree, node_id, "rotation", 0.0) as f32;
                body.angular_velocity = float_prop(tree, node_id, "angular_velocity", 0.0) as f32;

                let body_id = self.world.add_body(body);
                self.node_to_body.insert(node_id, body_id);
                self.body_to_node.insert(body_id, node_id);
            }

            // Register areas (Area2D).
            if class == "Area2D" && !self.node_to_area.contains_key(&node_id) {
                let pos = vec2_prop(tree, node_id, "position", Vector2::ZERO);
                let shape =
                    find_collision_shape(tree, node_id).unwrap_or(Shape2D::Circle { radius: 16.0 });

                let mut area = Area2D::new(AreaId(0), pos, shape);
                area.collision_layer = u32_prop(tree, node_id, "collision_layer", 1);
                area.collision_mask = u32_prop(tree, node_id, "collision_mask", 1);
                let area_id = self.area_store.add_area(area);
                self.node_to_area.insert(node_id, area_id);
                self.area_to_node.insert(area_id, node_id);
            }
        }
    }

    /// Pushes scene tree node transforms into the physics world.
    pub fn sync_to_physics(&mut self, tree: &SceneTree) {
        for (&node_id, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body_mut(body_id) {
                // Only sync kinematic body transforms from the scene tree.
                // Rigid bodies are driven by the physics engine.
                if body.body_type == BodyType::Kinematic {
                    body.position = vec2_prop(tree, node_id, "position", Vector2::ZERO);
                    body.rotation = float_prop(tree, node_id, "rotation", 0.0) as f32;
                }
            }
        }
    }

    /// Steps the physics simulation and detects overlaps.
    pub fn step_physics(&mut self, dt: f32) {
        self.last_collision_events = self.world.step(dt);

        // Detect area overlaps using the world's body storage.
        // We need to build a HashMap<BodyId, PhysicsBody2D> for the area store.
        let mut body_map: HashMap<BodyId, PhysicsBody2D> = HashMap::new();
        for (&_node_id, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                body_map.insert(body_id, body.clone());
            }
        }
        self.last_overlap_events = self.area_store.detect_overlaps(&body_map);

        self.trace_frame += 1;
    }

    /// Pulls physics body transforms back into the scene tree.
    pub fn sync_from_physics(&self, tree: &mut SceneTree) {
        for (&node_id, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                // Don't write back static body properties (they don't move).
                if body.body_type == BodyType::Static {
                    continue;
                }
                if let Some(node) = tree.get_node_mut(node_id) {
                    node.set_property("position", Variant::Vector2(body.position));
                    node.set_property("rotation", Variant::Float(body.rotation as f64));
                    node.set_property("linear_velocity", Variant::Vector2(body.linear_velocity));
                    node.set_property(
                        "angular_velocity",
                        Variant::Float(body.angular_velocity as f64),
                    );
                }
            }
        }
    }

    /// Records a trace snapshot of all registered bodies at the current frame.
    ///
    /// Entries are sorted by node name within each frame to ensure
    /// deterministic output regardless of HashMap iteration order.
    pub fn record_trace(&mut self, tree: &SceneTree) {
        if !self.tracing_enabled {
            return;
        }
        let mut frame_entries: Vec<PhysicsTraceEntry> = self
            .node_to_body
            .iter()
            .filter_map(|(&node_id, &body_id)| {
                let body = self.world.get_body(body_id)?;
                let name = tree
                    .get_node(node_id)
                    .map(|n| n.name().to_owned())
                    .unwrap_or_default();
                Some(PhysicsTraceEntry {
                    name,
                    frame: self.trace_frame,
                    position: body.position,
                    velocity: body.linear_velocity,
                })
            })
            .collect();
        frame_entries.sort_by(|a, b| a.name.cmp(&b.name));
        self.trace.extend(frame_entries);
    }
}

impl Default for PhysicsServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;

    fn make_physics_scene() -> (SceneTree, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // RigidBody2D with CollisionShape2D child
        let mut rigid = Node::new("Ball", "RigidBody2D");
        rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
        rigid.set_property("mass", Variant::Float(1.0));
        let rigid_id = tree.add_child(root, rigid).unwrap();

        let mut shape = Node::new("CollisionShape", "CollisionShape2D");
        shape.set_property("radius", Variant::Float(16.0));
        tree.add_child(rigid_id, shape).unwrap();

        // StaticBody2D with CollisionShape2D child
        let mut static_body = Node::new("Floor", "StaticBody2D");
        static_body.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        let static_id = tree.add_child(root, static_body).unwrap();

        let mut shape2 = Node::new("CollisionShape", "CollisionShape2D");
        shape2.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
        tree.add_child(static_id, shape2).unwrap();

        (tree, rigid_id, static_id)
    }

    #[test]
    fn register_bodies_finds_physics_nodes() {
        let (tree, _, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        assert_eq!(
            server.body_count(),
            2,
            "should find RigidBody2D and StaticBody2D"
        );
    }

    #[test]
    fn collision_shape_circle_registered() {
        let (tree, rigid_id, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(rigid_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.shape, Shape2D::Circle { radius: 16.0 });
    }

    #[test]
    fn collision_shape_rect_registered() {
        let (tree, _, static_id) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(static_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(
            body.shape,
            Shape2D::Rectangle {
                half_extents: Vector2::new(200.0, 10.0)
            }
        );
    }

    #[test]
    fn static_body_does_not_move_after_step() {
        let (tree, _, static_id) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(static_id).unwrap();
        let pos_before = server.world().get_body(body_id).unwrap().position;

        for _ in 0..10 {
            server.step_physics(1.0 / 60.0);
        }

        let pos_after = server.world().get_body(body_id).unwrap().position;
        assert_eq!(pos_before, pos_after, "static body must not move");
    }

    #[test]
    fn sync_from_physics_updates_node_position() {
        let (mut tree, rigid_id, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // Give the rigid body some velocity
        let body_id = server.body_for_node(rigid_id).unwrap();
        server.world.get_body_mut(body_id).unwrap().linear_velocity = Vector2::new(100.0, 0.0);

        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);

        let pos = vec2_prop(&tree, rigid_id, "position", Vector2::ZERO);
        assert!(
            pos.x > 100.0,
            "rigid body should have moved right, got {:?}",
            pos
        );
    }

    #[test]
    fn static_position_not_overwritten_by_sync() {
        let (mut tree, _, static_id) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let pos_before = vec2_prop(&tree, static_id, "position", Vector2::ZERO);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);

        let pos_after = vec2_prop(&tree, static_id, "position", Vector2::ZERO);
        assert_eq!(
            pos_before, pos_after,
            "static body node position must not change"
        );
    }

    #[test]
    fn register_bodies_idempotent() {
        let (tree, _, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.register_bodies(&tree);
        assert_eq!(
            server.body_count(),
            2,
            "second register should not duplicate"
        );
    }

    #[test]
    fn collision_events_generated_on_overlap() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Two overlapping circles
        let mut a = Node::new("A", "RigidBody2D");
        a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let a_id = tree.add_child(root, a).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(a_id, sa).unwrap();

        let mut b = Node::new("B", "RigidBody2D");
        b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
        let b_id = tree.add_child(root, b).unwrap();
        let mut sb = Node::new("Shape", "CollisionShape2D");
        sb.set_property("radius", Variant::Float(10.0));
        tree.add_child(b_id, sb).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);

        assert!(
            !server.last_collision_events().is_empty(),
            "overlapping bodies should generate collision events"
        );
    }

    #[test]
    fn area_overlap_detection() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // A rigid body
        let mut body = Node::new("Body", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_id, s).unwrap();

        // An area overlapping the body
        let mut area = Node::new("Zone", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(area_id, sa).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);

        assert!(
            !server.last_overlap_events().is_empty(),
            "area should detect overlapping body"
        );
    }

    #[test]
    fn physics_trace_records_positions() {
        let (tree, _, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.set_tracing(true);
        server.register_bodies(&tree);

        server.step_physics(1.0 / 60.0);
        server.record_trace(&tree);
        server.step_physics(1.0 / 60.0);
        server.record_trace(&tree);

        assert!(
            server.trace().len() >= 4,
            "should have 2 bodies * 2 frames = 4+ trace entries, got {}",
            server.trace().len()
        );
    }

    #[test]
    fn character_body_registered_as_kinematic() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(char_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.body_type, BodyType::Kinematic);
    }

    #[test]
    fn collision_layer_mask_from_node_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut body_node = Node::new("Body", "RigidBody2D");
        body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
        body_node.set_property("collision_layer", Variant::Int(3));
        body_node.set_property("collision_mask", Variant::Int(5));
        let id = tree.add_child(root, body_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.collision_layer, 3);
        assert_eq!(body.collision_mask, 5);
    }
}
