//! `MeshInstance3D` script-facing method dispatch (pat-mgixk.93).
//!
//! # What was broken
//!
//! `get_surface_override_material` and its siblings are registered in ClassDB
//! (`class_db.rs:1711`), so `has_method` answered **true** while a call returned
//! **Nil** — the silent failure this sweep closes. The per-surface getter
//! already existed (`node3d::get_surface_override_material`) and was reachable
//! only from the renderer; the blend-shape weights were readable only as raw
//! properties; and the `create_*_collision` family had no implementation at all.

use gdcore::math::Vector3;
use gdvariant::Variant;

use crate::node::{Node, NodeId};
// ⚠️ `RenderServer3DAdapter`, not `RenderServer3D`: no type of the latter name
// exists. This import broke the whole `gdscene` lib (E0432), which blocked every
// bead that builds it. Fixed by TurquoiseCastle to unblock the lane; the owning
// bead is pat-mgixk.93 (EmeraldCreek). The spelling is the crate's own idiom —
// cf. `physics_server_3d.rs:2499` and `nav_bake.rs:123`, which call the same
// `resolve_mesh(tree, nid)` through the adapter.
use crate::render_server_3d::RenderServer3DAdapter;
use crate::scene_tree::SceneTree;

/// Property key Godot uses for a per-surface override.
fn override_key(surface: usize) -> String {
    format!("surface_material_override/{surface}")
}

/// The mesh's surface count, as `MeshInstance3D.get_surface_override_material_count`
/// reports it.
///
/// A single-surface mesh leaves `Mesh3D::surfaces` EMPTY and carries its
/// geometry at the top level, so an empty list means one surface, not zero.
fn surface_count(tree: &SceneTree, node_id: NodeId) -> usize {
    let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
    mesh.surfaces.len().max(1)
}

/// Expands the mesh into a flat triangle soup in the mesh's own local space.
///
/// `ConcavePolygonShape3D` wants every three consecutive vertices to form one
/// triangle, so an indexed mesh has to be de-indexed here. A mesh with no index
/// buffer is already a soup and is taken verbatim.
fn triangle_soup(mesh: &gdserver3d::mesh::Mesh3D) -> Vec<Vector3> {
    if mesh.indices.is_empty() {
        return mesh.vertices.clone();
    }
    mesh.indices
        .iter()
        .filter_map(|i| mesh.vertices.get(*i as usize).copied())
        .collect()
}

/// Adds `<name>_col` (a `StaticBody3D`) holding one `CollisionShape3D`, which is
/// the structure Godot's `create_*_collision` builds.
///
/// Returns the new body's id, or `None` when the tree refuses the child.
fn add_collision_body(
    tree: &mut SceneTree,
    node_id: NodeId,
    shape_class: &str,
    geometry_key: &str,
    points: Vec<Vector3>,
    suffix: &str,
) -> Option<NodeId> {
    let base_name = tree.get_node(node_id)?.name().to_string();
    let body = Node::new(format!("{base_name}{suffix}"), "StaticBody3D");
    let body_id = tree.add_child(node_id, body).ok()?;

    let mut shape_node = Node::new("CollisionShape3D", "CollisionShape3D");
    // `shape_from_node_3d` keys off the `shape` property holding the shape's
    // CLASS NAME as a string, then reads the geometry from the key that class
    // uses: `points` for a convex hull, `data` for a triangle soup.
    shape_node.set_property("shape", Variant::String(shape_class.to_owned()));
    shape_node.set_property(geometry_key, Variant::PackedVector3Array(points));
    tree.add_child(body_id, shape_node).ok()?;
    Some(body_id)
}

