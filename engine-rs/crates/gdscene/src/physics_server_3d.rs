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

use std::collections::{HashMap, HashSet};

use gdcore::math::Vector3;
use gdphysics3d::area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, OverlapEvent3D, OverlapState3D, SpaceOverride,
};
use gdphysics3d::body::{BodyId3D, BodyType3D, FreezeMode, PhysicsBody3D};
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

/// Reads a `Vector3` property only if the node actually carries one, so a caller
/// can tell "authored (or last published) value" from "never set".
fn vec3_opt_prop_3d(tree: &SceneTree, id: NodeId, key: &str) -> Option<Vector3> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector3(v)) => Some(v),
        _ => None,
    }
}

/// Reads a `Vector3` property from a 3D node, defaulting to `default`.
fn vec3_prop_3d(tree: &SceneTree, id: NodeId, key: &str, default: Vector3) -> Vector3 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Vector3(v) => v,
            _ => default,
        })
        .unwrap_or(default)
}

/// Maps a Godot `Area3D.SpaceOverride` enum integer to [`SpaceOverride`].
///
/// **Not** a cast: Godot's order is DISABLED/COMBINE/COMBINE_REPLACE/REPLACE/
/// REPLACE_COMBINE (0-4, `Area3D.xml` + `godot_extension_api.json`), while
/// [`SpaceOverride`]'s own discriminants put `Replace` at 2 and `CombineReplace`
/// at 3 — so `value as SpaceOverride` would silently turn an authored REPLACE
/// into COMBINE_REPLACE. The 2D server maps explicitly for the same reason.
fn space_override_from_int_3d(value: u32) -> SpaceOverride {
    match value {
        1 => SpaceOverride::Combine,
        2 => SpaceOverride::CombineReplace,
        3 => SpaceOverride::Replace,
        4 => SpaceOverride::ReplaceCombine,
        _ => SpaceOverride::Disabled,
    }
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
    /// `Area3D` nodes registered into the world, both directions (pat-0q2kx.35).
    node_to_area: HashMap<NodeId, AreaId3D>,
    area_to_node: HashMap<AreaId3D, NodeId>,
    /// Enter/exit events recorded by the last [`step`](Self::step), drained when
    /// they are turned into signals.
    last_overlap_events: Vec<OverlapEvent3D>,
    last_area_overlap_events: Vec<AreaOverlapEvent3D>,
    /// Body pairs in contact after the last step, so the next step can report
    /// which pairs are new and which are gone (pat-0q2kx.37). Each pair is stored
    /// with the lower id first, so one entry represents the pair once.
    previous_contacts: HashSet<(BodyId3D, BodyId3D)>,
    /// `(a, b, entered)` for this step, drained when turned into signals.
    last_contact_events: Vec<(BodyId3D, BodyId3D, bool)>,
    gravity: Vector3,
}

