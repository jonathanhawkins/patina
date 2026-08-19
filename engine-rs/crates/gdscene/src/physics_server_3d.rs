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
use gdcore::math3d::{Quaternion, Transform3D};
use gdphysics3d::area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, OverlapEvent3D, OverlapState3D, SpaceOverride,
};
use gdphysics3d::body::{BodyId3D, BodyType3D, FreezeMode, PhysicsBody3D};
use gdphysics3d::joint::{
    AxisLimit, ConeTwistJoint3D, Generic6DofJoint3D, HingeJoint3D, Joint3D, JointId3D, PinJoint3D,
    SliderJoint3D,
};
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
        // A ragdoll bone registers as Kinematic so it follows the animated bone
        // pose, and `start_ragdoll_simulation` flips it to Rigid — matching
        // Godot, where a PhysicalBone3D is inert until
        // `physical_bones_start_simulation()` hands it to the solver
        // (pat-0q2kx.203).
        "PhysicalBone3D" => Some(BodyType3D::Kinematic),
        // ⚠️ Kinematic, NOT Static, though Godot parents it to StaticBody3D:
        // an animated platform must be driven by the scene tree every tick
        // (pat-s3rrv.93). Mirrors `AnimatableBody2D` in `physics_server.rs`.
        "AnimatableBody3D" => Some(BodyType3D::Kinematic),
        _ => None,
    }
}

/// `true` for the five concrete `Joint3D` node classes (pat-0q2kx.118).
fn is_joint_class_3d(class_name: &str) -> bool {
    matches!(
        class_name,
        "PinJoint3D" | "HingeJoint3D" | "SliderJoint3D" | "ConeTwistJoint3D" | "Generic6DOFJoint3D"
    )
}

/// A joint's `node_a`/`node_b` as a path string, or `None` when unset
/// (pat-0q2kx.118). Godot writes these as `NodePath`; a hand-written scene may
/// spell them as a plain String, so both are accepted.
fn joint_node_path_3d(node: &crate::node::Node, key: &str) -> Option<String> {
    let path = match node.get_property(key) {
        Variant::NodePath(p) => p.to_string(),
        Variant::String(s) => s,
        _ => return None,
    };
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Resolves a joint's `node_a`/`node_b` path, which Godot treats as relative to
/// the joint node itself; an absolute path is accepted too.
fn resolve_joint_target_3d(tree: &SceneTree, from: NodeId, path: &str) -> Option<NodeId> {
    if path.starts_with('/') {
        tree.get_node_by_path(path)
    } else {
        tree.get_node_relative(from, path)
    }
}

/// The joint node's own frame, re-expressed in one body's local space.
///
/// Godot configures every 3D joint from `body.global_transform.affine_inverse()
/// * joint.global_transform` (`scene/3d/physics/joints/*.cpp`), so the pivot and
/// the constraint axes all come from where the joint node sits, not from the
/// bodies. Only the two axes the solver actually reads are kept: the hinge runs
/// along the frame's **Z** column and the slider/cone-twist along **X**
/// (`modules/godot_physics_3d/joints/godot_{hinge,slider,cone_twist}_joint_3d.cpp`).
struct JointFrame3D {
    /// The joint's origin in the body's local space.
    pivot: Vector3,
    /// The joint frame's local X column, in the body's local space.
    x: Vector3,
    /// The joint frame's local Z column, in the body's local space.
    z: Vector3,
}

/// Projects `joint` into `body_node`'s local space.
///
/// Uses the same position/rotation pair that body registration writes into the
/// world — global origin, local rotation — so the frame the solver sees agrees
/// with the frame the body is simulated in.
fn joint_frame_in_body(tree: &SceneTree, body_node: NodeId, joint: &Transform3D) -> JointFrame3D {
    let body_position = node3d::get_global_transform(tree, body_node).origin;
    let inverse = Quaternion::from_euler(node3d::get_rotation(tree, body_node)).inverse();
    JointFrame3D {
        pivot: inverse.xform(joint.origin - body_position),
        x: inverse.xform(joint.basis.x).normalized(),
        z: inverse.xform(joint.basis.z).normalized(),
    }
}

// ---------------------------------------------------------------------------
// Ragdoll helpers (pat-0q2kx.203)
// ---------------------------------------------------------------------------

/// Marks a `PhysicalBoneSimulator3D` as actively simulating.
///
/// Godot keeps this as internal simulator state; carrying it as a node property
/// keeps it observable from a test and from the inspector without a parallel
/// side table that could drift out of sync with the tree.
pub const RAGDOLL_SIMULATING_PROP: &str = "__ragdoll_simulating";

/// Walks up from a node to the first ancestor of `class_name`.
fn ancestor_of_class(tree: &SceneTree, from: NodeId, class_name: &str) -> Option<NodeId> {
    let mut current = tree.get_node(from)?.parent();
    while let Some(id) = current {
        let node = tree.get_node(id)?;
        if node.class_name() == class_name {
            return Some(id);
        }
        current = node.parent();
    }
    None
}

/// The `PhysicalBoneSimulator3D` governing a `PhysicalBone3D`.
fn simulator_for_bone(tree: &SceneTree, bone: NodeId) -> Option<NodeId> {
    ancestor_of_class(tree, bone, "PhysicalBoneSimulator3D")
}

/// Every `PhysicalBone3D` governed by `simulator`.
fn ragdoll_bones_under(tree: &SceneTree, simulator: NodeId) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|id| {
            tree.get_node(*id)
                .is_some_and(|n| n.class_name() == "PhysicalBone3D")
                && simulator_for_bone(tree, *id) == Some(simulator)
        })
        .collect()
}

