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

use gdcore::math::{Vector2, Vector3};
use gdcore::math3d::{Plane, Quaternion, Transform3D};
use gdphysics3d::area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, OverlapEvent3D, OverlapState3D, SpaceOverride,
};
use gdphysics3d::body::{BodyId3D, BodyType3D, FreezeMode, PhysicsBody3D};
use gdphysics3d::joint::{
    AxisLimit, AxisMotor, AxisSpring, ConeTwistJoint3D, Generic6DofJoint3D, HingeJoint3D, Joint3D,
    JointId3D, PinJoint3D, SliderJoint3D,
};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::softbody3d::{SoftBody3D, SoftBodyId3D, DEFAULT_WELD_EPSILON};
use gdphysics3d::vehicle3d::{VehicleController3D, VehicleWheel3D};
use gdphysics3d::world::PhysicsWorld3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;

/// Maps scene node class names to 3D physics body types.
fn body_type_for_class_3d(class_name: &str) -> Option<BodyType3D> {
    match class_name {
        "RigidBody3D" => Some(BodyType3D::Rigid),
        // A VehicleBody3D IS a RigidBody3D in Godot (`VehicleBody3D.xml`); the
        // wheels only add forces on top (pat-s3rrv.194). Registering it here is
        // what gives the chassis its shape, mass and contact handling for free —
        // without this arm a `.tscn` vehicle registered as nothing at all.
        "VehicleBody3D" => Some(BodyType3D::Rigid),
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

/// Authored vertex indices pinned on a `SoftBody3D`.
///
/// Godot writes `_pinned_points` as a packed int array; a hand-built node may
/// use a plain array instead, so both are read. Negative entries are dropped
/// rather than wrapping into a huge `usize`.
fn pinned_point_indices_3d(tree: &SceneTree, id: NodeId) -> Vec<usize> {
    let Some(node) = tree.get_node(id) else {
        return Vec::new();
    };
    match node.get_property("_pinned_points") {
        Variant::PackedInt32Array(v) => {
            v.iter().filter(|&&i| i >= 0).map(|&i| i as usize).collect()
        }
        Variant::Array(v) => v
            .to_vec()
            .iter()
            .filter_map(|e| match e {
                Variant::Int(i) if *i >= 0 => Some(*i as usize),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
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
/// A rigid body's moments of inertia where Godot's differ from the
/// shape-centred ones Patina derives by default (pat-qg3nu).
///
/// - `center_of_mass_mode = CUSTOM` (1) with `center_of_mass` at the origin:
///   the body turns about its ORIGIN, which is also Patina's pivot, so the
///   shape's inertia moves there by the parallel-axis theorem (Jolt's
///   `OffsetCenterOfMassShape`). truck_town's cars are authored this way.
/// - an authored non-zero `inertia` axis replaces that axis
///   (`JoltBody3D::_calculate_mass_properties`).
///
/// `None` keeps the shape's own inertia: an AUTO centre of mass, or a custom
/// one away from the origin (Patina has no separate pivot to put it at).
fn authored_inertia_3d(tree: &SceneTree, nid: NodeId, body: &PhysicsBody3D) -> Option<Vector3> {
    let custom_com_at_origin = u32_prop_3d(tree, nid, "center_of_mass_mode", 0) == 1
        && vec3_prop_3d(tree, nid, "center_of_mass", Vector3::ZERO) == Vector3::ZERO;
    let authored = vec3_prop_3d(tree, nid, "inertia", Vector3::ZERO);
    if !custom_com_at_origin && authored == Vector3::ZERO {
        return None;
    }
    let mut i = if custom_com_at_origin {
        body.shape_inertia_about_origin()
    } else {
        body.shape_inertia()
    };
    if authored.x > 0.0 {
        i.x = authored.x;
    }
    if authored.y > 0.0 {
        i.y = authored.y;
    }
    if authored.z > 0.0 {
        i.z = authored.z;
    }
    Some(i)
}

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
            .to_vec()
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
            .to_vec()
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

/// Builds an extruded prism from a `CollisionPolygon3D`'s inline outline
/// (pat-s3rrv.189).
///
/// The prism geometry — AABB and support mapping — already existed in
/// `gdphysics2d::shape3d`, fully unit-tested, and nothing in the engine
/// produced one: `shape_from_node_3d` rejected every class but
/// `CollisionShape3D`, so a body whose only collider was a polygon registered
/// NO geometry at all.
///
/// 🔑 `depth` extrudes CENTRED on the node's Z: Godot emits each outline point
/// twice, at `+depth * 0.5` and `-depth * 0.5`
/// (`collision_polygon_3d.cpp:66-67`). Extruding 0..depth instead would sink
/// the prism half its thickness below where the author drew it.
///
/// ⚠️ TWO DIFFERENT `Shape3D` ENUMS EXIST. `gdphysics2d::shape3d::Shape3D` has
/// a `CollisionPolygon3D` variant with its own AABB and support mapping — and
/// it is NOT the enum this server uses. `physics_server_3d` builds
/// `gdphysics3d::shape::Shape3D`, which has no such variant. The prism is
/// therefore expressed as a `ConvexPolygonShape` of the extruded corner points,
/// which is both what Godot itself produces and what the 3D solver already
/// understands — adding a variant to an enum with ~110 match sites would be a
/// lane-wide outage for no gain.
fn polygon_shape_from_node_3d(tree: &SceneTree, node_id: NodeId) -> Option<Shape3D> {
    let node = tree.get_node(node_id)?;
    let vertices: Vec<Vector2> = match node.get_property("polygon") {
        Variant::PackedVector2Array(v) => v,
        _ => Vec::new(),
    };
    // Godot contributes no collision for a degenerate outline, and a default
    // stand-in prism here would be worse than none — the body would collide
    // with something the author never drew.
    if vertices.len() < 3 {
        return None;
    }
    // `depth` defaults to 1.0 (extension_api.json), NOT 0: a missing property
    // must not collapse the prism to a zero-thickness sheet.
    let depth = match node.get_property("depth") {
        Variant::Float(d) => d as f32,
        Variant::Int(d) => d as f32,
        _ => 1.0,
    };
    // Each outline point becomes two vertices, one per cap.
    let half_depth = depth * 0.5;
    let mut points = Vec::with_capacity(vertices.len() * 2);
    for v in &vertices {
        points.push(Vector3::new(v.x, v.y, half_depth));
        points.push(Vector3::new(v.x, v.y, -half_depth));
    }
    Some(Shape3D::ConvexPolygonShape { points })
}

/// Extracts a 3D collision shape from a `CollisionShape3D` or
/// `CollisionPolygon3D` node's properties.
pub(crate) fn shape_from_node_3d(tree: &SceneTree, node_id: NodeId) -> Option<Shape3D> {
    let node = tree.get_node(node_id)?;
    let is_polygon = node.class_name() == "CollisionPolygon3D";
    if !is_polygon && node.class_name() != "CollisionShape3D" {
        return None;
    }

    // If the shape is disabled, skip it. Checked before the class split so a
    // disabled polygon is skipped the same way a disabled shape is.
    if let Variant::Bool(true) = node.get_property("disabled") {
        return None;
    }

    // A CollisionPolygon3D carries its geometry inline as a `polygon`
    // PackedVector2Array rather than referencing a Shape3D sub-resource, so it
    // has no path through the sub-resource resolution below — exactly the split
    // the 2D server already makes for CollisionPolygon2D
    // (`physics_server::shape_from_node`).
    if is_polygon {
        return polygon_shape_from_node_3d(tree, node_id);
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
    // Same namespaced-first shape as `v3`: a sub-resource's copied value
    // (`__shape__plane`) wins over a property authored on the node itself.
    let plane = |key: &str| -> Option<Plane> {
        let ns = format!("__shape__{key}");
        let read = |k: &str| match node.get_property(k) {
            Variant::Plane(p) => Some(p),
            _ => None,
        };
        read(&ns).or_else(|| read(key))
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
        // 🔑 `height` defaults to 2.0, not 1.0 (`doc/classes/CapsuleShape3D.xml`
        // and `CylinderShape3D.xml`), and `gdphysics3d` reads it as the TOTAL
        // height (`shape.rs`: "defined by radius and total height", and its AABB
        // spans -height/2..+height/2), so Godot's number maps straight across.
        //
        // ⚠️ This is not a cosmetic default. A `.tscn` OMITS a property that
        // still holds its default, so an unedited `CapsuleShape3D` — the stock
        // CharacterBody3D collider — authors no `height` line at all and landed
        // here as a 1 m capsule: with radius 0.5 that is exactly a sphere. The
        // character then collided as a ball while rendering as a 2 m capsule,
        // and the gizmo path (`packed_scene.rs`, `unwrap_or(2.0)`) already used
        // Godot's value, so the two layers disagreed about the same node
        // (pat-mgixk.39).
        Some("CapsuleShape3D") => Some(Shape3D::CapsuleShape {
            radius: num("radius", 0.5),
            height: num("height", 2.0),
        }),
        Some("CylinderShape3D") => Some(Shape3D::CylinderShape {
            radius: num("radius", 0.5),
            height: num("height", 2.0),
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
        // 🔑 `WorldBoundaryShape3D.plane` is a real property (default
        // `Plane(0, 1, 0, 0)`), and this arm used to hardcode that default and
        // ignore the authored value entirely — so every world boundary was the
        // ground at y=0 however the scene tilted or raised it (pat-mgixk.39).
        //
        // Godot stores the normal and the signed distance in one `Plane`, which
        // `gdresource`'s `.tscn` reader already turns into `Variant::Plane`
        // (`loader.rs`, "Plane(a, b, c, d) — normal (a, b, c) and signed
        // distance d"), so the authored line arrives here typed. `normal` and
        // `distance` are accepted as a fallback because they are the shape's
        // OWN field names, which is what a runtime setter writes.
        Some("WorldBoundaryShape3D") => {
            let default_normal = Vector3::new(0.0, 1.0, 0.0);
            let (normal, distance) = match plane("plane") {
                Some(p) => (p.normal, p.d),
                None => (v3("normal", default_normal), num("distance", 0.0)),
            };
            // ⚠️ A zero normal would make every contact test degenerate (the
            // half-space has no orientation), so it falls back to Godot's +Y
            // rather than being stored. `normalized()` because the contact
            // maths treats the normal as a unit vector; Godot normalizes a
            // `Plane` on construction too.
            let normal = if normal.length() > 1e-6 {
                normal.normalized()
            } else {
                default_normal
            };
            Some(Shape3D::WorldBoundaryShape { normal, distance })
        }
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

/// Hash of everything a GridMap's static bodies are built from (pat-2ifdx).
fn gridmap_signature(
    node: &crate::node::Node,
    global: &Transform3D,
    layer: u32,
    mask: u32,
    friction: Option<f64>,
    bounce: Option<f64>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Variant::Dictionary(d) = node.get_property("data") {
        if let Some(Variant::PackedInt32Array(cells)) = d.get("cells") {
            cells.hash(&mut h);
        }
    }
    // Every other property `GridMap::from_node` reads, by its debug form —
    // enough to notice a change without decoding anything.
    for key in [
        "__extres__mesh_library",
        "mesh_library",
        "cell_size",
        "cell_center_x",
        "cell_center_y",
        "cell_center_z",
    ] {
        format!("{:?}", node.get_property(key)).hash(&mut h);
    }
    for v in [global.basis.x, global.basis.y, global.basis.z, global.origin] {
        [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()].hash(&mut h);
    }
    (layer, mask).hash(&mut h);
    friction.map(f64::to_bits).hash(&mut h);
    bounce.map(f64::to_bits).hash(&mut h);
    h.finish()
}

/// Decodes a standalone `Shape3D` resource (a `.tres`/`.res` file, as a
/// MeshLibrary's `item/<id>/shapes` references) into a physics shape
/// (pat-2ifdx).
///
/// 🔑 Routed through [`shape_from_node_3d`] rather than a second per-class
/// reader: the resource's class and properties are laid onto a throwaway
/// `CollisionShape3D` exactly the way `PackedScene` flattens a `shape =
/// SubResource(..)` (`__shape__class` + `__shape__<prop>`), so a GridMap cell's
/// box gets the same defaults and the same convex/concave handling as a box
/// authored in a scene.
pub(crate) fn shape_from_resource_3d(resource: &gdresource::Resource) -> Option<Shape3D> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = crate::node::Node::new("Shape", "CollisionShape3D");
    node.set_property("shape", Variant::String("SubResource:resource".into()));
    node.set_property("__shape__class", Variant::String(resource.class_name.clone()));
    for (key, value) in resource.properties() {
        node.set_property(&format!("__shape__{key}"), value.clone());
    }
    let id = tree.add_child(root, node).ok()?;
    shape_from_node_3d(&tree, id)
}

/// Every collision shape under `body_id`, paired with its offset in the body's
/// LOCAL space, in document order (pat-s3rrv.104).
///
/// Godot's `CollisionObject3D` owns a LIST of shape owners, each with its own
/// transform (`shape_owner_set_transform`), so a platform is routinely two boxes
/// and a character a capsule raised by half its height. The previous reader took
/// the FIRST matching direct child and dropped the rest, and dropped its
/// transform too, so every shape collided at the body origin.
///
/// ⚠️ Walks the whole descendant subtree, not just direct children — a
/// `CollisionShape3D` under an intermediate `Node3D` is normal authoring — but
/// stops at a nested `CollisionObject3D`, which owns its own shapes and would
/// otherwise have them counted twice, once for itself and once for its ancestor.
///
/// The offset is taken from GLOBAL transforms and then rotated back through the
/// body's inverse rotation, so it is expressed in the body's local frame. That
/// matters because [`PhysicsBody3D::shapes_world`] re-applies the body rotation:
/// handing it a world-space delta would rotate the offset twice for any tilted
/// body, and the error would be invisible in an axis-aligned test.
pub(crate) fn shapes_from_body_subtree(
    tree: &SceneTree,
    body_id: NodeId,
) -> Vec<(Vector3, Quaternion, Shape3D)> {
    let body_global = node3d::get_global_transform(tree, body_id);
    let body_origin = body_global.origin;
    let inv_rot = Quaternion::from_euler(node3d::get_rotation(tree, body_id)).inverse();
    // The body's GLOBAL orientation, so a shape's rotation can be expressed
    // relative to its body the way `Basis` composition does it — the same
    // "parent inverse * child" idiom `skeleton3d.rs` uses for bone poses.
    //
    // ⚠️ PRE-EXISTING INCONSISTENCY, deliberately left alone: `inv_rot` above
    // is built from the body's LOCAL euler (`node3d::get_rotation`), not its
    // global one, so the position delta below is already mis-rotated for a body
    // nested under a rotated parent. That is not this bead's bug and existing
    // green tests may pin the current behaviour, so the offset line is
    // untouched; the two agree whenever the body's ancestors are unrotated,
    // which covers every case in the tree today. Worth its own bead.
    let inv_rot_global = body_global.basis.get_rotation_quaternion().inverse();

    let mut out: Vec<(Vector3, Quaternion, Shape3D)> = Vec::new();
    let mut stack: Vec<NodeId> = tree
        .get_node(body_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    // Reversed so `pop()` yields children in document order, keeping the FIRST
    // authored shape primary — the shape the old `find_map` would have picked.
    stack.reverse();

    while let Some(nid) = stack.pop() {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        let class = node.class_name().to_string();
        let mut kids = node.children().to_vec();

        if body_type_for_class_3d(&class).is_some() || class == "Area3D" {
            continue;
        }

        if let Some(shape) = shape_from_node_3d(tree, nid) {
            let shape_global = node3d::get_global_transform(tree, nid);
            let delta = shape_global.origin - body_origin;
            // 🔑 The shape's FULL local transform is the shape owner's
            // transform in Godot (`CollisionObject3D::shape_owner_set_transform`),
            // rotation included. Only the translation was carried before, so a
            // cylinder authored on its side or a box turned 90 degrees into a
            // wall collided bolt upright (pat-mgixk.39).
            //
            // Taken from the GLOBAL basis rather than the node's own `rotation`
            // so an intermediate `Node3D` between the body and the shape — which
            // the walker explicitly supports — contributes its rotation too.
            let local_rot = inv_rot_global * shape_global.basis.get_rotation_quaternion();
            out.push((inv_rot.xform(delta), local_rot, shape));
        }

        kids.reverse();
        stack.extend(kids);
    }
    out
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
    /// Raycast-vehicle controllers, one per `VehicleBody3D` node
    /// (pat-s3rrv.194). The chassis itself is an ordinary rigid body in the
    /// world; the controller only pushes it.
    vehicles: HashMap<NodeId, VehicleController3D>,
    /// Each vehicle's `VehicleWheel3D` nodes, in the order of its controller's
    /// wheels (pat-qg3nu). A wheel's simulation state follows its NODE across
    /// ticks, and its transform is written back to that node.
    vehicle_wheel_nodes: HashMap<NodeId, Vec<NodeId>>,
    /// Vehicles whose `_body_state_changed` ran in this tick's step.
    vehicles_updated: HashSet<NodeId>,
    /// `physics/3d/default_linear_damp` / `default_angular_damp` (Godot: 0.1
    /// each), combined into every rigid body's damping (pat-qg3nu).
    default_linear_damp: f32,
    default_angular_damp: f32,
    /// Source of joint ids. `PhysicsWorld3D::add_joint` keeps whatever id the
    /// joint was built with, so the server is what has to hand out unique ones.
    next_joint_id: u64,
    /// `SoftBody3D` scene nodes simulated in the world (pat-s3rrv.193).
    ///
    /// The weld map lives beside the id because it is derived from the node's
    /// mesh at registration and is needed again to publish solved positions
    /// back in authored vertex order.
    soft_bodies: HashMap<NodeId, SoftBodyBinding3D>,
    /// Source of soft-body ids. `PhysicsWorld3D::add_soft_body` keeps whatever
    /// id the body was built with, so the server hands out unique ones — the
    /// same arrangement as `next_joint_id`.
    next_soft_body_id: u64,
    /// Static bodies built from each `GridMap` node's MeshLibrary shapes
    /// (pat-2ifdx), keyed by the GridMap node.
    gridmaps: HashMap<NodeId, GridMapBodies3D>,
    /// Shape resources decoded for GridMap cells, by `res://` path. `None`
    /// records a path that failed to load or is not a supported shape, so it
    /// is not retried every rebuild.
    gridmap_shape_cache: HashMap<String, Option<Shape3D>>,
    /// Which GridMap OCTANT each GridMap cell body belongs to (pat-kiuul).
    /// Godot gives an octant ONE body; Patina gives every cell its own, so a
    /// character's Jolt contact query groups them back by this key (its
    /// internal-edge removal runs per body). Rebuilt with the GridMap bodies.
    compound_groups: std::sync::Arc<HashMap<BodyId3D, u64>>,
    gravity: Vector3,
    /// Bodies registered by the sync of the CURRENT physics tick, cleared by
    /// [`begin_physics_tick`](Self::begin_physics_tick) (pat-d4ghb).
    ///
    /// Godot runs a tick's scripts BEFORE its space step, and a body's gravity
    /// is only computed by a step, so a body's first `_physics_process` reads
    /// `get_gravity() == ZERO` (measured on Godot 4.6.1 + Jolt: the 3D
    /// platformer's player reads `(0, 0, 0)` at physics frame 1 and
    /// `(0, -22, 0)` from frame 2). Patina registers and steps BEFORE the
    /// script phase, so without this the first tick already fell, and every
    /// later sample of the player was one tick of gravity ahead.
    fresh_bodies: HashSet<BodyId3D>,
    /// Rigid bodies whose step was held back this tick (pat-29zst): every
    /// fresh rigid body. See [`step`](Self::step).
    held_this_step: HashSet<BodyId3D>,
    /// True once a host drives this server tick by tick through
    /// [`begin_physics_tick`](Self::begin_physics_tick), as `MainLoop` does.
    /// Only then is a fresh body's step held (pat-29zst); a caller that just
    /// syncs and steps the server directly keeps plain step-per-call motion.
    tick_driven: bool,
    /// Set while [`sync_registrations`](Self::sync_registrations) runs
    /// `sync_to_physics` as a registration-only pass.
    registration_only: bool,
    /// True while `MainLoop` runs the `_integrate_forces` pass, which (unlike
    /// `_physics_process`) already sees the real gravity on a body's first
    /// tick — Godot calls it from inside the step that computes it.
    force_integration_phase: bool,
}

/// A `GridMap` node's registration in the physics world (pat-2ifdx).
///
/// Godot gives every GridMap octant a static body holding its cells' shapes
/// (`grid_map.cpp`, `_octant_update`). Here each cell SHAPE is its own static
/// body: the 3D world, character sweep and ray queries all test a body's
/// primary shape at the body's own position and rotation, so one body per shape
/// is what every consumer already handles exactly — including the orthogonal
/// rotation of a cell — without each of them learning about compound bodies.
struct GridMapBodies3D {
    /// Hash of everything the bodies were built from (cells, transform,
    /// layer/mask, library, cell size/centring). Rebuilt only when it changes,
    /// so a script's `set_cell_item` takes effect next tick and a static level
    /// costs nothing per tick.
    signature: u64,
    bodies: Vec<BodyId3D>,
    /// Octant key per body, parallel to `bodies` (pat-kiuul).
    groups: Vec<u64>,
}

/// One key per (GridMap node, octant) — Godot's one body per octant.
fn octant_group_key(nid: NodeId, octant: [i32; 3]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nid.hash(&mut h);
    octant.hash(&mut h);
    h.finish()
}

/// A `SoftBody3D` node's registration in the physics world.
struct SoftBodyBinding3D {
    id: SoftBodyId3D,
    /// One entry per authored mesh vertex, naming the particle carrying it.
    visual_to_physics: Vec<usize>,
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
            vehicles: HashMap::new(),
            vehicle_wheel_nodes: HashMap::new(),
            vehicles_updated: HashSet::new(),
            default_linear_damp: 0.1,
            default_angular_damp: 0.1,
            next_joint_id: 1,
            soft_bodies: HashMap::new(),
            next_soft_body_id: 1,
            gridmaps: HashMap::new(),
            gridmap_shape_cache: HashMap::new(),
            compound_groups: std::sync::Arc::new(HashMap::new()),
            gravity: Vector3::new(0.0, -9.8, 0.0),
            fresh_bodies: HashSet::new(),
            held_this_step: HashSet::new(),
            tick_driven: false,
            registration_only: false,
            force_integration_phase: false,
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
            vehicles: HashMap::new(),
            vehicle_wheel_nodes: HashMap::new(),
            vehicles_updated: HashSet::new(),
            default_linear_damp: 0.1,
            default_angular_damp: 0.1,
            next_joint_id: 1,
            soft_bodies: HashMap::new(),
            next_soft_body_id: 1,
            gridmaps: HashMap::new(),
            gridmap_shape_cache: HashMap::new(),
            compound_groups: std::sync::Arc::new(HashMap::new()),
            gravity,
            fresh_bodies: HashSet::new(),
            held_this_step: HashSet::new(),
            tick_driven: false,
            registration_only: false,
            force_integration_phase: false,
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

    /// `physics/3d/default_linear_damp` (Godot default 0.1).
    pub fn set_default_linear_damp(&mut self, damp: f32) {
        self.default_linear_damp = damp;
    }

    /// `physics/3d/default_angular_damp` (Godot default 0.1).
    pub fn set_default_angular_damp(&mut self, damp: f32) {
        self.default_angular_damp = damp;
    }

    /// A rigid body's total damping, as `JoltBody3D::_update_damp` combines it:
    /// the space default PLUS the body's own under `DAMP_MODE_COMBINE` (0, the
    /// default), the body's alone under `DAMP_MODE_REPLACE` (1) (pat-qg3nu).
    /// Patina had no 3D space default, so every body coasted forever where a
    /// Godot one loses 0.1 of its speed per second: truck_town's car ran 12%
    /// fast at full throttle. Area damp overrides are not folded in here.
    fn total_damp_3d(&self, tree: &SceneTree, nid: NodeId) -> (f32, f32) {
        let linear = float_prop_3d(tree, nid, "linear_damp", 0.0) as f32;
        let angular = float_prop_3d(tree, nid, "angular_damp", 0.0) as f32;
        let linear = match u32_prop_3d(tree, nid, "linear_damp_mode", 0) {
            1 => linear,
            _ => self.default_linear_damp + linear,
        };
        let angular = match u32_prop_3d(tree, nid, "angular_damp_mode", 0) {
            1 => angular,
            _ => self.default_angular_damp + angular,
        };
        (linear, angular)
    }

    /// The total gravity at a node's body — what `PhysicsBody3D.get_gravity()`
    /// answers (pat-v3co1): the space default plus any `Area3D` override the
    /// body overlaps. A node with no registered body (not yet synced) gets the
    /// space default.
    ///
    /// ⚠️ ZERO for a body registered in the current tick (pat-d4ghb): Godot
    /// has not stepped it yet when its first `_physics_process` runs, and the
    /// gravity a body reports is the one its last step computed. See
    /// `fresh_bodies`.
    pub fn body_total_gravity(&self, node_id: NodeId) -> Vector3 {
        match self.body_for_node(node_id) {
            Some(id) if self.fresh_bodies.contains(&id) && !self.force_integration_phase => {
                Vector3::ZERO
            }
            Some(id) => self.world.body_total_gravity(id),
            None => self.world.gravity,
        }
    }

    /// Selects Jolt's contact penetration model, or the full push-out with
    /// `None` (pat-29zst). See [`gdphysics3d::world::JoltContactParams`].
    pub fn set_jolt_contacts(&mut self, params: Option<gdphysics3d::world::JoltContactParams>) {
        self.world.jolt_contacts = params;
    }

    /// Marks the start of a physics tick: bodies registered by the previous
    /// tick's sync are no longer fresh (pat-d4ghb).
    ///
    /// `MainLoop` calls it at the start of every tick, before the sync that
    /// registers this tick's new bodies.
    pub fn begin_physics_tick(&mut self) {
        self.tick_driven = true;
        self.fresh_bodies.clear();
    }

    /// Brackets the `_integrate_forces` pass (see `force_integration_phase`).
    pub fn set_force_integration_phase(&mut self, on: bool) {
        self.force_integration_phase = on;
    }

    /// Every node that has a body in the world.
    pub fn registered_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_to_body.keys().copied()
    }

    /// Returns the number of tracked physics bodies.
    pub fn body_count(&self) -> usize {
        self.node_to_body.len()
    }

    /// Returns the underlying physics world (read-only).
    pub fn world(&self) -> &PhysicsWorld3D {
        &self.world
    }

    /// Mutable access to the underlying physics world, for tests and tools.
    pub fn world_mut(&mut self) -> &mut PhysicsWorld3D {
        &mut self.world
    }

    /// Godot `PhysicsBody3D.add_collision_exception_with(body)` (pat-s3rrv.105).
    ///
    /// Returns whether the exception was actually registered. ⚠️ **Both nodes
    /// must already be REGISTERED bodies.** A script calling this in `_ready` —
    /// before the first sync has run — resolves no `BodyId3D`, and the exception
    /// would be dropped; answering `false` lets the caller tell that apart from
    /// success instead of assuming it worked.
    ///
    /// The 3D twin of the 2D path in `scripting.rs`, which is why it reports the
    /// same bool.
    pub fn node_add_collision_exception(&mut self, node: NodeId, other: NodeId) -> bool {
        let (Some(a), Some(b)) = (self.body_for_node(node), self.body_for_node(other)) else {
            return false;
        };
        self.world.add_collision_exception(a, b);
        true
    }

    /// Godot `PhysicsBody3D.remove_collision_exception_with(body)`.
    ///
    /// Removing one that was never added is a no-op and still reports `true`:
    /// the request was applied, and Godot draws no distinction.
    pub fn node_remove_collision_exception(&mut self, node: NodeId, other: NodeId) -> bool {
        let (Some(a), Some(b)) = (self.body_for_node(node), self.body_for_node(other)) else {
            return false;
        };
        self.world.remove_collision_exception(a, b);
        true
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
            // A GridMap cell's body reports the GridMap itself, as Godot's
            // collider for a cell hit is the GridMap node.
            .or_else(|| {
                self.gridmaps
                    .iter()
                    .find(|(_, g)| g.bodies.contains(&body_id))
                    .map(|(&n, _)| n)
            })
    }

    /// GridMap cell body -> octant key (pat-kiuul); see `compound_groups`.
    pub fn compound_groups(&self) -> std::sync::Arc<HashMap<BodyId3D, u64>> {
        self.compound_groups.clone()
    }

    /// The static bodies registered for a `GridMap` node's cells (pat-2ifdx).
    pub fn gridmap_bodies(&self, node_id: NodeId) -> &[BodyId3D] {
        self.gridmaps
            .get(&node_id)
            .map(|g| g.bodies.as_slice())
            .unwrap_or(&[])
    }

    /// Registers, rebuilds or frees the static bodies of every `GridMap` in the
    /// tree (pat-2ifdx). Before this no physics code read a GridMap at all, so
    /// a level built from one — the official 3D platformer's whole stage — had
    /// no collision and the player fell straight through it.
    ///
    /// Godot places each item shape at `cell_xform * shape_xform`
    /// (`_octant_update`), with `cell_xform` the cell's orthogonal orientation
    /// at its centre, under the GridMap's own global transform. The bodies take
    /// the GridMap's `collision_layer` / `collision_mask` and physics material.
    fn sync_gridmaps(&mut self, tree: &SceneTree, all_nodes: &[NodeId]) {
        let mut live: HashSet<NodeId> = HashSet::new();
        let mut changed = false;
        for &nid in all_nodes {
            let Some(node) = tree.get_node(nid) else {
                continue;
            };
            if node.class_name() != "GridMap" {
                continue;
            }
            live.insert(nid);
            let global = node3d::get_global_transform(tree, nid);
            let layer = u32_prop_3d(tree, nid, "collision_layer", 1);
            let mask = u32_prop_3d(tree, nid, "collision_mask", 1);
            let friction = opt_float_prop_3d(tree, nid, "__physics_material__friction");
            let bounce = opt_float_prop_3d(tree, nid, "__physics_material__bounce");
            // Hashed straight off the node's properties, BEFORE decoding the
            // cells: this runs every tick, and a level is thousands of cells.
            let signature = gridmap_signature(node, &global, layer, mask, friction, bounce);
            if self
                .gridmaps
                .get(&nid)
                .is_some_and(|g| g.signature == signature)
            {
                continue;
            }

            // Rebuild: free the old bodies first.
            changed = true;
            if let Some(old) = self.gridmaps.remove(&nid) {
                for id in old.bodies {
                    self.world.remove_body(id);
                }
            }
            let Some(mut grid) = crate::grid_map::GridMap::from_node(tree, nid) else {
                continue;
            };
            if let Some(path) = grid.mesh_library_resource.clone() {
                if let Some(library) = crate::grid_map::MeshLibrary::load(&path) {
                    grid.mesh_library = library;
                }
            }
            let rough = truthy_prop_3d(tree, nid, "__physics_material__rough", false);
            let absorbent = truthy_prop_3d(tree, nid, "__physics_material__absorbent", false);
            let mut bodies = Vec::new();
            let mut groups = Vec::new();
            let octant_size = u32_prop_3d(tree, nid, "cell_octant_size", 8) as i32;
            for (local, path, octant) in grid.collision_shapes_by_octant(octant_size) {
                let shape = self
                    .gridmap_shape_cache
                    .entry(path.clone())
                    .or_insert_with(|| {
                        crate::array_mesh::read_resource_file(&path)
                            .and_then(|r| shape_from_resource_3d(&r))
                    })
                    .clone();
                let Some(shape) = shape else {
                    continue;
                };
                let placed = global * local;
                let mut body = PhysicsBody3D::new(
                    BodyId3D(0),
                    BodyType3D::Static,
                    placed.origin,
                    shape,
                    0.0,
                );
                body.rotation = placed.basis.get_rotation_quaternion();
                body.collision_layer = layer;
                body.collision_mask = mask;
                if let Some(f) = friction {
                    body.friction = f as f32;
                }
                if let Some(b) = bounce {
                    body.bounce = b as f32;
                }
                if rough {
                    body.friction = -body.friction.abs();
                }
                if absorbent {
                    body.bounce = -body.bounce.abs();
                }
                bodies.push(self.world.add_body(body));
                groups.push(octant_group_key(nid, octant));
            }
            self.gridmaps.insert(
                nid,
                GridMapBodies3D {
                    signature,
                    bodies,
                    groups,
                },
            );
        }

        let stale: Vec<NodeId> = self
            .gridmaps
            .keys()
            .filter(|n| !live.contains(n))
            .copied()
            .collect();
        for nid in stale {
            changed = true;
            if let Some(g) = self.gridmaps.remove(&nid) {
                for id in g.bodies {
                    self.world.remove_body(id);
                }
            }
        }
        if changed {
            let mut map = HashMap::new();
            for g in self.gridmaps.values() {
                for (id, group) in g.bodies.iter().zip(&g.groups) {
                    map.insert(*id, *group);
                }
            }
            self.compound_groups = std::sync::Arc::new(map);
        }
    }

    /// Syncs scene tree 3D body nodes into the physics world.
    ///
    /// Registers new bodies for untracked nodes and updates positions for
    /// existing tracked bodies.
    /// Registers the bodies of nodes that entered the tree since the last sync,
    /// and nothing else: no velocity, force or flag is read and no stale body
    /// is dropped (pat-29zst).
    ///
    /// `MainLoop` runs it right after each tick's script phase. Godot adds a
    /// body to its space when the node enters the tree, so a body a
    /// `_physics_process` spawns (a bullet) is part of THAT tick's step, which
    /// Patina runs at the start of the next tick. Registered here, the body is
    /// not fresh at that step and moves on the same tick it does in Godot.
    /// A body that entered the tree outside the script phase (the scene at
    /// boot, a `_process` spawn, a changed scene) is registered by the next
    /// tick's [`sync_to_physics`](Self::sync_to_physics) instead, as fresh.
    pub fn sync_registrations(&mut self, tree: &SceneTree) {
        self.registration_only = true;
        self.sync_to_physics(tree);
        self.registration_only = false;
    }

    /// Whether a body's step was held back by the last [`step`](Self::step)
    /// because it was fresh: it has not been stepped by Godot's reckoning
    /// yet, so its `_integrate_forces` does not run either (pat-29zst).
    pub fn step_was_held(&self, node_id: NodeId) -> bool {
        self.body_for_node(node_id)
            .is_some_and(|id| self.held_this_step.contains(&id))
    }

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
                        // ⚠️ `VehicleBody3D.mass` defaults to **40.0**, not
                        // RigidBody3D's 1.0 (`VehicleBody3D.xml`). Sharing the
                        // 1.0 default would make an unauthored car 40x too
                        // light, so the same `engine_force` would rocket it —
                        // wrong in a way that looks like the drive model is
                        // broken rather than like a bad default.
                        let default_mass = if class == "VehicleBody3D" { 40.0 } else { 1.0 };
                        let mass = float_prop_3d(tree, nid, "mass", default_mass) as f32;

                        // Every CollisionShape3D in the subtree, with offsets.
                        let mut authored = shapes_from_body_subtree(tree, nid);
                        let (shape, shape_offset, shape_rotation) = if authored.is_empty() {
                            (
                                Shape3D::Sphere { radius: 0.5 },
                                Vector3::ZERO,
                                Quaternion::IDENTITY,
                            )
                        } else {
                            let (off, rot, sh) = authored.remove(0);
                            (sh, off, rot)
                        };

                        let mut body =
                            PhysicsBody3D::new(placeholder_id, body_type, position, shape, mass);
                        body.shape_offset = shape_offset;
                        body.shape_rotation = shape_rotation;
                        body.extra_shapes = authored;
                        body.inertia_override = authored_inertia_3d(tree, nid, &body);

                        // Initial orientation: the node's WORLD one, like its
                        // position (pat-qg3nu). The LOCAL rotation dropped the
                        // parent's: truck_town's car, under an `InstancePos`
                        // turned 90 degrees, simulated facing world +Z while
                        // drawn facing +X, so it drove sideways.
                        body.rotation = node3d::get_global_rotation_quaternion(tree, nid);

                        // Read the physics material / body properties from the
                        // node so authored values drive the simulation.
                        body.collision_layer = u32_prop_3d(tree, nid, "collision_layer", 1);
                        body.collision_mask = u32_prop_3d(tree, nid, "collision_mask", 1);
                        body.gravity_scale = float_prop_3d(tree, nid, "gravity_scale", 1.0) as f32;
                        let (linear_damp, angular_damp) = self.total_damp_3d(tree, nid);
                        body.linear_damp = linear_damp;
                        body.angular_damp = angular_damp;
                        // pat-s3rrv.105 — Godot's `custom_integrator` hands
                        // force integration to the script's
                        // `_integrate_forces`. The engine then applies NO
                        // gravity and no damping of its own; the body moves
                        // only by whatever velocity the script leaves on the
                        // state, which is the whole point of the callback.
                        //
                        // Until now the flag only GATED the callback, so a
                        // script zeroing `state.linear_velocity` still fell —
                        // by exactly one tick's worth of gravity per tick,
                        // which reads as a solver tolerance rather than the
                        // engine ignoring the flag.
                        //
                        // ⚠️ Overridden here rather than as a flag on
                        // `PhysicsBody3D`: that struct has 18 literal
                        // construction sites, and this is self-correcting —
                        // the sync recomputes all three every tick, so clearing
                        // `custom_integrator` restores the authored values on
                        // the very next one.
                        if matches!(
                            tree.get_node(nid)
                                .map(|n| n.get_property("custom_integrator")),
                            Some(Variant::Bool(true))
                        ) {
                            body.gravity_scale = 0.0;
                            body.linear_damp = 0.0;
                            body.angular_damp = 0.0;
                        }
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
                        // A body registered right after the script phase that
                        // created it is in Godot's space for that tick's step,
                        // so it is not fresh (pat-29zst).
                        if !self.registration_only {
                            self.fresh_bodies.insert(actual_id);
                        }
                    }
                    if self.registration_only {
                        continue;
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
                            let rotation = node3d::get_global_rotation_quaternion(tree, nid);
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

                    // Re-seed the shape list EVERY tick, for every body type —
                    // deliberately outside the Rigid/else split above.
                    //
                    // ⚠️ Placing this inside the `else` arm (where the rest of the
                    // per-tick node sync lives) would skip RigidBody3D entirely,
                    // which is the one body type most likely to have a shape
                    // resized at runtime. Areas already re-seed unconditionally
                    // further down; bodies did not, which is why a CollisionShape3D
                    // added, moved, resized or disabled after the first tick was
                    // ignored forever.
                    if let Some(&body_id) = self.node_to_body.get(&nid) {
                        let mut authored = shapes_from_body_subtree(tree, nid);
                        if let Some(b) = self.world.get_body_mut(body_id) {
                            if authored.is_empty() {
                                // No authored shape: leave whatever the body already
                                // has rather than snapping it to the 0.5 sphere
                                // fallback part-way through a simulation.
                                b.shape_offset = Vector3::ZERO;
                                b.shape_rotation = Quaternion::IDENTITY;
                                b.extra_shapes.clear();
                            } else {
                                let (off, rot, sh) = authored.remove(0);
                                b.shape = sh;
                                b.shape_offset = off;
                                // Re-read every tick alongside the offset, so a
                                // CollisionShape3D ROTATED after the first frame
                                // is picked up — the same reason the offset is
                                // refreshed here rather than set once.
                                b.shape_rotation = rot;
                                b.extra_shapes = authored;
                            }
                        }
                    }
                }
            }
        }

        // Remove bodies for nodes that no longer exist.
        if self.registration_only {
            return;
        }
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

        self.sync_gridmaps(tree, &all_nodes);
        self.sync_areas(tree, &all_nodes);

        // Joints last: they resolve `node_a`/`node_b` to `BodyId3D`s, so every
        // body has to be in `node_to_body` before this runs (pat-0q2kx.118).
        self.sync_joints(tree, &all_nodes);
        self.sync_vehicles(tree, &all_nodes);
        self.sync_soft_bodies(tree, &all_nodes);
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
            // Godot's `PhysicalBone3D.joint_type` chooses the constraint that
            // ties this bone to its parent — it is what makes an elbow bend one
            // way and a shoulder swing in a cone (pat-s3rrv.192). Every bone
            // used to receive a pin regardless, so a ragdoll authored with hinge
            // elbows flailed freely in every direction.
            //
            // ⚠️ `joint_constraints/angular_limit_*`, `swing_span` and
            // `twist_span` are authored in DEGREES and converted with
            // `deg_to_rad` on the way in (`physical_bone_3d.cpp:253, :259,
            // :172, :178`). Reading them as radians would be an ~57x error that
            // silently produces a limit far too wide to constrain anything.
            let constraint = |key: &str, default: f64| -> f32 {
                float_prop_3d(
                    tree,
                    bone_node,
                    &format!("joint_constraints/{key}"),
                    default,
                ) as f32
            };
            // ⚠️ `u32_prop_3d`, not a hand-rolled i32 reader — `joint_type` is a
            // non-negative enum ordinal and this is the integer helper this file
            // already has.
            let joint_type = u32_prop_3d(tree, bone_node, "joint_type", 0);
            let joint = match joint_type {
                // JOINT_TYPE_NONE — the bone falls free of its parent.
                0 => None,
                // JOINT_TYPE_CONE = 2.
                2 => {
                    let mut cone = ConeTwistJoint3D::new(id, parent_body, body_id)
                        .with_pivots(frame_parent.pivot, frame_child.pivot)
                        .with_twist_axis(frame_parent.x);
                    cone.swing_span = constraint("swing_span", 45.0).to_radians();
                    cone.twist_span = constraint("twist_span", 180.0).to_radians();
                    cone.bias = constraint("bias", 0.3);
                    cone.softness = constraint("softness", 0.8);
                    cone.relaxation = constraint("relaxation", 1.0);
                    Some(Joint3D::ConeTwist(cone))
                }
                // JOINT_TYPE_HINGE = 3.
                3 => {
                    let mut hinge =
                        HingeJoint3D::new(id, parent_body, body_id).with_axis(frame_parent.z);
                    hinge.anchor_a = frame_parent.pivot;
                    hinge.anchor_b = frame_child.pivot;
                    hinge.use_limit = truthy_prop_3d(
                        tree,
                        bone_node,
                        "joint_constraints/angular_limit_enabled",
                        true,
                    );
                    hinge.lower_limit = constraint("angular_limit_lower", -90.0).to_radians();
                    hinge.upper_limit = constraint("angular_limit_upper", 90.0).to_radians();
                    Some(Joint3D::Hinge(hinge))
                }
                // JOINT_TYPE_SLIDER = 4.
                4 => {
                    let mut slider = SliderJoint3D::new(id, parent_body, body_id)
                        .with_anchors(frame_parent.pivot, frame_child.pivot)
                        .with_axis(frame_parent.x);
                    slider.use_linear_limit = true;
                    slider.linear_limit_lower = constraint("linear_limit_lower", -1.0);
                    slider.linear_limit_upper = constraint("linear_limit_upper", 1.0);
                    Some(Joint3D::Slider(slider))
                }
                // JOINT_TYPE_6DOF = 5.
                5 => Some(Joint3D::Generic6Dof(
                    Generic6DofJoint3D::new(id, parent_body, body_id)
                        .with_anchors(frame_parent.pivot, frame_child.pivot),
                )),
                // JOINT_TYPE_PIN = 1, and anything unrecognised. Pin is the
                // safest fallback: it holds the bones together, which is what
                // every ragdoll did before this switch existed.
                _ => Some(Joint3D::Pin(
                    PinJoint3D::new(id, parent_body, body_id)
                        .with_pivots(frame_parent.pivot, frame_child.pivot),
                )),
            };
            let Some(joint) = joint else {
                continue;
            };
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
                // The motion/ortho triples (pat-s3rrv.192). Godot exposes three
                // groups per axis pair — limit, motion and ortho — and only the
                // limit group was read here, so a slider authored with softer
                // off-axis coupling behaved exactly like a rigid one.
                //
                // ⚠️ Every default below is transcribed from
                // `doc/classes/SliderJoint3D.xml`. They are NOT uniform:
                // `linear_motion/damping` is 0.0 while the other three dampings
                // are 1.0, and getting that one wrong locks the slider.
                slider.linear_motion_softness =
                    float_prop_3d(tree, node_id, "linear_motion/softness", 1.0) as f32;
                slider.linear_motion_restitution =
                    float_prop_3d(tree, node_id, "linear_motion/restitution", 0.7) as f32;
                slider.linear_motion_damping =
                    float_prop_3d(tree, node_id, "linear_motion/damping", 0.0) as f32;
                slider.linear_ortho_softness =
                    float_prop_3d(tree, node_id, "linear_ortho/softness", 1.0) as f32;
                slider.linear_ortho_restitution =
                    float_prop_3d(tree, node_id, "linear_ortho/restitution", 0.7) as f32;
                slider.linear_ortho_damping =
                    float_prop_3d(tree, node_id, "linear_ortho/damping", 1.0) as f32;
                slider.angular_motion_softness =
                    float_prop_3d(tree, node_id, "angular_motion/softness", 1.0) as f32;
                slider.angular_motion_restitution =
                    float_prop_3d(tree, node_id, "angular_motion/restitution", 0.7) as f32;
                slider.angular_motion_damping =
                    float_prop_3d(tree, node_id, "angular_motion/damping", 1.0) as f32;
                slider.angular_ortho_softness =
                    float_prop_3d(tree, node_id, "angular_ortho/softness", 1.0) as f32;
                slider.angular_ortho_restitution =
                    float_prop_3d(tree, node_id, "angular_ortho/restitution", 0.7) as f32;
                slider.angular_ortho_damping =
                    float_prop_3d(tree, node_id, "angular_ortho/damping", 1.0) as f32;
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

                    // -- motors and springs (pat-s3rrv.192) -----------------
                    //
                    // ⚠️ The defaults are ASYMMETRIC between linear and angular
                    // and are transcribed from `Generic6DOFJoint3D.xml`, not
                    // derived: a linear motor's `force_limit` defaults to 0.0
                    // while an angular one defaults to 300.0, and a linear
                    // spring's `stiffness`/`damping` default to 0.01 while the
                    // angular pair default to 0.0. Passing one shared default
                    // here would silently give every joint the wrong strength on
                    // whichever side it did not match — and a `force_limit` of 0
                    // makes an enabled motor powerless, so the error looks like
                    // "motors do not work" rather than like a bad default.
                    let lm = AxisMotor::linear_default();
                    dof.linear_motors[index] = AxisMotor {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_motor_{axis}/enabled"),
                            lm.enabled,
                        ),
                        target_velocity: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_motor_{axis}/target_velocity"),
                            lm.target_velocity as f64,
                        ) as f32,
                        force_limit: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_motor_{axis}/force_limit"),
                            lm.force_limit as f64,
                        ) as f32,
                    };
                    let am = AxisMotor::angular_default();
                    dof.angular_motors[index] = AxisMotor {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_motor_{axis}/enabled"),
                            am.enabled,
                        ),
                        target_velocity: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_motor_{axis}/target_velocity"),
                            am.target_velocity as f64,
                        ) as f32,
                        force_limit: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_motor_{axis}/force_limit"),
                            am.force_limit as f64,
                        ) as f32,
                    };
                    let ls = AxisSpring::linear_default();
                    dof.linear_springs[index] = AxisSpring {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_spring_{axis}/enabled"),
                            ls.enabled,
                        ),
                        stiffness: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_spring_{axis}/stiffness"),
                            ls.stiffness as f64,
                        ) as f32,
                        damping: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_spring_{axis}/damping"),
                            ls.damping as f64,
                        ) as f32,
                        equilibrium_point: float_prop_3d(
                            tree,
                            node_id,
                            &format!("linear_spring_{axis}/equilibrium_point"),
                            ls.equilibrium_point as f64,
                        ) as f32,
                    };
                    let as_ = AxisSpring::angular_default();
                    dof.angular_springs[index] = AxisSpring {
                        enabled: truthy_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_spring_{axis}/enabled"),
                            as_.enabled,
                        ),
                        stiffness: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_spring_{axis}/stiffness"),
                            as_.stiffness as f64,
                        ) as f32,
                        damping: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_spring_{axis}/damping"),
                            as_.damping as f64,
                        ) as f32,
                        equilibrium_point: float_prop_3d(
                            tree,
                            node_id,
                            &format!("angular_spring_{axis}/equilibrium_point"),
                            as_.equilibrium_point as f64,
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
        // `RigidBody3D.set_axis_velocity` (pat-s3rrv.105). Read as a Vector3:
        // the interpreter now writes the argument through with its own kind, so
        // a 3D call arrives as a Vector3 and a 2D one never reaches this server.
        let axis_velocity = vec3_opt_prop_3d(tree, nid, "_pending_axis_velocity");
        // doc/classes/RigidBody3D.xml defaults.
        let gravity_scale = float_prop_3d(tree, nid, "gravity_scale", 1.0) as f32;
        let (linear_damp, angular_damp) = self.total_damp_3d(tree, nid);
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
        // Godot: `axis = p_axis.normalized(); vel -= axis * axis.dot(vel); vel += p_axis`
        // (`rigid_body_3d.cpp:463`). The component ALONG the axis is replaced
        // and everything perpendicular is retained — which is why this runs
        // after the plain `linear_velocity` write rather than instead of it.
        //
        // ⚠️ Guarded on a non-degenerate length: `Vector3::ZERO.normalized()`
        // has no direction, and dividing by it would poison the velocity with
        // NaN rather than doing nothing.
        if let Some(av) = axis_velocity {
            let len_sq = av.dot(av);
            if len_sq > 1e-12 {
                let dir = av * (1.0 / len_sq.sqrt());
                let along = body.linear_velocity.dot(dir);
                body.linear_velocity = body.linear_velocity - dir * along + av;
            }
        }
        body.constant_force = constant_force;
        body.constant_torque = constant_torque;
        body.gravity_scale = gravity_scale;
        body.linear_damp = linear_damp;
        body.angular_damp = angular_damp;
        // Same `custom_integrator` override as the per-tick sync above — a body
        // registered while the flag is already set must not fall for the one
        // tick before the next sync runs (pat-s3rrv.105).
        if matches!(
            tree.get_node(nid)
                .map(|n| n.get_property("custom_integrator")),
            Some(Variant::Bool(true))
        ) {
            body.gravity_scale = 0.0;
            body.linear_damp = 0.0;
            body.angular_damp = 0.0;
        }
        body.lock_rotation = lock_rotation;
        // Per-axis freezes (pat-s3rrv.105). Godot spells these as six separate
        // booleans; the body stores them indexed [x, y, z] so the order here is
        // the contract between the two.
        //
        // ⚠️ Read EVERY tick alongside the other authored properties, not once
        // at registration: a script toggling `axis_lock_linear_y` mid-flight is
        // the normal way these are used (freeze a body while a cutscene plays),
        // and a registration-time read would ignore it forever.
        body.axis_lock_linear = [
            truthy_prop_3d(tree, nid, "axis_lock_linear_x", false),
            truthy_prop_3d(tree, nid, "axis_lock_linear_y", false),
            truthy_prop_3d(tree, nid, "axis_lock_linear_z", false),
        ];
        body.axis_lock_angular = [
            truthy_prop_3d(tree, nid, "axis_lock_angular_x", false),
            truthy_prop_3d(tree, nid, "axis_lock_angular_y", false),
            truthy_prop_3d(tree, nid, "axis_lock_angular_z", false),
        ];
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
                // ⚠️ Cleared to Nil, not to ZERO like the accumulators above.
                // `set_axis_velocity` is ONE-SHOT — Godot applies it once, at
                // the call — and a lingering value would re-apply the override
                // every tick, pinning the body's velocity along that axis
                // forever. Nil is also what the 2D server writes back, so
                // "pending" stays distinguishable from "asked for zero".
                if !matches!(node.get_property("_pending_axis_velocity"), Variant::Nil) {
                    node.set_property("_pending_axis_velocity", Variant::Nil);
                }
            }
        }
    }

    /// Writes each wheel's simulated state back onto its `VehicleWheel3D`
    /// node (pat-s3rrv.194, pat-qg3nu).
    ///
    /// 🔑 The wheel node's TRANSFORM is written here, as Godot's
    /// `_body_state_changed` does without physics interpolation
    /// (`wheels[i]->set_transform(state.transform.inverse() * m_worldTransform)`):
    /// the suspension drop, steering and spin. Before pat-qg3nu it never was,
    /// so a wheel stayed at its authored pose forever.
    ///
    /// The contact state travels as node properties because that is the only
    /// place a script can reach it: `is_in_contact()` reads
    /// `_wheel_in_contact`, `get_skidinfo()` `_wheel_skidinfo`, `get_rpm()`
    /// `_wheel_rpm`.
    fn publish_wheel_state(&self, tree: &mut SceneTree) {
        if self.vehicles.is_empty() {
            return;
        }
        struct WheelOut {
            node: NodeId,
            local: Option<gdcore::math3d::Transform3D>,
            in_contact: bool,
            skid: f32,
            rpm: f32,
            point: Vector3,
            normal: Vector3,
            compression: f32,
        }
        let mut updates: Vec<WheelOut> = Vec::new();
        for (body_node, controller) in &self.vehicles {
            let Some(body) = self.world.get_body(controller.chassis) else {
                continue;
            };
            let chassis = gdcore::math3d::Transform3D {
                basis: gdcore::math3d::Basis::from_quaternion(body.rotation),
                origin: body.position,
            };
            // Only once the vehicle has run: a held (fresh) chassis has had no
            // callback yet, and its wheels keep their authored pose.
            let posed = self.vehicles_updated.contains(body_node);
            let Some(wheel_nodes) = self.vehicle_wheel_nodes.get(body_node) else {
                continue;
            };
            for (i, (wnode, w)) in wheel_nodes.iter().zip(controller.wheels.iter()).enumerate() {
                let travel = w.max_suspension_travel;
                let compression = if travel > 0.0 {
                    ((w.suspension_rest_length - w.suspension_length) / travel).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                updates.push(WheelOut {
                    node: *wnode,
                    local: if posed { controller.wheel_local_transform(i, &chassis) } else { None },
                    in_contact: w.in_contact,
                    skid: w.skid_info,
                    rpm: w.rpm,
                    point: w.contact_point,
                    normal: w.contact_normal,
                    compression,
                });
            }
        }

        for u in updates {
            let Some(node) = tree.get_node_mut(u.node) else {
                continue;
            };
            if let Some(local) = u.local {
                node3d::set_transform_component(node, "transform", &Variant::Transform3D(local));
            }
            node.set_property("_wheel_in_contact", Variant::Bool(u.in_contact));
            node.set_property("_wheel_compression", Variant::Float(u.compression as f64));
            node.set_property("_wheel_skidinfo", Variant::Float(u.skid as f64));
            node.set_property("_wheel_rpm", Variant::Float(u.rpm as f64));
            node.set_property("_wheel_contact_point", Variant::Vector3(u.point));
            node.set_property("_wheel_contact_normal", Variant::Vector3(u.normal));
        }
    }

    /// Builds and refreshes a [`VehicleController3D`] for every `VehicleBody3D`
    /// node from its `VehicleWheel3D` children (pat-s3rrv.194, pat-qg3nu).
    ///
    /// ⚠️ A wheel's GEOMETRY (connection point, direction, axle) is read ONCE,
    /// when the wheel first joins, as Godot reads it at the wheel's
    /// `NOTIFICATION_ENTER_TREE`. From then on the node's transform is the
    /// vehicle's OUTPUT (`publish_wheel_state`); re-reading it each tick would
    /// feed the suspension drop back in as a new hard point and the car would
    /// sink by a suspension length per tick.
    ///
    /// The tunables and `engine_force`/`brake`/`steering` are re-read every
    /// tick: they are what a script writes each frame, and Godot's setters
    /// take effect at once.
    fn sync_vehicles(&mut self, tree: &SceneTree, all_nodes: &[NodeId]) {
        let mut live: Vec<NodeId> = Vec::new();
        for &nid in all_nodes {
            let Some(node) = tree.get_node(nid) else {
                continue;
            };
            if node.class_name() != "VehicleBody3D" {
                continue;
            }
            let Some(&chassis) = self.node_to_body.get(&nid) else {
                continue;
            };
            live.push(nid);

            let wheel_nodes: Vec<NodeId> = node
                .children()
                .iter()
                .copied()
                .filter(|&c| {
                    tree.get_node(c)
                        .map(|n| n.class_name() == "VehicleWheel3D")
                        .unwrap_or(false)
                })
                .collect();

            let previous = self.vehicle_wheel_nodes.get(&nid).cloned().unwrap_or_default();
            let controller = self
                .vehicles
                .entry(nid)
                .or_insert_with(|| VehicleController3D::new(chassis));
            controller.chassis = chassis;
            // doc/classes/VehicleBody3D.xml: mass defaults to 40.
            controller.mass = float_prop_3d(tree, nid, "mass", 40.0) as f32;
            controller.collision_mask = u32_prop_3d(tree, nid, "collision_mask", 1);

            // A wheel keeps its simulation state across ticks; one seen for the
            // first time caches its authored pose.
            let mut wheels = Vec::with_capacity(wheel_nodes.len());
            for &wnode in &wheel_nodes {
                let kept = previous
                    .iter()
                    .position(|&p| p == wnode)
                    .and_then(|i| controller.wheels.get(i).cloned());
                let mut wheel = kept.unwrap_or_else(|| {
                    VehicleWheel3D::new(node3d::get_local_transform(tree, wnode))
                });
                // Defaults from scene/3d/physics/vehicle_body_3d.h.
                wheel.radius = float_prop_3d(tree, wnode, "wheel_radius", 0.5) as f32;
                wheel.suspension_rest_length =
                    float_prop_3d(tree, wnode, "wheel_rest_length", 0.15) as f32;
                wheel.max_suspension_travel =
                    float_prop_3d(tree, wnode, "suspension_travel", 0.2) as f32;
                wheel.suspension_stiffness =
                    float_prop_3d(tree, wnode, "suspension_stiffness", 5.88) as f32;
                wheel.max_suspension_force =
                    float_prop_3d(tree, wnode, "suspension_max_force", 6000.0) as f32;
                wheel.damping_compression =
                    float_prop_3d(tree, wnode, "damping_compression", 0.83) as f32;
                wheel.damping_relaxation =
                    float_prop_3d(tree, wnode, "damping_relaxation", 0.88) as f32;
                wheel.friction_slip =
                    float_prop_3d(tree, wnode, "wheel_friction_slip", 10.5) as f32;
                wheel.roll_influence =
                    float_prop_3d(tree, wnode, "wheel_roll_influence", 0.1) as f32;
                wheel.traction = truthy_prop_3d(tree, wnode, "use_as_traction", false);
                wheel.steers = truthy_prop_3d(tree, wnode, "use_as_steering", false);
                // The wheel's own engine_force/brake/steering, for a wheel the
                // body's setters skip (Godot's body setter writes only traction
                // or steering wheels).
                wheel.engine_force = float_prop_3d(tree, wnode, "engine_force", 0.0) as f32;
                wheel.brake = float_prop_3d(tree, wnode, "brake", 0.0) as f32;
                wheel.steering = float_prop_3d(tree, wnode, "steering", 0.0) as f32;
                wheels.push(wheel);
            }
            controller.wheels = wheels;
            controller.set_engine_force(float_prop_3d(tree, nid, "engine_force", 0.0) as f32);
            let brake = float_prop_3d(tree, nid, "brake", 0.0) as f32;
            if brake != 0.0 {
                controller.set_brake(brake);
            }
            controller.set_steering(float_prop_3d(tree, nid, "steering", 0.0) as f32);
            self.vehicle_wheel_nodes.insert(nid, wheel_nodes);
        }
        self.vehicles.retain(|nid, _| live.contains(nid));
        self.vehicle_wheel_nodes.retain(|nid, _| live.contains(nid));
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
        // 🔑 Godot steps the space at the END of a tick, after the scripts,
        // and copies the result onto the nodes at the NEXT tick's
        // `flush_queries` (`main.cpp`: sync, flush_queries, physics_process,
        // step). Patina runs sync, step and publish together at the START of
        // a tick, so its step at tick k stands for Godot's step at the end of
        // tick k-1, and what it publishes is exactly what Godot's flush
        // publishes at tick k (pat-29zst).
        //
        // That correspondence fails for a body that was NOT in Godot's space
        // at the end of the previous tick: the scene's bodies at boot (Godot
        // has no step before the first tick), a body spawned from `_process`
        // or a changed scene. Godot first steps such a body at the end of
        // this tick, so Patina must not move it until the next one. Those are
        // exactly the fresh bodies; a body spawned by `_physics_process` is
        // registered right after the script phase (`sync_registrations`) and
        // is not fresh.
        //
        // Measured on Godot 4.6.1 + Jolt (3D platformer, Enemy1, a custom
        // integrator): after two ticks the node still reads its authored
        // y = 5.40827 with one `_integrate_forces` run (velocity -0.1833);
        // after four ticks it has moved by two steps (5.40369) with three.
        // Without the hold Patina moved it at the first tick (5.4067 at f1).
        //
        // Held, not left out: the body stays in the world, so other bodies and
        // queries still meet it. Its state is put back after the step, which
        // keeps a velocity a script or `_ready` impulse gave it for the step
        // that really moves it.
        self.held_this_step.clear();
        let held: Vec<(BodyId3D, Vector3, gdcore::math3d::Quaternion, Vector3, Vector3)> = self
            .fresh_bodies
            .iter()
            .filter(|_| self.tick_driven)
            .filter_map(|&id| {
                let b = self.world.get_body(id)?;
                (b.body_type == BodyType3D::Rigid).then_some((
                    id,
                    b.position,
                    b.rotation,
                    b.linear_velocity,
                    b.angular_velocity,
                ))
            })
            .collect();
        self.world.step(dt);
        for (id, position, rotation, lin, ang) in held {
            if let Some(b) = self.world.get_body_mut(id) {
                b.position = position;
                b.rotation = rotation;
                b.linear_velocity = lin;
                b.angular_velocity = ang;
            }
            self.held_this_step.insert(id);
        }
        // 🔑 The vehicles run AFTER the step (pat-qg3nu). Godot's
        // `VehicleBody3D::_body_state_changed` is the chassis' state-sync
        // callback, called from the next tick's `flush_queries`: it sees the
        // stepped chassis, reads the `engine_force`/`steering` the scripts
        // wrote last tick, and its impulses move the body in the FOLLOWING
        // step, like `_integrate_forces`. Running it before the step (as this
        // used to) moved the car by forces of the same step and read controls
        // one tick early. A held (fresh) chassis gets no callback yet, since
        // Godot has not stepped it. Controllers are taken out and put back
        // because `update` needs `&mut self.world`.
        self.vehicles_updated.clear();
        if !self.vehicles.is_empty() && dt > 0.0 {
            let mut taken: Vec<(NodeId, VehicleController3D)> = self.vehicles.drain().collect();
            for (nid, controller) in &mut taken {
                if self.held_this_step.contains(&controller.chassis) {
                    continue;
                }
                controller.update(&mut self.world, dt);
                self.vehicles_updated.insert(*nid);
            }
            self.vehicles.extend(taken);
        }
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

    /// Publishes each monitoring body's contact list onto its node as
    /// `_body_contacts`, the same array the 2D server writes (pat-s3rrv.105).
    ///
    /// 🔑 This is what makes `get_contact_count()` / `get_colliding_bodies()`
    /// work for a `RigidBody3D`. Those script arms read the node property and
    /// hand each field straight back, so they are dimension-agnostic already —
    /// the 3D gap was a missing PUBLISHER, not missing arms. Before this they
    /// answered 0 for every 3D body however hard it was pressed against
    /// something.
    ///
    /// ⚠️ Separate from `process_body_contact_signals` below, which reports
    /// enter/exit EDGES and drains them. A body resting on the floor persists
    /// and produces no edge, so publishing there would make a resting body
    /// visible for exactly one frame.
    fn publish_body_contacts(&mut self, tree: &mut SceneTree) {
        // The solver already applies Godot's gate — a body only collects
        // contacts while its own `contact_monitor` is set and it is under its
        // `max_contacts_reported` budget — so an unmonitored body simply has an
        // empty list and publishes one.
        let records: Vec<(NodeId, Vec<Variant>)> = self
            .node_to_body
            .iter()
            .filter_map(|(&nid, &body_id)| {
                let body = self.world.get_body(body_id)?;
                if !body.contact_monitor {
                    return None;
                }
                let list = body
                    .get_contacts()
                    .iter()
                    .map(|c| {
                        let mut rec = gdvariant::Dictionary::new();
                        // ⚠️ NEGATED. `ContactPoint3D.normal` runs from THIS
                        // body toward the other, while Godot's
                        // `get_contact_local_normal` points away from the
                        // surface the body is resting against — up, for a body
                        // on a floor. Publishing the raw normal would report
                        // every floor contact as pointing down and silently
                        // invert the standard floor test, exactly as the 2D
                        // publisher documents.
                        rec.insert("normal", Variant::Vector3(c.normal * -1.0));
                        rec.insert("position", Variant::Vector3(c.position));
                        rec.insert("depth", Variant::Float(c.depth as f64));
                        rec.insert(
                            "collider",
                            self.node_for_body(c.other_body)
                                .map(|n| Variant::ObjectId(n.object_id()))
                                .unwrap_or(Variant::Nil),
                        );
                        Variant::Dictionary(rec.into())
                    })
                    .collect();
                Some((nid, list))
            })
            .collect();

        for (nid, list) in records {
            if let Some(node) = tree.get_node_mut(nid) {
                node.set_property("_body_contacts", Variant::Array(list.into()));
            }
        }
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

    /// The audio bus an `Area3D` imposes at `point`, if any — Godot's
    /// `Area3D.audio_bus_override` / `audio_bus_name` (pat-s3rrv.102).
    ///
    /// Godot resolves this the same way it resolves the space overrides: every
    /// area containing the point is a candidate and the one with the highest
    /// `priority` wins (`area_3d.cpp`). An `AudioStreamPlayer3D` standing
    /// inside such an area has its voice rerouted to that bus — the usual use
    /// being a reverb bus for a cave or a room.
    ///
    /// ⚠️ This is a POINT test against the area's shape, not a body-overlap
    /// query. An `AudioStreamPlayer3D` is not a physics body and never appears
    /// in `_overlapping_bodies`, so the overlap machinery cannot answer this.
    ///
    /// Returns `None` when no overriding area contains the point, which leaves
    /// the voice on whatever bus it already had.
    pub fn audio_bus_at(&self, tree: &SceneTree, point: Vector3) -> Option<String> {
        let mut best: Option<(f32, String)> = None;
        for (&area_id, &node_id) in &self.area_to_node {
            let Some(area) = self.world.get_area(area_id) else {
                continue;
            };
            // The shape is stored in the area's local space, so the point has to
            // come back to it before the containment test.
            if !area.shape.contains_point(point - area.position) {
                continue;
            }
            if !truthy_prop_3d(tree, node_id, "audio_bus_override", false) {
                continue;
            }
            let bus = match tree
                .get_node(node_id)
                .map(|n| n.get_property("audio_bus_name"))
            {
                Some(Variant::String(name)) if !name.is_empty() => name,
                // Godot's default bus name. An override with no name still
                // means "route somewhere", and Master is where it routes.
                _ => "Master".to_string(),
            };
            let better = match &best {
                Some((p, _)) => area.priority > *p,
                None => true,
            };
            if better {
                best = Some((area.priority, bus));
            }
        }
        best.map(|(_, bus)| bus)
    }

    /// Publishes each `Area3D` node's current overlap set onto the node as
    /// `_overlapping_bodies`, which is what the script-facing
    /// `get_overlapping_bodies()` / `has_overlapping_bodies()` read.
    ///
    /// Same bridge shape as RayCast2D's `_hit_normal`: the script phase has no
    /// handle on the physics server, so the physics phase leaves the answer on
    /// the node (pat-0q2kx.35).
    pub fn publish_overlapping_bodies(&self, tree: &mut SceneTree) {
        for (&area_node, &area_id) in &self.node_to_area {
            // Read from the overlap set `step` just detected rather than
            // re-testing every area against every body (pat-2ifdx): this runs
            // straight after that detection, against the same positions, so the
            // answer is identical — and on a GridMap level the second full
            // area-by-body pass was most of the tick.
            let bodies: Vec<Variant> = self
                .world
                .overlapping_bodies_detected(area_id)
                .into_iter()
                .filter_map(|body_id| self.node_for_body(body_id))
                .map(|nid| Variant::ObjectId(nid.object_id()))
                .collect();
            if let Some(node) = tree.get_node_mut(area_node) {
                node.set_property("_overlapping_bodies", Variant::Array(bodies.into()));
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
                node.set_property("_overlapping_areas", Variant::Array(areas.into()));
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
    /// Registers `SoftBody3D` nodes into the world and keeps their tuning live.
    ///
    /// The node's mesh is the source of geometry, so this goes through
    /// `resolve_mesh` — the same entry point the render collection uses — rather
    /// than re-deriving `__mesh__*`/SubResource flattening here. A node whose
    /// mesh has not resolved yet is skipped, not registered empty, so it can be
    /// picked up on a later tick once the resource loads.
    fn sync_soft_bodies(&mut self, tree: &SceneTree, all_nodes: &[NodeId]) {
        for &nid in all_nodes {
            let Some(node) = tree.get_node(nid) else {
                continue;
            };
            if node.class_name() != "SoftBody3D" {
                continue;
            }

            if !self.soft_bodies.contains_key(&nid) {
                let mesh = crate::render_server_3d::RenderServer3DAdapter::resolve_mesh(tree, nid);
                if mesh.vertices.is_empty() {
                    continue;
                }

                // Simulate in GLOBAL space: gravity is a world vector, so a
                // rotated or translated node whose cloth was stepped in local
                // space would sag along the wrong axis. A node at the origin
                // with an identity basis cannot tell the difference, which is
                // exactly why this is done rather than assumed.
                let xf = node3d::get_global_transform(tree, nid);
                let world_positions: Vec<Vector3> =
                    mesh.vertices.iter().map(|&v| xf.xform(v)).collect();

                self.next_soft_body_id += 1;
                let id = SoftBodyId3D(self.next_soft_body_id);
                let (mut body, visual_to_physics) = SoftBody3D::from_mesh(
                    id,
                    &world_positions,
                    &mesh.indices,
                    DEFAULT_WELD_EPSILON,
                );

                // `_pinned_points` holds AUTHORED vertex indices. Welding means
                // those are not particle indices, so pinning them directly would
                // pin whichever particles happened to land at those offsets.
                for visual in pinned_point_indices_3d(tree, nid) {
                    if let Some(&particle) = visual_to_physics.get(visual) {
                        body.pin_vertex(particle);
                    }
                }

                self.world.add_soft_body(body);
                self.soft_bodies.insert(
                    nid,
                    SoftBodyBinding3D {
                        id,
                        visual_to_physics,
                    },
                );
            }

            self.apply_soft_body_tuning(tree, nid);
        }
    }

    /// Re-reads the inspector-facing tuning properties every tick, so editing
    /// one takes effect without rebuilding the cloth and losing its state.
    fn apply_soft_body_tuning(&mut self, tree: &SceneTree, nid: NodeId) {
        let Some(binding) = self.soft_bodies.get(&nid) else {
            return;
        };
        let id = binding.id;

        // Read before borrowing the world mutably.
        let stiffness = float_prop_3d(tree, nid, "linear_stiffness", 0.5) as f32;
        let damping = float_prop_3d(tree, nid, "damping_coefficient", 0.01) as f32;
        let pressure = float_prop_3d(tree, nid, "pressure_coefficient", 0.0) as f32;
        let precision = u32_prop_3d(tree, nid, "simulation_precision", 4);

        if let Some(body) = self
            .world
            .soft_bodies_mut()
            .iter_mut()
            .find(|sb| sb.id == id)
        {
            body.stiffness = stiffness;
            body.damping = damping;
            body.pressure = pressure;
            // Godot allows 0 but a zero-iteration solver silently applies no
            // constraints at all, which reads as "the cloth has no edges".
            body.solver_iterations = precision.max(1);
        }
    }

    /// Publishes solved vertex positions back onto each soft body's node.
    ///
    /// Converted back into the node's LOCAL space, because that is the space the
    /// mesh the renderer draws is expressed in — the node transform is applied
    /// on top of it. Publishing world positions would apply the transform twice.
    fn publish_soft_body_vertices(&mut self, tree: &mut SceneTree) {
        let bindings: Vec<(NodeId, SoftBodyId3D, Vec<usize>)> = self
            .soft_bodies
            .iter()
            .map(|(&nid, b)| (nid, b.id, b.visual_to_physics.clone()))
            .collect();

        for (nid, id, visual_to_physics) in bindings {
            let inverse = node3d::get_global_transform(tree, nid).inverse();
            let Some(body) = self
                .world
                .soft_bodies_mut()
                .iter_mut()
                .find(|sb| sb.id == id)
            else {
                continue;
            };

            crate::softbody_sync_3d::publish_vertices_with_transform(
                tree,
                nid,
                body,
                &visual_to_physics,
                inverse,
            );
        }
    }

    pub fn sync_from_physics(&mut self, tree: &mut SceneTree) {
        self.publish_body_contacts(tree);
        self.publish_soft_body_vertices(tree);
        self.publish_wheel_state(tree);
        for (&nid, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                if body.body_type == BodyType3D::Rigid {
                    // 🔑 `body.position` is WORLD space (registration reads the
                    // global origin), and `position` is the node's LOCAL one.
                    // Writing it straight across re-applied the parent's offset:
                    // the platformer's enemies, under an `Enemies` node at
                    // (-16, -6, -12), were drawn at TWICE that offset while their
                    // bodies simulated where the scene put them (pat-1unro). The
                    // character pass already de-parents the same way.
                    node3d::set_global_position(tree, nid, body.position);
                    // Pull the simulated orientation back so tumbling/rolling
                    // rigid bodies rotate their scene nodes too.
                    // World orientation, de-parented like the position.
                    node3d::set_global_rotation_quaternion(tree, nid, body.rotation);
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

    /// pat-qg3nu: `DAMP_MODE_REPLACE` (1) drops the space default; a
    /// project's `default_linear_damp` feeds the COMBINE sum.
    #[test]
    fn damp_mode_replace_ignores_the_space_default() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Ball", "RigidBody3D");
        node.set_property("linear_damp", Variant::Float(0.3));
        node.set_property("linear_damp_mode", Variant::Int(1));
        let replaced = tree.add_child(root, node).unwrap();
        let combined = tree.add_child(root, Node::new("Other", "RigidBody3D")).unwrap();
        let mut server = PhysicsServer3D::new();
        server.set_default_linear_damp(0.5);
        server.sync_to_physics(&tree);
        let damp = |nid| server.world().get_body(server.body_for_node(nid).unwrap()).unwrap().linear_damp;
        assert!((damp(replaced) - 0.3).abs() < 1e-6, "replace: {}", damp(replaced));
        assert!((damp(combined) - 0.5).abs() < 1e-6, "combine: {}", damp(combined));
    }

    /// pat-qg3nu: a rigid body under a ROTATED parent simulates in its world
    /// orientation and writes its local one back. truck_town's car sits under
    /// an `InstancePos` turned 90 degrees about Y: registered with its LOCAL
    /// (identity) rotation, it was simulated facing world +Z and drove off
    /// sideways relative to its drawn heading.
    #[test]
    fn rigid_body_under_a_rotated_parent_uses_its_world_orientation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = tree.add_child(root, Node::new("InstancePos", "Node3D")).unwrap();
        node3d::set_rotation(&mut tree, parent, Vector3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0));
        let mut ball = Node::new("Ball", "RigidBody3D");
        ball.set_property("gravity_scale", Variant::Float(0.0));
        ball.set_property("can_sleep", Variant::Bool(false));
        let body_nid = tree.add_child(parent, ball).unwrap();

        let mut server = PhysicsServer3D::new();
        server.sync_to_physics(&tree);
        let bid = server.body_for_node(body_nid).unwrap();
        let world_z = server.world().get_body(bid).unwrap().rotation.xform(Vector3::new(0.0, 0.0, 1.0));
        assert!((world_z - Vector3::new(1.0, 0.0, 0.0)).length() < 1e-5, "body +Z in the world: {world_z:?}");

        server.step(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
        let local = node3d::get_rotation(&tree, body_nid);
        assert!(local.length() < 1e-4, "an unmoved body keeps its local rotation, got {local:?}");
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
        // DAMP_MODE_COMBINE (the default): the space's 0.1 plus the body's 0.3.
        assert!((body.linear_damp - 0.4).abs() < 1e-5, "linear_damp {}", body.linear_damp);
        assert!((body.angular_damp - 0.1).abs() < 1e-5, "angular_damp {}", body.angular_damp);
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