/// Dispatches `MeshInstance3D`'s registered methods, declining every other class.
pub fn call_mesh_instance_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if tree.get_node(node_id)?.class_name() != "MeshInstance3D" {
        return None;
    }

    let idx_arg = |i: usize| -> usize {
        match args.get(i) {
            Some(Variant::Int(v)) if *v >= 0 => *v as usize,
            Some(Variant::Float(f)) if *f >= 0.0 => *f as usize,
            _ => 0,
        }
    };
    let prop = |tree: &SceneTree, key: &str| -> Variant {
        tree.get_node(node_id)
            .map(|n| n.get_property(key))
            .unwrap_or(Variant::Nil)
    };

    Some(match method {
        // ⚠️ `set_surface_override_material` and `get_surface_override_material`
        // are DELIBERATELY ABSENT. pat-mgixk.31 already serves both from
        // `core_node_methods::call_surface_override_material_method`, which
        // `call_node_method` reaches at scripting.rs:3286 — BEFORE this arm — so
        // a copy here would be unreachable code that silently drifts from the
        // live one. That arm also declines a negative index on purpose, since
        // `surface_material_override/-1` is a real key nothing ever reads.
        // Do not "complete the set" by adding them back.
        "get_surface_override_material_count" => Variant::Int(surface_count(tree, node_id) as i64),
        "get_active_material" => {
            // Godot's precedence: material_override beats a per-surface
            // override, which beats the mesh's own surface material.
            let surface = idx_arg(0);
            match prop(tree, "material_override") {
                Variant::Nil => match prop(tree, &override_key(surface)) {
                    Variant::Nil => prop(tree, "material"),
                    surf => surf,
                },
                over => over,
            }
        }
        "get_blend_shape_count" => Variant::Int(
            RenderServer3DAdapter::resolve_mesh(tree, node_id)
                .blend_shapes
                .len() as i64,
        ),
        "get_blend_shape_name" => {
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            match mesh.blend_shapes.get(idx_arg(0)) {
                Some(shape) => Variant::StringName(shape.name.clone().into()),
                None => Variant::Nil,
            }
        }
        "find_blend_shape_by_name" => {
            let name = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                Some(Variant::StringName(s)) => s.to_string(),
                _ => String::new(),
            };
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            Variant::Int(
                mesh.blend_shapes
                    .iter()
                    .position(|s| s.name == name)
                    // Godot answers -1 for an unknown shape name.
                    .map(|i| i as i64)
                    .unwrap_or(-1),
            )
        }
        "get_blend_shape_value" => {
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            match mesh.blend_shapes.get(idx_arg(0)) {
                Some(shape) => match prop(tree, &format!("blend_shapes/{}", shape.name)) {
                    Variant::Float(f) => Variant::Float(f),
                    Variant::Int(i) => Variant::Float(i as f64),
                    _ => Variant::Float(0.0),
                },
                None => Variant::Float(0.0),
            }
        }
        "set_blend_shape_value" => {
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            // Godot addresses shapes by index here but stores them by NAME, and
            // the render path reads `blend_shapes/<name>` — so writing the name
            // is what makes this reach the next frame.
            if let Some(shape) = mesh.blend_shapes.get(idx_arg(0)) {
                let key = format!("blend_shapes/{}", shape.name);
                let weight = match args.get(1) {
                    Some(Variant::Float(f)) => *f,
                    Some(Variant::Int(i)) => *i as f64,
                    _ => 0.0,
                };
                if let Some(node) = tree.get_node_mut(node_id) {
                    node.set_property(&key, Variant::Float(weight));
                }
            }
            Variant::Nil
        }
        "create_trimesh_collision" => {
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            let soup = triangle_soup(&mesh);
            add_collision_body(tree, node_id, "ConcavePolygonShape3D", "data", soup, "_col");
            Variant::Nil
        }
        "create_convex_collision" => {
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            // The hull is built from the vertex cloud, so the index buffer is
            // irrelevant here — unlike the trimesh case.
            let points = mesh.vertices.clone();
            add_collision_body(
                tree,
                node_id,
                "ConvexPolygonShape3D",
                "points",
                points,
                "_col",
            );
            Variant::Nil
        }
        "create_multiple_convex_collisions" => {
            // ⚠️ Godot runs a convex DECOMPOSITION here and emits one body per
            // resulting hull. This engine has no decomposer, so this produces
            // the single-hull result instead of silently doing nothing. That is
            // a known approximation, not parity: a concave mesh gets its convex
            // hull, which over-covers. Reported on the bead.
            let mesh = RenderServer3DAdapter::resolve_mesh(tree, node_id);
            let points = mesh.vertices.clone();
            add_collision_body(
                tree,
                node_id,
                "ConvexPolygonShape3D",
                "points",
                points,
                "_col",
            );
            Variant::Nil
        }
        _ => return None,
    })
}