/// Resolves a `PhysicalBone3D` to the `(skeleton, bone_index)` it drives.
///
/// Godot binds by **name** (`bone_name`), resolving to an index at runtime —
/// so a `.tscn` that reorders bones still binds correctly.
fn bone_binding(tree: &SceneTree, bone_node: NodeId) -> Option<(NodeId, usize)> {
    let simulator = simulator_for_bone(tree, bone_node)?;
    let skeleton = ancestor_of_class(tree, simulator, "Skeleton3D")?;
    let name = match tree.get_node(bone_node)?.get_property("bone_name") {
        Variant::String(s) if !s.is_empty() => s,
        _ => return None,
    };
    let idx = crate::skeleton3d::find_bone(tree, skeleton, &name)?;
    Some((skeleton, idx))
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
/// Reads a float property, distinguishing "absent" from a legitimate `0.0`.
///
/// The `__physics_material__*` keys need this: `bounce` is very often authored
/// as `0.0`, and a `float_prop_3d(.., 0.0)` fallback could not tell that apart
/// from "no override present", so the flat body property could never win.
fn opt_float_prop_3d(tree: &SceneTree, id: NodeId, key: &str) -> Option<f64> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Float(f) => Some(f),
        Variant::Int(i) => Some(i as f64),
        _ => None,
    }
}

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

