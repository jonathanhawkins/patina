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
use gdphysics2d::area2d::{
    Area2D, AreaId, AreaOverlapEvent, AreaSpaceOverride, AreaStore, OverlapEvent, OverlapState,
};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::character::CharacterBody2D as PhysicsCharacterBody2D;
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::{CollisionEvent, ContactState, PhysicsWorld2D};
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

/// Reads a signed integer property from a node, defaulting to `default`.
fn i32_prop(tree: &SceneTree, id: NodeId, key: &str, default: i32) -> i32 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Int(i) => i as i32,
            Variant::Float(f) => f as i32,
            _ => default,
        })
        .unwrap_or(default)
}

/// Maps a Godot `Area2D.SpaceOverride` enum integer to [`AreaSpaceOverride`].
fn space_override_from_int(value: u32) -> AreaSpaceOverride {
    match value {
        1 => AreaSpaceOverride::Combine,
        2 => AreaSpaceOverride::CombineReplace,
        3 => AreaSpaceOverride::Replace,
        4 => AreaSpaceOverride::ReplaceCombine,
        _ => AreaSpaceOverride::Disabled,
    }
}

/// Reads a bool property from a node, defaulting to `default`.
fn bool_prop(tree: &SceneTree, id: NodeId, key: &str, default: bool) -> bool {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Bool(b) => b,
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

/// Reads a `PackedVector2Array` property from a node (used for polygon points
/// and concave-polygon segments). Also accepts a generic `Array` of `Vector2`.
/// Returns an empty vec when the property is missing or the wrong type.
fn vec2array_prop(tree: &SceneTree, id: NodeId, key: &str) -> Vec<Vector2> {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::PackedVector2Array(v) => v,
            Variant::Array(items) => items
                .into_iter()
                .filter_map(|it| match it {
                    Variant::Vector2(v) => Some(v),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
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
            "CapsuleShape2D" => {
                let radius = float_prop(tree, node_id, "radius", 10.0) as f32;
                let height = float_prop(tree, node_id, "height", 20.0) as f32;
                Some(Shape2D::Capsule { radius, height })
            }
            "SegmentShape2D" => {
                let a = vec2_prop(tree, node_id, "a", Vector2::ZERO);
                let b = vec2_prop(tree, node_id, "b", Vector2::new(0.0, 10.0));
                Some(Shape2D::Segment { a, b })
            }
            "ConvexPolygonShape2D" => {
                let points = vec2array_prop(tree, node_id, "points");
                if points.is_empty() {
                    None
                } else {
                    Some(Shape2D::convex_polygon(&points))
                }
            }
            "ConcavePolygonShape2D" => {
                // Godot stores concave collision as a flat "segments" array of
                // point pairs.
                let segments = vec2array_prop(tree, node_id, "segments");
                if segments.is_empty() {
                    None
                } else {
                    Some(Shape2D::concave_polygon(&segments))
                }
            }
            "WorldBoundaryShape2D" => {
                let normal = vec2_prop(tree, node_id, "normal", Vector2::new(0.0, -1.0));
                let distance = float_prop(tree, node_id, "distance", 0.0) as f32;
                Some(Shape2D::world_boundary(normal, distance))
            }
            "SeparationRayShape2D" => {
                let length = float_prop(tree, node_id, "length", 20.0) as f32;
                Some(Shape2D::SeparationRay { length })
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
        Shape2D::ConvexPolygon { points, count } => {
            let n = (count as usize).min(points.len());
            let scaled: Vec<Vector2> = points[..n]
                .iter()
                .map(|p| Vector2::new(p.x * scale.x, p.y * scale.y))
                .collect();
            Shape2D::convex_polygon(&scaled)
        }
        Shape2D::ConcavePolygon { points, count } => {
            let n = (count as usize).min(points.len());
            let scaled: Vec<Vector2> = points[..n]
                .iter()
                .map(|p| Vector2::new(p.x * scale.x, p.y * scale.y))
                .collect();
            Shape2D::concave_polygon(&scaled)
        }
        Shape2D::WorldBoundary { normal, distance } => {
            // A half-plane's offset scales with the average axis scale; the
            // normal direction is preserved (exact only for uniform scale).
            let s = (scale.x.abs() + scale.y.abs()) * 0.5;
            Shape2D::WorldBoundary {
                normal,
                distance: distance * s,
            }
        }
        Shape2D::SeparationRay { length } => Shape2D::SeparationRay {
            length: length * scale.y.abs(),
        },
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
    /// Body-area overlap events from the last physics step.
    last_overlap_events: Vec<OverlapEvent>,
    /// Area-area overlap events from the last physics step.
    last_area_overlap_events: Vec<AreaOverlapEvent>,
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
            last_area_overlap_events: Vec::new(),
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

    /// Returns the body-area overlap events from the last physics step.
    pub fn last_overlap_events(&self) -> &[OverlapEvent] {
        &self.last_overlap_events
    }

    /// Returns the area-area overlap events from the last physics step.
    pub fn last_area_overlap_events(&self) -> &[AreaOverlapEvent] {
        &self.last_area_overlap_events
    }

    /// Returns a reference to the underlying physics world.
    pub fn world(&self) -> &PhysicsWorld2D {
        &self.world
    }

    /// Sets the global 2D gravity vector (px/s²) on the underlying world.
    ///
    /// Mirrors Godot's `physics/2d/default_gravity` × `default_gravity_vector`
    /// project settings. The world defaults to `(0, 980)`, so an integrator only
    /// needs to call this when a project overrides the default gravity.
    pub fn set_gravity(&mut self, gravity: Vector2) {
        self.world.set_gravity(gravity);
    }

    /// Returns the global 2D gravity vector (px/s²) from the underlying world.
    pub fn gravity(&self) -> Vector2 {
        self.world.gravity()
    }

    /// Returns the body ID for a scene node, if registered.
    pub fn body_for_node(&self, node_id: NodeId) -> Option<BodyId> {
        self.node_to_body.get(&node_id).copied()
    }

    /// Returns the scene node owning a body, if any (reverse of
    /// [`body_for_node`](Self::body_for_node)). Used to resolve a raycast hit's
    /// `body_id` back to the collider node for `RayCast2D.get_collider`.
    pub fn node_for_body(&self, body_id: BodyId) -> Option<NodeId> {
        self.body_to_node.get(&body_id).copied()
    }

    /// Returns the area ID for a scene node, if registered.
    pub fn area_for_node(&self, node_id: NodeId) -> Option<AreaId> {
        self.node_to_area.get(&node_id).copied()
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
                // Godot RigidBody2D gravity/damping properties.
                body.gravity_scale = float_prop(tree, node_id, "gravity_scale", 1.0) as f32;
                body.linear_damp = float_prop(tree, node_id, "linear_damp", 0.0) as f32;
                body.angular_damp = float_prop(tree, node_id, "angular_damp", 0.0) as f32;

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
                area.monitoring = bool_prop(tree, node_id, "monitoring", true);
                // pat-u3l8c.51 — Area2D gravity/damp space overrides.
                area.gravity_space_override =
                    space_override_from_int(u32_prop(tree, node_id, "gravity_space_override", 0));
                area.gravity = float_prop(tree, node_id, "gravity", 980.0) as f32;
                area.gravity_direction =
                    vec2_prop(tree, node_id, "gravity_direction", Vector2::new(0.0, 1.0));
                area.gravity_is_point = bool_prop(tree, node_id, "gravity_point", false);
                // `gravity_point_center` is stored world-space; the Godot node
                // value is relative to the area, so offset by the area position.
                area.gravity_point_center =
                    pos + vec2_prop(tree, node_id, "gravity_point_center", Vector2::ZERO);
                area.gravity_point_unit_distance =
                    float_prop(tree, node_id, "gravity_point_unit_distance", 0.0) as f32;
                area.linear_damp_space_override = space_override_from_int(u32_prop(
                    tree,
                    node_id,
                    "linear_damp_space_override",
                    0,
                ));
                area.linear_damp = float_prop(tree, node_id, "linear_damp", 0.1) as f32;
                area.priority = i32_prop(tree, node_id, "priority", 0);
                let area_id = self.area_store.add_area(area);
                self.node_to_area.insert(node_id, area_id);
                self.area_to_node.insert(area_id, node_id);
            }
        }
    }

    /// Pushes scene tree node transforms into the physics world.
    ///
    /// For kinematic bodies the scene tree is always authoritative.  For rigid
    /// and static bodies we also sync position/rotation so that manual
    /// `set_property("position", ...)` calls (e.g. teleporting a body) are
    /// picked up by the physics engine and the area-overlap detector.
    pub fn sync_to_physics(&mut self, tree: &mut SceneTree) {
        for (&node_id, &body_id) in &self.node_to_body {
            // Snapshot the node's transform, script-set velocity, and any
            // accumulated force/impulse bridge properties before mutably
            // borrowing the world body (all immutable tree reads).
            let scene_pos = vec2_prop(tree, node_id, "position", Vector2::ZERO);
            let scene_rot = float_prop(tree, node_id, "rotation", 0.0) as f32;
            let node_lin = tree
                .get_node(node_id)
                .map(|n| n.get_property("linear_velocity"));
            let node_ang = tree
                .get_node(node_id)
                .map(|n| n.get_property("angular_velocity"));
            let pending_impulse = vec2_prop(tree, node_id, "_pending_impulse", Vector2::ZERO);
            let pending_ang_impulse =
                float_prop(tree, node_id, "_pending_angular_impulse", 0.0) as f32;
            let pending_force = vec2_prop(tree, node_id, "_pending_force", Vector2::ZERO);
            let pending_torque = float_prop(tree, node_id, "_pending_torque", 0.0) as f32;
            let constant_force = vec2_prop(tree, node_id, "constant_force", Vector2::ZERO);
            let constant_torque = float_prop(tree, node_id, "constant_torque", 0.0) as f32;
            let axis_velocity = match tree
                .get_node(node_id)
                .map(|n| n.get_property("_pending_axis_velocity"))
            {
                Some(Variant::Vector2(v)) => Some(v),
                _ => None,
            };

            let mut is_rigid = false;
            if let Some(body) = self.world.get_body_mut(body_id) {
                match body.body_type {
                    BodyType::Kinematic => {
                        body.position = scene_pos;
                        body.rotation = scene_rot;
                    }
                    BodyType::Static => {
                        // Only pick up manual teleports.
                        if (body.position - scene_pos).length_squared() > 1e-6 {
                            body.position = scene_pos;
                        }
                        if (body.rotation - scene_rot).abs() > 1e-6 {
                            body.rotation = scene_rot;
                        }
                    }
                    BodyType::Rigid => {
                        is_rigid = true;
                        // Teleport detection for the transform.
                        if (body.position - scene_pos).length_squared() > 1e-6 {
                            body.position = scene_pos;
                        }
                        if (body.rotation - scene_rot).abs() > 1e-6 {
                            body.rotation = scene_rot;
                        }
                        // Forward script-set velocity. After `sync_from_physics`
                        // the node value equals the body's own velocity, so this
                        // round-trips harmlessly unless a script overwrote it.
                        if let Some(Variant::Vector2(v)) = node_lin {
                            body.linear_velocity = v;
                        }
                        match node_ang {
                            Some(Variant::Float(f)) => body.angular_velocity = f as f32,
                            Some(Variant::Int(i)) => body.angular_velocity = i as f32,
                            _ => {}
                        }
                        // set_axis_velocity: replace the velocity component along
                        // the given axis with the requested one.
                        if let Some(av) = axis_velocity {
                            let len_sq = av.x * av.x + av.y * av.y;
                            if len_sq > 1e-12 {
                                let dir = av * (1.0 / len_sq.sqrt());
                                let along = body.linear_velocity.dot(dir);
                                body.linear_velocity = body.linear_velocity - dir * along + av;
                            }
                        }
                        // Drain accumulated forces/impulses into the body.
                        if pending_impulse.length_squared() > 0.0 {
                            body.apply_impulse(pending_impulse);
                        }
                        if pending_ang_impulse != 0.0 {
                            body.apply_torque_impulse(pending_ang_impulse);
                        }
                        if pending_force.length_squared() > 0.0 {
                            body.apply_force(pending_force);
                        }
                        if pending_torque != 0.0 {
                            body.apply_torque(pending_torque);
                        }
                        // Constant forces/torques persist each tick (Godot
                        // `constant_force`/`constant_torque`) — not cleared.
                        if constant_force.length_squared() > 0.0 {
                            body.apply_force(constant_force);
                        }
                        if constant_torque != 0.0 {
                            body.apply_torque(constant_torque);
                        }
                    }
                }
            }

            // Clear the one-shot bridge properties so impulses/forces fire
            // exactly once per frame (constant_* are intentionally preserved).
            if is_rigid {
                if let Some(n) = tree.get_node_mut(node_id) {
                    n.set_property("_pending_impulse", Variant::Vector2(Vector2::ZERO));
                    n.set_property("_pending_angular_impulse", Variant::Float(0.0));
                    n.set_property("_pending_force", Variant::Vector2(Vector2::ZERO));
                    n.set_property("_pending_torque", Variant::Float(0.0));
                    if axis_velocity.is_some() {
                        n.set_property("_pending_axis_velocity", Variant::Nil);
                    }
                }
            }
        }

        // Sync Area2D positions and monitoring flag from the scene tree.
        for (&node_id, &area_id) in &self.node_to_area {
            if let Some(area) = self.area_store.get_area_mut(area_id) {
                area.position = vec2_prop(tree, node_id, "position", Vector2::ZERO);
                area.monitoring = bool_prop(tree, node_id, "monitoring", true);
            }
        }
    }

    /// Steps the physics simulation and detects overlaps.
    ///
    /// When any registered [`Area2D`] carries a non-`Disabled` gravity or
    /// linear-damp space override (pat-u3l8c.51), the effective per-body gravity
    /// and damping are computed from the [`AreaStore`] before integration and
    /// fed into the world via
    /// [`PhysicsWorld2D::step_with_area_overrides`](gdphysics2d::world::PhysicsWorld2D::step_with_area_overrides).
    /// Otherwise the plain [`step`](gdphysics2d::world::PhysicsWorld2D::step) is
    /// used, leaving default gravity behaviour untouched.
    pub fn step_physics(&mut self, dt: f32) {
        // Phase 0: compute per-body area gravity/damp overrides (only when an
        // area actually declares one, so default behaviour is unaffected).
        if self.area_store.has_space_overrides() {
            let default_gravity = self.world.gravity();
            let mut gravity_overrides: HashMap<BodyId, Vector2> = HashMap::new();
            let mut damp_overrides: HashMap<BodyId, f32> = HashMap::new();
            for &body_id in self.node_to_body.values() {
                if let Some(body) = self.world.get_body(body_id) {
                    // Only dynamic bodies integrate under gravity/damping.
                    if body.body_type != BodyType::Rigid {
                        continue;
                    }
                    let g = self.area_store.compute_body_gravity(body, default_gravity);
                    let d = self.area_store.compute_body_damp(body, body.linear_damp);
                    gravity_overrides.insert(body_id, g);
                    damp_overrides.insert(body_id, d);
                }
            }
            self.last_collision_events =
                self.world
                    .step_with_area_overrides(dt, &gravity_overrides, &damp_overrides);
        } else {
            self.last_collision_events = self.world.step(dt);
        }

        // Detect area overlaps using the world's body storage.
        // We need to build a HashMap<BodyId, PhysicsBody2D> for the area store.
        let mut body_map: HashMap<BodyId, PhysicsBody2D> = HashMap::new();
        for (&_node_id, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                body_map.insert(body_id, body.clone());
            }
        }
        self.last_overlap_events = self.area_store.detect_overlaps(&body_map);
        // Detect area-area overlaps (pat-u3l8c.51) for area_entered/area_exited.
        self.last_area_overlap_events = self.area_store.detect_area_overlaps();

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

    /// Processes overlap events from the last physics step and emits
    /// `body_entered` / `body_exited` signals on the corresponding Area2D
    /// scene nodes.
    ///
    /// For each `Entered` event the Area2D node emits `"body_entered"` with
    /// the overlapping body's `ObjectId` as an argument.  For `Exited` events
    /// it emits `"body_exited"` with the same argument.
    pub fn process_overlap_signals(&self, tree: &mut SceneTree) {
        for event in &self.last_overlap_events {
            let area_node_id = match self.area_to_node.get(&event.area_id) {
                Some(&id) => id,
                None => continue,
            };
            let body_node_id = match self.body_to_node.get(&event.body_id) {
                Some(&id) => id,
                None => continue,
            };
            let signal_name = match event.state {
                OverlapState::Entered => "body_entered",
                OverlapState::Exited => "body_exited",
            };
            tree.emit_signal(
                area_node_id,
                signal_name,
                &[Variant::ObjectId(body_node_id.object_id())],
            );
        }
    }

    /// Processes area-area overlap events from the last physics step and emits
    /// `area_entered` / `area_exited` signals on the monitoring Area2D node,
    /// passing the *other* area's node `ObjectId` (pat-u3l8c.51).
    ///
    /// Mirrors [`process_overlap_signals`](Self::process_overlap_signals) but
    /// for the directional area-area relation: the event's `area_id` is the
    /// monitoring node that emits, and `other_area_id` is the argument. Because
    /// the relation is directional, a mutually-overlapping pair emits on both
    /// nodes, matching Godot's `Area2D.area_entered` / `area_exited` contract.
    pub fn process_area_overlap_signals(&self, tree: &mut SceneTree) {
        for event in &self.last_area_overlap_events {
            let area_node_id = match self.area_to_node.get(&event.area_id) {
                Some(&id) => id,
                None => continue,
            };
            let other_node_id = match self.area_to_node.get(&event.other_area_id) {
                Some(&id) => id,
                None => continue,
            };
            let signal_name = match event.state {
                OverlapState::Entered => "area_entered",
                OverlapState::Exited => "area_exited",
            };
            tree.emit_signal(
                area_node_id,
                signal_name,
                &[Variant::ObjectId(other_node_id.object_id())],
            );
        }
    }

    /// Processes body-body contact events from the last physics step and emits
    /// `body_entered` / `body_exited` signals on `RigidBody2D` nodes that have
    /// contact monitoring enabled.
    ///
    /// Mirrors Godot's `RigidBody2D` contact reporting: a body only reports
    /// contacts when its `contact_monitor` property is `true` **and** its
    /// `max_contacts_reported` property is greater than zero. Each side of a
    /// contact pair is evaluated independently — a monitoring body emits the
    /// signal with the *other* body's node `ObjectId` as the argument, matching
    /// Godot's `body_entered(body: Node)` / `body_exited(body: Node)` contract.
    /// Static bodies (which lack a `contact_monitor` property) naturally never
    /// emit, while a monitoring rigid body still reports contact with them.
    pub fn process_body_contact_signals(&self, tree: &mut SceneTree) {
        for event in &self.last_collision_events {
            let signal_name = match event.state {
                ContactState::Entered => "body_entered",
                ContactState::Exited => "body_exited",
                // Persisting contacts do not re-fire enter/exit signals.
                ContactState::Persisting => continue,
            };
            // Evaluate both directions: either body in the pair may monitor.
            for (self_body, other_body) in
                [(event.body_a, event.body_b), (event.body_b, event.body_a)]
            {
                let self_node = match self.body_to_node.get(&self_body) {
                    Some(&id) => id,
                    None => continue,
                };
                let other_node = match self.body_to_node.get(&other_body) {
                    Some(&id) => id,
                    None => continue,
                };
                // Godot gates contact reporting on both flags.
                if !bool_prop(tree, self_node, "contact_monitor", false) {
                    continue;
                }
                if u32_prop(tree, self_node, "max_contacts_reported", 0) == 0 {
                    continue;
                }
                tree.emit_signal(
                    self_node,
                    signal_name,
                    &[Variant::ObjectId(other_node.object_id())],
                );
            }
        }
    }

    /// Runs `move_and_slide` for every `CharacterBody2D` node that has a
    /// non-zero velocity, then syncs the resulting position back to the
    /// scene node.
    ///
    /// `dt` is the physics timestep — velocity is multiplied by `dt` to get
    /// the per-tick displacement, matching Godot's `move_and_slide` contract
    /// where velocity is specified in pixels-per-second.
    pub fn process_character_movement(&mut self, dt: f32, tree: &mut SceneTree) {
        // Collect CharacterBody2D node IDs and their body IDs.
        let char_entries: Vec<(NodeId, BodyId)> = self
            .node_to_body
            .iter()
            .filter_map(|(&node_id, &body_id)| {
                let body = self.world.get_body(body_id)?;
                if body.body_type == BodyType::Kinematic {
                    // Verify it's actually a CharacterBody2D node
                    if let Some(node) = tree.get_node(node_id) {
                        if node.class_name() == "CharacterBody2D" {
                            return Some((node_id, body_id));
                        }
                    }
                }
                None
            })
            .collect();

        for (node_id, body_id) in char_entries {
            // `velocity` is in pixels-per-second (Godot contract). Cross-frame
            // state (is_on_floor, platform_velocity) is persisted on the node.
            let velocity = vec2_prop(tree, node_id, "velocity", Vector2::ZERO);
            let was_on_floor = bool_prop(tree, node_id, "is_on_floor", false);
            let stored_platform_vel = vec2_prop(tree, node_id, "platform_velocity", Vector2::ZERO);

            // Skip idle bodies unless a moving platform would still carry them.
            if velocity.length_squared() < 1e-8 && stored_platform_vel.length_squared() < 1e-8 {
                continue;
            }

            let body = match self.world.get_body(body_id) {
                Some(b) => b,
                None => continue,
            };

            // Build a CharacterBody2D seeded from the node's persisted state and
            // configuration properties.
            let mut character = PhysicsCharacterBody2D::new(body.position, body.shape.clone());
            character.collision_layer = body.collision_layer;
            character.collision_mask = body.collision_mask;
            character.up_direction =
                vec2_prop(tree, node_id, "up_direction", Vector2::new(0.0, -1.0));
            character.floor_max_angle = float_prop(
                tree,
                node_id,
                "floor_max_angle",
                std::f64::consts::FRAC_PI_4,
            ) as f32;
            character.floor_snap_length =
                float_prop(tree, node_id, "floor_snap_length", 0.0) as f32;
            character.floor_stop_on_slope =
                bool_prop(tree, node_id, "floor_stop_on_slope", false);
            character.floor_constant_speed =
                bool_prop(tree, node_id, "floor_constant_speed", false);
            character.floor_block_on_wall = bool_prop(tree, node_id, "floor_block_on_wall", true);
            character.slide_on_ceiling = bool_prop(tree, node_id, "slide_on_ceiling", true);
            character.max_slides = u32_prop(tree, node_id, "max_slides", 4).max(1) as usize;
            character.set_on_floor(was_on_floor);
            character.platform_velocity = stored_platform_vel;

            // Collect static/other bodies to collide against (exclude self)
            let obstacle_refs: Vec<&PhysicsBody2D> = self
                .node_to_body
                .iter()
                .filter_map(|(&other_node, &other_body_id)| {
                    if other_node == node_id {
                        return None;
                    }
                    self.world.get_body(other_body_id)
                })
                .collect();

            // move_and_slide_delta applies dt internally and returns px/s velocity.
            let result_velocity = character.move_and_slide_delta(velocity, dt, &obstacle_refs);
            let result_platform_vel = character.get_platform_velocity();
            let new_position = character.position;
            let on_floor = character.is_on_floor();
            let on_wall = character.is_on_wall();
            let on_ceiling = character.is_on_ceiling();

            // Release the immutable borrow of `self.world` before the mutable one.
            drop(obstacle_refs);

            // Write position back to the physics body
            if let Some(phys_body) = self.world.get_body_mut(body_id) {
                phys_body.position = new_position;
            }

            // Sync to scene tree
            if let Some(node) = tree.get_node_mut(node_id) {
                node.set_property("position", Variant::Vector2(new_position));
                node.set_property("velocity", Variant::Vector2(result_velocity));
                node.set_property("platform_velocity", Variant::Vector2(result_platform_vel));
                node.set_property("is_on_floor", Variant::Bool(on_floor));
                node.set_property("is_on_wall", Variant::Bool(on_wall));
                node.set_property("is_on_ceiling", Variant::Bool(on_ceiling));
            }
        }
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

    /// Builds a tree with a single `CollisionShape2D` under root carrying the
    /// given shape class and properties, returning the tree and shape node id.
    fn shape_node(class: &str, props: &[(&str, Variant)]) -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("shape", Variant::String(class.to_string()));
        for (k, v) in props {
            s.set_property(k, v.clone());
        }
        let id = tree.add_child(root, s).unwrap();
        (tree, id)
    }

    #[test]
    fn shape_from_node_builds_capsule() {
        let (tree, id) = shape_node(
            "CapsuleShape2D",
            &[
                ("radius", Variant::Float(6.0)),
                ("height", Variant::Float(30.0)),
            ],
        );
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::Capsule {
                radius: 6.0,
                height: 30.0
            })
        );
    }

    #[test]
    fn shape_from_node_builds_segment() {
        let (tree, id) = shape_node(
            "SegmentShape2D",
            &[
                ("a", Variant::Vector2(Vector2::new(-3.0, 0.0))),
                ("b", Variant::Vector2(Vector2::new(3.0, 0.0))),
            ],
        );
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::Segment {
                a: Vector2::new(-3.0, 0.0),
                b: Vector2::new(3.0, 0.0),
            })
        );
    }

    #[test]
    fn shape_from_node_builds_convex_polygon() {
        let pts = vec![
            Vector2::new(-1.0, -1.0),
            Vector2::new(1.0, -1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(-1.0, 1.0),
        ];
        let (tree, id) = shape_node(
            "ConvexPolygonShape2D",
            &[("points", Variant::PackedVector2Array(pts.clone()))],
        );
        let shape = shape_from_node(&tree, id).unwrap();
        assert_eq!(shape, Shape2D::convex_polygon(&pts));
        assert_eq!(shape.polygon_points().len(), 4);
    }

    #[test]
    fn shape_from_node_builds_concave_polygon() {
        let segs = vec![Vector2::new(-5.0, 0.0), Vector2::new(5.0, 0.0)];
        let (tree, id) = shape_node(
            "ConcavePolygonShape2D",
            &[("segments", Variant::PackedVector2Array(segs.clone()))],
        );
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::concave_polygon(&segs))
        );
    }

    #[test]
    fn shape_from_node_builds_world_boundary() {
        let (tree, id) = shape_node(
            "WorldBoundaryShape2D",
            &[
                ("normal", Variant::Vector2(Vector2::new(0.0, -1.0))),
                ("distance", Variant::Float(4.0)),
            ],
        );
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::world_boundary(Vector2::new(0.0, -1.0), 4.0))
        );
    }

    #[test]
    fn shape_from_node_builds_separation_ray() {
        let (tree, id) = shape_node("SeparationRayShape2D", &[("length", Variant::Float(25.0))]);
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::SeparationRay { length: 25.0 })
        );
    }

    #[test]
    fn scale_shape_scales_convex_polygon() {
        let poly = Shape2D::convex_polygon(&[
            Vector2::new(-1.0, -2.0),
            Vector2::new(1.0, -2.0),
            Vector2::new(0.0, 2.0),
        ]);
        let scaled = scale_shape(poly, Vector2::new(2.0, 3.0));
        let pts = scaled.polygon_points();
        assert_eq!(pts.len(), 3);
        assert!((pts[0].x + 2.0).abs() < 1e-5); // -1 * 2
        assert!((pts[0].y + 6.0).abs() < 1e-5); // -2 * 3
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

    #[test]
    fn gravity_scale_and_damp_read_from_node_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut body_node = Node::new("Body", "RigidBody2D");
        body_node.set_property("position", Variant::Vector2(Vector2::ZERO));
        body_node.set_property("gravity_scale", Variant::Float(2.5));
        body_node.set_property("linear_damp", Variant::Float(0.75));
        body_node.set_property("angular_damp", Variant::Float(0.25));
        let id = tree.add_child(root, body_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert!((body.gravity_scale - 2.5).abs() < 1e-6);
        assert!((body.linear_damp - 0.75).abs() < 1e-6);
        assert!((body.angular_damp - 0.25).abs() < 1e-6);
    }

    #[test]
    fn gravity_scale_and_damp_default_when_absent() {
        // A RigidBody2D with no explicit gravity/damping properties uses the
        // Godot defaults (gravity_scale 1.0, damping 0.0).
        let (tree, rigid_id, _) = make_physics_scene();
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(rigid_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.gravity_scale, 1.0);
        assert_eq!(body.linear_damp, 0.0);
        assert_eq!(body.angular_damp, 0.0);
    }

    #[test]
    fn server_default_gravity_is_godot_default() {
        let server = PhysicsServer::new();
        assert_eq!(server.gravity(), Vector2::new(0.0, 980.0));
    }

    #[test]
    fn server_set_gravity_overrides_world_gravity() {
        // A single isolated rigid body (no floor) registered via the bridge
        // falls under the world gravity; set_gravity can zero it out (e.g. for a
        // zero-gravity project).
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut body = Node::new("Ball", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let rigid_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(rigid_id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        let body_id = server.body_for_node(rigid_id).unwrap();
        let start_y = server.world().get_body(body_id).unwrap().position.y;

        // Default gravity → body falls in +Y.
        for _ in 0..30 {
            server.step_physics(1.0 / 60.0);
        }
        let fell_y = server.world().get_body(body_id).unwrap().position.y;
        assert!(fell_y > start_y, "body should fall under default gravity");

        // Zero gravity → body no longer accelerates downward.
        server.set_gravity(Vector2::ZERO);
        assert_eq!(server.gravity(), Vector2::ZERO);
        server.world.get_body_mut(body_id).unwrap().linear_velocity = Vector2::ZERO;
        let held_y = server.world().get_body(body_id).unwrap().position.y;
        for _ in 0..30 {
            server.step_physics(1.0 / 60.0);
        }
        let after_y = server.world().get_body(body_id).unwrap().position.y;
        assert!(
            (after_y - held_y).abs() < 1e-4,
            "body must not fall with zero gravity: {held_y} -> {after_y}"
        );
    }

    // -----------------------------------------------------------------------
    // #159 — Area2D body_entered / body_exited signal dispatch
    // -----------------------------------------------------------------------

    #[test]
    fn area2d_body_entered_signal_fires_on_overlap() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // RigidBody2D overlapping an Area2D
        let mut body = Node::new("Body", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_id, s).unwrap();

        let mut area = Node::new("Zone", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(area_id, sa).unwrap();

        // Connect a callback to body_entered on the Area2D
        let entered_count = Arc::new(AtomicUsize::new(0));
        let cnt = entered_count.clone();
        tree.connect_signal(
            area_id,
            "body_entered",
            Connection::with_callback(area_id.object_id(), "on_body_entered", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);

        assert!(
            !server.last_overlap_events().is_empty(),
            "should detect overlap"
        );

        server.process_overlap_signals(&mut tree);
        assert_eq!(
            entered_count.load(Ordering::SeqCst),
            1,
            "body_entered signal should fire once"
        );
    }

    #[test]
    fn area2d_body_exited_signal_fires_when_body_leaves() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut body = Node::new("Body", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_id, s).unwrap();

        let mut area = Node::new("Zone", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(area_id, sa).unwrap();

        let exited_count = Arc::new(AtomicUsize::new(0));
        let cnt = exited_count.clone();
        tree.connect_signal(
            area_id,
            "body_exited",
            Connection::with_callback(area_id.object_id(), "on_body_exited", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // Frame 1: body enters area
        server.step_physics(0.0);
        server.process_overlap_signals(&mut tree);
        assert_eq!(exited_count.load(Ordering::SeqCst), 0);

        // Move body far away
        if let Some(bid) = server.body_for_node(body_id) {
            server.world.get_body_mut(bid).unwrap().position = Vector2::new(200.0, 0.0);
        }

        // Frame 2: body exits area
        server.step_physics(0.0);
        server.process_overlap_signals(&mut tree);
        assert_eq!(
            exited_count.load(Ordering::SeqCst),
            1,
            "body_exited signal should fire once"
        );
    }

    #[test]
    fn area2d_signal_includes_body_object_id() {
        use gdobject::signal::Connection;
        use std::sync::{Arc, Mutex};

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut body = Node::new("Body", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_id, s).unwrap();

        let mut area = Node::new("Zone", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(area_id, sa).unwrap();

        let received_args: Arc<Mutex<Vec<Variant>>> = Arc::new(Mutex::new(Vec::new()));
        let args_clone = received_args.clone();
        tree.connect_signal(
            area_id,
            "body_entered",
            Connection::with_callback(area_id.object_id(), "on_body_entered", move |args| {
                args_clone.lock().unwrap().extend_from_slice(args);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);
        server.process_overlap_signals(&mut tree);

        let args = received_args.lock().unwrap();
        assert_eq!(
            args.len(),
            1,
            "signal should have 1 arg (the body ObjectId)"
        );
        assert_eq!(
            args[0],
            Variant::ObjectId(body_id.object_id()),
            "signal arg should be the body node's ObjectId"
        );
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.50 — RigidBody2D contact_monitor body_entered / body_exited
    // -----------------------------------------------------------------------

    /// Builds a `RigidBody2D` with a circular collision shape at `pos`.
    /// When `monitor` is true the node opts into contact reporting via
    /// `contact_monitor` + `max_contacts_reported` (Godot's gate).
    fn add_rigid_circle(
        tree: &mut SceneTree,
        parent: NodeId,
        name: &str,
        pos: Vector2,
        radius: f64,
        monitor: bool,
    ) -> NodeId {
        let mut body = Node::new(name, "RigidBody2D");
        body.set_property("position", Variant::Vector2(pos));
        if monitor {
            body.set_property("contact_monitor", Variant::Bool(true));
            body.set_property("max_contacts_reported", Variant::Int(4));
        }
        let body_id = tree.add_child(parent, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(radius));
        tree.add_child(body_id, s).unwrap();
        body_id
    }

    #[test]
    fn rigidbody2d_contact_monitor_emits_body_entered() {
        use gdobject::signal::Connection;
        use std::sync::{Arc, Mutex};

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Two overlapping rigid bodies (centres 8 apart, radii 5 → depth 2).
        let a_id = add_rigid_circle(&mut tree, root, "A", Vector2::new(0.0, 0.0), 5.0, true);
        let b_id = add_rigid_circle(&mut tree, root, "B", Vector2::new(8.0, 0.0), 5.0, false);

        let received: Arc<Mutex<Vec<Variant>>> = Arc::new(Mutex::new(Vec::new()));
        let got = received.clone();
        tree.connect_signal(
            a_id,
            "body_entered",
            Connection::with_callback(a_id.object_id(), "on_body_entered", move |args| {
                got.lock().unwrap().extend_from_slice(args);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);
        server.process_body_contact_signals(&mut tree);

        let args = received.lock().unwrap();
        assert_eq!(
            args.len(),
            1,
            "monitoring RigidBody2D should fire body_entered exactly once"
        );
        assert_eq!(
            args[0],
            Variant::ObjectId(b_id.object_id()),
            "body_entered arg should be the *other* body's node ObjectId"
        );
    }

    #[test]
    fn rigidbody2d_without_contact_monitor_emits_nothing() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Neither body enables contact_monitor.
        let a_id = add_rigid_circle(&mut tree, root, "A", Vector2::new(0.0, 0.0), 5.0, false);
        let _b_id = add_rigid_circle(&mut tree, root, "B", Vector2::new(8.0, 0.0), 5.0, false);

        let count = Arc::new(AtomicUsize::new(0));
        let cnt = count.clone();
        tree.connect_signal(
            a_id,
            "body_entered",
            Connection::with_callback(a_id.object_id(), "on_body_entered", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.step_physics(0.0);
        // A genuine contact must have been detected...
        assert!(
            !server.last_collision_events().is_empty(),
            "overlapping bodies should produce a contact event"
        );
        server.process_body_contact_signals(&mut tree);

        // ...but with contact_monitor disabled, no signal is emitted.
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "no body_entered without contact_monitor"
        );
    }

    #[test]
    fn rigidbody2d_contact_monitor_emits_body_exited() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a_id = add_rigid_circle(&mut tree, root, "A", Vector2::new(0.0, 0.0), 5.0, true);
        let b_id = add_rigid_circle(&mut tree, root, "B", Vector2::new(8.0, 0.0), 5.0, false);

        let exited = Arc::new(AtomicUsize::new(0));
        let cnt = exited.clone();
        tree.connect_signal(
            a_id,
            "body_exited",
            Connection::with_callback(a_id.object_id(), "on_body_exited", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // Frame 1: bodies overlap → contact entered (no exit yet).
        server.step_physics(0.0);
        server.process_body_contact_signals(&mut tree);
        assert_eq!(exited.load(Ordering::SeqCst), 0, "no exit while overlapping");

        // Move B far away so the pair separates.
        if let Some(bid) = server.body_for_node(b_id) {
            server.world.get_body_mut(bid).unwrap().position = Vector2::new(500.0, 0.0);
        }

        // Frame 2: contact ends → body_exited fires once.
        server.step_physics(0.0);
        server.process_body_contact_signals(&mut tree);
        assert_eq!(
            exited.load(Ordering::SeqCst),
            1,
            "body_exited should fire once when the contact ends"
        );
    }

    // -----------------------------------------------------------------------
    // #162 — CharacterBody2D move_and_slide wired through MainLoop
    // -----------------------------------------------------------------------

    #[test]
    fn character_body_move_and_slide_updates_position() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        char_node.set_property("velocity", Variant::Vector2(Vector2::new(100.0, 0.0)));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.process_character_movement(1.0 / 60.0, &mut tree);

        let pos = vec2_prop(&tree, char_id, "position", Vector2::ZERO);
        assert!(
            pos.x > 0.0,
            "CharacterBody2D should have moved right, got {:?}",
            pos
        );
    }

    #[test]
    fn character_body_stops_at_static_wall() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // CharacterBody2D moving right at 600 px/s — with dt=1/60,
        // displacement per tick = 10, which will collide with the nearby wall.
        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        char_node.set_property("velocity", Variant::Vector2(Vector2::new(600.0, 0.0)));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(char_id, s).unwrap();

        // StaticBody2D wall very close: center at x=12, half_extents (5, 100).
        // Wall left edge at x=7. Character radius 4, so collision when center >= 3.
        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(12.0, 0.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut ws = Node::new("Shape", "CollisionShape2D");
        ws.set_property("size", Variant::Vector2(Vector2::new(10.0, 200.0)));
        tree.add_child(wall_id, ws).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.process_character_movement(1.0 / 60.0, &mut tree);

        let pos = vec2_prop(&tree, char_id, "position", Vector2::ZERO);
        // The character should not have passed through the wall
        assert!(
            pos.x < 12.0,
            "CharacterBody2D should stop before the wall at x=12, got x={:.1}",
            pos.x
        );

        // The velocity should have been zeroed on the X axis due to wall collision
        let vel = vec2_prop(&tree, char_id, "velocity", Vector2::ZERO);
        assert!(
            vel.x.abs() < 1.0,
            "X velocity should be near zero after hitting wall, got {:.1}",
            vel.x
        );
    }

    #[test]
    fn character_body_sets_is_on_wall_after_collision() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        char_node.set_property("velocity", Variant::Vector2(Vector2::new(600.0, 0.0)));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(char_id, s).unwrap();

        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(12.0, 0.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut ws = Node::new("Shape", "CollisionShape2D");
        ws.set_property("size", Variant::Vector2(Vector2::new(10.0, 200.0)));
        tree.add_child(wall_id, ws).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.process_character_movement(1.0 / 60.0, &mut tree);

        let on_wall = tree
            .get_node(char_id)
            .map(|n| n.get_property("is_on_wall"))
            .unwrap_or(Variant::Nil);
        assert_eq!(
            on_wall,
            Variant::Bool(true),
            "is_on_wall should be true after hitting a wall"
        );
    }

    #[test]
    fn character_body_zero_velocity_does_not_move() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
        // velocity defaults to ZERO
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        server.process_character_movement(1.0 / 60.0, &mut tree);

        let pos = vec2_prop(&tree, char_id, "position", Vector2::ZERO);
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 20.0);
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.48 — platformer app test: a CharacterBody2D runs across a
    // floor under gravity for many frames. The horizontal velocity must stay
    // in px/s (the pre-fix bug collapsed it to ~v*dt each frame), and the body
    // must settle onto the floor.
    // -----------------------------------------------------------------------

    #[test]
    fn platformer_character_keeps_px_per_second_velocity_and_lands() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Player starts in the air, running right at a constant 100 px/s.
        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        char_node.set_property("velocity", Variant::Vector2(Vector2::new(100.0, 0.0)));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        // Wide floor: top edge at y=90 so the player (radius 8) rests at y=82.
        let mut floor = Node::new("Floor", "StaticBody2D");
        floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
        let floor_id = tree.add_child(root, floor).unwrap();
        let mut fs = Node::new("Shape", "CollisionShape2D");
        fs.set_property("size", Variant::Vector2(Vector2::new(4000.0, 20.0)));
        tree.add_child(floor_id, fs).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let dt = 1.0 / 60.0;
        let gravity = 980.0_f32;
        // Simulate 3 seconds of a platformer loop: gravity is applied by the
        // "script" each frame, exactly like Godot's `_physics_process`.
        for _ in 0..180 {
            let mut v = vec2_prop(&tree, char_id, "velocity", Vector2::ZERO);
            v.y += gravity * dt;
            if let Some(node) = tree.get_node_mut(char_id) {
                node.set_property("velocity", Variant::Vector2(v));
            }
            server.process_character_movement(dt, &mut tree);
        }

        let vel = vec2_prop(&tree, char_id, "velocity", Vector2::ZERO);
        let pos = vec2_prop(&tree, char_id, "position", Vector2::ZERO);
        let on_floor = tree
            .get_node(char_id)
            .map(|n| n.get_property("is_on_floor"))
            .unwrap_or(Variant::Nil);

        // The KEY regression: horizontal velocity must remain ~100 px/s.
        assert!(
            (vel.x - 100.0).abs() < 5.0,
            "horizontal velocity must stay in px/s (~100), got {:.3}",
            vel.x
        );
        // The body should have travelled a long way horizontally.
        assert!(
            pos.x > 150.0,
            "player should have run right across the floor, got x={:.1}",
            pos.x
        );
        // ...and be resting on the floor (top at y=90, radius 8 => center ~82).
        assert!(
            (pos.y - 82.0).abs() < 3.0,
            "player should rest on the floor (~y=82), got y={:.1}",
            pos.y
        );
        assert_eq!(
            on_floor,
            Variant::Bool(true),
            "player should be on the floor at rest"
        );
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.51 — Area2D gravity space override wired through step_physics,
    // plus area_entered/area_exited signal dispatch.
    // -----------------------------------------------------------------------

    #[test]
    fn area_replace_gravity_lifts_body_while_control_falls() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Rigid body sitting inside an anti-gravity Area2D.
        let mut body = Node::new("Floater", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(body_id, s).unwrap();

        // Control rigid body far outside any area — falls under world gravity.
        let mut control = Node::new("Faller", "RigidBody2D");
        control.set_property("position", Variant::Vector2(Vector2::new(5000.0, 0.0)));
        let control_id = tree.add_child(root, control).unwrap();
        let mut cs = Node::new("Shape", "CollisionShape2D");
        cs.set_property("radius", Variant::Float(4.0));
        tree.add_child(control_id, cs).unwrap();

        // Area2D covering the floater with a Replace override pointing up.
        let mut area = Node::new("AntiGravity", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        area.set_property("gravity_space_override", Variant::Int(3)); // Replace
        area.set_property("gravity", Variant::Float(980.0));
        area.set_property("gravity_direction", Variant::Vector2(Vector2::new(0.0, -1.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut asx = Node::new("Shape", "CollisionShape2D");
        asx.set_property("radius", Variant::Float(500.0));
        tree.add_child(area_id, asx).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        for _ in 0..30 {
            server.step_physics(1.0 / 60.0);
        }

        let floater_y = server
            .world()
            .get_body(server.body_for_node(body_id).unwrap())
            .unwrap()
            .position
            .y;
        let faller_y = server
            .world()
            .get_body(server.body_for_node(control_id).unwrap())
            .unwrap()
            .position
            .y;
        assert!(
            floater_y < -1.0,
            "body in a Replace anti-gravity area should rise (negative Y), got {floater_y}"
        );
        assert!(
            faller_y > 1.0,
            "control body should fall under default gravity, got {faller_y}"
        );
    }

    #[test]
    fn default_area_does_not_change_gravity() {
        // An Area2D with the default (Disabled) override must not perturb a
        // body's fall — the plain step path is used.
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut body = Node::new("Ball", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let body_id = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(body_id, s).unwrap();

        let mut area = Node::new("Zone", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let area_id = tree.add_child(root, area).unwrap();
        let mut asx = Node::new("Shape", "CollisionShape2D");
        asx.set_property("radius", Variant::Float(500.0));
        tree.add_child(area_id, asx).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);
        for _ in 0..30 {
            server.step_physics(1.0 / 60.0);
        }

        let y = server
            .world()
            .get_body(server.body_for_node(body_id).unwrap())
            .unwrap()
            .position
            .y;
        // 30 steps of default gravity ⇒ the body should have fallen well past 0.
        assert!(y > 1.0, "disabled-override area must not stop the fall, got {y}");
    }

    #[test]
    fn area2d_area_entered_and_exited_signals_fire() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Two overlapping Area2D nodes (default layer/mask ⇒ mutually detected).
        let mut a = Node::new("A", "Area2D");
        a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        let a_id = tree.add_child(root, a).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(a_id, sa).unwrap();

        let mut b = Node::new("B", "Area2D");
        b.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
        let b_id = tree.add_child(root, b).unwrap();
        let mut sb = Node::new("Shape", "CollisionShape2D");
        sb.set_property("radius", Variant::Float(10.0));
        tree.add_child(b_id, sb).unwrap();

        let entered = Arc::new(AtomicUsize::new(0));
        let e = entered.clone();
        tree.connect_signal(
            a_id,
            "area_entered",
            Connection::with_callback(a_id.object_id(), "on_area_entered", move |_args| {
                e.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );
        let exited = Arc::new(AtomicUsize::new(0));
        let x = exited.clone();
        tree.connect_signal(
            a_id,
            "area_exited",
            Connection::with_callback(a_id.object_id(), "on_area_exited", move |_args| {
                x.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // Frame 1: areas overlap ⇒ area_entered fires on A (for B).
        server.step_physics(0.0);
        assert!(
            !server.last_area_overlap_events().is_empty(),
            "overlapping areas should produce area-area events"
        );
        server.process_area_overlap_signals(&mut tree);
        assert_eq!(entered.load(Ordering::SeqCst), 1, "area_entered fires once");
        assert_eq!(exited.load(Ordering::SeqCst), 0);

        // Move B far away so the pair separates.
        let b_area = server.area_for_node(b_id).unwrap();
        server.area_store.get_area_mut(b_area).unwrap().position = Vector2::new(1000.0, 0.0);

        // Frame 2: no longer overlapping ⇒ area_exited fires on A.
        server.step_physics(0.0);
        server.process_area_overlap_signals(&mut tree);
        assert_eq!(
            exited.load(Ordering::SeqCst),
            1,
            "area_exited should fire once when the areas separate"
        );
    }
}