impl PhysicsServer3D {
    /// Creates a new 3D physics server with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            node_to_area: HashMap::new(),
            area_to_node: HashMap::new(),
            last_overlap_events: Vec::new(),
            last_area_overlap_events: Vec::new(),
            previous_contacts: HashSet::new(),
            last_contact_events: Vec::new(),
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }

    /// Creates a new 3D physics server with custom gravity.
    pub fn with_gravity(gravity: Vector3) -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            node_to_area: HashMap::new(),
            area_to_node: HashMap::new(),
            last_overlap_events: Vec::new(),
            last_area_overlap_events: Vec::new(),
            previous_contacts: HashSet::new(),
            last_contact_events: Vec::new(),
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
                    }

                    // Rigid bodies used to get nothing after registration, so every
                    // scripted force, velocity write and flag was a silent no-op
                    // (pat-0q2kx.37).
                    //
                    // This runs on the **registration tick too**, which is not
                    // optional: `sync_from_physics` publishes the body's own
                    // `sleeping` / velocities onto the node at the end of that same
                    // tick, so a flag the scene authored (`sleeping = true`) would
                    // otherwise be overwritten by the fresh body's default before it
                    // was ever adopted — the body fell for one tick and stayed awake
                    // forever after.
                    if body_type == BodyType3D::Rigid {
                        self.sync_rigid_body(tree, nid);
                    } else {
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

        self.sync_areas(tree, &all_nodes);
    }

    /// Pushes a registered `RigidBody3D` node's authored state into its world
    /// body, then drains the forces its script accumulated (pat-0q2kx.37).
    ///
    /// Ordering is load-bearing in two places:
    /// * flags first, forces second — applying an impulse wakes the body
    ///   (`PhysicsBody3D::apply_impulse`), so draining after writing
    ///   `sleeping` is what lets an impulse wake a body the script put to sleep,
    ///   rather than the flag silently undoing the impulse;
    /// * `linear_velocity` / `angular_velocity` are read back unconditionally,
    ///   which is safe only because [`sync_from_physics`](Self::sync_from_physics)
    ///   publishes the simulated values to the same properties — so an untouched
    ///   node hands back what physics just produced, while a script write in
    ///   between wins. Same contract as the 2D server.
    fn sync_rigid_body(&mut self, tree: &SceneTree, nid: NodeId) {
        let Some(&body_id) = self.node_to_body.get(&nid) else {
            return;
        };

        // All tree reads first: the world borrow below is mutable.
        let node_lin = vec3_opt_prop_3d(tree, nid, "linear_velocity");
        let node_ang = vec3_opt_prop_3d(tree, nid, "angular_velocity");
        let constant_force = vec3_prop_3d(tree, nid, "constant_force", Vector3::ZERO);
        let constant_torque = vec3_prop_3d(tree, nid, "constant_torque", Vector3::ZERO);
        let pending_impulse = vec3_prop_3d(tree, nid, "_pending_impulse", Vector3::ZERO);
        let pending_force = vec3_prop_3d(tree, nid, "_pending_force", Vector3::ZERO);
        let pending_torque = vec3_prop_3d(tree, nid, "_pending_torque", Vector3::ZERO);
        let pending_torque_impulse =
            vec3_prop_3d(tree, nid, "_pending_torque_impulse", Vector3::ZERO);
        // doc/classes/RigidBody3D.xml defaults.
        let gravity_scale = float_prop_3d(tree, nid, "gravity_scale", 1.0) as f32;
        let linear_damp = float_prop_3d(tree, nid, "linear_damp", 0.0) as f32;
        let angular_damp = float_prop_3d(tree, nid, "angular_damp", 0.0) as f32;
        let lock_rotation = truthy_prop_3d(tree, nid, "lock_rotation", false);
        let can_sleep = truthy_prop_3d(tree, nid, "can_sleep", true);
        let sleeping = truthy_prop_3d(tree, nid, "sleeping", false);
        let contact_monitor = truthy_prop_3d(tree, nid, "contact_monitor", false);
        let max_contacts_reported = u32_prop_3d(tree, nid, "max_contacts_reported", 0) as usize;
        // Godot splits this across a bool and a mode int (FREEZE_MODE_STATIC = 0,
        // FREEZE_MODE_KINEMATIC = 1); the library folds both into one enum whose
        // `None` variant has no Godot counterpart, so map rather than cast.
        let freeze_mode = if truthy_prop_3d(tree, nid, "freeze", false) {
            match u32_prop_3d(tree, nid, "freeze_mode", 0) {
                1 => FreezeMode::Kinematic,
                _ => FreezeMode::Static,
            }
        } else {
            FreezeMode::None
        };

        let Some(body) = self.world.get_body_mut(body_id) else {
            return;
        };
        if let Some(v) = node_lin {
            body.linear_velocity = v;
        }
        if let Some(v) = node_ang {
            body.angular_velocity = v;
        }
        body.constant_force = constant_force;
        body.constant_torque = constant_torque;
        body.gravity_scale = gravity_scale;
        body.linear_damp = linear_damp;
        body.angular_damp = angular_damp;
        body.lock_rotation = lock_rotation;
        body.can_sleep = can_sleep;
        body.sleeping = sleeping;
        body.freeze_mode = freeze_mode;
        body.contact_monitor = contact_monitor;
        body.max_contacts_reported = max_contacts_reported;

        if pending_impulse.length_squared() > 0.0 {
            body.apply_central_impulse(pending_impulse);
        }
        if pending_torque_impulse.length_squared() > 0.0 {
            body.apply_torque_impulse(pending_torque_impulse);
        }
        if pending_force.length_squared() > 0.0 {
            body.apply_central_force(pending_force);
        }
        if pending_torque.length_squared() > 0.0 {
            body.apply_torque(pending_torque);
        }
    }

    /// Zeroes the one-shot `_pending_*` force properties on every registered
    /// rigid body, after [`sync_rigid_body`](Self::sync_rigid_body) has drained
    /// them into the world.
    ///
    /// Separate pass because draining reads an immutable tree; Godot's
    /// `apply_impulse` is a one-frame event, so a script that stops applying
    /// force stops being pushed.
    fn clear_pending_forces(&self, tree: &mut SceneTree) {
        for (&nid, &body_id) in &self.node_to_body {
            let is_rigid = self
                .world
                .get_body(body_id)
                .map(|b| b.body_type == BodyType3D::Rigid)
                .unwrap_or(false);
            if !is_rigid {
                continue;
            }
            if let Some(node) = tree.get_node_mut(nid) {
                for key in [
                    "_pending_impulse",
                    "_pending_force",
                    "_pending_torque",
                    "_pending_torque_impulse",
                ] {
                    if !matches!(node.get_property(key), Variant::Nil) {
                        node.set_property(key, Variant::Vector3(Vector3::ZERO));
                    }
                }
            }
        }
    }

    /// Registers `Area3D` nodes into the world and refreshes the per-tick state
    /// of the ones already registered (pat-0q2kx.35).
    ///
    /// Mirrors the 2D server's Area2D handling: an area's shape comes from its
    /// `CollisionShape3D` child, its transform and authored properties are read
    /// back every tick so a moved or retuned area takes effect, and areas whose
    /// nodes left the tree are freed.
    fn sync_areas(&mut self, tree: &SceneTree, all_nodes: &[NodeId]) {
        let mut current_area_nodes: HashMap<NodeId, ()> = HashMap::new();

        for &nid in all_nodes {
            let is_area = tree
                .get_node(nid)
                .map(|n| n.class_name() == "Area3D")
                .unwrap_or(false);
            if !is_area {
                continue;
            }
            current_area_nodes.insert(nid, ());

            // World-space, like bodies: an Area3D under an offset parent must
            // monitor the region it actually occupies.
            let position = node3d::get_global_transform(tree, nid).origin;
            let shape = tree
                .get_node(nid)
                .map(|n| n.children().to_vec())
                .unwrap_or_default()
                .iter()
                .find_map(|&child_id| shape_from_node_3d(tree, child_id))
                .unwrap_or(Shape3D::Sphere { radius: 0.5 });

            // `.copied()` ends the map borrow before the arms touch `self` again —
            // the `None` arm inserts into the same map.
            let existing = self.node_to_area.get(&nid).copied();
            let area_id = match existing {
                Some(id) => {
                    // Re-seed the shape too: a CollisionShape3D child resized or
                    // added after the first tick would otherwise be ignored.
                    if let Some(area) = self.world.get_area_mut(id) {
                        area.position = position;
                        area.shape = shape;
                    }
                    id
                }
                None => {
                    let id = self
                        .world
                        .add_area(Area3D::new(AreaId3D(0), position, shape));
                    self.node_to_area.insert(nid, id);
                    self.area_to_node.insert(id, nid);
                    id
                }
            };

            // Authored properties, defaults from doc/classes/Area3D.xml.
            if let Some(area) = self.world.get_area_mut(area_id) {
                area.collision_layer = u32_prop_3d(tree, nid, "collision_layer", 1);
                area.collision_mask = u32_prop_3d(tree, nid, "collision_mask", 1);
                area.monitoring = truthy_prop_3d(tree, nid, "monitoring", true);
                area.monitorable = truthy_prop_3d(tree, nid, "monitorable", true);
                // Godot types `priority` as int; the library stores it as f32.
                area.priority = float_prop_3d(tree, nid, "priority", 0.0) as f32;
                area.gravity_space_override =
                    space_override_from_int_3d(u32_prop_3d(tree, nid, "gravity_space_override", 0));
                area.gravity = float_prop_3d(tree, nid, "gravity", 9.8) as f32;
                area.gravity_direction =
                    vec3_prop_3d(tree, nid, "gravity_direction", Vector3::new(0.0, -1.0, 0.0));
                area.gravity_point = truthy_prop_3d(tree, nid, "gravity_point", false);
                area.linear_damp = float_prop_3d(tree, nid, "linear_damp", 0.1) as f32;
                area.angular_damp = float_prop_3d(tree, nid, "angular_damp", 0.1) as f32;
            }
        }

        let stale: Vec<NodeId> = self
            .node_to_area
            .keys()
            .filter(|nid| !current_area_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(area_id) = self.node_to_area.remove(&nid) {
                self.area_to_node.remove(&area_id);
                self.world.remove_area(area_id);
            }
        }
    }

    /// Steps the 3D physics simulation by `dt` seconds, then records this step's
    /// area enter/exit events for [`process_overlap_signals`](Self::process_overlap_signals).
    pub fn step(&mut self, dt: f32) {
        self.world.step(dt);
        self.detect_body_contacts();
        if self.world.area_count() > 0 {
            let (body_events, area_events) = self.world.detect_overlaps();
            self.last_overlap_events = body_events;
            self.last_area_overlap_events = area_events;
        }
    }

    /// Diffs this step's contact pairs against the previous step's to produce
    /// `body_entered` / `body_exited` events (pat-0q2kx.37).
    ///
    /// The world records contacts on the bodies themselves during the step, and
    /// `PhysicsBody3D::add_contact` already honours Godot's gating — a body only
    /// collects contacts when its own `contact_monitor` is set and it is under its
    /// `max_contacts_reported` budget — so reading them back needs no extra
    /// filtering. A body with monitoring off contributes nothing, which is also
    /// why the pair is recorded from whichever side saw it.
    fn detect_body_contacts(&mut self) {
        let mut current: HashSet<(BodyId3D, BodyId3D)> = HashSet::new();
        for body in self.world.bodies() {
            for other in body.get_colliding_bodies() {
                // Order the pair so the same contact is one entry regardless of
                // which body reported it.
                let pair = if body.id <= other {
                    (body.id, other)
                } else {
                    (other, body.id)
                };
                current.insert(pair);
            }
        }

        for &pair in current.difference(&self.previous_contacts) {
            self.last_contact_events.push((pair.0, pair.1, true));
        }
        for &pair in self.previous_contacts.difference(&current) {
            self.last_contact_events.push((pair.0, pair.1, false));
        }
        self.previous_contacts = current;
    }

    /// Emits `body_entered` / `body_exited` on `RigidBody3D` nodes whose
    /// `contact_monitor` is on, passing the other body's node `ObjectId`.
    ///
    /// Both directions are evaluated because either body in a pair may be the
    /// monitoring one, and Godot gates on `contact_monitor` **and** a non-zero
    /// `max_contacts_reported` — mirroring the 2D server. Draining, for the same
    /// reason as the area signals.
    pub fn process_body_contact_signals(&mut self, tree: &mut SceneTree) {
        for (a, b, entered) in std::mem::take(&mut self.last_contact_events) {
            let signal = if entered {
                "body_entered"
            } else {
                "body_exited"
            };
            for (self_body, other_body) in [(a, b), (b, a)] {
                let Some(self_node) = self.node_for_body(self_body) else {
                    continue;
                };
                let Some(other_node) = self.node_for_body(other_body) else {
                    continue;
                };
                if !truthy_prop_3d(tree, self_node, "contact_monitor", false) {
                    continue;
                }
                if u32_prop_3d(tree, self_node, "max_contacts_reported", 0) == 0 {
                    continue;
                }
                tree.emit_signal(
                    self_node,
                    signal,
                    &[Variant::ObjectId(other_node.object_id())],
                );
            }
        }
    }

    /// Number of registered `Area3D` nodes.
    pub fn area_count(&self) -> usize {
        self.node_to_area.len()
    }

    /// The world area registered for an `Area3D` node, if any.
    pub fn area_for_node(&self, node_id: NodeId) -> Option<AreaId3D> {
        self.node_to_area.get(&node_id).copied()
    }

    /// The nodes of the bodies currently overlapping an `Area3D` node, in the
    /// world's body order — Godot's `Area3D.get_overlapping_bodies()`.
    ///
    /// A one-shot query against the current world state, so it answers correctly
    /// on the same tick a body enters (unlike the enter/exit events, which are
    /// differences between ticks).
    pub fn overlapping_body_nodes(&self, area_node: NodeId) -> Vec<NodeId> {
        let Some(area_id) = self.node_to_area.get(&area_node).copied() else {
            return Vec::new();
        };
        let Some(area) = self.world.get_area(area_id) else {
            return Vec::new();
        };
        area.get_overlapping_bodies(self.world.bodies())
            .into_iter()
            .filter_map(|body_id| self.node_for_body(body_id))
            .collect()
    }

    /// Emits `body_entered` / `body_exited` on the monitoring `Area3D` node with
    /// the other node's `ObjectId`, for the events recorded by the last
    /// [`step`](Self::step) (pat-0q2kx.35).
    ///
    /// Draining, unlike the 2D twin: the frame flow calls this from
    /// [`sync_from_physics`](Self::sync_from_physics), so clearing the events is
    /// what keeps an additional explicit call from double-firing the signals.
    pub fn process_overlap_signals(&mut self, tree: &mut SceneTree) {
        for event in std::mem::take(&mut self.last_overlap_events) {
            let Some(&area_node) = self.area_to_node.get(&event.area_id) else {
                continue;
            };
            let Some(body_node) = self.node_for_body(event.body_id) else {
                continue;
            };
            let signal = match event.state {
                OverlapState3D::Entered => "body_entered",
                OverlapState3D::Exited => "body_exited",
            };
            tree.emit_signal(
                area_node,
                signal,
                &[Variant::ObjectId(body_node.object_id())],
            );
        }
    }

    /// Emits `area_entered` / `area_exited` on the monitoring `Area3D` node with
    /// the *other* area's `ObjectId`. Directional, so a mutually overlapping pair
    /// emits on both nodes, matching Godot.
    pub fn process_area_overlap_signals(&mut self, tree: &mut SceneTree) {
        for event in std::mem::take(&mut self.last_area_overlap_events) {
            let Some(&area_node) = self.area_to_node.get(&event.area_id) else {
                continue;
            };
            let Some(&other_node) = self.area_to_node.get(&event.other_area_id) else {
                continue;
            };
            let signal = match event.state {
                OverlapState3D::Entered => "area_entered",
                OverlapState3D::Exited => "area_exited",
            };
            tree.emit_signal(
                area_node,
                signal,
                &[Variant::ObjectId(other_node.object_id())],
            );
        }
    }

    /// Publishes each `Area3D` node's current overlap set onto the node as
    /// `_overlapping_bodies`, which is what the script-facing
    /// `get_overlapping_bodies()` / `has_overlapping_bodies()` read.
    ///
    /// Same bridge shape as RayCast2D's `_hit_normal`: the script phase has no
    /// handle on the physics server, so the physics phase leaves the answer on
    /// the node (pat-0q2kx.35).
    pub fn publish_overlapping_bodies(&self, tree: &mut SceneTree) {
        for &area_node in self.node_to_area.keys() {
            let bodies: Vec<Variant> = self
                .overlapping_body_nodes(area_node)
                .into_iter()
                .map(|nid| Variant::ObjectId(nid.object_id()))
                .collect();
            if let Some(node) = tree.get_node_mut(area_node) {
                node.set_property("_overlapping_bodies", Variant::Array(bodies));
            }
        }
    }

    /// Syncs updated physics transforms back into the scene tree, then flushes
    /// this step's `Area3D` overlap results.
    ///
    /// Only rigid bodies have their positions updated (static/kinematic
    /// bodies are driven by the scene tree, not by physics).
    ///
    /// The area work rides along here (pat-0q2kx.35) because this is the last 3D
    /// physics call the frame loop makes with a `&mut SceneTree` in hand, so
    /// `body_entered` reaches a connected script on the same tick it happened.
    /// The three steps are public individually, and signal emission drains its
    /// events, so a frame loop that later calls them explicitly stays correct
    /// rather than firing everything twice.
    pub fn sync_from_physics(&mut self, tree: &mut SceneTree) {
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
        // Publish the simulated rigid-body state so a script can read it, and so
        // the next tick's `sync_rigid_body` hands back what physics produced
        // unless a script overwrote it in between (pat-0q2kx.37).
        for (&nid, &body_id) in &self.node_to_body {
            let Some(body) = self.world.get_body(body_id) else {
                continue;
            };
            if body.body_type != BodyType3D::Rigid {
                continue;
            }
            let (lin, ang, sleeping) = (body.linear_velocity, body.angular_velocity, body.sleeping);
            if let Some(node) = tree.get_node_mut(nid) {
                node.set_property("linear_velocity", Variant::Vector3(lin));
                node.set_property("angular_velocity", Variant::Vector3(ang));
                node.set_property("sleeping", Variant::Bool(sleeping));
            }
        }
        self.clear_pending_forces(tree);
        self.process_body_contact_signals(tree);

        if !self.node_to_area.is_empty() {
            self.process_overlap_signals(tree);
            self.process_area_overlap_signals(tree);
            self.publish_overlapping_bodies(tree);
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