/// Reads a packed float array (or an `Array` of numbers) from a node, used for
/// `HeightMapShape3D.map_data` (pat-0q2kx.119). Returns an empty vec when the
/// property is absent or not numeric.
fn float_array_prop_3d(node: &crate::node::Node, key: &str) -> Vec<f32> {
    match node.get_property(key) {
        Variant::PackedFloat32Array(v) => v,
        Variant::PackedFloat64Array(v) => v.into_iter().map(|f| f as f32).collect(),
        Variant::PackedInt32Array(v) => v.into_iter().map(|i| i as f32).collect(),
        Variant::Array(items) => items
            .into_iter()
            .filter_map(|v| match v {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
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

    // A real `.tscn` writes `shape = SubResource("id")`, which the parser leaves
    // as `"SubResource:<id>"`; the geometry lives on the sub-resource and is
    // copied onto this node under `__shape__*` at instancing (packed_scene.rs).
    // Hand-built nodes instead put the class name in `shape` with the dimensions
    // directly on the node. Both are accepted — same resolution the 2D server
    // already does in `physics_server::shape_from_node`. Without this a
    // `.tscn`-authored CollisionShape3D resolved to nothing and its body silently
    // fell back to a default sphere.
    let shape_prop = node.get_property("shape");
    let resolved_class: Option<String> = match &shape_prop {
        Variant::String(s) if s.starts_with("SubResource:") => {
            match node.get_property("__shape__class") {
                Variant::String(class) => Some(class),
                // An unresolved reference has no geometry — better no shape than
                // a default-sized one standing in for the authored size.
                _ => None,
            }
        }
        Variant::String(s) => Some(s.clone()),
        _ => None,
    };

    // Dimension readers that prefer the sub-resource's copied value.
    let has = |key: &str| !matches!(node.get_property(key), Variant::Nil);
    let num = |key: &str, default: f64| -> f32 {
        let ns = format!("__shape__{key}");
        if has(&ns) {
            float_prop_3d(tree, node_id, &ns, default) as f32
        } else {
            float_prop_3d(tree, node_id, key, default) as f32
        }
    };
    let v3 = |key: &str, default: Vector3| -> Vector3 {
        let ns = format!("__shape__{key}");
        let read = |k: &str| match node.get_property(k) {
            Variant::Vector3(v) => Some(v),
            _ => None,
        };
        read(&ns).or_else(|| read(key)).unwrap_or(default)
    };
    let v3s = |key: &str| -> Vec<Vector3> {
        let namespaced = vec3_array_prop_3d(node, &format!("__shape__{key}"));
        if namespaced.is_empty() {
            vec3_array_prop_3d(node, key)
        } else {
            namespaced
        }
    };
    let f32s = |key: &str| -> Vec<f32> {
        let namespaced = float_array_prop_3d(node, &format!("__shape__{key}"));
        if namespaced.is_empty() {
            float_array_prop_3d(node, key)
        } else {
            namespaced
        }
    };

    match resolved_class.as_deref() {
        Some("SphereShape3D") => Some(Shape3D::Sphere {
            radius: num("radius", 0.5),
        }),
        Some("BoxShape3D") => {
            let size = v3("size", Vector3::new(1.0, 1.0, 1.0));
            Some(Shape3D::BoxShape {
                half_extents: Vector3::new(size.x / 2.0, size.y / 2.0, size.z / 2.0),
            })
        }
        Some("CapsuleShape3D") => Some(Shape3D::CapsuleShape {
            radius: num("radius", 0.5),
            height: num("height", 1.0),
        }),
        Some("CylinderShape3D") => Some(Shape3D::CylinderShape {
            radius: num("radius", 0.5),
            height: num("height", 1.0),
        }),
        Some("ConvexPolygonShape3D") => Some(Shape3D::ConvexPolygonShape {
            points: v3s("points"),
        }),
        Some("ConcavePolygonShape3D") => Some(Shape3D::ConcavePolygonShape {
            // Godot stores the triangle soup in `data` (every 3 vertices =
            // one triangle); accept `faces` as an alias.
            faces: {
                let data = v3s("data");
                if data.is_empty() {
                    v3s("faces")
                } else {
                    data
                }
            },
            // `ConcavePolygonShape3D.backface_collision` defaults to false.
            backface_collision: if has("__shape__backface_collision") {
                truthy_prop_3d(tree, node_id, "__shape__backface_collision", false)
            } else {
                truthy_prop_3d(tree, node_id, "backface_collision", false)
            },
        }),
        // pat-0q2kx.119 — heightmap terrain and separation rays.
        Some("HeightMapShape3D") => Some(Shape3D::HeightMapShape {
            map_width: num("map_width", 2.0) as i32,
            map_depth: num("map_depth", 2.0) as i32,
            map_data: f32s("map_data"),
        }),
        Some("SeparationRayShape3D") => Some(Shape3D::SeparationRayShape {
            length: num("length", 1.0),
            // `SeparationRayShape3D.slide_on_slope` defaults to false.
            slide_on_slope: if has("__shape__slide_on_slope") {
                truthy_prop_3d(tree, node_id, "__shape__slide_on_slope", false)
            } else {
                truthy_prop_3d(tree, node_id, "slide_on_slope", false)
            },
        }),
        Some("WorldBoundaryShape3D") => Some(Shape3D::WorldBoundaryShape {
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: 0.0,
        }),
        Some(_) => None,
        None => {
            // Fallback: check for "radius" (sphere) or "size" (box) properties.
            match node.get_property("radius") {
                Variant::Float(r) => Some(Shape3D::Sphere { radius: r as f32 }),
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
    /// Per-tick world-space motion of each `AnimatableBody3D` whose
    /// `sync_to_physics` is on — a DISPLACEMENT, converted to a velocity by
    /// [`step`](Self::step), which is the half of the pair that has `dt`
    /// (pat-s3rrv.93). The 3D twin of `PhysicsServer::animatable_motion`.
    animatable_motion: HashMap<BodyId3D, Vector3>,
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
    /// `Joint3D` scene nodes registered into the world (pat-0q2kx.118).
    node_to_joint: HashMap<NodeId, JointId3D>,
    /// Auto-created ragdoll joints, keyed by the `PhysicalBone3D` node they
    /// constrain (pat-0q2kx.203).
    ///
    /// Deliberately NOT stored in `node_to_joint`: that map's stale sweep in
    /// `sync_joints` drops every entry whose node is not one of the five
    /// authored `Joint3D` classes, which would delete these on the very next
    /// sync and let the ragdoll fall apart.
    ragdoll_joints: HashMap<NodeId, JointId3D>,
    /// Source of joint ids. `PhysicsWorld3D::add_joint` keeps whatever id the
    /// joint was built with, so the server is what has to hand out unique ones.
    next_joint_id: u64,
    gravity: Vector3,
}

impl PhysicsServer3D {
    /// Creates a new 3D physics server with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            animatable_motion: HashMap::new(),
            node_to_area: HashMap::new(),
            area_to_node: HashMap::new(),
            last_overlap_events: Vec::new(),
            last_area_overlap_events: Vec::new(),
            previous_contacts: HashSet::new(),
            last_contact_events: Vec::new(),
            node_to_joint: HashMap::new(),
            ragdoll_joints: HashMap::new(),
            next_joint_id: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }

    /// Creates a new 3D physics server with custom gravity.
    pub fn with_gravity(gravity: Vector3) -> Self {
        Self {
            world: PhysicsWorld3D::new(),
            node_to_body: HashMap::new(),
            animatable_motion: HashMap::new(),
            node_to_area: HashMap::new(),
            area_to_node: HashMap::new(),
            last_overlap_events: Vec::new(),
            last_area_overlap_events: Vec::new(),
            previous_contacts: HashSet::new(),
            last_contact_events: Vec::new(),
            node_to_joint: HashMap::new(),
            ragdoll_joints: HashMap::new(),
            next_joint_id: 1,
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
                        // `physics_material_override`, which `PackedScene`
                        // flattens onto the node as `__physics_material__*`
                        // (packed_scene.rs). 3D dropped these entirely: the
                        // loader wrote all four keys and nothing here ever read
                        // one, so a RigidBody3D's material was silently inert
                        // while the identical 2D path honoured it.
                        //
                        // `rough` / `absorbent` are Godot's combine modes, and
                        // it signals them by NEGATING the stored value —
                        // `computed_friction() { return rough ? -friction :
                        // friction; }` (`physics_material.h:42-67`). The 3D
                        // solver already reads that sign back out through the
                        // same rules as 2D: `abs(MIN(a, b))` for friction
                        // (`world.rs`, the `.min(..).abs()` pair) and
                        // `CLAMP(a + b, 0, 1)` for restitution. So the sign is
                        // the whole implementation — no branch is needed here
                        // or in the solver.
                        if let Some(f) =
                            opt_float_prop_3d(tree, nid, "__physics_material__friction")
                        {
                            body.friction = f as f32;
                        }
                        if let Some(b) = opt_float_prop_3d(tree, nid, "__physics_material__bounce")
                        {
                            body.bounce = b as f32;
                        }
                        if truthy_prop_3d(tree, nid, "__physics_material__rough", false) {
                            body.friction = -body.friction.abs();
                        }
                        if truthy_prop_3d(tree, nid, "__physics_material__absorbent", false) {
                            body.bounce = -body.bounce.abs();
                        }
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
                            // ⚠️ Absent `sync_to_physics` means ON (it defaults
                            // to true in Godot), and the displacement has to be
                            // taken BEFORE `b.position` is overwritten below.
                            // Recorded even when zero, so a platform that stops
                            // moving — or has the flag turned off — actually
                            // stops carrying instead of keeping last tick's
                            // velocity (pat-s3rrv.93).
                            let animatable = class == "AnimatableBody3D";
                            let synced = truthy_prop_3d(tree, nid, "sync_to_physics", true);
                            if let Some(b) = self.world.get_body_mut(body_id) {
                                if animatable {
                                    let delta = if synced {
                                        position - b.position
                                    } else {
                                        Vector3::ZERO
                                    };
                                    self.animatable_motion.insert(body_id, delta);
                                }
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

        // Joints last: they resolve `node_a`/`node_b` to `BodyId3D`s, so every
        // body has to be in `node_to_body` before this runs (pat-0q2kx.118).
        self.sync_joints(tree, &all_nodes);
    }

    /// Builds world joints from `Joint3D` scene nodes, and frees the ones whose
    /// node has left the tree (pat-0q2kx.118).
    ///
    /// `gdphysics3d::joint` carried the parameters for all five kinds and
    /// `world.add_joint` registered them, but every caller was one of that
    /// crate's own tests — no scene path ever created a joint, so an authored
    /// `PinJoint3D` constrained nothing.
    ///
    /// A joint naming a node that is not a registered body is skipped rather
    /// than guessed at: a half-connected joint would yank its one real body
    /// toward the origin.
    /// Godot's `PhysicalBoneSimulator3D.physical_bones_start_simulation()`
    /// (pat-0q2kx.203).
    ///
    /// Flips every `PhysicalBone3D` under `simulator` from Kinematic (following
    /// the animated pose) to Rigid, and auto-creates the joint tying each bone's
    /// body to its **parent bone's** body. Without those joints the bodies are
    /// independent boxes and the ragdoll scatters instead of hanging.
    ///
    /// Known limitation, reported with the bead: only `JOINT_TYPE_PIN` is
    /// modelled faithfully. Cone/hinge/slider/6DOF bones still receive a pin so
    /// the chain holds together, which constrains position but not the
    /// per-type angular limits.
    /// `only` restricts which bones are handed to the solver, matching Godot's
    /// optional `bones` argument; an empty slice simulates all of them. Bones
    /// left out stay Kinematic and act as anchors — which is how a ragdoll
    /// hangs from a fixed hip instead of falling as a loose pile. Joints are
    /// still created for excluded bones, or the chain would not be attached to
    /// its anchor at all.
    pub fn start_ragdoll_simulation(
        &mut self,
        tree: &mut SceneTree,
        simulator: NodeId,
        only: &[String],
    ) {
        let bones = ragdoll_bones_under(tree, simulator);

        // bone index -> the PhysicalBone3D node driving it, so a child bone can
        // find the body it must be jointed to.
        let mut by_bone_idx: HashMap<usize, NodeId> = HashMap::new();
        for &b in &bones {
            if let Some((_, idx)) = bone_binding(tree, b) {
                by_bone_idx.insert(idx, b);
            }
        }

        for &bone_node in &bones {
            let Some(body_id) = self.node_to_body.get(&bone_node).copied() else {
                continue;
            };
            let simulate_this = only.is_empty()
                || tree.get_node(bone_node).is_some_and(|n| {
                    matches!(n.get_property("bone_name"), Variant::String(ref s) if only.iter().any(|b| b == s))
                });
            if simulate_this {
                if let Some(body) = self.world.get_body_mut(body_id) {
                    body.body_type = BodyType3D::Rigid;
                }
            }

            // Joint this bone to the body of its parent bone, if that bone is
            // also simulated. A root bone simply has no joint.
            if self.ragdoll_joints.contains_key(&bone_node) {
                continue;
            }
            let Some((skeleton, idx)) = bone_binding(tree, bone_node) else {
                continue;
            };
            let parent_idx = crate::skeleton3d::get_bone_parent(tree, skeleton, idx);
            if parent_idx < 0 {
                continue;
            }
            let Some(&parent_node) = by_bone_idx.get(&(parent_idx as usize)) else {
                continue;
            };
            let Some(parent_body) = self.node_to_body.get(&parent_node).copied() else {
                continue;
            };

            // The joint sits at this bone's head — its global rest origin — so
            // the chain pivots where the bones actually meet.
            let joint_global = crate::skeleton3d::get_bone_global_pose(tree, skeleton, idx);
            let frame_child = joint_frame_in_body(tree, bone_node, &joint_global);
            let frame_parent = joint_frame_in_body(tree, parent_node, &joint_global);

            let id = JointId3D(self.next_joint_id);
            self.next_joint_id += 1;
            let joint = Joint3D::Pin(
                PinJoint3D::new(id, parent_body, body_id)
                    .with_pivots(frame_parent.pivot, frame_child.pivot),
            );
            self.world.add_joint(joint);
            self.ragdoll_joints.insert(bone_node, id);
        }

        if let Some(node) = tree.get_node_mut(simulator) {
            node.set_property(RAGDOLL_SIMULATING_PROP, Variant::Bool(true));
        }
    }

    /// Godot's `physical_bones_stop_simulation()`: hands the bones back to the
    /// animation system by returning them to Kinematic and releasing the
    /// auto-created joints.
    pub fn stop_ragdoll_simulation(&mut self, tree: &mut SceneTree, simulator: NodeId) {
        for bone_node in ragdoll_bones_under(tree, simulator) {
            if let Some(body_id) = self.node_to_body.get(&bone_node).copied() {
                if let Some(body) = self.world.get_body_mut(body_id) {
                    body.body_type = BodyType3D::Kinematic;
                    // Otherwise the stored momentum reappears the moment
                    // simulation is started again.
                    body.linear_velocity = Vector3::ZERO;
                    body.angular_velocity = Vector3::ZERO;
                }
            }
            if let Some(id) = self.ragdoll_joints.remove(&bone_node) {
                // `remove_joint` returns the removed joint; nothing needs it.
                let _ = self.world.remove_joint(id);
            }
        }

        if let Some(node) = tree.get_node_mut(simulator) {
            node.set_property(RAGDOLL_SIMULATING_PROP, Variant::Bool(false));
        }
    }

    /// Writes each simulated bone body's transform back onto its skeleton bone
    /// so the skinned mesh follows the ragdoll.
    ///
    /// Runs only for simulators that are actually simulating — otherwise this
    /// would overwrite animation output with the kinematic bodies' own poses
    /// every tick, freezing the character.
    pub fn write_back_ragdoll_poses(&self, tree: &mut SceneTree) {
        struct Writeback {
            skeleton: NodeId,
            bone_idx: usize,
            global: Transform3D,
        }
        let mut writebacks: Vec<Writeback> = Vec::new();

        for bone_node in tree.all_nodes_in_tree_order() {
            let is_bone = tree
                .get_node(bone_node)
                .is_some_and(|n| n.class_name() == "PhysicalBone3D");
            if !is_bone {
                continue;
            }
            let Some(simulator) = simulator_for_bone(tree, bone_node) else {
                continue;
            };
            let simulating = tree.get_node(simulator).is_some_and(|n| {
                matches!(n.get_property(RAGDOLL_SIMULATING_PROP), Variant::Bool(true))
            });
            if !simulating {
                continue;
            }
            let Some((skeleton, bone_idx)) = bone_binding(tree, bone_node) else {
                continue;
            };
            let Some(body_id) = self.node_to_body.get(&bone_node).copied() else {
                continue;
            };
            let Some(body) = self.world.get_body(body_id) else {
                continue;
            };

            writebacks.push(Writeback {
                skeleton,
                bone_idx,
                global: Transform3D {
                    basis: gdcore::math3d::Basis::from_quaternion(body.rotation),
                    origin: body.position,
                },
            });
        }

        // Applied parent-first: `set_bone_global_pose` solves against the
        // parent's CURRENT global pose, so writing a child before its parent
        // would solve against a stale parent and drift down the chain.
        writebacks.sort_by_key(|w| w.bone_idx);
        for w in writebacks {
            crate::skeleton3d::set_bone_global_pose(tree, w.skeleton, w.bone_idx, w.global);
        }
    }

    fn sync_joints(&mut self, tree: &SceneTree, all_nodes: &[NodeId]) {
        let mut current: HashSet<NodeId> = HashSet::new();
        for &nid in all_nodes {
            let is_joint = tree
                .get_node(nid)
                .is_some_and(|n| is_joint_class_3d(n.class_name()));
            if !is_joint {
                continue;
            }
            current.insert(nid);
            if self.node_to_joint.contains_key(&nid) {
                continue;
            }
            if let Some(joint) = self.build_joint(tree, nid) {
                let id = self.world.add_joint(joint);
                self.node_to_joint.insert(nid, id);
            }
        }

        // A joint node that left the tree must release its constraint, or the
        // bodies stay welded to something that is no longer in the scene.
        let stale: Vec<NodeId> = self
            .node_to_joint
            .keys()
            .filter(|nid| !current.contains(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(joint_id) = self.node_to_joint.remove(&nid) {
                self.world.remove_joint(joint_id);
            }
        }
    }

    /// Translates one `Joint3D` scene node into a world joint, or `None` when
    /// either end fails to resolve to a registered body.
    ///
    /// Every parameter is read back under the exact key a `.tscn` writes
    /// (`angular_limit/upper`, `params/bias`, `linear_limit_x/enabled`, …), with
    /// the defaults from `doc/classes/<Name>.xml`.
    ///
    /// Known simplification: the constraint axes are taken from the joint's
    /// frame as seen by **body A**. Godot stores a separate frame per body, so a
    /// scene that authors body B pre-rotated relative to the joint keeps that
    /// offset there and snaps it here.
    fn build_joint(&mut self, tree: &SceneTree, node_id: NodeId) -> Option<Joint3D> {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

        let node = tree.get_node(node_id)?;
        let class = node.class_name().to_owned();
        let path_a = joint_node_path_3d(node, "node_a")?;
        let path_b = joint_node_path_3d(node, "node_b")?;
        let target_a = resolve_joint_target_3d(tree, node_id, &path_a)?;
        let target_b = resolve_joint_target_3d(tree, node_id, &path_b)?;
        let body_a = self.node_to_body.get(&target_a).copied()?;
        let body_b = self.node_to_body.get(&target_b).copied()?;
        if body_a == body_b {
            return None;
        }

        let global = node3d::get_global_transform(tree, node_id);
        let frame_a = joint_frame_in_body(tree, target_a, &global);
        let frame_b = joint_frame_in_body(tree, target_b, &global);

        let id = JointId3D(self.next_joint_id);
        self.next_joint_id += 1;

        let mut joint = match class.as_str() {
            "PinJoint3D" => {
                let mut pin =
                    PinJoint3D::new(id, body_a, body_b).with_pivots(frame_a.pivot, frame_b.pivot);
                pin.bias = float_prop_3d(tree, node_id, "params/bias", 0.3) as f32;
                pin.damping = float_prop_3d(tree, node_id, "params/damping", 1.0) as f32;
                pin.impulse_clamp =
                    float_prop_3d(tree, node_id, "params/impulse_clamp", 0.0) as f32;
                Joint3D::Pin(pin)
            }
            "HingeJoint3D" => {
                let mut hinge = HingeJoint3D::new(id, body_a, body_b).with_axis(frame_a.z);
                hinge.anchor_a = frame_a.pivot;
                hinge.anchor_b = frame_b.pivot;
                hinge.use_limit = truthy_prop_3d(tree, node_id, "angular_limit/enable", false);
                hinge.lower_limit =
                    float_prop_3d(tree, node_id, "angular_limit/lower", -FRAC_PI_2 as f64) as f32;
                hinge.upper_limit =
                    float_prop_3d(tree, node_id, "angular_limit/upper", FRAC_PI_2 as f64) as f32;
                hinge.limit_bias = float_prop_3d(tree, node_id, "angular_limit/bias", 0.3) as f32;
                hinge.limit_softness =
                    float_prop_3d(tree, node_id, "angular_limit/softness", 0.9) as f32;
                hinge.limit_relaxation =
                    float_prop_3d(tree, node_id, "angular_limit/relaxation", 1.0) as f32;
                hinge.motor_enabled = truthy_prop_3d(tree, node_id, "motor/enable", false);
                hinge.motor_target_velocity =
                    float_prop_3d(tree, node_id, "motor/target_velocity", 1.0) as f32;
                hinge.motor_max_impulse =
                    float_prop_3d(tree, node_id, "motor/max_impulse", 1.0) as f32;
                Joint3D::Hinge(hinge)
            }
            "SliderJoint3D" => {
                let mut slider = SliderJoint3D::new(id, body_a, body_b)
                    .with_axis(frame_a.x)
                    .with_anchors(frame_a.pivot, frame_b.pivot);
                // Godot has no enable flag for the slider's limits — they are
                // always on, and an author widens them instead.
                slider.use_linear_limit = true;
                slider.linear_limit_lower =
                    float_prop_3d(tree, node_id, "linear_limit/lower_distance", -1.0) as f32;
                slider.linear_limit_upper =
                    float_prop_3d(tree, node_id, "linear_limit/upper_distance", 1.0) as f32;
                slider.linear_limit_softness =
                    float_prop_3d(tree, node_id, "linear_limit/softness", 1.0) as f32;
                slider.linear_limit_restitution =
                    float_prop_3d(tree, node_id, "linear_limit/restitution", 0.7) as f32;
                slider.linear_limit_damping =
                    float_prop_3d(tree, node_id, "linear_limit/damping", 1.0) as f32;
                slider.use_angular_limit = true;
                slider.angular_limit_lower =
                    float_prop_3d(tree, node_id, "angular_limit/lower_angle", 0.0) as f32;
                slider.angular_limit_upper =
                    float_prop_3d(tree, node_id, "angular_limit/upper_angle", 0.0) as f32;
                slider.angular_limit_softness =
                    float_prop_3d(tree, node_id, "angular_limit/softness", 1.0) as f32;
                slider.angular_limit_restitution =
                    float_prop_3d(tree, node_id, "angular_limit/restitution", 0.7) as f32;
                slider.angular_limit_damping =
                    float_prop_3d(tree, node_id, "angular_limit/damping", 0.0) as f32;
                Joint3D::Slider(slider)
            }
            "ConeTwistJoint3D" => {
                let mut cone = ConeTwistJoint3D::new(id, body_a, body_b)
                    .with_pivots(frame_a.pivot, frame_b.pivot)
                    .with_twist_axis(frame_a.x);
                cone.swing_span =
                    float_prop_3d(tree, node_id, "swing_span", FRAC_PI_4 as f64) as f32;
                cone.twist_span = float_prop_3d(tree, node_id, "twist_span", PI as f64) as f32;
                cone.bias = float_prop_3d(tree, node_id, "bias", 0.3) as f32;
                cone.softness = float_prop_3d(tree, node_id, "softness", 0.8) as f32;
                cone.relaxation = float_prop_3d(tree, node_id, "relaxation", 1.0) as f32;
                Joint3D::ConeTwist(cone)
            }
            _ => {
                // Generic6DOFJoint3D. Godot enables every axis limit by default
                // with a zero-width range, so an unconfigured 6-DOF joint is a
                // weld — matching that is the difference between "inert" and
                // "wrong".
                let mut dof = Generic6DofJoint3D::new(id, body_a, body_b)
                    .with_anchors(frame_a.pivot, frame_b.pivot);
                for (index, axis) in ["x", "y", "z"].iter().enumerate() {
                    dof.linear_limits[index] = AxisLimit {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_limit_{axis}/enabled"),
                            true,
                        ),
                        lower: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_limit_{axis}/lower_distance"),
                            0.0,
                        ) as f32,
                        upper: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_limit_{axis}/upper_distance"),
                            0.0,
                        ) as f32,
                    };
                    dof.angular_limits[index] = AxisLimit {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_limit_{axis}/enabled"),
                            true,
                        ),
                        lower: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_limit_{axis}/lower_angle"),
                            0.0,
                        ) as f32,
                        upper: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_limit_{axis}/upper_angle"),
                            0.0,
                        ) as f32,
                    };
                }
                Joint3D::Generic6Dof(dof)
            }
        };

        let base = joint.base_mut();
        base.exclude_nodes_from_collision =
            truthy_prop_3d(tree, node_id, "exclude_nodes_from_collision", true);
        base.solver_priority = float_prop_3d(tree, node_id, "solver_priority", 1.0) as i32;
        Some(joint)
    }

    /// Number of joints the scene tree has registered into the world.
    pub fn joint_count(&self) -> usize {
        self.world.joint_count()
    }

    /// Returns the world joint built for a `Joint3D` scene node, if any.
    pub fn joint_for_node(&self, node_id: NodeId) -> Option<JointId3D> {
        self.node_to_joint.get(&node_id).copied()
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
                // Point-gravity shape (pat-0q2kx.193). Defaults from Area3D.xml:
                // the centre offset is Vector3(0, -1, 0) and the unit distance
                // is 0.0, which means constant strength rather than falloff.
                area.gravity_point_center = vec3_prop_3d(
                    tree,
                    nid,
                    "gravity_point_center",
                    Vector3::new(0.0, -1.0, 0.0),
                );
                area.gravity_point_unit_distance =
                    float_prop_3d(tree, nid, "gravity_point_unit_distance", 0.0) as f32;
                // The two damp channels carry their OWN override modes
                // (pat-s3rrv.102). Godot resolves gravity, linear damp and
                // angular damp independently, so reading only
                // `gravity_space_override` above left an authored
                // `linear_damp_space_override = 0` (DISABLED) applying damping
                // anyway — the mode was never consulted at any layer.
                area.linear_damp_space_override = space_override_from_int_3d(u32_prop_3d(
                    tree,
                    nid,
                    "linear_damp_space_override",
                    0,
                ));
                area.angular_damp_space_override = space_override_from_int_3d(u32_prop_3d(
                    tree,
                    nid,
                    "angular_damp_space_override",
                    0,
                ));
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
        // An AnimatableBody3D's per-tick displacement becomes the body's
        // velocity for the step about to run (pat-s3rrv.93) — the value
        // `CharacterBody3D`'s carry reads off the floor body it stands on
        // (`gdphysics3d/src/character.rs` takes `platform_velocity` from that
        // body's `linear_velocity` and adds it straight into its motion).
        //
        // ⚠️ **`linear_velocity`, NOT `constant_linear_velocity`.** The latter
        // is the conveyor field — friction drag from a surface that does not
        // itself move — and no carry path reads it. Mirrors the 2D server.
        if dt > 0.0 {
            let motions: Vec<(BodyId3D, Vector3)> = self.animatable_motion.drain().collect();
            for (body_id, delta) in motions {
                if let Some(body) = self.world.get_body_mut(body_id) {
                    body.linear_velocity = delta / dt;
                }
            }
        }
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
            emit_shape_indexed(
                tree,
                area_node,
                match event.state {
                    OverlapState3D::Entered => "body_shape_entered",
                    OverlapState3D::Exited => "body_shape_exited",
                },
                event.body_id.0,
                Variant::ObjectId(body_node.object_id()),
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
            emit_shape_indexed(
                tree,
                area_node,
                match event.state {
                    OverlapState3D::Entered => "area_shape_entered",
                    OverlapState3D::Exited => "area_shape_exited",
                },
                event.other_area_id.0,
                Variant::ObjectId(other_node.object_id()),
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

    /// Publishes each `Area3D`'s current area-vs-area overlap set onto its node
    /// as `_overlapping_areas`, backing the script-facing
    /// `get_overlapping_areas()` / `has_overlapping_areas()` / `overlaps_area()`
    /// (pat-s3rrv.102).
    ///
    /// ⚠️ The area twin of [`publish_overlapping_bodies`](Self::publish_overlapping_bodies)
    /// and the 3D twin of `PhysicsServer::publish_overlapping_areas`. Only the
    /// 2D server published this property, yet `scripting.rs` answers all three
    /// script methods from it for areas of BOTH dimensions — so every `Area3D`
    /// reported an empty overlap set forever, while `area_entered` fired
    /// correctly. The signals and the query disagreed, which reads as a script
    /// bug rather than a missing publisher.
    ///
    /// Godot gates area-vs-area detection on the TARGET being `monitorable`,
    /// and that gate lives inside `detect_area_overlaps`, so it is already
    /// applied to the set read here.
    pub fn publish_overlapping_areas(&self, tree: &mut SceneTree) {
        for (&area_node, &area_id) in &self.node_to_area {
            let areas: Vec<Variant> = self
                .world
                .overlapping_areas(area_id)
                .into_iter()
                .filter_map(|aid| self.area_to_node.get(&aid).copied())
                .map(|nid| Variant::ObjectId(nid.object_id()))
                .collect();
            if let Some(node) = tree.get_node_mut(area_node) {
                node.set_property("_overlapping_areas", Variant::Array(areas));
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
            self.publish_overlapping_areas(tree);
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
            Some(Shape3D::ConcavePolygonShape { faces, .. }) => {
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

/// Emits one of Godot's four shape-indexed Area3D signals (pat-s3rrv.102).
///
/// Godot pairs every `body_entered` / `area_entered` with a `*_shape_entered`
/// carrying the collider's RID, the node, and the two shape indices:
/// `body_shape_entered(body_rid, body, body_shape_index, local_shape_index)`
/// (`doc/classes/Area3D.xml`). A script connecting only the shape-indexed form —
/// which is the one you need to tell WHICH collider of a multi-collider body
/// touched you — previously received nothing at all.
///
/// ⚠️ BOTH INDICES ARE 0, and that is CORRECT rather than a placeholder here:
/// 3D bodies and areas are single-shape in Patina today (`gdphysics3d`'s
/// `PhysicsBody3D`/`Area3D` carry no `extra_shapes`, unlike their 2D twins), so
/// shape 0 is the only shape there is. When 3D gains multi-shape support these
/// two arguments become real indices and this helper is where they arrive —
/// the signal's ARITY and argument order are already Godot's, so scripts
/// written against it keep working.
///
/// ⚠️ The RID argument is an `Int`, matching how the rest of the codebase models
/// RIDs from script (`rid_arg` in the interpreter reads them as integers);
/// Patina's Variant has no RID type.
fn emit_shape_indexed(
    tree: &mut SceneTree,
    area_node: NodeId,
    signal: &str,
    collider_rid: u64,
    collider_object: Variant,
) {
    tree.emit_signal(
        area_node,
        signal,
        &[
            Variant::Int(collider_rid as i64),
            collider_object,
            Variant::Int(0),
            Variant::Int(0),
        ],
    );
}
