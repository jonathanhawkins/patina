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

use std::collections::{HashMap, HashSet};

use gdcore::math::{Transform2D, Vector2, Vector2i};
use gdphysics2d::area2d::{
    Area2D, AreaId, AreaOverlapEvent, AreaSpaceOverride, AreaStore, OverlapEvent, OverlapState,
};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D, ShapeOwner2D};
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
        // ⚠️ Kinematic, NOT Static, even though Godot parents it to
        // StaticBody2D. A moving platform has to be driven by the scene tree
        // every tick, and the Static arm below only picks up teleports past an
        // epsilon — an animated platform would judder or stall (pat-s3rrv.93).
        "AnimatableBody2D" => Some(BodyType::Kinematic),
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

/// A joint's `node_a`/`node_b` as a path string, or `None` when unset
/// (pat-0q2kx.114). Godot writes these as `NodePath`; a hand-written scene may
/// spell them as a plain String, so both are accepted.
fn joint_node_path(node: &crate::node::Node, key: &str) -> Option<String> {
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
fn resolve_joint_target(tree: &SceneTree, from: NodeId, path: &str) -> Option<NodeId> {
    if path.starts_with('/') {
        tree.get_node_by_path(path)
    } else {
        tree.get_node_relative(from, path)
    }
}

/// Reads a float property, or `None` when the node does not carry it
/// (pat-0q2kx.115).
///
/// Distinct from [`float_prop`]: an override that is merely *absent* must leave
/// the existing value alone, which a defaulting read cannot express — a
/// `PhysicsMaterial` with no `bounce` would otherwise stamp 0.0 over whatever
/// the body already had.
fn opt_float_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<f64> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Float(f) => Some(f),
        Variant::Int(i) => Some(i as f64),
        _ => None,
    }
}

/// Reads a `Vector2` property, or `None` when absent — the same
/// leave-it-alone semantics as [`opt_float_prop`].
fn opt_vec2_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<Vector2> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Vector2(v) => Some(v),
        _ => None,
    }
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

/// Builds a shape from a `CollisionPolygon2D`'s inline `polygon` outline.
///
/// Godot's `build_mode` decides how the outline becomes collision geometry
/// (`scene/2d/physics/collision_polygon_2d.cpp`): `BUILD_SOLIDS` (0, the
/// default) makes a solid convex body, `BUILD_SEGMENTS` (1) makes a hollow
/// outline of segments. Treating a Segments polygon as solid would let bodies
/// rest on the inside of what the author drew as a thin wall.
fn polygon_shape_from_node(tree: &SceneTree, node_id: NodeId) -> Option<Shape2D> {
    let points = vec2array_prop(tree, node_id, "polygon");
    if points.len() < 3 {
        // Godot contributes no collision for a degenerate polygon, and a
        // default stand-in shape here would be worse than none.
        return None;
    }

    let build_mode = match tree.get_node(node_id).map(|n| n.get_property("build_mode")) {
        Some(Variant::Int(i)) => i,
        _ => 0,
    };

    if build_mode == 1 {
        // Expand the outline into the consecutive point PAIRS that
        // `ConcavePolygon2D` stores, closing the loop back to the first point.
        let mut segments: Vec<Vector2> = Vec::with_capacity(points.len() * 2);
        for i in 0..points.len() {
            segments.push(points[i]);
            segments.push(points[(i + 1) % points.len()]);
        }
        Some(Shape2D::concave_polygon(&segments))
    } else {
        Some(Shape2D::convex_polygon(&points))
    }
}

/// Extracts a collision shape from a `CollisionShape2D` or `CollisionPolygon2D`
/// node's properties.
pub(crate) fn shape_from_node(tree: &SceneTree, node_id: NodeId) -> Option<Shape2D> {
    let node = tree.get_node(node_id)?;
    // A CollisionPolygon2D carries its geometry inline as a `polygon`
    // PackedVector2Array rather than referencing a Shape2D sub-resource, so it
    // never had a path through the sub-resource logic below. Rejecting it here
    // meant a body whose only collider was a polygon silently fell back to the
    // default 16px circle — the authored ramp/terrain shape simply did not
    // exist to the solver (pat-0q2kx.34).
    if node.class_name() == "CollisionPolygon2D" {
        return polygon_shape_from_node(tree, node_id);
    }
    if node.class_name() != "CollisionShape2D" {
        return None;
    }
    shape_from_shape_props(tree, node_id)
}

/// Builds a shape from a node's own `shape` property and its `__shape__*`
/// companions, WITHOUT the collider-class guard above.
///
/// Split out for `ShapeCast2D` (pat-s3rrv.87), which carries a `shape`
/// sub-resource of its own and is flattened identically — but which must NOT be
/// accepted by [`shape_from_node`]. That function is called on **every child**
/// when registering a body's colliders, and a shape cast is routinely a child of
/// the body it casts from, so widening the class guard there would silently give
/// that body an extra collision shape.
pub(crate) fn shape_from_shape_props(tree: &SceneTree, node_id: NodeId) -> Option<Shape2D> {
    let node = tree.get_node(node_id)?;

    // A real `.tscn` writes `shape = SubResource("id")`, which the parser leaves as
    // `"SubResource:<id>"`; the geometry lives on the sub-resource and is copied
    // onto this node under `__shape__*` at instancing (packed_scene.rs). Hand-built
    // nodes instead put the class name in `shape` with the dimensions directly on
    // the node. Both are accepted: resolve the class, then read each dimension from
    // the namespaced key if present, else the bare one (pat-0q2kx.28).
    let shape_prop = node.get_property("shape");
    let resolved_class: Option<String> = match &shape_prop {
        Variant::String(s) if s.starts_with("SubResource:") => {
            match node.get_property("__shape__class") {
                Variant::String(class) => Some(class),
                // An unresolved reference has no geometry — better no shape than a
                // default-sized one silently standing in for the authored size.
                _ => None,
            }
        }
        Variant::String(s) => Some(s.clone()),
        // A shape built at RUNTIME — `$Col.shape = CircleShape2D.new()`, which
        // is Godot's standard runtime shape authoring — arrives as a live
        // resource rather than a string: it has no `res://` path and no
        // `__shape__*` companions, because nothing ever wrote it to a `.tscn`.
        // Its class name and its dimensions both live INSIDE the resource
        // (pat-s3rrv.98).
        //
        // ⚠️ This used to fall into `_ => None`, and the failure was silent in
        // the worst way: the body still registered, it just took stock geometry,
        // so a script asking for `radius = 40` got collision at the default with
        // nothing reporting an error.
        Variant::Resource(r) => Some(r.class_name.clone()),
        _ => None,
    };

    // The script-assigned resource's own properties, when there is one. The
    // dimension readers below consult it BETWEEN the two node-property lookups.
    let shape_res: Option<&gdvariant::ResourceRef> = match &shape_prop {
        Variant::Resource(r) => Some(r.as_ref()),
        _ => None,
    };
    let res_prop = move |key: &str| -> Option<Variant> {
        shape_res.and_then(|r| r.properties.get(key).cloned())
    };

    // Dimension readers, in priority order:
    //   1. `__shape__<key>` — the .tscn sub-resource's value, copied onto the
    //      node at instancing;
    //   2. the assigned resource's own property — a script-built shape;
    //   3. the bare node key — a hand-built node with dimensions inline.
    // The .tscn value stays first so this cannot change how an authored scene
    // resolves; the new arm only fires where there was previously no answer.
    let has = |key: &str| !matches!(node.get_property(key), Variant::Nil);
    let num = |key: &str, default: f64| -> f32 {
        let ns = format!("__shape__{key}");
        if has(&ns) {
            float_prop(tree, node_id, &ns, default) as f32
        } else if let Some(v) = res_prop(key) {
            match v {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                _ => default as f32,
            }
        } else {
            float_prop(tree, node_id, key, default) as f32
        }
    };
    let v2 = |key: &str, default: Vector2| -> Vector2 {
        let ns = format!("__shape__{key}");
        if has(&ns) {
            vec2_prop(tree, node_id, &ns, default)
        } else if let Some(Variant::Vector2(v)) = res_prop(key) {
            v
        } else {
            vec2_prop(tree, node_id, key, default)
        }
    };
    let v2s = |key: &str| -> Vec<Vector2> {
        let ns = format!("__shape__{key}");
        let namespaced = vec2array_prop(tree, node_id, &ns);
        if !namespaced.is_empty() {
            return namespaced;
        }
        if let Some(Variant::PackedVector2Array(v)) = res_prop(key) {
            if !v.is_empty() {
                return v;
            }
        }
        vec2array_prop(tree, node_id, key)
    };

    match resolved_class.as_deref() {
        Some(class) => match class {
            "CircleShape2D" => {
                let radius = num("radius", 10.0);
                Some(Shape2D::Circle { radius })
            }
            "RectangleShape2D" => {
                let size = v2("size", Vector2::new(20.0, 20.0));
                Some(Shape2D::Rectangle {
                    half_extents: Vector2::new(size.x / 2.0, size.y / 2.0),
                })
            }
            "CapsuleShape2D" => {
                let radius = num("radius", 10.0);
                // ⚠️ 30.0, not 20.0 (pat-s3rrv.98). `doc/classes/CapsuleShape2D.xml`
                // gives `height` a default of **30.0**, and Godot omits
                // default-valued properties when it writes a `.tscn` — so a
                // capsule authored in Godot and left at its default arrives here
                // with NO `height` key at all, and the old 20.0 silently gave it
                // a shape two thirds the authored size.
                //
                // This also closes a disagreement inside our own engine: ClassDB
                // registers 30.0 (from the same XML), so a script-built
                // `CapsuleShape2D.new()` was 30 while a `.tscn`-authored one was
                // 20 — the exact mismatch this bead set out to prevent.
                let height = num("height", 30.0);
                Some(Shape2D::Capsule { radius, height })
            }
            "SegmentShape2D" => {
                let a = v2("a", Vector2::ZERO);
                let b = v2("b", Vector2::new(0.0, 10.0));
                Some(Shape2D::Segment { a, b })
            }
            "ConvexPolygonShape2D" => {
                let points = v2s("points");
                if points.is_empty() {
                    None
                } else {
                    Some(Shape2D::convex_polygon(&points))
                }
            }
            "ConcavePolygonShape2D" => {
                // Godot stores concave collision as a flat "segments" array of
                // point pairs.
                let segments = v2s("segments");
                if segments.is_empty() {
                    None
                } else {
                    Some(Shape2D::concave_polygon(&segments))
                }
            }
            "WorldBoundaryShape2D" => {
                let normal = v2("normal", Vector2::new(0.0, -1.0));
                let distance = num("distance", 0.0);
                Some(Shape2D::world_boundary(normal, distance))
            }
            "SeparationRayShape2D" => {
                let length = num("length", 20.0);
                Some(Shape2D::SeparationRay { length })
            }
            _ => None,
        },
        None => {
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

/// Finds the first *enabled* `CollisionShape2D` child of a node and returns its
/// shape.
///
/// Godot skips a shape whose `disabled` is true when it builds the body's shape
/// list (collision_shape_2d.cpp), so a disabled shape must contribute no collision
/// at all rather than a default-sized stand-in (pat-0q2kx.28).
fn find_collision_shape(tree: &SceneTree, parent_id: NodeId) -> Option<Shape2D> {
    let children = tree.get_node(parent_id)?.children().to_vec();
    for child_id in children {
        if bool_prop(tree, child_id, "disabled", false) {
            continue;
        }
        if let Some(shape) = shape_from_node(tree, child_id) {
            return Some(shape);
        }
    }
    None
}

/// The local transform of the collider child that [`find_collision_shape`]
/// selected — Godot's shape-owner transform (pat-s3rrv.25).
///
/// `collision_shape_2d.cpp::_update_in_shape_owner` calls
/// `shape_owner_set_transform(owner_id, get_transform())`, so a
/// `CollisionShape2D`'s own `position`/`rotation` place its geometry relative to
/// the body. Patina read the body's transform and nothing else, so an authored
/// collider offset — a character whose collider sits at the feet, a hitbox on a
/// muzzle — collided at the body origin instead, silently.
///
/// ⚠️ Walks the children with the **same** disabled-skipping order as
/// `find_collision_shape`, so the transform returned always belongs to the shape
/// that was actually taken. Reading the first child unconditionally would pair a
/// disabled collider's offset with an enabled collider's geometry, which is
/// worse than the bug being fixed: it would be wrong on scenes that currently
/// work.
///
/// Scale is deliberately excluded — it keeps flowing through `scale_shape`,
/// which bakes it into the geometry. `PhysicsBody2D::shape_world_transform`
/// documents why that split matters.
fn find_collision_shape_transform(tree: &SceneTree, parent_id: NodeId) -> Option<Transform2D> {
    let children = tree.get_node(parent_id)?.children().to_vec();
    for child_id in children {
        if bool_prop(tree, child_id, "disabled", false) {
            continue;
        }
        if shape_from_node(tree, child_id).is_some() {
            let offset = crate::node2d::get_position(tree, child_id);
            let rotation = crate::node2d::get_rotation(tree, child_id);
            return Some(Transform2D::from_rotation_origin(rotation, offset));
        }
    }
    None
}

/// Every collider child *after* the one [`find_collision_shape`] selected, as
/// [`ShapeOwner2D`]s already scaled by `scale` (pat-s3rrv.25).
///
/// Godot gives a `CollisionObject2D` a list of shape owners and Patina now
/// consults all of them: body-vs-body, both area-vs-body sites, area-vs-area,
/// the character sweeps and every query iterate `shape_owners()`.
///
/// Each entry carries the child's OWN `one_way_collision` flag, because Godot
/// puts that flag on the
/// `CollisionShape2D` node. Lifting it to the body — which is what
/// `one_way_collision_for` still does for the PRIMARY shape — made "solid torso
/// plus one-way foot sensor" impossible to express.
///
/// The walk **must** stay identical to `find_collision_shape` /
/// `find_collision_shape_transform` / `collider_local_transforms` — same
/// `disabled` skip, same "usable shape" test, same child order — so that
/// "everything after the first" means the same thing to all four. A different
/// order here would pair one collider's geometry with another's offset, and the
/// script-facing shape-owner ids would stop matching picking's `shape_idx`.
///
/// Scale is applied to the offset exactly as the single-shape path applies it,
/// because the geometry is scaled too; the rotation is left alone.
fn extra_collision_shapes(
    tree: &SceneTree,
    parent_id: NodeId,
    scale: Vector2,
) -> Vec<ShapeOwner2D> {
    let Some(children) = tree.get_node(parent_id).map(|n| n.children().to_vec()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen_first = false;
    for child_id in children {
        if bool_prop(tree, child_id, "disabled", false) {
            continue;
        }
        let Some(shape) = shape_from_node(tree, child_id) else {
            continue;
        };
        if !seen_first {
            // This is the one the single-shape path already took.
            seen_first = true;
            continue;
        }
        let offset = crate::node2d::get_position(tree, child_id);
        let rotation = crate::node2d::get_rotation(tree, child_id);
        let mut tf = Transform2D::from_rotation_origin(rotation, offset);
        tf.origin = Vector2::new(tf.origin.x * scale.x, tf.origin.y * scale.y);
        out.push(ShapeOwner2D {
            shape: scale_shape(shape, scale),
            transform: tf,
            one_way_collision: bool_prop(tree, child_id, "one_way_collision", false),
            // Same direction the PRIMARY shape gets: `PhysicsBody2D::new` seeds
            // `one_way_direction` to world up and `one_way_collision_for` never
            // rewrites it. Deriving it from the collider's rotation here instead
            // would make owner 0 and owner 1 disagree on a rotated platform —
            // turning the blocking face with the collider is a real gap, but it
            // belongs to both shapes at once, not to this one.
            one_way_direction: Vector2::new(0.0, -1.0),
            // `CollisionShape2D.one_way_collision_margin` for THIS collider —
            // per-shape in Godot, so it is read here rather than taken from the
            // body (`collision_shape_2d.cpp:49`).
            one_way_margin: float_prop(tree, child_id, "one_way_collision_margin", 1.0) as f32,
        });
    }
    out
}

/// The local transform of every usable collider child, in shape-owner order
/// (pat-s3rrv.25).
///
/// This is what Godot's `CollisionObject2D.get_shape_owners()` enumerates and
/// what `shape_owner_get_transform(id)` returns — index `i` is owner `i`.
///
/// ⚠️ Lives here, beside `find_collision_shape` and
/// `find_collision_shape_transform`, so every child walk in the engine stays in
/// one file with the same `disabled` skip and the same usable-shape test. A
/// second walk written elsewhere would drift, and the script-facing owner ids
/// would stop matching the `shape_idx` picking reports.
///
/// Transforms are the child's OWN, unscaled: `shape_owner_get_transform` is
/// defined relative to the body, and the accumulated scale belongs to the
/// geometry rather than to this value.
pub(crate) fn collider_local_transforms(tree: &SceneTree, parent_id: NodeId) -> Vec<Transform2D> {
    let Some(children) = tree.get_node(parent_id).map(|n| n.children().to_vec()) else {
        return Vec::new();
    };
    children
        .into_iter()
        .filter(|&c| !bool_prop(tree, c, "disabled", false))
        .filter(|&c| shape_from_node(tree, c).is_some())
        .map(|c| {
            Transform2D::from_rotation_origin(
                crate::node2d::get_rotation(tree, c),
                crate::node2d::get_position(tree, c),
            )
        })
        .collect()
}

/// Whether `parent_id` has any `CollisionShape2D` child at all, enabled or not.
///
/// Distinguishes "this body's shapes are all disabled" (no collision, per Godot)
/// from "this body was built without shape children", which the older
/// test-convention scenes rely on and which keeps the legacy default shape
/// (pat-0q2kx.28).
fn has_collision_shape_child(tree: &SceneTree, parent_id: NodeId) -> bool {
    tree.get_node(parent_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .any(|child| {
            tree.get_node(child)
                .map(|n| {
                    // CollisionPolygon2D counts too — otherwise a body whose
                    // only collider is a disabled polygon looks like a body
                    // authored with no shapes at all, and gets the legacy
                    // default circle instead of no collision (pat-0q2kx.34).
                    matches!(n.class_name(), "CollisionShape2D" | "CollisionPolygon2D")
                })
                .unwrap_or(false)
        })
}

/// One-way collision settings of the collider child that
/// [`find_collision_shape`] selected, as `(enabled, margin)`.
///
/// Godot puts `one_way_collision` on the *shape* node (collision_shape_2d.cpp).
/// Every collider AFTER the first carries its own flag on its
/// [`ShapeOwner2D`](gdphysics2d::body::ShapeOwner2D); this lifts the FIRST one
/// onto the body, where `PhysicsBody2D::shape_owners` hands it back as owner
/// `0`. The direction is the shape's local "up" — Godot blocks motion coming
/// from the side the shape faces, which for an unrotated platform is straight
/// up.
///
/// ⚠️ Walks with the same `disabled` skip and the same `shape_from_node` test as
/// `find_collision_shape`, so the flag returned belongs to the shape actually
/// taken. It used to require `class_name() == "CollisionShape2D"`, which is a
/// DIFFERENT filter: on a body whose first collider is a `CollisionPolygon2D`,
/// that walk skipped past it and lifted a later sibling's one-way flag onto the
/// polygon — the exact cross-wiring the "four walkers stay in lockstep" rule
/// exists to prevent.
fn one_way_collision_for(tree: &SceneTree, parent_id: NodeId) -> Option<(bool, f32)> {
    let children = tree.get_node(parent_id)?.children().to_vec();
    for child_id in children {
        if bool_prop(tree, child_id, "disabled", false) {
            continue;
        }
        if shape_from_node(tree, child_id).is_none() {
            continue;
        }
        let enabled = bool_prop(tree, child_id, "one_way_collision", false);
        let margin = float_prop(tree, child_id, "one_way_collision_margin", 1.0) as f32;
        return Some((enabled, margin));
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
    /// `PhysicsServer2D` script-API handles (pat-0q2kx.200): shape RID -> its
    /// geometry, body RID -> the world body once it has been given a space.
    server_shapes: std::collections::HashMap<u64, Shape2D>,
    server_bodies: std::collections::HashMap<u64, Option<BodyId>>,
    /// Server-created areas by RID, `None` until `area_set_space` admits them
    /// — the same two-phase shape `server_bodies` uses, and for the same reason:
    /// Godot's spaceless area affects nothing.
    server_areas: std::collections::HashMap<u64, Option<AreaId>>,
    /// RIDs are opaque; starting above zero keeps `0` meaning "no space", which
    /// is how Godot spells a spaceless body.
    next_server_rid: u64,
    /// Maps scene node IDs to physics body IDs.
    node_to_body: HashMap<NodeId, BodyId>,
    /// Maps physics body IDs back to scene node IDs.
    body_to_node: HashMap<BodyId, NodeId>,
    /// Per-tick world-space motion of each `AnimatableBody2D` whose
    /// `sync_to_physics` is on (pat-s3rrv.93).
    ///
    /// ⚠️ A DISPLACEMENT, not a velocity: `sync_to_physics` reads the scene and
    /// has no `dt`, while `step_physics` has `dt` but no memory of where the
    /// node used to be. The delta is recorded by the first and divided by the
    /// second, which is also the only ordering that yields the velocity for the
    /// step that is about to run.
    animatable_motion: HashMap<BodyId, Vector2>,
    /// Maps scene node IDs to area IDs.
    node_to_area: HashMap<NodeId, AreaId>,
    /// Maps joint scene nodes to the world joints built from them, so a
    /// re-registration pass does not duplicate them (pat-0q2kx.114).
    node_to_joint: HashMap<NodeId, gdphysics2d::joint::JointId>,
    /// Maps area IDs back to scene node IDs.
    area_to_node: HashMap<AreaId, NodeId>,
    /// Static bodies generated from each tilemap node's painted tiles, so a
    /// later pass can drop the previous generation (pat-0q2kx.117).
    tilemap_bodies: HashMap<NodeId, Vec<BodyId>>,
    /// What the tile bodies of each tilemap node were built from, so an
    /// unchanged map is not rebuilt every frame.
    tilemap_fingerprints: HashMap<NodeId, TileMapFingerprint>,
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

/// Everything about a tilemap node that changes the static bodies it generates
/// (pat-0q2kx.117). Equality decides whether the tile bodies are rebuilt, so it
/// must cover the painted cells themselves — a same-size edit (one cell erased,
/// another painted) would slip past a body-count comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TileMapFingerprint {
    /// Raw `tile_map_data` bytes: which cells are painted, with what.
    cells: Vec<u8>,
    /// Tile source ids that carry collision.
    sources: Vec<i32>,
    /// Cell size in pixels.
    tile_size: Vector2i,
    /// The layer's global position, as bits — `Vector2` is not `Eq`, and an
    /// exact bit comparison is what "unchanged" means here.
    origin: (u32, u32),
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
            server_shapes: std::collections::HashMap::new(),
            server_bodies: std::collections::HashMap::new(),
            server_areas: std::collections::HashMap::new(),
            next_server_rid: 1,
            node_to_body: HashMap::new(),
            body_to_node: HashMap::new(),
            animatable_motion: HashMap::new(),
            node_to_area: HashMap::new(),
            node_to_joint: HashMap::new(),
            area_to_node: HashMap::new(),
            tilemap_bodies: HashMap::new(),
            tilemap_fingerprints: HashMap::new(),
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

    /// The audio bus an `Area2D` imposes on a point — `Area2D.audio_bus_override`
    /// / `audio_bus_name` (pat-s3rrv.190).
    ///
    /// The 2D twin of [`PhysicsServer3D::audio_bus_at`]. Godot resolves it the
    /// same way it resolves the space overrides: every area containing the point
    /// is a candidate and the highest `priority` wins (`area_2d.cpp`). An
    /// `AudioStreamPlayer2D` standing inside such an area has its voice rerouted
    /// to that bus — the usual use being a reverb bus for a room or a cave.
    ///
    /// ⚠️ A POINT test, not a body-overlap query. An `AudioStreamPlayer2D` is
    /// not a physics body and never appears in `overlapping_bodies`, so the
    /// overlap machinery cannot answer this.
    ///
    /// ⚠️ Uses `AreaStore::point_query` rather than the 3D side's hand-rolled
    /// `shape.contains_point(point - area.position)`. That helper goes
    /// world -> local through `shape_owners()`, so it honours a ROTATED area, an
    /// authored `shape_transform` offset, and an area with several colliders —
    /// all three of which the subtraction misses (pat-s3rrv.25 fixed exactly
    /// that bug for area picking).
    ///
    /// Returns `None` when no overriding area contains the point, which leaves
    /// the voice on whatever bus it already had. A player outside every
    /// overriding area is deliberately NOT forced back to Master: its bus is
    /// whatever the node itself asked for.
    pub fn audio_bus_at(&self, tree: &SceneTree, point: Vector2) -> Option<String> {
        let mut best: Option<(i32, String)> = None;
        for area_id in self.area_store.point_query(point) {
            let Some(node_id) = self.area_to_node.get(&area_id).copied() else {
                continue;
            };
            if !bool_prop(tree, node_id, "audio_bus_override", false) {
                continue;
            }
            let bus = match tree
                .get_node(node_id)
                .map(|n| n.get_property("audio_bus_name"))
            {
                Some(Variant::String(name)) if !name.is_empty() => name,
                // Godot's default bus name. An override with no name still means
                // "route somewhere", and Master is where it routes.
                _ => "Master".to_string(),
            };
            let priority = self
                .area_store
                .get_area(area_id)
                .map(|a| a.priority)
                .unwrap_or(0);
            let better = match &best {
                Some((p, _)) => priority > *p,
                None => true,
            };
            if better {
                best = Some((priority, bus));
            }
        }
        best.map(|(_, bus)| bus)
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

    /// Mutable access to the world, for the `PhysicsServer2D` script API
    /// (pat-0q2kx.200) — a server-created body has to join the *same* world the
    /// scene's nodes are simulated in, or it could never collide with them.
    pub fn world_mut(&mut self) -> &mut PhysicsWorld2D {
        &mut self.world
    }

    // -- PhysicsServer2D singleton API (pat-0q2kx.200) ---------------------
    //
    // Godot scripts can bypass nodes entirely and drive physics through the
    // `PhysicsServer2D` singleton, addressing objects by RID. Patina exposed
    // none of it. These are the script-facing subset the acceptance drives;
    // RIDs are opaque u64 handles, which is exactly what Godot's RID is.

    /// `PhysicsServer2D.circle_shape_create()` — a shape RID with a placeholder
    /// radius until `shape_set_data` supplies one.
    pub fn server_circle_shape_create(&mut self) -> u64 {
        let rid = self.next_server_rid;
        self.next_server_rid += 1;
        self.server_shapes
            .insert(rid, Shape2D::Circle { radius: 1.0 });
        rid
    }

    /// `PhysicsServer2D.rectangle_shape_create()`.
    pub fn server_rectangle_shape_create(&mut self) -> u64 {
        let rid = self.next_server_rid;
        self.next_server_rid += 1;
        self.server_shapes.insert(
            rid,
            Shape2D::Rectangle {
                half_extents: Vector2::new(1.0, 1.0),
            },
        );
        rid
    }

    /// `PhysicsServer2D.shape_set_data(shape, data)`.
    ///
    /// Godot's `data` is shape-specific: a float radius for a circle, a
    /// `Vector2` of *half*-extents for a rectangle. Returns `false` for an
    /// unknown RID rather than panicking, matching the server's tolerance for a
    /// freed handle.
    pub fn server_shape_set_data(
        &mut self,
        shape: u64,
        radius: f32,
        extents: Option<Vector2>,
    ) -> bool {
        match self.server_shapes.get_mut(&shape) {
            Some(Shape2D::Circle { radius: r }) => {
                *r = radius.max(0.0);
                true
            }
            Some(Shape2D::Rectangle { half_extents }) => {
                *half_extents = extents.unwrap_or(Vector2::new(radius, radius));
                true
            }
            // 🔑 A capsule takes `Vector2(radius, height)`, the SAME call shape a
            // rectangle uses for its half-extents. Godot has one
            // `shape_set_data` and disambiguates by the shape the RID already
            // names — so the dispatch must happen here, on the stored shape, not
            // in the interpreter where only the argument's type is visible and
            // the two are indistinguishable (pat-s3rrv.191).
            Some(Shape2D::Capsule {
                radius: r,
                height: h,
            }) => {
                let e = extents.unwrap_or(Vector2::new(radius, radius));
                *r = e.x.max(0.0);
                *h = e.y.max(0.0);
                true
            }
            _ => false,
        }
    }

    /// `PhysicsServer2D.body_create()` — a rigid body not yet in any space.
    ///
    /// Deliberately not added to the world until `body_set_space`: Godot's
    /// spaceless body is inert, and adding it here would make it fall before the
    /// script asked for it.
    pub fn server_body_create(&mut self) -> u64 {
        let rid = self.next_server_rid;
        self.next_server_rid += 1;
        self.server_bodies.insert(rid, None);
        rid
    }

    /// `PhysicsServer2D.body_set_space(body, space)` — a non-zero space admits
    /// the body to this server's world; `0` removes it.
    pub fn server_body_set_space(&mut self, body: u64, space: u64) -> bool {
        let Some(slot) = self.server_bodies.get_mut(&body) else {
            return false;
        };
        match (space, *slot) {
            // Already in the space: nothing to do.
            (s, Some(_)) if s != 0 => true,
            (s, None) if s != 0 => {
                let b = PhysicsBody2D::new(
                    BodyId(0),
                    BodyType::Rigid,
                    Vector2::ZERO,
                    // A body with no shape yet gets a unit circle; `body_add_shape`
                    // replaces it. A zero-size shape would collide with nothing and
                    // look like a physics bug rather than a missing call.
                    Shape2D::Circle { radius: 1.0 },
                    1.0,
                );
                let id = self.world.add_body(b);
                *slot = Some(id);
                true
            }
            (_, Some(id)) => {
                self.world.remove_body(id);
                *slot = None;
                true
            }
            _ => true,
        }
    }

    /// `PhysicsServer2D.body_add_shape(body, shape)` — adopts the shape RID's
    /// geometry. One shape per body here; Godot allows several.
    pub fn server_body_add_shape(&mut self, body: u64, shape: u64) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        let Some(sh) = self.server_shapes.get(&shape).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                b.shape = sh;
                true
            }
            None => false,
        }
    }

    /// `PhysicsServer2D.body_set_state(body, state, value)` for the two states
    /// the script API needs: `TRANSFORM` (0) position and `LINEAR_VELOCITY` (2).
    pub fn server_body_set_state_vec2(&mut self, body: u64, state: i64, value: Vector2) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                // ⚠️ Godot's `BodyState` is TRANSFORM 0, LINEAR_VELOCITY 1,
                // ANGULAR_VELOCITY 2, SLEEPING 3, CAN_SLEEP 4
                // (`servers/physics_2d/physics_server_2d.h`). This mapped
                // LINEAR_VELOCITY to **2**, which is ANGULAR_VELOCITY: a script
                // passing Godot's real value (1) got `false` and no effect,
                // while passing 2 set the linear velocity it did not ask for.
                // The getter carried the same wrong table in a comment
                // (pat-s3rrv.191).
                match state {
                    0 => b.position = value,
                    1 => b.linear_velocity = value,
                    _ => return false,
                }
                true
            }
            None => false,
        }
    }

    /// `PhysicsServer2D.area_create()` — an area not yet in any space
    /// (pat-s3rrv.191).
    pub fn server_area_create(&mut self) -> u64 {
        let rid = self.next_server_rid;
        self.next_server_rid += 1;
        self.server_areas.insert(rid, None);
        rid
    }

    /// `PhysicsServer2D.area_set_space(area, space)` — a non-zero space admits
    /// the area to this server's `AreaStore`; `0` removes it.
    ///
    /// 🔑 `Area2D::new` already defaults `monitoring = true` and
    /// `collision_mask = 1`, which matters: `overlapping_areas_sorted` skips any
    /// area with `monitoring == false` OR whose mask misses the body's layer, so
    /// an area created with either unset would be registered, queryable, and
    /// completely inert — the exact "wired but does nothing" shape this bead
    /// exists to remove.
    pub fn server_area_set_space(&mut self, area: u64, space: u64) -> bool {
        let Some(slot) = self.server_areas.get(&area).copied() else {
            return false;
        };
        match (space, slot) {
            (s, Some(_)) if s != 0 => true,
            (s, None) if s != 0 => {
                let a = Area2D::new(
                    AreaId(0),
                    Vector2::ZERO,
                    // A unit circle until `area_add_shape` replaces it, matching
                    // how `body_set_space` seeds a shapeless body.
                    Shape2D::Circle { radius: 1.0 },
                );
                let id = self.area_store.add_area(a);
                self.server_areas.insert(area, Some(id));
                true
            }
            (0, Some(id)) => {
                let _ = self.area_store.remove_area(id);
                self.server_areas.insert(area, None);
                true
            }
            _ => true,
        }
    }

    /// `PhysicsServer2D.area_add_shape(area, shape)`.
    pub fn server_area_add_shape(&mut self, area: u64, shape: u64) -> bool {
        let Some(Some(id)) = self.server_areas.get(&area).copied() else {
            return false;
        };
        let Some(sh) = self.server_shapes.get(&shape).copied() else {
            return false;
        };
        match self.area_store.get_area_mut(id) {
            Some(a) => {
                a.shape = sh;
                true
            }
            None => false,
        }
    }

    /// `PhysicsServer2D.area_set_transform(area, xform)` — origin only.
    pub fn server_area_set_position(&mut self, area: u64, position: Vector2) -> bool {
        let Some(Some(id)) = self.server_areas.get(&area).copied() else {
            return false;
        };
        match self.area_store.get_area_mut(id) {
            Some(a) => {
                a.position = position;
                true
            }
            None => false,
        }
    }

    /// `PhysicsServer2D.area_set_param(area, param, value)`.
    ///
    /// Godot's `AreaParameter`: GRAVITY_OVERRIDE_MODE 0, GRAVITY 1,
    /// GRAVITY_VECTOR 2, GRAVITY_IS_POINT 3, GRAVITY_POINT_UNIT_DISTANCE 4,
    /// LINEAR_DAMP_OVERRIDE_MODE 5, LINEAR_DAMP 6, ANGULAR_DAMP_OVERRIDE_MODE 7,
    /// ANGULAR_DAMP 8, PRIORITY 9 — transcribed from
    /// `servers/physics_2d/physics_server_2d.h`.
    ///
    /// ⚠️ Setting GRAVITY alone changes nothing until the override MODE leaves
    /// `Disabled`, which is `Area2D`'s default and Godot's. That is upstream
    /// behaviour, not a gap: an area that merely names a gravity is not yet
    /// overriding anything.
    pub fn server_area_set_param(
        &mut self,
        area: u64,
        param: i64,
        scalar: f32,
        vector: Vector2,
    ) -> bool {
        let Some(Some(id)) = self.server_areas.get(&area).copied() else {
            return false;
        };
        let Some(a) = self.area_store.get_area_mut(id) else {
            return false;
        };
        match param {
            0 => {
                a.gravity_space_override = match scalar as i64 {
                    1 => AreaSpaceOverride::Combine,
                    2 => AreaSpaceOverride::CombineReplace,
                    3 => AreaSpaceOverride::Replace,
                    4 => AreaSpaceOverride::ReplaceCombine,
                    _ => AreaSpaceOverride::Disabled,
                }
            }
            1 => a.gravity = scalar,
            2 => a.gravity_direction = vector,
            3 => a.gravity_is_point = scalar != 0.0,
            4 => a.gravity_point_unit_distance = scalar,
            9 => a.priority = scalar as i32,
            _ => return false,
        }
        true
    }

    /// `PhysicsServer2D.area_set_monitorable(area, monitorable)`.
    pub fn server_area_set_monitorable(&mut self, area: u64, monitorable: bool) -> bool {
        let Some(Some(id)) = self.server_areas.get(&area).copied() else {
            return false;
        };
        match self.area_store.get_area_mut(id) {
            Some(a) => {
                a.monitorable = monitorable;
                true
            }
            None => false,
        }
    }

    /// `PhysicsServer2D.capsule_shape_create()` (pat-s3rrv.191).
    pub fn server_capsule_shape_create(&mut self) -> u64 {
        let rid = self.next_server_rid;
        self.next_server_rid += 1;
        self.server_shapes.insert(
            rid,
            Shape2D::Capsule {
                radius: 10.0,
                height: 40.0,
            },
        );
        rid
    }

    /// Sets a capsule shape's `(radius, height)`.
    pub fn server_shape_set_data_capsule(&mut self, shape: u64, radius: f32, height: f32) -> bool {
        match self.server_shapes.get_mut(&shape) {
            Some(Shape2D::Capsule {
                radius: r,
                height: h,
            }) => {
                *r = radius.max(0.0);
                *h = height.max(0.0);
                true
            }
            _ => false,
        }
    }

    /// `PhysicsServer2D.body_set_param(body, param, value)`.
    ///
    /// Godot's `BodyParameter`: BOUNCE 0, FRICTION 1, MASS 2, INERTIA 3,
    /// CENTER_OF_MASS 4, GRAVITY_SCALE 5, LINEAR_DAMP_MODE 6,
    /// ANGULAR_DAMP_MODE 7, LINEAR_DAMP 8, ANGULAR_DAMP 9 — transcribed from
    /// `servers/physics_2d/physics_server_2d.h`, not derived.
    ///
    /// ⚠️ INERTIA and the two DAMP_MODE params are DECLINED, not accepted and
    /// dropped: `PhysicsBody2D` has no field for them, and a setter that reports
    /// success while changing nothing is worse than one that reports failure —
    /// the caller cannot tell the difference until the simulation misbehaves.
    pub fn server_body_set_param(&mut self, body: u64, param: i64, value: f32) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        let Some(b) = self.world.get_body_mut(id) else {
            return false;
        };
        match param {
            0 => b.bounce = value,
            1 => b.friction = value,
            2 => b.mass = value.max(0.0),
            5 => b.gravity_scale = value,
            8 => b.linear_damp = value,
            9 => b.angular_damp = value,
            _ => return false,
        }
        true
    }

    /// `PhysicsServer2D.body_get_param(body, param)`.
    pub fn server_body_get_param(&self, body: u64, param: i64) -> Option<f32> {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return None;
        };
        let b = self.world.get_body(id)?;
        match param {
            0 => Some(b.bounce),
            1 => Some(b.friction),
            2 => Some(b.mass),
            5 => Some(b.gravity_scale),
            8 => Some(b.linear_damp),
            9 => Some(b.angular_damp),
            _ => None,
        }
    }

    /// `body_set_collision_layer` / `body_set_collision_mask`.
    pub fn server_body_set_collision_layers(
        &mut self,
        body: u64,
        layers: u32,
        is_mask: bool,
    ) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                if is_mask {
                    b.collision_mask = layers;
                } else {
                    b.collision_layer = layers;
                }
                true
            }
            None => false,
        }
    }

    /// `body_apply_impulse` / `body_apply_central_impulse` (server form).
    ///
    /// 🔑 An impulse changes velocity instantaneously; a force accumulates over
    /// the step. They are separate methods upstream for that reason, and
    /// `PhysicsBody2D` already distinguishes them — routing both here would be
    /// wrong by the mass factor and invisible to a test that only asks "did it
    /// move".
    pub fn server_body_apply_impulse(
        &mut self,
        body: u64,
        impulse: Vector2,
        at_point: Option<Vector2>,
    ) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                match at_point {
                    Some(p) => b.apply_impulse_at_point(impulse, p),
                    None => b.apply_central_impulse(impulse),
                }
                true
            }
            None => false,
        }
    }

    /// `body_apply_force` / `body_apply_central_force` (server form).
    pub fn server_body_apply_force(
        &mut self,
        body: u64,
        force: Vector2,
        _at_point: Option<Vector2>,
    ) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                b.apply_force(force);
                true
            }
            None => false,
        }
    }

    /// `body_apply_torque_impulse` (server form).
    pub fn server_body_apply_torque_impulse(&mut self, body: u64, torque: f32) -> bool {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return false;
        };
        match self.world.get_body_mut(id) {
            Some(b) => {
                b.apply_torque_impulse(torque);
                true
            }
            None => false,
        }
    }

    /// `body_get_state(body, LINEAR_VELOCITY)`.
    pub fn server_body_linear_velocity(&self, body: u64) -> Option<Vector2> {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return None;
        };
        self.world.get_body(id).map(|b| b.linear_velocity)
    }

    /// `body_get_state(body, ANGULAR_VELOCITY)`.
    pub fn server_body_angular_velocity(&self, body: u64) -> Option<f32> {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return None;
        };
        self.world.get_body(id).map(|b| b.angular_velocity)
    }

    /// `body_get_state(body, SLEEPING | CAN_SLEEP)`.
    pub fn server_body_sleep_state(&self, body: u64, can_sleep: bool) -> Option<bool> {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return None;
        };
        self.world
            .get_body(id)
            .map(|b| if can_sleep { b.can_sleep } else { b.sleeping })
    }

    /// The world-space position of a server-created body, for
    /// `body_get_state(body, TRANSFORM)`.
    pub fn server_body_position(&self, body: u64) -> Option<Vector2> {
        let Some(Some(id)) = self.server_bodies.get(&body).copied() else {
            return None;
        };
        self.world.get_body(id).map(|b| b.position)
    }

    /// The [`BodyId`] behind a server RID, so a test can cross-check that a
    /// script-created body really is in the same world as the node-created ones.
    pub fn server_body_id(&self, body: u64) -> Option<BodyId> {
        self.server_bodies.get(&body).copied().flatten()
    }

    /// `PhysicsServer2D.body_test_motion(body, params)` → `(collided, travel)`
    /// (pat-0q2kx.200).
    ///
    /// Sweeps the body's own shape from its current transform along `motion`
    /// against every *other* body in the world, via the shared
    /// `character::test_motion` helper `move_and_slide` substeps with — so a fast
    /// test cannot tunnel through a thin wall and report "clear".
    pub fn server_body_test_motion(&self, body: u64, motion: Vector2) -> (bool, Vector2) {
        let Some(id) = self.server_body_id(body) else {
            return (false, motion);
        };
        let Some(subject) = self.world.get_body(id) else {
            return (false, motion);
        };
        let others: Vec<&gdphysics2d::body::PhysicsBody2D> =
            self.world.bodies().filter(|b| b.id != id).collect();
        gdphysics2d::character::test_motion(
            subject.position,
            subject.rotation,
            &subject.shape,
            motion,
            &others,
        )
    }

    /// `PhysicsBody2D.test_move(transform, motion)` for a *node* — the same
    /// sweep, addressed by scene node instead of RID (pat-0q2kx.200).
    ///
    /// `true` when the motion would be blocked, which is Godot's return.
    pub fn node_test_move(&self, node_id: NodeId, motion: Vector2) -> bool {
        let Some(id) = self.body_for_node(node_id) else {
            return false;
        };
        let Some(subject) = self.world.get_body(id) else {
            return false;
        };
        let others: Vec<&gdphysics2d::body::PhysicsBody2D> =
            self.world.bodies().filter(|b| b.id != id).collect();
        gdphysics2d::character::test_motion(
            subject.position,
            subject.rotation,
            &subject.shape,
            motion,
            &others,
        )
        .0
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

    /// Every registered collider node whose shape contains `world_point`
    /// (pat-0q2kx.138) — Godot's `intersect_point` with
    /// `collide_with_areas = true`, resolved back to scene nodes.
    ///
    /// Bodies and areas both, because physics picking targets either and an
    /// `Area2D` is the usual click target. Bodies come first, then areas, each
    /// group in id order, so the result is deterministic.
    /// Like [`colliders_at_point`](Self::colliders_at_point) but pairs each node
    /// with the **index of the collider that was struck** (pat-s3rrv.25).
    ///
    /// That index is Godot's `shape_idx`, the third argument of
    /// `CollisionObject2D._input_event` and the argument of
    /// `_mouse_shape_enter`/`_mouse_shape_exit`. Picking used to send a constant
    /// `0`, which was correct only while a body could own one shape; multi-shape
    /// bodies made it silently wrong for every hit on a second collider.
    pub fn colliders_at_point_with_shape(&self, world_point: Vector2) -> Vec<(NodeId, usize)> {
        let bodies = self
            .world
            .point_query_with_shape(world_point)
            .into_iter()
            .filter_map(|(id, idx)| self.node_for_body(id).map(|n| (n, idx)));
        let areas = self
            .area_store
            .point_query_with_shape(world_point)
            .into_iter()
            .filter_map(|(id, idx)| self.area_to_node.get(&id).copied().map(|n| (n, idx)));
        bodies.chain(areas).collect()
    }

    pub fn colliders_at_point(&self, world_point: Vector2) -> Vec<NodeId> {
        let bodies = self
            .world
            .point_query(world_point)
            .into_iter()
            .filter_map(|id| self.node_for_body(id));
        let areas = self
            .area_store
            .point_query(world_point)
            .into_iter()
            .filter_map(|id| self.area_to_node.get(&id).copied());
        bodies.chain(areas).collect()
    }

    /// Applies the space-wide `physics/2d/*` project settings — default damping,
    /// sleep thresholds and `time_before_sleep` (pat-0q2kx.116).
    ///
    /// Godot's space reads these at construction and its bodies consult them
    /// every step (`godot_space_2d.cpp:1219-1222`); here the world stamps them
    /// onto its bodies instead, which is the same thing from the body's side.
    pub fn set_space_params(&mut self, params: gdphysics2d::world::SpaceParams2D) {
        self.world.set_space_params(params);
    }

    /// Sets the contact solver's `physics/2d/solver/*` tunables.
    pub fn set_solver_params(&mut self, iterations: usize, contact_bias: f32) {
        let solver = self.world.contact_solver_mut();
        // Godot clamps the iteration count to at least one; a zero would turn
        // every contact into a no-op rather than a fast one.
        solver.iterations = iterations.max(1);
        solver.contact_bias = contact_bias;
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
                // Godot's physics server always sees a CollisionObject2D's
                // GLOBAL transform (collision_object_2d.cpp). Reading the LOCAL
                // `position` here made a body under an offset parent register at
                // the wrong world spot — a RigidBody2D under `Node2D
                // position=(500,300)` fell from the origin (pat-0q2kx.31).
                let pos = crate::node2d::get_global_position(tree, node_id);
                let base_shape = find_collision_shape(tree, node_id).unwrap_or_else(|| {
                    if has_collision_shape_child(tree, node_id) {
                        // Shape children exist but none is usable — every one is
                        // `disabled`, or the sub-resource did not resolve. Godot
                        // leaves such a body with no shapes, so a degenerate rect
                        // collides with nothing; the legacy default circle would
                        // silently stand in for the authored geometry
                        // (pat-0q2kx.28).
                        Shape2D::Rectangle {
                            half_extents: Vector2::ZERO,
                        }
                    } else {
                        Shape2D::Circle { radius: 16.0 }
                    }
                });
                // Accumulated scale, not just the body's own: a collider under a
                // scaled parent must collide at the size it visually occupies.
                let node_scale = crate::node2d::get_global_scale(tree, node_id);
                let shape = scale_shape(base_shape, node_scale);

                let mass = float_prop(tree, node_id, "mass", 1.0) as f32;
                let mut body = PhysicsBody2D::new(BodyId(0), body_type, pos, shape, mass);
                // The collider child's own offset/rotation relative to the body
                // (pat-s3rrv.25). The origin is scaled by the same accumulated
                // scale already baked into the geometry above: a body scaled 2x
                // carries an offset collider twice as far from its origin, and
                // scaling the shape without scaling the offset would pull the
                // collider back toward the body as the body grew.
                //
                // ⚠️ Read once, at registration. A `CollisionShape2D` that MOVES
                // at runtime (a crouch animation retargeting the collider) still
                // keeps its original offset — `register_bodies` skips bodies it
                // has already registered, so nothing re-reads this. Making it
                // live means re-walking the collider children inside
                // `sync_to_physics` every tick; that is a real follow-up, not
                // something to slip in unmeasured.
                if let Some(mut shape_tf) = find_collision_shape_transform(tree, node_id) {
                    shape_tf.origin = Vector2::new(
                        shape_tf.origin.x * node_scale.x,
                        shape_tf.origin.y * node_scale.y,
                    );
                    body.shape_transform = shape_tf;
                }
                // Colliders beyond the first. Collected now so the one part of
                // the multi-shape change with a subtle ordering requirement is
                // already done and tested; the narrow phase does not consult
                // them yet (pat-s3rrv.25).
                body.extra_shapes = extra_collision_shapes(tree, node_id, node_scale);
                body.collision_layer = u32_prop(tree, node_id, "collision_layer", 1);
                body.collision_mask = u32_prop(tree, node_id, "collision_mask", 1);
                body.bounce = float_prop(tree, node_id, "bounce", 0.0) as f32;
                body.friction = float_prop(tree, node_id, "friction", 0.5) as f32;
                // Godot has no flat `bounce`/`friction` on a body — they live on
                // a `PhysicsMaterial` in `physics_material_override`, which
                // `PackedScene` flattens onto the node as `__physics_material__*`
                // (pat-0q2kx.115). The override wins when present; the flat
                // props above stay as the fallback so hand-built scenes and the
                // existing tests keep working.
                if let Some(f) = opt_float_prop(tree, node_id, "__physics_material__friction") {
                    body.friction = f as f32;
                }
                if let Some(b) = opt_float_prop(tree, node_id, "__physics_material__bounce") {
                    body.bounce = b as f32;
                }
                // `rough` / `absorbent` are Godot's COMBINE-MODE flags, and it
                // signals them by storing the value NEGATED:
                // `computed_friction() { return rough ? -friction : friction; }`
                // and the same for `absorbent`/`bounce`
                // (`physics_material.h:42-67`). The pair solver then reads the
                // sign back out through its combine rules —
                // `friction = abs(MIN(a, b))` and `bounce = CLAMP(a + b, 0, 1)`
                // (`godot_body_pair_2d.cpp:242-248`), both already implemented
                // in `solver::combine_friction` / `combine_bounce`.
                //
                // 🔑 So the modes need NO branch here or in the solver: the sign
                // IS the mode. `abs(MIN(..))` makes a rough surface's friction
                // WIN the pair instead of the usual "slipperiest side wins",
                // and a negative bounce SUBTRACTS under the clamped sum, which
                // is how an absorbent surface deadens a bouncy one.
                //
                // ⚠️ NOT multiplication. Godot 4.6 has no multiplicative branch
                // in either combine function — the bead's own prose says 0.5 and
                // 0.5 combine to 0.25 under `rough`, and the vendored source
                // says otherwise. Verified against the checked-in tree.
                if bool_prop(tree, node_id, "__physics_material__rough", false) {
                    body.friction = -body.friction.abs();
                }
                if bool_prop(tree, node_id, "__physics_material__absorbent", false) {
                    body.bounce = -body.bounce.abs();
                }
                // The rest of Godot's RigidBody2D surface. `PhysicsBody2D`
                // already models and honors these — `integrate` skips a frozen
                // or sleeping body outright and zeroes angular motion under
                // `lock_rotation` (gdphysics2d/src/body.rs:307-341) — but the
                // bridge never read them off the node, so an authored
                // `freeze = true` did nothing at runtime.
                body.freeze = bool_prop(tree, node_id, "freeze", false);
                body.lock_rotation = bool_prop(tree, node_id, "lock_rotation", false);
                body.can_sleep = bool_prop(tree, node_id, "can_sleep", true);
                body.sleeping = bool_prop(tree, node_id, "sleeping", false);
                if let Some(com) = opt_vec2_prop(tree, node_id, "center_of_mass") {
                    body.center_of_mass = com;
                }
                body.linear_velocity = vec2_prop(tree, node_id, "linear_velocity", Vector2::ZERO);
                body.rotation = crate::node2d::get_global_rotation(tree, node_id);
                body.angular_velocity = float_prop(tree, node_id, "angular_velocity", 0.0) as f32;
                // Godot RigidBody2D gravity/damping properties.
                body.gravity_scale = float_prop(tree, node_id, "gravity_scale", 1.0) as f32;
                // `RigidBody2D.continuous_cd` (pat-s3rrv.100). Godot's CCDMode
                // enum: 0 = DISABLED, 1 = CAST_RAY, 2 = CAST_SHAPE — the solver
                // sweeps the same way for either non-disabled mode, so anything
                // non-zero enables it.
                //
                // ⚠️ Until this read existed the property was a FALSE GREEN: it
                // is registered in ClassDB, so a .tscn could author it and the
                // inspector would show it, while `gdphysics2d::ccd` sat there
                // with zero callers and the bullet went through the wall.
                body.continuous_cd = u32_prop(tree, node_id, "continuous_cd", 0) != 0;
                body.linear_damp = float_prop(tree, node_id, "linear_damp", 0.0) as f32;
                body.angular_damp = float_prop(tree, node_id, "angular_damp", 0.0) as f32;
                // StaticBody2D's conveyor surface (pat-s3rrv.101). Godot
                // defaults both to zero (`doc/classes/StaticBody2D.xml`), and a
                // zero surface velocity is exactly "no conveyor", so the plain
                // defaults are safe for every other body class.
                //
                // ⚠️ These are read for ALL body types, not just Static: Godot
                // puts them on `StaticBody2D`, which `AnimatableBody2D`
                // inherits, and this bridge maps the latter to
                // `BodyType::Kinematic`. Gating on `BodyType::Static` would
                // silently drop an authored conveyor on an animated platform.
                body.constant_linear_velocity =
                    vec2_prop(tree, node_id, "constant_linear_velocity", Vector2::ZERO);
                body.constant_angular_velocity =
                    float_prop(tree, node_id, "constant_angular_velocity", 0.0) as f32;
                // One-way collision lives on the shape node in Godot but is solved
                // per body here, so lift it (pat-0q2kx.28). The default direction
                // (0,-1) already means "blocks from above", which is what an
                // unrotated one-way platform does.
                if let Some((one_way, margin)) = one_way_collision_for(tree, node_id) {
                    body.one_way_collision = one_way;
                    // The margin was previously read and discarded here, which
                    // left `one_way_collision_margin` inert for the PRIMARY
                    // shape (extras get theirs in `extra_collision_shapes`).
                    body.one_way_margin = margin;
                }

                let body_id = self.world.add_body(body);
                self.node_to_body.insert(node_id, body_id);
                self.body_to_node.insert(body_id, node_id);
            }

            // Register areas (Area2D).
            if class == "Area2D" && !self.node_to_area.contains_key(&node_id) {
                // Global, like bodies — an Area2D under an offset parent was
                // monitoring the wrong region of the world (pat-0q2kx.31).
                // `gravity_point_center` below stays `pos + <node-relative
                // offset>`, which is now correctly world-space.
                let pos = crate::node2d::get_global_position(tree, node_id);
                // Same three-way fallback as the body path (pat-0q2kx.28, applied
                // to areas by pat-s3rrv.25): a usable shape wins; shape children
                // that exist but are all `disabled` mean NO collision, per Godot,
                // which the degenerate rect expresses; only a body authored with
                // no shape children at all keeps the legacy default circle.
                //
                // Areas used the circle for both of the last two cases, so an
                // `Area2D` whose only collider was disabled still monitored a
                // 16px blob. It also disagreed with the per-tick sync below,
                // which does use the three-way form — registration and sync
                // resolving the same scene differently is worse than either
                // answer alone.
                let base_shape = match find_collision_shape(tree, node_id) {
                    Some(s) => s,
                    None if has_collision_shape_child(tree, node_id) => Shape2D::Rectangle {
                        half_extents: Vector2::ZERO,
                    },
                    None => Shape2D::Circle { radius: 16.0 },
                };
                // Accumulated scale, exactly as the body path does it
                // (pat-s3rrv.25). The area path used its shape RAW, so an
                // `Area2D` under a scaled parent monitored the region it would
                // have covered at scale 1 — a trigger drawn twice as wide in the
                // editor fired at its authored size. Bodies had been scaling
                // since pat-0q2kx.31; areas simply never got the same treatment,
                // which is why a body and an area drawn the same size under the
                // same scaled parent disagreed about their own extent.
                let node_scale = crate::node2d::get_global_scale(tree, node_id);
                let shape = scale_shape(base_shape, node_scale);

                let mut area = Area2D::new(AreaId(0), pos, shape);
                // The collider child's own offset/rotation (pat-s3rrv.25), in
                // the same scaled space the geometry above was baked into.
                //
                // ⚠️ These two lines are ONE change. The offset was correctly
                // taken *unscaled* while the area's geometry was unscaled; now
                // that `scale_shape` runs above, an unscaled offset would leave
                // the collider behind as the area grew. Scaling geometry and
                // offset are not independent decisions — changing either alone
                // reintroduces the mismatch.
                if let Some(mut shape_tf) = find_collision_shape_transform(tree, node_id) {
                    shape_tf.origin = Vector2::new(
                        shape_tf.origin.x * node_scale.x,
                        shape_tf.origin.y * node_scale.y,
                    );
                    area.shape_transform = shape_tf;
                }
                // Colliders beyond the first, same as bodies (pat-s3rrv.25).
                area.extra_shapes = extra_collision_shapes(tree, node_id, node_scale);
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
                // The rotational twin of the linear override (pat-0q2kx.112).
                area.angular_damp_space_override = space_override_from_int(u32_prop(
                    tree,
                    node_id,
                    "angular_damp_space_override",
                    0,
                ));
                area.angular_damp = float_prop(tree, node_id, "angular_damp", 1.0) as f32;
                area.monitorable = bool_prop(tree, node_id, "monitorable", true);
                area.rotation = crate::node2d::get_global_rotation(tree, node_id);
                area.priority = i32_prop(tree, node_id, "priority", 0);
                let area_id = self.area_store.add_area(area);
                self.node_to_area.insert(node_id, area_id);
                self.area_to_node.insert(area_id, node_id);
            }
        }

        // Painted tiles become static bodies of their own (pat-0q2kx.117).
        self.register_tilemap_bodies(tree, &all_ids);

        // Joints come last: they resolve `node_a`/`node_b` to BodyIds, so every
        // body has to be in `node_to_body` before this runs (pat-0q2kx.114).
        self.register_joints(tree, &all_ids);
    }

    /// Makes the physics world track the scene tree: registers nodes that have
    /// appeared and frees the ones that have left (pat-s3rrv.25).
    ///
    /// Godot does this per node — `CollisionObject2D::_notification` calls
    /// `PhysicsServer2D::body_set_space`/`area_set_space` on
    /// `NOTIFICATION_ENTER_TREE` and clears the space on `NOTIFICATION_EXIT_TREE`
    /// (`scene/2d/physics/collision_object_2d.cpp`), so a body instanced
    /// mid-game simulates on the very next physics tick and a freed one stops
    /// colliding immediately.
    ///
    /// Patina built 2D bodies exactly once. `register_bodies` was reachable only
    /// through `MainLoop::register_physics_bodies`, which the runner calls a
    /// single time before the frame loop starts, and nothing ever removed a
    /// scene-node body — `world.remove_body` was called only from the
    /// `PhysicsServer2D` RID-free path and tilemap regeneration. So
    /// `add_child(preload("res://Bullet.tscn").instantiate())` produced a
    /// RigidBody2D that never fell, never collided and emitted no signals, and
    /// `queue_free()` on a body left a permanent ghost collider. The 3D server
    /// already re-scanned the tree every tick inside its own `sync_to_physics`
    /// (`physics_server_3d.rs`); this is the 2D half of that.
    ///
    /// Cheap to re-run: the body, area and joint passes all skip nodes already
    /// in their maps, and the tilemap pass is fingerprinted.
    pub fn sync_registrations(&mut self, tree: &SceneTree) {
        self.register_bodies(tree);
        self.prune_departed_nodes(tree);
    }

    /// Frees every registration whose scene node has left the tree.
    ///
    /// Kept separate from `register_bodies` so that function stays a pure "add
    /// what is missing" pass for its existing callers.
    fn prune_departed_nodes(&mut self, tree: &SceneTree) {
        let live: HashSet<NodeId> = tree.all_nodes_in_tree_order().into_iter().collect();

        let stale_bodies: Vec<NodeId> = self
            .node_to_body
            .keys()
            .filter(|nid| !live.contains(nid))
            .copied()
            .collect();
        let mut removed_bodies: HashSet<BodyId> = HashSet::new();
        for nid in stale_bodies {
            if let Some(body_id) = self.node_to_body.remove(&nid) {
                self.body_to_node.remove(&body_id);
                let _ = self.world.remove_body(body_id);
                removed_bodies.insert(body_id);
            }
        }

        // Removing a body must WAKE whatever was resting on it. Godot's space
        // destroys a departing body's contacts, and a destroyed contact wakes
        // both islands; `World2D::remove_body` only drops the body out of the
        // map, so its neighbours keep whatever sleep state they had.
        //
        // That is not a detail — it is the difference between the fix working
        // and only appearing to. A body that has been resting for more than
        // `time_before_sleep` (0.5s) is asleep and does not integrate, so
        // freeing the platform under it leaves it hanging in mid-air on a
        // support that no longer exists. The registration is already correct at
        // that point and the scene still looks broken. This is exactly how
        // `physics2d_freed_body_stops_colliding` first failed: the body sat at
        // y=185.3 before the free and at y=185.3 after it, to the decimal.
        //
        // `last_collision_events` holds the previous step's contacts, which for
        // a body at rest is the persistent pair being dissolved here. Waking is
        // safe to over-apply — Godot wakes liberally, and a body with nothing
        // to do simply falls asleep again after its countdown.
        if !removed_bodies.is_empty() {
            let to_wake: Vec<BodyId> = self
                .last_collision_events
                .iter()
                .filter_map(|event| {
                    if removed_bodies.contains(&event.body_a) {
                        Some(event.body_b)
                    } else if removed_bodies.contains(&event.body_b) {
                        Some(event.body_a)
                    } else {
                        None
                    }
                })
                .collect();
            for body_id in to_wake {
                if let Some(body) = self.world.get_body_mut(body_id) {
                    body.wake();
                }
            }
        }

        let stale_areas: Vec<NodeId> = self
            .node_to_area
            .keys()
            .filter(|nid| !live.contains(nid))
            .copied()
            .collect();
        for nid in stale_areas {
            if let Some(area_id) = self.node_to_area.remove(&nid) {
                self.area_to_node.remove(&area_id);
                let _ = self.area_store.remove_area(area_id);
            }
        }

        // A joint goes when its own node leaves, and also when either body it
        // constrains does: `World2D::remove_body` only drops the body, so a
        // joint left behind would keep solving against a `BodyId` that no
        // longer resolves. Dropping the `node_to_joint` entry too lets
        // `register_joints` rebuild the joint if its endpoints come back.
        // Collected up front rather than filtered in place: the predicate reads
        // `self.world`, and a closure holding that borrow while
        // `self.node_to_joint` is iterated is the same shape that already bit
        // `register_joints` (see its `target_a`/`target_b` note).
        let joint_entries: Vec<(NodeId, gdphysics2d::joint::JointId)> = self
            .node_to_joint
            .iter()
            .map(|(&nid, &jid)| (nid, jid))
            .collect();
        let mut stale_joints: Vec<NodeId> = Vec::new();
        for (nid, joint_id) in joint_entries {
            let dead = if !live.contains(&nid) {
                true
            } else {
                match self.world.get_joint(joint_id) {
                    Some(joint) => {
                        let base = joint.base();
                        self.world.get_body(base.body_a).is_none()
                            || self.world.get_body(base.body_b).is_none()
                    }
                    None => true,
                }
            };
            if dead {
                stale_joints.push(nid);
            }
        }
        for nid in stale_joints {
            if let Some(joint_id) = self.node_to_joint.remove(&nid) {
                let _ = self.world.remove_joint(joint_id);
            }
        }

        // Tile bodies have no scene node of their own — they belong to the
        // `TileMapLayer`/`TileMap` that painted them, so they leave with it.
        // `register_tilemap_bodies` walks the nodes that are present, so a
        // departed map's bodies would otherwise sit in the world forever.
        let stale_maps: Vec<NodeId> = self
            .tilemap_bodies
            .keys()
            .filter(|nid| !live.contains(nid))
            .copied()
            .collect();
        for nid in stale_maps {
            if let Some(body_ids) = self.tilemap_bodies.remove(&nid) {
                for body_id in body_ids {
                    let _ = self.world.remove_body(body_id);
                }
            }
            self.tilemap_fingerprints.remove(&nid);
        }
    }

    /// Builds static collision bodies from `TileMapLayer`/`TileMap` tiles
    /// (pat-0q2kx.117).
    ///
    /// Godot's `TileMapLayer::_physics_update` creates a static body per painted
    /// cell out of the tile's physics layers, which is what makes a character
    /// walk on painted ground. `gdphysics2d::tilemap_collision` did the
    /// conversion and was unit-tested, but nothing called it — `tilemap_to_physics`
    /// had zero callers outside the crate's own re-export, so a `.tscn` tilemap
    /// contributed no collision at all and characters fell straight through.
    ///
    /// Re-runs cheaply: the painted cells and their tile set are fingerprinted,
    /// and the tile bodies are rebuilt only when that fingerprint changes — so
    /// `set_cell`/`erase_cell` add and remove collision, while an untouched map
    /// keeps its bodies (and their ids) across frames.
    /// Re-registers tilemap collision from the CURRENT `tile_map_data`
    /// (pat-s3rrv.88).
    ///
    /// [`Self::register_tilemap_bodies`] only ever ran from
    /// [`Self::register_bodies`], an explicit start-up call, so a cell painted
    /// from a **script** reached pixels — the render path decodes
    /// `tile_map_data` afresh every frame — but never reached collision. The
    /// per-node fingerprint check inside makes this a no-op on frames where no
    /// layer changed, so it is cheap enough to run every tick.
    pub fn refresh_tilemap_bodies(&mut self, tree: &SceneTree) {
        let all_ids = tree.all_nodes_in_tree_order();
        self.register_tilemap_bodies(tree, &all_ids);
    }

    fn register_tilemap_bodies(&mut self, tree: &SceneTree, all_ids: &[NodeId]) {
        for &node_id in all_ids {
            let Some(node) = tree.get_node(node_id) else {
                continue;
            };
            if !matches!(node.class_name(), "TileMapLayer" | "TileMap") {
                continue;
            }

            let physics_sources = match node.get_property(crate::tilemap::TILE_PHYSICS_SOURCES) {
                Variant::PackedInt32Array(ids) => ids,
                _ => Vec::new(),
            };
            let raw_cells = match node.get_property("tile_map_data") {
                Variant::PackedByteArray(bytes) => bytes,
                _ => Vec::new(),
            };
            let tile_size = match node.get_property("tile_size") {
                Variant::Vector2i(v) => v,
                Variant::Vector2(v) => Vector2i::new(v.x as i32, v.y as i32),
                // Godot's TileSet default.
                _ => Vector2i::new(16, 16),
            };
            let origin = crate::node2d::get_global_position(tree, node_id);

            // Fingerprint everything that changes the generated bodies. Cheap
            // enough to compute every registration pass and exact — comparing
            // body counts would miss a same-size edit (one cell erased, another
            // painted).
            let fingerprint = TileMapFingerprint {
                cells: raw_cells.clone(),
                sources: physics_sources.clone(),
                tile_size,
                origin: (origin.x.to_bits(), origin.y.to_bits()),
            };
            if self.tilemap_fingerprints.get(&node_id) == Some(&fingerprint) {
                continue;
            }

            // Drop the previous generation before building the new one, so an
            // erased cell actually stops colliding.
            if let Some(old) = self.tilemap_bodies.remove(&node_id) {
                for body_id in old {
                    let _ = self.world.remove_body(body_id);
                }
            }

            let cells = crate::tilemap::decode_tile_map_data(&raw_cells);
            let infos = crate::tilemap::tile_collision_infos(
                &cells,
                tile_size,
                &physics_sources,
                |source_id, atlas| {
                    let points = match node
                        .get_property(&crate::tilemap::tile_physics_points_key(source_id, atlas))
                    {
                        Variant::PackedVector2Array(p) => p,
                        _ => Vec::new(),
                    };
                    let one_way = matches!(
                        node.get_property(&crate::tilemap::tile_physics_one_way_key(
                            source_id, atlas
                        )),
                        Variant::Bool(true)
                    );
                    (points, one_way)
                },
            );

            let result = gdphysics2d::tilemap_collision::tilemap_to_physics(
                &mut self.world,
                &infos,
                tile_size,
                origin,
            );
            self.tilemap_bodies.insert(node_id, result.body_ids);
            self.tilemap_fingerprints.insert(node_id, fingerprint);
        }
    }

    /// Builds world joints from `Joint2D` scene nodes (pat-0q2kx.114).
    ///
    /// `gdphysics2d::joint` solved all three kinds and `world.add_joint`
    /// registered them, but every caller was one of that crate's own tests — no
    /// scene path ever created a joint, so an authored `PinJoint2D` constrained
    /// nothing.
    ///
    /// Godot resolves `node_a`/`node_b` as NodePaths **relative to the joint**
    /// and anchors the joint at its own global position
    /// (upstream/godot/scene/2d/physics/joints/joint_2d.cpp). A joint naming a
    /// node that is not a registered body is skipped rather than guessed at: a
    /// half-connected joint would yank a body toward the origin.
    fn register_joints(&mut self, tree: &SceneTree, all_ids: &[NodeId]) {
        use gdphysics2d::joint::{DampedSpringJoint2D, GrooveJoint2D, Joint2D, PinJoint2D};

        for &node_id in all_ids {
            let Some(node) = tree.get_node(node_id) else {
                continue;
            };
            let class = node.class_name().to_owned();
            if !matches!(
                class.as_str(),
                "PinJoint2D" | "GrooveJoint2D" | "DampedSpringJoint2D"
            ) {
                continue;
            }
            if self.node_to_joint.contains_key(&node_id) {
                continue;
            }

            // `node_a`/`node_b` are NodePaths relative to the joint node.
            //
            // Resolved in three sequential steps rather than one closure: a
            // closure capturing `&self.node_to_body` would still be live when
            // `self.world.add_joint` needs `&mut self` below.
            let (Some(path_a), Some(path_b)) = (
                joint_node_path(node, "node_a"),
                joint_node_path(node, "node_b"),
            ) else {
                continue;
            };
            let (Some(target_a), Some(target_b)) = (
                resolve_joint_target(tree, node_id, &path_a),
                resolve_joint_target(tree, node_id, &path_b),
            ) else {
                continue;
            };
            let (Some(body_a), Some(body_b)) = (
                self.node_to_body.get(&target_a).copied(),
                self.node_to_body.get(&target_b).copied(),
            ) else {
                continue;
            };

            let anchor = crate::node2d::get_global_position(tree, node_id);
            // The bodies' current placement is what the joint is bound around,
            // so read it before building anything.
            let pos_a = crate::node2d::get_global_position(tree, target_a);
            let rot_a = crate::node2d::get_global_rotation(tree, target_a);
            let pos_b = crate::node2d::get_global_position(tree, target_b);
            let rot_b = crate::node2d::get_global_rotation(tree, target_b);

            let mut joint = match class.as_str() {
                "PinJoint2D" => {
                    // `bind`, not `new`: Godot hinges the bodies where they
                    // already stand rather than collapsing both onto the anchor,
                    // which is what lets a pendulum swing.
                    let mut pin =
                        PinJoint2D::bind(body_a, pos_a, rot_a, body_b, pos_b, rot_b, anchor);
                    pin.softness = float_prop(tree, node_id, "softness", 0.0) as f32;
                    pin.angular_limit_enabled =
                        bool_prop(tree, node_id, "angular_limit_enabled", false);
                    pin.angular_limit_lower =
                        float_prop(tree, node_id, "angular_limit_lower", 0.0) as f32;
                    pin.angular_limit_upper =
                        float_prop(tree, node_id, "angular_limit_upper", 0.0) as f32;
                    pin.motor_enabled = bool_prop(tree, node_id, "motor_enabled", false);
                    pin.motor_target_velocity =
                        float_prop(tree, node_id, "motor_target_velocity", 0.0) as f32;
                    Joint2D::Pin(pin)
                }
                "DampedSpringJoint2D" => {
                    // Godot's `rest_length` defaults to 0, which means "use
                    // `length`" (damped_spring_joint_2d.cpp); passing the 0
                    // through would collapse the spring to a point.
                    let length = float_prop(tree, node_id, "length", 50.0) as f32;
                    let rest = float_prop(tree, node_id, "rest_length", 0.0) as f32;
                    let rest_length = if rest > 0.0 { rest } else { length };
                    Joint2D::DampedSpring(
                        DampedSpringJoint2D::new(
                            body_a,
                            body_b,
                            rest_length,
                            float_prop(tree, node_id, "stiffness", 20.0) as f32,
                            float_prop(tree, node_id, "damping", 1.0) as f32,
                        )
                        .with_length(length),
                    )
                }
                _ => {
                    // GrooveJoint2D: the groove runs along the joint node's
                    // local +Y for `length`, with body B anchored at
                    // `initial_offset` along it (groove_joint_2d.cpp).
                    //
                    // `GrooveJoint2D` stores its endpoints as offsets **from the
                    // bodies**, not in world space, so the world-space groove is
                    // rebased onto each body here — passing world points made the
                    // groove drift by the bodies' own positions.
                    let length = float_prop(tree, node_id, "length", 50.0) as f32;
                    let offset = float_prop(tree, node_id, "initial_offset", 25.0) as f32;
                    let rot = crate::node2d::get_global_rotation(tree, node_id);
                    let dir = Vector2::new(-rot.sin(), rot.cos());
                    Joint2D::Groove(GrooveJoint2D::new(
                        body_a,
                        body_b,
                        anchor - pos_a,
                        (anchor + dir * length) - pos_a,
                        (anchor + dir * offset) - pos_b,
                    ))
                }
            };

            // Shared `Joint2D` properties.
            let base = joint.base_mut();
            base.bias = float_prop(tree, node_id, "bias", 0.0) as f32;
            base.disable_collision = bool_prop(tree, node_id, "disable_collision", true);

            let joint_id = self.world.add_joint(joint);
            self.node_to_joint.insert(node_id, joint_id);
        }
    }

    /// Pushes scene tree node transforms into the physics world.
    ///
    /// For kinematic bodies the scene tree is always authoritative.  For rigid
    /// and static bodies we also sync position/rotation so that manual
    /// `set_property("position", ...)` calls (e.g. teleporting a body) are
    /// picked up by the physics engine and the area-overlap detector.
    pub fn sync_to_physics(&mut self, tree: &mut SceneTree) {
        // Track the tree before reading transforms off it, so a node added
        // since the last tick is simulated on this one and a node freed since
        // the last tick stops colliding on this one (pat-s3rrv.25). Mirrors the
        // 3D server, which does its scan at the head of its own
        // `sync_to_physics`.
        self.sync_registrations(tree);

        for (&node_id, &body_id) in &self.node_to_body {
            // Snapshot the node's transform, script-set velocity, and any
            // accumulated force/impulse bridge properties before mutably
            // borrowing the world body (all immutable tree reads).
            // Global, not local — same reason as registration: the server
            // works in world space (pat-0q2kx.31).
            let scene_pos = crate::node2d::get_global_position(tree, node_id);
            let scene_rot = crate::node2d::get_global_rotation(tree, node_id);
            // An AnimatableBody2D turns its own motion into rider-carrying
            // velocity, but only while `sync_to_physics` is on (pat-s3rrv.93).
            //
            // ⚠️ Absent means ON. The property defaults to true in Godot, so
            // only an explicit `false` reverts the platform to plain-static
            // behaviour; treating "unset" as off would leave every authored
            // platform inert.
            let animatable = tree.get_node(node_id).is_some_and(|n| {
                n.class_name() == "AnimatableBody2D"
                    && !matches!(n.get_property("sync_to_physics"), Variant::Bool(false))
            });
            let is_animatable_class = tree
                .get_node(node_id)
                .is_some_and(|n| n.class_name() == "AnimatableBody2D");
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
            // The collider child, re-read each tick — both its transform and its
            // geometry (pat-s3rrv.25). Godot re-pushes both on any change:
            // `collision_shape_2d.cpp::_update_in_shape_owner` for the transform,
            // and `_shape_changed` for the shape itself. Read once at
            // registration, a crouch animation dropping the collider, a door's
            // hitbox swinging, or a hitbox that grows for one attack frame all
            // did nothing.
            let node_scale = crate::node2d::get_global_scale(tree, node_id);
            let collider_tf = find_collision_shape_transform(tree, node_id).map(|mut tf| {
                tf.origin = Vector2::new(tf.origin.x * node_scale.x, tf.origin.y * node_scale.y);
                tf
            });
            // Mirrors `register_bodies`' fallback exactly, which is what makes
            // enabling/disabling a collider at runtime work:
            //   * a usable shape -> that shape, scaled;
            //   * shape children exist but none usable (all `disabled`, or the
            //     sub-resource did not resolve) -> the degenerate rect that
            //     collides with nothing, per Godot (pat-0q2kx.28);
            //   * NO shape children at all -> `None`, leaving whatever was
            //     registered. That last arm matters: the older test-convention
            //     scenes get a default 16px circle at registration, and
            //     overwriting it here would silently strip their collision.
            let collider_shape = match find_collision_shape(tree, node_id) {
                Some(s) => Some(scale_shape(s, node_scale)),
                None if has_collision_shape_child(tree, node_id) => Some(Shape2D::Rectangle {
                    half_extents: Vector2::ZERO,
                }),
                None => None,
            };

            let extra_shapes = extra_collision_shapes(tree, node_id, node_scale);

            // Re-read each tick so a script can change a belt's speed or stop
            // it (pat-s3rrv.101). `opt_*` rather than a defaulted read: a class
            // that has no such property (RigidBody2D, CharacterBody2D) yields
            // `None` and its body is left alone, instead of being rewritten to
            // zero sixty times a second.
            //
            // ⚠️ Read here, with the other `tree` lookups, because the body is
            // borrowed mutably below and `tree` cannot be consulted inside that
            // borrow.
            let conveyor_linear = opt_vec2_prop(tree, node_id, "constant_linear_velocity");
            let conveyor_angular = opt_float_prop(tree, node_id, "constant_angular_velocity");

            let mut is_rigid = false;
            if let Some(body) = self.world.get_body_mut(body_id) {
                // Ahead of the per-type match: a collider's shape and offset are
                // geometry, not motion, so they apply to static, kinematic and
                // rigid bodies alike.
                if let Some(tf) = collider_tf {
                    body.shape_transform = tf;
                }
                // Compared before assigning rather than written unconditionally.
                // `Shape2D` is `Copy + PartialEq`, so this is the whole guard —
                // the common case (nothing changed) leaves the body's shape
                // field untouched instead of rewriting it 60 times a second.
                if let Some(sh) = collider_shape {
                    if sh != body.shape {
                        body.shape = sh;
                    }
                }
                // Kept live for the same reason as the primary shape above: a
                // collider list that only reflected registration would drift the
                // moment a hitbox moved or was toggled.
                if body.extra_shapes != extra_shapes {
                    body.extra_shapes = extra_shapes;
                }
                // Surface velocity is a property of the belt, not of its motion,
                // so like the collider geometry above it applies ahead of the
                // per-type match rather than inside one arm.
                if let Some(v) = conveyor_linear {
                    body.constant_linear_velocity = v;
                }
                if let Some(a) = conveyor_angular {
                    body.constant_angular_velocity = a as f32;
                }
                match body.body_type {
                    BodyType::Kinematic => {
                        // ⚠️ Read the displacement BEFORE the assignment below
                        // overwrites the body's old position — that difference
                        // is the whole signal, and one line later it is gone.
                        //
                        // Recorded for every AnimatableBody2D, including a
                        // stationary one (delta zero) and one with
                        // `sync_to_physics` off (forced to zero), so a platform
                        // that stops carrying actually stops: an entry that
                        // simply vanished would leave last tick's velocity on
                        // the body forever.
                        if is_animatable_class {
                            let delta = if animatable {
                                scene_pos - body.position
                            } else {
                                Vector2::ZERO
                            };
                            self.animatable_motion.insert(body_id, delta);
                        }
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

        // Sync Area2D transform and monitoring flags from the scene tree.
        for (&node_id, &area_id) in &self.node_to_area {
            // Read before the mutable borrow so the tree is not borrowed twice.
            let position = crate::node2d::get_global_position(tree, node_id);
            let rotation = crate::node2d::get_global_rotation(tree, node_id);
            let monitoring = bool_prop(tree, node_id, "monitoring", true);
            let monitorable = bool_prop(tree, node_id, "monitorable", true);
            let point_offset = vec2_prop(tree, node_id, "gravity_point_center", Vector2::ZERO);
            // The area's own collider, transform AND geometry, re-read per tick
            // for the same reasons as the body loop above (pat-s3rrv.25). Kept in
            // step with bodies deliberately: an Area2D and a StaticBody2D whose
            // colliders both animate must not start disagreeing about where they
            // are or how big they are. The area registration path uses the same
            // `scale_shape` as bodies, so the same fallback arms apply here.
            let node_scale = crate::node2d::get_global_scale(tree, node_id);
            let collider_tf = find_collision_shape_transform(tree, node_id).map(|mut tf| {
                tf.origin = Vector2::new(tf.origin.x * node_scale.x, tf.origin.y * node_scale.y);
                tf
            });
            let collider_shape = match find_collision_shape(tree, node_id) {
                Some(s) => Some(scale_shape(s, node_scale)),
                None if has_collision_shape_child(tree, node_id) => Some(Shape2D::Rectangle {
                    half_extents: Vector2::ZERO,
                }),
                None => None,
            };
            let extra_shapes = extra_collision_shapes(tree, node_id, node_scale);
            if let Some(area) = self.area_store.get_area_mut(area_id) {
                if let Some(tf) = collider_tf {
                    area.shape_transform = tf;
                }
                if let Some(sh) = collider_shape {
                    if sh != area.shape {
                        area.shape = sh;
                    }
                }
                if area.extra_shapes != extra_shapes {
                    area.extra_shapes = extra_shapes;
                }
                // Areas had the identical local-as-world bug as bodies.
                area.position = position;
                // Overlap tests build the area's transform from this, so a
                // rotated area now tests as the rotated shape it is
                // (pat-0q2kx.112).
                area.rotation = rotation;
                area.monitoring = monitoring;
                area.monitorable = monitorable;
                // Godot's point-gravity attractor is node-relative and moves
                // with the area. Resolving it once at registration froze the
                // attractor in place, so moving the area left its gravity
                // behind (pat-0q2kx.112).
                area.gravity_point_center = position + point_offset;
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
        // Phase 0a: an AnimatableBody2D's per-tick displacement becomes the
        // body's velocity for the step about to run (pat-s3rrv.93). This is the
        // value `CharacterBody2D`'s floor carry reads off whatever it is
        // standing on (`gdphysics2d/src/character.rs` takes
        // `floor_platform_vel` from the floor body's `linear_velocity`), so
        // setting it here is what makes a rider travel with the platform.
        //
        // ⚠️ **`linear_velocity`, NOT `constant_linear_velocity`.** The latter
        // is the CONVEYOR field: it drags contacts by friction while the
        // surface itself stays put, and the carry path never reads it. Writing
        // the platform's motion there instead would leave a rider standing
        // still while the platform slid out from under it.
        if dt > 0.0 {
            let motions: Vec<(BodyId, Vector2)> = self.animatable_motion.drain().collect();
            for (body_id, delta) in motions {
                if let Some(body) = self.world.get_body_mut(body_id) {
                    body.linear_velocity = delta / dt;
                }
            }
        }

        // Phase 0: compute per-body area gravity/damp overrides (only when an
        // area actually declares one, so default behaviour is unaffected).
        if self.area_store.has_space_overrides() {
            let default_gravity = self.world.gravity();
            let mut gravity_overrides: HashMap<BodyId, Vector2> = HashMap::new();
            let mut damp_overrides: HashMap<BodyId, f32> = HashMap::new();
            // The rotational twin of `damp_overrides` (pat-0q2kx.112).
            let mut angular_damp_overrides: HashMap<BodyId, f32> = HashMap::new();
            for &body_id in self.node_to_body.values() {
                if let Some(body) = self.world.get_body(body_id) {
                    // Only dynamic bodies integrate under gravity/damping.
                    if body.body_type != BodyType::Rigid {
                        continue;
                    }
                    let g = self.area_store.compute_body_gravity(body, default_gravity);
                    let d = self.area_store.compute_body_damp(body, body.linear_damp);
                    angular_damp_overrides.insert(
                        body.id,
                        self.area_store
                            .compute_body_angular_damp(body, body.angular_damp),
                    );
                    gravity_overrides.insert(body_id, g);
                    damp_overrides.insert(body_id, d);
                }
            }
            self.last_collision_events = self.world.step_with_area_overrides(
                dt,
                &gravity_overrides,
                &damp_overrides,
                &angular_damp_overrides,
            );
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

    /// Publishes each `Area2D`'s current overlap set onto its node as
    /// `_overlapping_bodies`, which is what the script-facing
    /// `get_overlapping_bodies()` / `has_overlapping_bodies()` read
    /// (pat-0q2kx.112).
    ///
    /// The same bridge the 3D server uses (pat-0q2kx.35): the script phase holds
    /// no handle on the physics server, so the physics phase leaves the answer
    /// on the node. The interpreter arm is already shared between 2D and 3D —
    /// only the 2D side never filled it in, so `get_overlapping_bodies()` on an
    /// `Area2D` always returned an empty array however many bodies were inside.
    pub fn publish_overlapping_bodies(&self, tree: &mut SceneTree) {
        for (&area_node, &area_id) in &self.node_to_area {
            let bodies: Vec<Variant> = self
                .area_store
                .overlapping_bodies(area_id)
                .into_iter()
                .filter_map(|bid| self.body_to_node.get(&bid).copied())
                .map(|nid| Variant::ObjectId(nid.object_id()))
                .collect();
            if let Some(node) = tree.get_node_mut(area_node) {
                node.set_property("_overlapping_bodies", Variant::Array(bodies));
            }
        }
    }

    /// Publishes each `Area2D`'s current area-vs-area overlap set onto its node
    /// as `_overlapping_areas`, backing the script-facing
    /// `get_overlapping_areas()` / `has_overlapping_areas()` / `overlaps_area()`
    /// (pat-0q2kx.112).
    ///
    /// The area twin of [`publish_overlapping_bodies`](Self::publish_overlapping_bodies),
    /// and it uses the same node bridge for the same reason: the script phase
    /// holds no handle on the physics server. Godot gates area-vs-area detection
    /// on the TARGET being `monitorable`, and that gate lives inside
    /// `detect_area_overlaps`, so it is already applied to the set read here —
    /// an area that opted out of being seen never appears in another area's
    /// answer.
    pub fn publish_overlapping_areas(&self, tree: &mut SceneTree) {
        for (&area_node, &area_id) in &self.node_to_area {
            let areas: Vec<Variant> = self
                .area_store
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

    /// Pulls physics body transforms back into the scene tree.
    pub fn sync_from_physics(&self, tree: &mut SceneTree) {
        // The overlap set the last step produced reaches scripts here, on the
        // same tick the enter/exit signals fire.
        self.publish_overlapping_bodies(tree);
        self.publish_overlapping_areas(tree);
        for (&node_id, &body_id) in &self.node_to_body {
            if let Some(body) = self.world.get_body(body_id) {
                // Don't write back static body properties (they don't move).
                if body.body_type == BodyType::Static {
                    continue;
                }
                // ⚠️ An `AnimatableBody2D`'s transform is AUTHORED: animation
                // moves the NODE, and `sync_to_physics` reads that motion into
                // the body's constant velocity so riders are carried. Writing
                // the integrated body position BACK applies the same motion a
                // SECOND time — the platform advanced 4px per tick for a 2px
                // animation step, which `animatable_body_carries_riders` caught
                // at its fixture control (pat-s3rrv.93).
                //
                // 🔑 Physics READS an authored transform and never writes it.
                //
                // ⚠️ This must NOT skip `BodyType::Kinematic` generally.
                // `CharacterBody2D` is kinematic too and genuinely needs the
                // write-back, because `move_and_slide` moves it inside the
                // solver — a broad kinematic skip would freeze every character.
                // The test is the animatable CLASS plus its flag, including the
                // "absent means ON" rule used at the `sync_to_physics` end:
                // Godot defaults the property to true, so treating unset as off
                // would leave every authored platform inert.
                let animatable_authored = tree.get_node(node_id).is_some_and(|n| {
                    n.class_name() == "AnimatableBody2D"
                        && !matches!(n.get_property("sync_to_physics"), Variant::Bool(false))
                });
                if animatable_authored {
                    continue;
                }
                // The body's transform is world-space; the node's `position` /
                // `rotation` properties are PARENT-relative. Writing the world
                // value straight into the local property re-applied the parent
                // offset on the next `sync_to_physics`, so a body under an
                // offset parent accelerated away by that offset every frame
                // (pat-0q2kx.31). Velocities stay global, as in Godot.
                crate::node2d::set_global_position(tree, node_id, body.position);
                crate::node2d::set_global_rotation(tree, node_id, body.rotation);
                if let Some(node) = tree.get_node_mut(node_id) {
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
    /// Publishes each monitoring body's CURRENT contacts as `_body_contacts`,
    /// so `_integrate_forces(state)` can report them (pat-s3rrv.94).
    ///
    /// `PhysicsDirectBodyState2D`'s contact getters were registered in ClassDB
    /// and backed by a real `BodyContact2D` type, but `build_state` filled the
    /// list with `Vec::new()` — so `get_contact_count()` answered 0 however many
    /// bodies were touching.
    ///
    /// ⚠️ A separate pass from [`process_body_contact_signals`](Self::process_body_contact_signals),
    /// which deliberately skips `Persisting` contacts because it reports
    /// enter/exit EDGES. A body resting on a floor is persisting on every tick
    /// after the first, so reusing that pass would report contacts for exactly
    /// one frame and none thereafter.
    ///
    /// Same bridge as `publish_overlapping_bodies`: the script phase holds no
    /// handle on the physics server, so the physics phase leaves the answer on
    /// the node.
    pub fn publish_body_contacts(&self, tree: &mut SceneTree) {
        // Every monitoring body is reset first, so a body that stopped touching
        // reports zero rather than keeping last tick's list forever.
        let monitoring: Vec<NodeId> = self
            .node_to_body
            .keys()
            .copied()
            .filter(|&n| bool_prop(tree, n, "contact_monitor", false))
            .collect();

        let mut per_node: HashMap<NodeId, Vec<Variant>> = HashMap::new();
        for event in &self.last_collision_events {
            // Either side may be the monitoring body, so both directions are
            // evaluated — mirroring the signal pass.
            for (self_body, other_body, self_is_a) in [
                (event.body_a, event.body_b, true),
                (event.body_b, event.body_a, false),
            ] {
                let (Some(&self_node), Some(&other_node)) = (
                    self.body_to_node.get(&self_body),
                    self.body_to_node.get(&other_body),
                ) else {
                    continue;
                };
                if !bool_prop(tree, self_node, "contact_monitor", false) {
                    continue;
                }
                // Godot gates on BOTH flags, and caps the list at the reported
                // maximum rather than growing without bound.
                let cap = u32_prop(tree, self_node, "max_contacts_reported", 0) as usize;
                let entry = per_node.entry(self_node).or_default();
                if cap == 0 || entry.len() >= cap {
                    continue;
                }
                // ⚠️ `CollisionEvent.normal` runs A -> B. Godot's
                // `get_contact_local_normal` points AWAY from the surface this
                // body is resting against — up, for a body on a floor — so the
                // sign flips when this body is A. Publishing the raw event
                // normal would report every floor contact as pointing down and
                // silently invert the standard floor test.
                let normal = if self_is_a {
                    event.normal * -1.0
                } else {
                    event.normal
                };
                let mut rec = gdvariant::Dictionary::new();
                rec.insert("normal", Variant::Vector2(normal));
                rec.insert("position", Variant::Vector2(event.contact_point));
                rec.insert("collider", Variant::ObjectId(other_node.object_id()));
                entry.push(Variant::Dictionary(rec));
            }
        }

        for node in monitoring {
            let recs = per_node.remove(&node).unwrap_or_default();
            if let Some(n) = tree.get_node_mut(node) {
                n.set_property("_body_contacts", Variant::Array(recs));
            }
        }
    }

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

    /// Every body a character should collide against, excluding itself.
    ///
    /// ⚠️ Tile bodies are chained in explicitly. They are generated from painted
    /// cells and have no scene node, so they never appear in `node_to_body` —
    /// walking that map alone once let a character pass straight through painted
    /// ground while the static bodies sat in the world (pat-0q2kx.117).
    /// Extracted so the per-tick slide and the script-driven
    /// [`script_move_and_collide`](Self::script_move_and_collide) sweep cannot
    /// drift apart on which bodies are solid.
    fn obstacle_bodies(&self, exclude: NodeId) -> Vec<&PhysicsBody2D> {
        // The character's OWN body, so its collision exceptions can be honoured
        // (pat-s3rrv.100).
        //
        // ⚠️ A `CharacterBody2D` carries no `BodyId` of its own — the solver
        // type is built fresh from node properties each tick — so the exception
        // test cannot live inside `move_and_slide_delta`. It has to happen where
        // the obstacle list is built, which is here.
        let self_body = self
            .node_to_body
            .get(&exclude)
            .and_then(|id| self.world.get_body(*id));
        // Asked of BOTH sides via `excludes`: the exception is stored on
        // whichever body the script called the method on, but the effect is
        // mutual, so testing one direction would make it depend on which of the
        // pair happened to be the character.
        let excepted = |other: &PhysicsBody2D| -> bool {
            self_body.is_some_and(|me| PhysicsBody2D::excludes(me, other))
        };
        self.node_to_body
            .iter()
            .filter_map(|(&other_node, &other_body_id)| {
                if other_node == exclude {
                    return None;
                }
                let other = self.world.get_body(other_body_id)?;
                if excepted(other) {
                    return None;
                }
                Some(other)
            })
            .chain(
                self.tilemap_bodies
                    .values()
                    .flatten()
                    .filter_map(|&tile_body| {
                        // ⚠️ The tilemap chain is filtered too. Painted tile
                        // bodies have no scene node, so they never appear in
                        // `node_to_body` — a character excepted from one would
                        // still have collided with it if only the map half were
                        // filtered.
                        let other = self.world.get_body(tile_body)?;
                        if excepted(other) {
                            return None;
                        }
                        Some(other)
                    }),
            )
            .collect()
    }

    /// Godot's `PhysicsBody2D.move_and_collide(motion)`: sweep `motion`, stop at
    /// the first blocking contact, and report it (pat-s3rrv.97).
    ///
    /// Returns `true` when something was hit, having published the contact as
    /// `_move_and_collide` for the script surface to hand back as a
    /// `KinematicCollision2D`.
    ///
    /// ⚠️ Writes the result onto the NODE, not onto the physics body. This runs
    /// during the script phase, and `sync_to_physics` pushes node transforms
    /// into the world at the top of the next tick — so writing the body here
    /// would be overwritten by the node a moment later, and the two would
    /// disagree for the rest of this frame.
    ///
    /// ⚠️ Unlike `move_and_slide`, this does NOT slide along the surface: Godot
    /// stops dead at the first contact, which is the whole reason the method
    /// exists alongside the sliding one.
    pub fn script_move_and_collide(
        &self,
        tree: &mut SceneTree,
        node_id: NodeId,
        motion: Vector2,
    ) -> bool {
        let Some(&body_id) = self.node_to_body.get(&node_id) else {
            return false;
        };
        let Some(body) = self.world.get_body(body_id) else {
            return false;
        };
        let mut character = PhysicsCharacterBody2D::new(body.position, body.shape.clone());
        character.collision_layer = body.collision_layer;
        character.collision_mask = body.collision_mask;
        let obstacles = self.obstacle_bodies(node_id);
        let hit = character.move_and_collide(motion, &obstacles);
        let end = character.position;
        drop(obstacles);

        crate::node2d::set_global_position(tree, node_id, end);
        let record = match &hit {
            Some(c) => {
                let mut rec = gdvariant::Dictionary::new();
                rec.insert("normal", Variant::Vector2(c.normal));
                rec.insert("depth", Variant::Float(c.depth as f64));
                rec.insert("position", Variant::Vector2(c.position));
                rec.insert("remainder", Variant::Vector2(c.remainder));
                rec.insert(
                    "collider",
                    c.collider
                        .and_then(|bid| self.body_to_node.get(&bid).copied())
                        .map(|nid| Variant::ObjectId(nid.object_id()))
                        .unwrap_or(Variant::Nil),
                );
                Variant::Dictionary(rec)
            }
            // Cleared rather than left stale, so a later call that hits nothing
            // cannot hand back the previous call's contact.
            None => Variant::Nil,
        };
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property("_move_and_collide", record);
        }
        hit.is_some()
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

            // Godot only moves a CharacterBody2D when the script calls
            // `move_and_slide()`; the engine never auto-moves it. Patina used to
            // move every kinematic body with a velocity, so a scripted body
            // drifted whether or not it asked to (pat-0q2kx.16).
            //
            // The gate applies only to **scripted** bodies: a body with no
            // script has nothing that could call `move_and_slide`, and scenes
            // and tests that drive `velocity` directly rely on the old
            // behavior, so those keep auto-moving.
            if tree.has_script(node_id) {
                if !bool_prop(tree, node_id, "_move_and_slide_requested", false) {
                    continue;
                }
                // One-shot: the request is consumed by this step, so a script
                // that stops calling move_and_slide stops moving.
                if let Some(node) = tree.get_node_mut(node_id) {
                    node.set_property("_move_and_slide_requested", Variant::Bool(false));
                }
            }

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
            // Captured BEFORE the move: `get_real_velocity()` is the motion
            // actually achieved, which is only knowable by differencing the
            // start and end positions (pat-s3rrv.97).
            let start_position = body.position;
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
            character.floor_stop_on_slope = bool_prop(tree, node_id, "floor_stop_on_slope", false);
            character.floor_constant_speed =
                bool_prop(tree, node_id, "floor_constant_speed", false);
            character.floor_block_on_wall = bool_prop(tree, node_id, "floor_block_on_wall", true);
            character.slide_on_ceiling = bool_prop(tree, node_id, "slide_on_ceiling", true);
            character.max_slides = u32_prop(tree, node_id, "max_slides", 4).max(1) as usize;
            character.set_on_floor(was_on_floor);
            character.platform_velocity = stored_platform_vel;

            // Collect static/other bodies to collide against (exclude self).
            //
            // Tile bodies are included explicitly: they are generated from
            // painted cells and so have no scene node of their own, which means
            // they never appear in `node_to_body`. Walking that map alone let a
            // character pass straight through painted ground even though the
            // static bodies were sitting in the world (pat-0q2kx.117).
            let obstacle_refs: Vec<&PhysicsBody2D> = self.obstacle_bodies(node_id);

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

            // Sync to scene tree. `new_position` is world-space (the character
            // was seeded from `body.position`), so it has to be de-parented
            // before landing in the local `position` property (pat-0q2kx.31).
            crate::node2d::set_global_position(tree, node_id, new_position);

            // Slide contacts, published for `get_slide_collision(i)`
            // (pat-s3rrv.95). The slide loop computed these every tick and then
            // dropped them, so `get_slide_collision_count()` had nothing to read
            // and the standard CharacterBody damage idiom was unwritable.
            //
            // Same bridge as `publish_overlapping_bodies`: the script phase
            // holds no handle on the physics server, so the physics phase leaves
            // the answer on the node.
            //
            // ⚠️ The collider is resolved to a NODE id HERE, not carried out as
            // a `BodyId`. This is the only layer that owns `body_to_node`, and
            // `get_collider()` has to hand a script something it can call
            // `is_in_group()` on — a raw body id would be a dead end.
            let slide_records: Vec<Variant> = (0..character.get_slide_collision_count())
                .filter_map(|i| character.get_slide_collision(i))
                .map(|c| {
                    let mut rec = gdvariant::Dictionary::new();
                    rec.insert("normal", Variant::Vector2(c.normal));
                    rec.insert("depth", Variant::Float(c.depth as f64));
                    rec.insert("position", Variant::Vector2(c.position));
                    rec.insert("remainder", Variant::Vector2(c.remainder));
                    rec.insert(
                        "collider",
                        c.collider
                            .and_then(|bid| self.body_to_node.get(&bid).copied())
                            .map(|nid| Variant::ObjectId(nid.object_id()))
                            .unwrap_or(Variant::Nil),
                    );
                    Variant::Dictionary(rec)
                })
                .collect();

            // Godot's `get_real_velocity()` reports the motion the last move
            // ACHIEVED, not the one that was asked for — they differ whenever a
            // wall cancelled part of the requested motion, which is precisely
            // when a script wants to know (pat-s3rrv.97).
            //
            // ⚠️ Derived from the position DELTA, not from the post-slide
            // velocity. The two look interchangeable and are not: the slide
            // solver reports the velocity it would carry into the next tick,
            // which stays non-zero along a wall the body did not actually move
            // along this tick.
            let real_velocity = if dt > 0.0 {
                (new_position - start_position) / dt
            } else {
                Vector2::ZERO
            };

            if let Some(node) = tree.get_node_mut(node_id) {
                node.set_property("_real_velocity", Variant::Vector2(real_velocity));
                node.set_property("_slide_collisions", Variant::Array(slide_records));
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

    /// 🔑 A CAPSULE WITH NO AUTHORED `height` IS 30, NOT 20 (pat-s3rrv.98).
    ///
    /// Godot omits default-valued properties when writing a `.tscn`, so a
    /// capsule left at its default arrives here with no `height` key at all —
    /// which makes the DEFAULT the value that ships, not a fallback that rarely
    /// fires. `doc/classes/CapsuleShape2D.xml` says 30.0; the parser said 20.0,
    /// silently shrinking every such shape by a third.
    ///
    /// ⚠️ `radius` is asserted in the same breath as a positive control: it was
    /// already correct at 10.0, so a test that only checked `height` could pass
    /// against a parser that had stopped reading defaults altogether.
    #[test]
    fn shape_from_node_capsule_defaults_match_godot() {
        let (tree, id) = shape_node("CapsuleShape2D", &[]);
        assert_eq!(
            shape_from_node(&tree, id),
            Some(Shape2D::Capsule {
                radius: 10.0,
                height: 30.0
            }),
            "an unauthored CapsuleShape2D must match doc/classes/CapsuleShape2D.xml"
        );
    }

    /// 🔑 THE PARSER AND ClassDB MUST NOT DRIFT APART.
    ///
    /// These are two independent transcriptions of the same Godot XML — the
    /// `.tscn` path reads defaults here, and the script path
    /// (`CapsuleShape2D.new()`) reads them from ClassDB. They disagreed for as
    /// long as both existed: ClassDB had Godot's 30.0 while the parser had 20.0,
    /// so a script-built capsule and a scene-authored one were different shapes.
    ///
    /// ⚠️ Comparing them to EACH OTHER rather than both to a literal is the
    /// point: this fails when either side is edited alone, which is how the
    /// original mismatch survived.
    #[test]
    fn capsule_defaults_agree_between_the_parser_and_classdb() {
        gdobject::class_db::register_2d_classes();
        let props = gdobject::class_db::get_property_list("CapsuleShape2D");
        let classdb = |name: &str| {
            props
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("CapsuleShape2D.{name} is registered"))
                .default_value
                .clone()
        };

        let (tree, id) = shape_node("CapsuleShape2D", &[]);
        let Some(Shape2D::Capsule { radius, height }) = shape_from_node(&tree, id) else {
            panic!("the parser must build a capsule");
        };

        assert_eq!(
            Variant::Float(radius as f64),
            classdb("radius"),
            "parser radius default disagrees with ClassDB"
        );
        assert_eq!(
            Variant::Float(height as f64),
            classdb("height"),
            "parser height default disagrees with ClassDB — a script-built \
             capsule and a .tscn-authored one would be different shapes"
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

    /// 🔑 pat-s3rrv.101 — THE CONVEYOR PROPERTIES REACH THE BODY.
    ///
    /// The solver has folded `constant_linear_velocity` into the tangential
    /// contact velocity for a long time, with its own green unit test. This
    /// bridge never read the property off the node, so an authored belt was
    /// inert while every Rust-level test passed — the gap was exactly this one
    /// hop.
    #[test]
    fn conveyor_velocities_read_from_node_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut belt = Node::new("Belt", "StaticBody2D");
        belt.set_property("position", Variant::Vector2(Vector2::ZERO));
        belt.set_property(
            "constant_linear_velocity",
            Variant::Vector2(Vector2::new(100.0, 0.0)),
        );
        belt.set_property("constant_angular_velocity", Variant::Float(2.5));
        let id = tree.add_child(root, belt).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.constant_linear_velocity, Vector2::new(100.0, 0.0));
        assert!((body.constant_angular_velocity - 2.5).abs() < 1e-6);
    }

    /// 🔑 AND THEY DEFAULT TO ZERO ON AN ORDINARY STATIC BODY.
    ///
    /// ⚠️ The paired negative case. Godot documents both defaults as zero
    /// (`doc/classes/StaticBody2D.xml`), and zero means "not a conveyor" — so
    /// this is what guarantees the new read cannot make an ordinary floor start
    /// dragging things. Without it the test above would pass just as well if
    /// the bridge wrote some non-zero value onto every body.
    #[test]
    fn conveyor_velocities_default_to_zero_when_absent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut floor = Node::new("Floor", "StaticBody2D");
        floor.set_property("position", Variant::Vector2(Vector2::ZERO));
        let id = tree.add_child(root, floor).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(body.constant_linear_velocity, Vector2::ZERO);
        assert_eq!(body.constant_angular_velocity, 0.0);
    }

    /// 🔑 A SCRIPT CAN CHANGE A BELT'S SPEED AT RUNTIME.
    ///
    /// ⚠️ Registration-time reads alone would freeze a belt at whatever the
    /// `.tscn` authored, so `belt.constant_linear_velocity = Vector2(-100, 0)`
    /// in `_process` would do nothing. `sync_to_physics` re-reads it every tick
    /// for the same reason it re-reads the collider each tick.
    #[test]
    fn conveyor_velocity_is_re_read_each_tick() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut belt = Node::new("Belt", "StaticBody2D");
        belt.set_property("position", Variant::Vector2(Vector2::ZERO));
        belt.set_property(
            "constant_linear_velocity",
            Variant::Vector2(Vector2::new(100.0, 0.0)),
        );
        let id = tree.add_child(root, belt).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(id, s).unwrap();

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        // What a script does: reverse the belt.
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property(
                "constant_linear_velocity",
                Variant::Vector2(Vector2::new(-100.0, 0.0)),
            );
        }
        server.sync_to_physics(&mut tree);

        let body_id = server.body_for_node(id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert_eq!(
            body.constant_linear_velocity,
            Vector2::new(-100.0, 0.0),
            "the belt must follow the node property, not stay at its registered value"
        );
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
        assert_eq!(
            exited.load(Ordering::SeqCst),
            0,
            "no exit while overlapping"
        );

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
        area.set_property(
            "gravity_direction",
            Variant::Vector2(Vector2::new(0.0, -1.0)),
        );
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
        assert!(
            y > 1.0,
            "disabled-override area must not stop the fall, got {y}"
        );
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

    // ---- pat-0q2kx.34: CollisionPolygon2D is real collision geometry ------
    //
    // `shape_from_node` rejected every class except CollisionShape2D, so a
    // CollisionPolygon2D — which carries its outline inline as `polygon`
    // rather than referencing a Shape2D sub-resource — resolved to nothing and
    // the body fell back to the default 16px circle. A hand-drawn ramp or
    // terrain collider silently became a small ball, with no error.

    /// A right-triangle ramp spanning x ∈ [-50,50], y ∈ [-25,25].
    fn ramp_points() -> Vec<Variant> {
        vec![
            Variant::Vector2(Vector2::new(-50.0, 25.0)),
            Variant::Vector2(Vector2::new(50.0, 25.0)),
            Variant::Vector2(Vector2::new(50.0, -25.0)),
        ]
    }

    fn body_with_polygon(build_mode: Option<i64>, disabled: bool) -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Ramp", "StaticBody2D");
        let body_id = tree.add_child(root, body).unwrap();

        let mut poly = Node::new("Poly", "CollisionPolygon2D");
        poly.set_property("polygon", Variant::Array(ramp_points()));
        if let Some(bm) = build_mode {
            poly.set_property("build_mode", Variant::Int(bm));
        }
        if disabled {
            poly.set_property("disabled", Variant::Bool(true));
        }
        tree.add_child(body_id, poly).unwrap();
        (tree, body_id)
    }

    /// ⚠️ The core gap: the authored outline becomes the body's real shape
    /// instead of the 16px-circle fallback.
    #[test]
    fn collision_polygon2d_builds_the_authored_shape() {
        let (tree, body_id) = body_with_polygon(None, false);
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(body_id).expect("body registered");
        let body = server.world().get_body(body_id).expect("body exists");
        match body.shape {
            Shape2D::ConvexPolygon { count, .. } => {
                assert_eq!(
                    count, 3,
                    "the 3-point ramp outline must reach the solver intact"
                );
            }
            Shape2D::Circle { radius } => panic!(
                "the polygon fell back to the default circle (radius {radius}) — \
                 CollisionPolygon2D was skipped entirely"
            ),
            other => panic!("expected a convex polygon, got {other:?}"),
        }
    }

    /// `build_mode = BUILD_SEGMENTS` (1) makes a hollow outline, not a solid.
    /// Treating it as solid would let bodies rest inside a thin wall.
    #[test]
    fn collision_polygon2d_segments_build_mode_is_hollow() {
        let (tree, body_id) = body_with_polygon(Some(1), false);
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(body_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        match body.shape {
            // Three outline points close into three segments = six endpoints.
            Shape2D::ConcavePolygon { count, .. } => assert_eq!(
                count, 6,
                "a closed 3-point outline must expand to 3 segments (6 endpoints)"
            ),
            other => panic!("BUILD_SEGMENTS must not produce a solid shape, got {other:?}"),
        }
    }

    /// A disabled polygon contributes nothing — and must NOT silently fall back
    /// to the default circle, which is what "no shape children" would give.
    #[test]
    fn collision_polygon2d_disabled_contributes_no_collision() {
        let (tree, body_id) = body_with_polygon(None, true);
        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let body_id = server.body_for_node(body_id).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        match body.shape {
            Shape2D::Rectangle { half_extents } => assert_eq!(
                half_extents,
                Vector2::ZERO,
                "a disabled-only body gets the degenerate no-collision shape"
            ),
            Shape2D::Circle { radius } => {
                panic!("a disabled polygon must not become a {radius}px default circle")
            }
            other => panic!("expected the degenerate shape, got {other:?}"),
        }
    }

    /// A degenerate outline (fewer than 3 points) yields no shape rather than a
    /// stand-in.
    #[test]
    fn collision_polygon2d_degenerate_outline_is_ignored() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Ramp", "StaticBody2D");
        let body_id = tree.add_child(root, body).unwrap();
        let mut poly = Node::new("Poly", "CollisionPolygon2D");
        poly.set_property(
            "polygon",
            Variant::Array(vec![Variant::Vector2(Vector2::ZERO)]),
        );
        tree.add_child(body_id, poly).unwrap();

        assert!(
            shape_from_node(&tree, tree.get_node(body_id).unwrap().children()[0]).is_none(),
            "a 1-point polygon must resolve to no shape"
        );
    }

    // ---- pat-0q2kx.31: the server works in world space -------------------
    //
    // Godot's physics server is fed a CollisionObject2D's GLOBAL transform
    // (`upstream/godot/scene/2d/physics/collision_object_2d.cpp` —
    // `NOTIFICATION_TRANSFORM_CHANGED` pushes `get_global_transform()`).
    // Patina read the node's LOCAL `position`/`rotation`/`scale` properties, so
    // every body under a non-identity parent registered at the wrong world
    // spot. Nothing errored: a level built the normal Godot way — everything
    // parented under one offset `Node2D` — simply collided somewhere else, and
    // because the write-back put world coordinates straight into the local
    // property, the error compounded each frame.
    //
    // Every scene below needs a parent with a non-identity transform: with an
    // identity parent, local and global coincide and the bug is invisible.

    /// The bead's scene: a floor and a ball parented under a `Node2D` offset by
    /// (500, 300), authored as the Godot editor writes it.
    const NESTED_PARENT_SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="RectangleShape2D" id="Rect_floor"]
size = Vector2(200, 20)

[sub_resource type="CircleShape2D" id="Circle_ball"]
radius = 10.0

[node name="Root" type="Node2D"]

[node name="Level" type="Node2D" parent="."]
position = Vector2(500, 300)

[node name="Floor" type="StaticBody2D" parent="Level"]
position = Vector2(0, 100)

[node name="FloorShape" type="CollisionShape2D" parent="Level/Floor"]
shape = SubResource("Rect_floor")

[node name="Ball" type="RigidBody2D" parent="Level"]
position = Vector2(0, 0)

[node name="BallShape" type="CollisionShape2D" parent="Level/Ball"]
shape = SubResource("Circle_ball")
"#;

    /// **The acceptance test.**
    #[test]
    fn physics2d_nested_parent_transforms_respected() {
        let scene =
            crate::packed_scene::PackedScene::from_tscn(NESTED_PARENT_SCENE).expect("scene parses");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
            .expect("scene instances");
        let level = tree
            .get_node_or_null(scene_root, "Level")
            .expect("Level node");
        let ball = tree.get_node_or_null(level, "Ball").expect("Ball node");
        let floor = tree.get_node_or_null(level, "Floor").expect("Floor node");

        let mut server = PhysicsServer::new();
        // Pinned rather than inherited, so the fall distance below is arithmetic
        // rather than a hostage to the world's default.
        server.set_gravity(Vector2::new(0.0, 980.0));
        server.register_bodies(&tree);

        // ⚠️ Registration alone already exposed the bug: the ball authored at
        // local (0,0) is at world (500,300), and the floor at local (0,100) is
        // at world (500,400). Before the fix both registered at their local
        // coordinates — the ball at the origin, 583px from where it was drawn.
        let ball_body = server.body_for_node(ball).expect("ball registered");
        let floor_body = server.body_for_node(floor).expect("floor registered");
        let ball_start = server.world().get_body(ball_body).unwrap().position;
        let floor_at = server.world().get_body(floor_body).unwrap().position;
        assert!(
            (ball_start.x - 500.0).abs() < 0.01 && (ball_start.y - 300.0).abs() < 0.01,
            "the ball must register at its authored WORLD position (500,300), got {ball_start:?}"
        );
        assert!(
            (floor_at.x - 500.0).abs() < 0.01 && (floor_at.y - 400.0).abs() < 0.01,
            "the floor must register at its authored WORLD position (500,400), got {floor_at:?}"
        );

        // One second of simulation. Free-fall from y=300 to the floor's top
        // surface (y=390) minus the ball's 10px radius is 80px, reached in
        // ~0.40s, so 60 ticks land it and leave it resting.
        for _ in 0..60 {
            server.sync_to_physics(&mut tree);
            server.step_physics(1.0 / 60.0);
            server.sync_from_physics(&mut tree);
        }

        let resting = server.world().get_body(ball_body).unwrap().position;

        // It collided with the floor at the authored world location: stopped on
        // top of it rather than falling past. With local-space registration the
        // ball collided with the floor near the origin and rested at y≈80.
        assert!(
            resting.y > 360.0 && resting.y < 400.0,
            "the ball must come to rest on the floor's world surface (y≈380), got {resting:?}"
        );
        assert!(
            (resting.x - 500.0).abs() < 2.0,
            "the ball must not drift horizontally, got {resting:?}"
        );

        // ⚠️ The round-trip assertion: the node's GLOBAL position — parent
        // offset composed with the local property physics wrote back — equals
        // the body's position. This is what fails if `sync_from_physics` dumps
        // world coordinates into the local property: the parent offset then
        // gets added a second time, and the two drift apart by (500,300) on the
        // very first frame.
        let ball_global = crate::node2d::get_global_position(&tree, ball);
        assert!(
            (ball_global.x - resting.x).abs() < 0.01 && (ball_global.y - resting.y).abs() < 0.01,
            "node global position {ball_global:?} must equal the physics body position {resting:?}"
        );

        // …and the property physics wrote is genuinely PARENT-RELATIVE, not a
        // world coordinate that happens to compose correctly.
        let ball_local = vec2_prop(&tree, ball, "position", Vector2::ZERO);
        assert!(
            ball_local.x.abs() < 2.0 && (ball_local.y - (resting.y - 300.0)).abs() < 0.01,
            "the stored `position` must stay local (≈(0,{:.0})), got {ball_local:?}",
            resting.y - 300.0
        );
    }

    /// The bead's second case: a `CharacterBody2D` under a ROTATED parent has to
    /// collide against world geometry, which needs the parent's rotation — not
    /// just its offset — on both the read and the write-back.
    #[test]
    fn nested_parent_character_body_collides_in_world_space() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Parent offset AND rotated a quarter turn.
        let mut level = Node::new("Level", "Node2D");
        level.set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));
        level.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_2));
        let level_id = tree.add_child(root, level).unwrap();

        // Player at the parent's origin ⇒ world (500,300). Velocity is global in
        // Godot, so it drives the player toward +X in world space regardless of
        // how the parent is turned.
        let mut player = Node::new("Player", "CharacterBody2D");
        player.set_property("position", Variant::Vector2(Vector2::ZERO));
        player.set_property("velocity", Variant::Vector2(Vector2::new(600.0, 0.0)));
        let player_id = tree.add_child(level_id, player).unwrap();
        let mut ps = Node::new("Shape", "CollisionShape2D");
        ps.set_property("radius", Variant::Float(10.0));
        tree.add_child(player_id, ps).unwrap();

        // Wall authored in world space under the UNROTATED root: x ∈ [550,570].
        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(560.0, 300.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut ws = Node::new("Shape", "CollisionShape2D");
        ws.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
        tree.add_child(wall_id, ws).unwrap();

        let mut server = PhysicsServer::new();
        server.set_gravity(Vector2::ZERO);
        server.register_bodies(&tree);

        let player_body = server.body_for_node(player_id).expect("player registered");
        let start = server.world().get_body(player_body).unwrap().position;
        assert!(
            (start.x - 500.0).abs() < 0.01 && (start.y - 300.0).abs() < 0.01,
            "the player must register at world (500,300), got {start:?}"
        );

        // 10px per tick; the wall is 60px away, so 20 ticks is far more than
        // enough to reach it and stay stopped.
        for _ in 0..20 {
            server.process_character_movement(1.0 / 60.0, &mut tree);
        }

        let world = crate::node2d::get_global_position(&tree, player_id);
        assert!(
            world.x < 550.0,
            "the player must not enter the wall at world x=550, got {world:?}"
        );
        assert!(
            world.x > 520.0,
            "the player must have moved toward the wall, got {world:?}"
        );
        assert!(
            (world.y - 300.0).abs() < 2.0,
            "the player must not drift off its world row, got {world:?}"
        );

        // ⚠️ The rotation-specific assertion. World +X maps onto the rotated
        // parent's local ∓Y, so the stored local `position` must be almost
        // entirely on its y axis. If the write-back ignored the parent (storing
        // world coordinates) or de-parented with the offset only (giving local
        // ≈(40,0)), local.x lands near 540 or 40 — never near 0.
        let local = vec2_prop(&tree, player_id, "position", Vector2::ZERO);
        assert!(
            local.x.abs() < 2.0,
            "under a 90°-rotated parent, world +X motion must show up on the \
             local y axis; got local {local:?}"
        );
        assert!(
            (local.y.abs() - (world.x - 500.0)).abs() < 2.0,
            "the local offset magnitude must match the world one ({:.1}), got {local:?}",
            world.x - 500.0
        );

        // The wall actually stopped it, rather than the player merely running
        // out of somewhere to be.
        let vel = vec2_prop(&tree, player_id, "velocity", Vector2::ZERO);
        assert!(
            vel.x.abs() < 1.0,
            "X velocity should be zeroed by the wall, got {vel:?}"
        );
    }

    /// Areas had the identical bug, and it is just as silent: a trigger volume
    /// under an offset parent monitored a region of the world nothing ever
    /// entered, so `body_entered` simply never fired.
    #[test]
    fn nested_parent_area_monitors_world_region() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut level = Node::new("Level", "Node2D");
        level.set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));
        let level_id = tree.add_child(root, level).unwrap();

        // Trigger at the parent's origin ⇒ world (500,300).
        let mut area = Node::new("Trigger", "Area2D");
        area.set_property("position", Variant::Vector2(Vector2::ZERO));
        tree.add_child(level_id, area).unwrap();

        // Body authored directly in world space under the root, standing exactly
        // where the trigger is drawn. The two are 583px apart in local
        // coordinates, so this overlap exists only in world space.
        let mut body = Node::new("Walker", "RigidBody2D");
        body.set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));
        tree.add_child(root, body).unwrap();

        let mut server = PhysicsServer::new();
        server.set_gravity(Vector2::ZERO);
        server.register_bodies(&tree);
        assert_eq!(server.area_count(), 1, "the Area2D must register");

        server.step_physics(1.0 / 60.0);

        assert!(
            !server.last_overlap_events().is_empty(),
            "the body standing on the trigger's authored world position must \
             overlap it; with local-space registration the area sat at the \
             origin and nothing ever entered it"
        );
    }
}
