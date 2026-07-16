//! 3D node property helpers for Node3D, Camera3D, MeshInstance3D, and Light3D.
//!
//! Follows the same pattern as [`node2d`](crate::node2d): typed helper
//! functions that read and write well-known properties on nodes via the
//! [`SceneTree`].

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Node3D properties
// ===========================================================================

/// Sets the `"position"` property on a 3D node.
pub fn set_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("position", Variant::Vector3(pos));
    }
}

/// Reads the `"position"` property, defaulting to [`Vector3::ZERO`].
pub fn get_position(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("position") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Sets the `"rotation"` property (Euler angles in radians, YXZ convention).
pub fn set_rotation(tree: &mut SceneTree, node_id: NodeId, euler: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("rotation", Variant::Vector3(euler));
    }
}

/// Reads the `"rotation"` property as Euler angles, defaulting to [`Vector3::ZERO`].
pub fn get_rotation(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("rotation") {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Sets the `"scale"` property on a 3D node.
pub fn set_scale(tree: &mut SceneTree, node_id: NodeId, scale: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("scale", Variant::Vector3(scale));
    }
}

/// Reads the `"scale"` property, defaulting to [`Vector3::ONE`].
pub fn get_scale(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("scale") {
            Variant::Vector3(v) => v,
            _ => Vector3::ONE,
        })
        .unwrap_or(Vector3::ONE)
}

/// Returns the local [`Transform3D`] for a node.
///
/// If the node has an explicit `"transform"` property (as stored by `.tscn`
/// files using `transform = Transform3D(…)`), that value is returned directly.
/// Otherwise the transform is composed from separate `position`, `rotation`,
/// and `scale` properties (composition order matches Godot: T * R * S).
pub fn get_local_transform(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    // Check for an explicit Transform3D property first (set by .tscn parser).
    if let Some(node) = tree.get_node(node_id) {
        if let Variant::Transform3D(t) = node.get_property("transform") {
            return t;
        }
    }

    let pos = get_position(tree, node_id);
    let rot = get_rotation(tree, node_id);
    let scl = get_scale(tree, node_id);

    let basis = Basis::from_euler(rot);
    Transform3D {
        basis: Basis {
            x: basis.x * scl.x,
            y: basis.y * scl.y,
            z: basis.z * scl.z,
        },
        origin: pos,
    }
}

/// Computes the global transform by walking the parent chain and multiplying
/// local transforms from the root downward.
pub fn get_global_transform(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    let mut chain = Vec::new();
    let mut current = node_id;
    loop {
        chain.push(current);
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent_id) => current = parent_id,
            None => break,
        }
    }
    chain.reverse();
    let mut global = Transform3D::IDENTITY;
    for id in chain {
        global = global * get_local_transform(tree, id);
    }
    global
}

/// Sets the node's local position such that its global position equals `pos`.
pub fn set_global_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector3) {
    let parent_global = tree
        .get_node(node_id)
        .and_then(|n| n.parent())
        .map(|pid| get_global_transform(tree, pid))
        .unwrap_or(Transform3D::IDENTITY);

    let local_pos = parent_global.inverse().xform(pos);
    set_position(tree, node_id, local_pos);
}

/// Sets the `"visible"` property on a 3D node.
pub fn set_visible(tree: &mut SceneTree, node_id: NodeId, visible: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("visible", Variant::Bool(visible));
    }
}

/// Reads the `"visible"` property, defaulting to `true`.
pub fn is_visible(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("visible") {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Node3D transform operations: look_at / rotate / translate / to_local /
// to_global, plus `top_level` and visible-in-tree propagation.
// ---------------------------------------------------------------------------

/// Writes a node's local [`Transform3D`] via the `"transform"` property.
///
/// [`get_local_transform`] prefers this property over the separate
/// `position`/`rotation`/`scale` fields, so writing it here makes the composed
/// transform authoritative for every downstream transform read. This is the
/// same `"transform"` storage key the `.tscn` parser uses — not a parallel
/// convention.
fn set_local_transform(tree: &mut SceneTree, node_id: NodeId, t: Transform3D) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("transform", Variant::Transform3D(t));
    }
}

/// Sets the `"top_level"` property. When `true`, the node ignores its parent's
/// transform (its global transform equals its own local transform), matching
/// Godot's `Node3D.top_level`.
pub fn set_top_level(tree: &mut SceneTree, node_id: NodeId, top_level: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("top_level", Variant::Bool(top_level));
    }
}

/// Reads the `"top_level"` property, defaulting to `false`.
pub fn is_top_level(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("top_level") {
            Variant::Bool(b) => b,
            _ => false,
        })
        .unwrap_or(false)
}

/// Computes the global [`Transform3D`] while honoring `top_level`.
///
/// Like [`get_global_transform`] this walks the parent chain multiplying local
/// transforms from the root downward, but it stops as soon as it reaches a node
/// whose `top_level` flag is set: that node's local transform becomes the base
/// and everything above it in the hierarchy is ignored.
pub fn get_global_transform_top_level(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    let mut chain = Vec::new();
    let mut current = node_id;
    loop {
        chain.push(current);
        // A top-level node ignores its parent chain entirely.
        if is_top_level(tree, current) {
            break;
        }
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent_id) => current = parent_id,
            None => break,
        }
    }
    chain.reverse();
    let mut global = Transform3D::IDENTITY;
    for id in chain {
        global = global * get_local_transform(tree, id);
    }
    global
}

/// Returns the global transform of the node's *effective* parent: the parent's
/// [`get_global_transform_top_level`], or identity when the node is `top_level`
/// or has no parent. Used to convert world-space orientations into local space.
fn effective_parent_global(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    if is_top_level(tree, node_id) {
        return Transform3D::IDENTITY;
    }
    match tree.get_node(node_id).and_then(|n| n.parent()) {
        Some(parent_id) => get_global_transform_top_level(tree, parent_id),
        None => Transform3D::IDENTITY,
    }
}

/// Transforms `local` from this node's local space to world space using its
/// global transform (Godot's `Node3D.to_global`).
pub fn to_global(tree: &SceneTree, node_id: NodeId, local: Vector3) -> Vector3 {
    get_global_transform_top_level(tree, node_id).xform(local)
}

/// Transforms `global` from world space into this node's local space
/// (Godot's `Node3D.to_local`), the inverse of [`to_global`].
pub fn to_local(tree: &SceneTree, node_id: NodeId, global: Vector3) -> Vector3 {
    get_global_transform_top_level(tree, node_id)
        .inverse()
        .xform(global)
}

/// Builds an orthonormal rotation basis whose local **-Z** axis points along
/// `forward`, with `up` establishing the roll. Mirrors Godot's
/// `Basis::looking_at`.
///
/// Note: this is the *opposite* Z convention from this crate's
/// [`Transform3D::looking_at`], which orients +Z toward the target. Godot's
/// `Node3D.look_at` faces the **-Z** axis at the target, which is what we need.
fn basis_looking_at(forward: Vector3, up: Vector3) -> Basis {
    // +Z column points away from the target so that -Z faces it.
    let v_z = (-forward).normalized();
    let v_x = up.cross(v_z).normalized();
    let v_y = v_z.cross(v_x);
    Basis {
        x: v_x,
        y: v_y,
        z: v_z,
    }
}

/// Orients the node so its local **-Z** axis points at `target` (world space),
/// using `up` for roll — Godot's `Node3D.look_at`. The node's global position
/// is used as the eye; its local translation and per-axis scale are preserved.
pub fn look_at(tree: &mut SceneTree, node_id: NodeId, target: Vector3, up: Vector3) {
    let eye = get_global_transform_top_level(tree, node_id).origin;
    let forward = target - eye;
    if forward.length_squared() < 1e-12 {
        // Degenerate: target coincides with the eye — leave orientation as-is.
        return;
    }
    let world_basis = basis_looking_at(forward, up);

    // Convert the world-space rotation into the node's local space so that the
    // resulting global orientation matches `world_basis` regardless of parent.
    let parent_basis = effective_parent_global(tree, node_id).basis;
    let local_rot = parent_basis.inverse() * world_basis;

    // Preserve the current local translation and per-axis scale.
    let current = get_local_transform(tree, node_id);
    let scale = current.basis.get_scale();
    let new_basis = Basis {
        x: local_rot.x * scale.x,
        y: local_rot.y * scale.y,
        z: local_rot.z * scale.z,
    };
    set_local_transform(
        tree,
        node_id,
        Transform3D {
            basis: new_basis,
            origin: current.origin,
        },
    );
}

/// Convenience wrapper for [`look_at`] using the default up vector `(0, 1, 0)`.
pub fn look_at_default_up(tree: &mut SceneTree, node_id: NodeId, target: Vector3) {
    look_at(tree, node_id, target, Vector3::new(0.0, 1.0, 0.0));
}

/// Rotates the node in place about `axis` by `angle_rad` radians, composing the
/// rotation onto its current local basis (Godot's `Node3D.rotate`, a
/// parent-space pre-multiply).
pub fn rotate(tree: &mut SceneTree, node_id: NodeId, axis: Vector3, angle_rad: f32) {
    let current = get_local_transform(tree, node_id);
    let new_basis = current.basis.rotated(axis, angle_rad);
    set_local_transform(
        tree,
        node_id,
        Transform3D {
            basis: new_basis,
            origin: current.origin,
        },
    );
}

/// Translates the node in its own **local** space by `offset` (Godot's
/// `Node3D.translate`): the offset is rotated by the current basis before being
/// added to the local origin.
pub fn translate(tree: &mut SceneTree, node_id: NodeId, offset: Vector3) {
    let current = get_local_transform(tree, node_id);
    let new_origin = current.origin + current.basis.xform(offset);
    set_local_transform(
        tree,
        node_id,
        Transform3D {
            basis: current.basis,
            origin: new_origin,
        },
    );
}

/// Returns `true` only when this node's own `visible` flag is set **and** every
/// ancestor up to the root is visible (Godot's `Node3D.is_visible_in_tree`).
///
/// Reuses the [`is_visible`] accessor (the `"visible"` property that
/// [`set_visible`] writes).
pub fn is_visible_in_tree(tree: &SceneTree, node_id: NodeId) -> bool {
    let mut current = node_id;
    loop {
        if !is_visible(tree, current) {
            return false;
        }
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent_id) => current = parent_id,
            None => return true,
        }
    }
}

// ===========================================================================
// Camera3D properties
// ===========================================================================

/// Sets the `"fov"` property (field of view in degrees).
pub fn set_fov(tree: &mut SceneTree, node_id: NodeId, fov: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("fov", Variant::Float(fov));
    }
}

/// Reads the `"fov"` property, defaulting to `75.0`.
pub fn get_fov(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("fov") {
            Variant::Float(f) => f,
            _ => 75.0,
        })
        .unwrap_or(75.0)
}

/// Sets the `"near"` property (near clipping plane distance).
pub fn set_near(tree: &mut SceneTree, node_id: NodeId, near: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("near", Variant::Float(near));
    }
}

/// Reads the `"near"` property, defaulting to `0.05`.
pub fn get_near(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("near") {
            Variant::Float(f) => f,
            _ => 0.05,
        })
        .unwrap_or(0.05)
}

/// Sets the `"far"` property (far clipping plane distance).
pub fn set_far(tree: &mut SceneTree, node_id: NodeId, far: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("far", Variant::Float(far));
    }
}

/// Reads the `"far"` property, defaulting to `4000.0`.
pub fn get_far(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("far") {
            Variant::Float(f) => f,
            _ => 4000.0,
        })
        .unwrap_or(4000.0)
}

/// Sets the `"projection"` property (`"perspective"` or `"orthographic"`).
pub fn set_projection_type(tree: &mut SceneTree, node_id: NodeId, projection: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("projection", Variant::String(projection.to_owned()));
    }
}

/// Sets the `"current"` property on a Camera3D node.
pub fn set_camera_current(tree: &mut SceneTree, node_id: NodeId, current: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current", Variant::Bool(current));
    }
}

// ===========================================================================
// MeshInstance3D properties
// ===========================================================================

/// Sets the `"mesh"` property to a resource path string.
pub fn set_mesh_path(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("mesh", Variant::String(path.to_owned()));
    }
}

/// Reads the viewport mesh marker as a path string, if present. Prefers the
/// `__viewport_mesh__` companion (the resolved `primitive:<Type>` marker the
/// `.tscn` parser stores, leaving the real `mesh` as its `SubResource:<id>`),
/// falling back to a marker set directly on `mesh` (tests / legacy).
pub fn get_mesh_path(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("__viewport_mesh__") {
            Variant::String(s) => Some(s),
            _ => match n.get_property("mesh") {
                Variant::String(s) => Some(s),
                _ => None,
            },
        })
}

/// Sets the `"material"` property to a resource path string.
pub fn set_material_path(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("material", Variant::String(path.to_owned()));
    }
}

/// Sets the `"cast_shadow"` property on a MeshInstance3D.
pub fn set_cast_shadow(tree: &mut SceneTree, node_id: NodeId, cast: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("cast_shadow", Variant::Bool(cast));
    }
}

/// Sets the `"visibility_range_begin"` property.
pub fn set_visibility_range_begin(tree: &mut SceneTree, node_id: NodeId, begin: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("visibility_range_begin", Variant::Float(begin));
    }
}

/// Sets the `"visibility_range_end"` property.
pub fn set_visibility_range_end(tree: &mut SceneTree, node_id: NodeId, end: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("visibility_range_end", Variant::Float(end));
    }
}

// ===========================================================================
// Light3D properties
// ===========================================================================

/// Sets the `"light_energy"` property.
pub fn set_light_energy(tree: &mut SceneTree, node_id: NodeId, energy: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("light_energy", Variant::Float(energy));
    }
}

/// Reads the `"light_energy"` property, defaulting to `1.0`.
pub fn get_light_energy(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("light_energy") {
            Variant::Float(f) => f,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// Sets the `"light_color"` property.
pub fn set_light_color(tree: &mut SceneTree, node_id: NodeId, color: Color) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("light_color", Variant::Color(color));
    }
}

/// Reads the `"light_color"` property, defaulting to [`Color::WHITE`].
pub fn get_light_color(tree: &SceneTree, node_id: NodeId) -> Color {
    tree.get_node(node_id)
        .map(|n| match n.get_property("light_color") {
            Variant::Color(c) => c,
            _ => Color::WHITE,
        })
        .unwrap_or(Color::WHITE)
}

/// Sets the `"light_angular_distance"` property (DirectionalLight3D, degrees).
pub fn set_light_angular_distance(tree: &mut SceneTree, node_id: NodeId, degrees: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("light_angular_distance", Variant::Float(degrees));
    }
}

/// Reads the `"light_angular_distance"` property (degrees), defaulting to `0.0`
/// (a point-like sun disc). This is the apparent angular diameter a sky
/// material uses to size the sun.
pub fn get_light_angular_distance(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("light_angular_distance") {
            Variant::Float(f) => f,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Reads a `DirectionalLight3D`'s `"sky_mode"` property, defaulting to `0`
/// (`LIGHT_AND_SKY`). Godot's `DirectionalLight3D.SkyMode`: `0 = LIGHT_AND_SKY`,
/// `1 = LIGHT_ONLY` (excluded from sky rendering — no sky sun disc),
/// `2 = SKY_ONLY`.
pub fn get_light_sky_mode(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("sky_mode") {
            Variant::Int(v) => v,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Sets the `"shadow_enabled"` property on a light.
pub fn set_shadow_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("shadow_enabled", Variant::Bool(enabled));
    }
}

/// Sets the `"direction"` property on a DirectionalLight3D.
pub fn set_direction(tree: &mut SceneTree, node_id: NodeId, dir: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("direction", Variant::Vector3(dir));
    }
}

/// Sets the `"range"` property on an OmniLight3D.
pub fn set_range(tree: &mut SceneTree, node_id: NodeId, range: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("range", Variant::Float(range));
    }
}

/// Sets the `"attenuation"` property on an OmniLight3D.
pub fn set_attenuation(tree: &mut SceneTree, node_id: NodeId, attenuation: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("attenuation", Variant::Float(attenuation));
    }
}

// ---------------------------------------------------------------------------
// MultiMeshInstance3D helpers
// ---------------------------------------------------------------------------

/// Returns the number of instances in a MultiMeshInstance3D.
pub fn get_multimesh_instance_count(tree: &SceneTree, node_id: NodeId) -> u32 {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("instance_count") {
            Variant::Int(v) => Some(v as u32),
            _ => None,
        })
        .unwrap_or(0)
}

/// Returns the per-instance transform for the given instance index.
pub fn get_multimesh_instance_transform(
    tree: &SceneTree,
    node_id: NodeId,
    _index: usize,
) -> gdcore::math3d::Transform3D {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("instance_transform") {
            Variant::Transform3D(t) => Some(t),
            _ => None,
        })
        .unwrap_or(gdcore::math3d::Transform3D::IDENTITY)
}

/// Returns the per-instance color for the given instance index.
pub fn get_multimesh_instance_color(tree: &SceneTree, node_id: NodeId, _index: usize) -> Color {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("instance_color") {
            Variant::Color(c) => Some(c),
            _ => None,
        })
        .unwrap_or(Color::WHITE)
}

/// Returns the mesh type name for a MultiMeshInstance3D, if set.
pub fn get_multimesh_mesh_type(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("multimesh_mesh_type") {
            Variant::String(s) => Some(s),
            _ => None,
        })
}

// ---------------------------------------------------------------------------
// Material override helpers
// ---------------------------------------------------------------------------

/// Returns the `material_override` path on a MeshInstance3D, if set.
pub fn get_material_override(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("material_override") {
            Variant::String(s) => Some(s),
            _ => None,
        })
}

/// Returns the per-surface material override path for the given surface index.
pub fn get_surface_override_material(
    tree: &SceneTree,
    node_id: NodeId,
    surface: usize,
) -> Option<String> {
    let key = format!("surface_material_override/{surface}");
    tree.get_node(node_id)
        .and_then(|n| match n.get_property(&key) {
            Variant::String(s) => Some(s),
            _ => None,
        })
}

// ---------------------------------------------------------------------------
// SpotLight3D helpers
// ---------------------------------------------------------------------------

/// Returns the spot angle in degrees.
pub fn get_spot_angle(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("spot_angle") {
            Variant::Float(v) => Some(v),
            _ => None,
        })
        .unwrap_or(45.0)
}

/// Returns the spot range.
pub fn get_spot_range(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("spot_range") {
            Variant::Float(v) => Some(v),
            _ => None,
        })
        .unwrap_or(10.0)
}

/// Returns the spot attenuation.
pub fn get_spot_attenuation(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("spot_attenuation") {
            Variant::Float(v) => Some(v),
            _ => None,
        })
        .unwrap_or(1.0)
}

/// Returns the spot angle attenuation.
pub fn get_spot_angle_attenuation(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("spot_angle_attenuation") {
            Variant::Float(v) => Some(v),
            _ => None,
        })
        .unwrap_or(1.0)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn approx_vec3(a: Vector3, b: Vector3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    fn make_tree() -> SceneTree {
        SceneTree::new()
    }

    // -- Node3D position/rotation/scale -------------------------------------

    #[test]
    fn default_position_is_zero() {
        let tree = make_tree();
        assert_eq!(get_position(&tree, tree.root_id()), Vector3::ZERO);
    }

    #[test]
    fn set_get_position() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cube", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        set_position(&mut tree, id, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(get_position(&tree, id), Vector3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn set_get_rotation() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Rotated", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        let euler = Vector3::new(0.5, 1.0, 0.2);
        set_rotation(&mut tree, id, euler);
        assert_eq!(get_rotation(&tree, id), euler);
    }

    #[test]
    fn default_rotation_is_zero() {
        let tree = make_tree();
        assert_eq!(get_rotation(&tree, tree.root_id()), Vector3::ZERO);
    }

    #[test]
    fn set_get_scale() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Scaled", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        set_scale(&mut tree, id, Vector3::new(2.0, 3.0, 4.0));
        assert_eq!(get_scale(&tree, id), Vector3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn default_scale_is_one() {
        let tree = make_tree();
        assert_eq!(get_scale(&tree, tree.root_id()), Vector3::ONE);
    }

    // -- Local transform composition ----------------------------------------

    #[test]
    fn local_transform_translate_only() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        set_position(&mut tree, id, Vector3::new(10.0, 20.0, 30.0));
        let t = get_local_transform(&tree, id);
        let p = t.xform(Vector3::ZERO);
        assert!(approx_vec3(p, Vector3::new(10.0, 20.0, 30.0)));
    }

    #[test]
    fn local_transform_scale_only() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        set_scale(&mut tree, id, Vector3::new(2.0, 3.0, 4.0));
        let t = get_local_transform(&tree, id);
        let p = t.xform(Vector3::new(1.0, 1.0, 1.0));
        assert!(approx_vec3(p, Vector3::new(2.0, 3.0, 4.0)));
    }

    // -- Global transform through hierarchy ---------------------------------

    #[test]
    fn global_transform_three_levels() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let gp = Node::new("GP", "Node3D");
        let gp_id = tree.add_child(root, gp).unwrap();
        set_position(&mut tree, gp_id, Vector3::new(100.0, 0.0, 0.0));

        let parent = Node::new("P", "Node3D");
        let p_id = tree.add_child(gp_id, parent).unwrap();
        set_position(&mut tree, p_id, Vector3::new(0.0, 50.0, 0.0));

        let child = Node::new("C", "Node3D");
        let c_id = tree.add_child(p_id, child).unwrap();
        set_position(&mut tree, c_id, Vector3::new(0.0, 0.0, 10.0));

        let global = get_global_transform(&tree, c_id);
        let world_pos = global.xform(Vector3::ZERO);
        assert!(approx_vec3(world_pos, Vector3::new(100.0, 50.0, 10.0)));
    }

    #[test]
    fn moving_parent_updates_child_global() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node3D");
        let p_id = tree.add_child(root, parent).unwrap();
        set_position(&mut tree, p_id, Vector3::new(10.0, 0.0, 0.0));

        let child = Node::new("Child", "Node3D");
        let c_id = tree.add_child(p_id, child).unwrap();
        set_position(&mut tree, c_id, Vector3::new(5.0, 5.0, 5.0));

        let g1 = get_global_transform(&tree, c_id).xform(Vector3::ZERO);
        assert!(approx_vec3(g1, Vector3::new(15.0, 5.0, 5.0)));

        set_position(&mut tree, p_id, Vector3::new(100.0, 0.0, 0.0));
        let g2 = get_global_transform(&tree, c_id).xform(Vector3::ZERO);
        assert!(approx_vec3(g2, Vector3::new(105.0, 5.0, 5.0)));
    }

    // -- set_global_position ------------------------------------------------

    #[test]
    fn set_global_position_inverse_transform() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node3D");
        let p_id = tree.add_child(root, parent).unwrap();
        set_position(&mut tree, p_id, Vector3::new(100.0, 50.0, 25.0));

        let child = Node::new("Child", "Node3D");
        let c_id = tree.add_child(p_id, child).unwrap();

        set_global_position(&mut tree, c_id, Vector3::new(150.0, 75.0, 50.0));

        let local = get_position(&tree, c_id);
        assert!(approx_vec3(local, Vector3::new(50.0, 25.0, 25.0)));

        let g = get_global_transform(&tree, c_id).xform(Vector3::ZERO);
        assert!(approx_vec3(g, Vector3::new(150.0, 75.0, 50.0)));
    }

    // -- Visibility ---------------------------------------------------------

    #[test]
    fn default_visible_is_true() {
        let tree = make_tree();
        assert!(is_visible(&tree, tree.root_id()));
    }

    #[test]
    fn set_visible_false() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node3D");
        let id = tree.add_child(root, node).unwrap();

        set_visible(&mut tree, id, false);
        assert!(!is_visible(&tree, id));
    }

    // -- Node3D transform ops / top_level / visible-in-tree -----------------

    #[test]
    fn node3d_look_at_toplevel_and_visibility_propagation_from_scene() {
        // (1) Visibility propagation: a visible child under a hidden parent is
        //     not visible in the tree; revealing the parent makes it visible.
        {
            let mut tree = make_tree();
            let root = tree.root_id();

            let parent = Node::new("HiddenParent", "Node3D");
            let p_id = tree.add_child(root, parent).unwrap();
            set_visible(&mut tree, p_id, false);

            let child = Node::new("Mesh", "MeshInstance3D");
            let c_id = tree.add_child(p_id, child).unwrap();

            // The child is itself visible (default) ...
            assert!(is_visible(&tree, c_id));
            // ... but a hidden ancestor hides it in the tree.
            assert!(!is_visible_in_tree(&tree, c_id));

            // Reveal the parent → the whole chain is visible.
            set_visible(&mut tree, p_id, true);
            assert!(is_visible_in_tree(&tree, c_id));
        }

        // (2) top_level: a child under a translated parent ignores the parent's
        //     transform, so its global origin equals its own local origin.
        {
            let mut tree = make_tree();
            let root = tree.root_id();

            let parent = Node::new("Parent", "Node3D");
            let p_id = tree.add_child(root, parent).unwrap();
            set_position(&mut tree, p_id, Vector3::new(10.0, 0.0, 0.0));

            let child = Node::new("Child", "Node3D");
            let c_id = tree.add_child(p_id, child).unwrap();
            set_position(&mut tree, c_id, Vector3::new(1.0, 2.0, 3.0));

            // Without top_level the parent offset is included.
            let without = to_global(&tree, c_id, Vector3::ZERO);
            assert!(approx_vec3(without, Vector3::new(11.0, 2.0, 3.0)));

            // With top_level the parent offset is ignored: global == own local.
            set_top_level(&mut tree, c_id, true);
            let with = to_global(&tree, c_id, Vector3::ZERO);
            assert!(approx_vec3(with, Vector3::new(1.0, 2.0, 3.0)));

            // The difference is exactly the parent translation.
            assert!(approx_vec3(without - with, Vector3::new(10.0, 0.0, 0.0)));
        }

        // (3) look_at: the node's local -Z axis points at the target.
        {
            let mut tree = make_tree();
            let root = tree.root_id();

            let node = Node::new("Looker", "Node3D");
            let id = tree.add_child(root, node).unwrap();
            set_position(&mut tree, id, Vector3::ZERO);

            // Look toward -Z: local -Z should point at (0, 0, -1).
            look_at(
                &mut tree,
                id,
                Vector3::new(0.0, 0.0, -5.0),
                Vector3::new(0.0, 1.0, 0.0),
            );
            let neg_z = (to_global(&tree, id, Vector3::new(0.0, 0.0, -1.0))
                - to_global(&tree, id, Vector3::ZERO))
            .normalized();
            assert!(approx_vec3(neg_z, Vector3::new(0.0, 0.0, -1.0)));

            // Look toward +X: local -Z should point at (1, 0, 0).
            look_at(
                &mut tree,
                id,
                Vector3::new(5.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            );
            let neg_z2 = (to_global(&tree, id, Vector3::new(0.0, 0.0, -1.0))
                - to_global(&tree, id, Vector3::ZERO))
            .normalized();
            assert!(approx_vec3(neg_z2, Vector3::new(1.0, 0.0, 0.0)));
        }
    }

    // -- Camera3D properties ------------------------------------------------

    #[test]
    fn camera3d_fov_default_and_set() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera3D");
        let id = tree.add_child(root, node).unwrap();

        assert!((get_fov(&tree, id) - 75.0).abs() < 1e-6);
        set_fov(&mut tree, id, 90.0);
        assert!((get_fov(&tree, id) - 90.0).abs() < 1e-6);
    }

    #[test]
    fn camera3d_near_far_defaults() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera3D");
        let id = tree.add_child(root, node).unwrap();

        assert!((get_near(&tree, id) - 0.05).abs() < 1e-6);
        assert!((get_far(&tree, id) - 4000.0).abs() < 1e-6);

        set_near(&mut tree, id, 0.1);
        set_far(&mut tree, id, 1000.0);
        assert!((get_near(&tree, id) - 0.1).abs() < 1e-6);
        assert!((get_far(&tree, id) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn camera3d_projection_type() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera3D");
        let id = tree.add_child(root, node).unwrap();

        set_projection_type(&mut tree, id, "orthographic");
        let n = tree.get_node(id).unwrap();
        assert_eq!(
            n.get_property("projection"),
            Variant::String("orthographic".into())
        );
    }

    #[test]
    fn camera3d_current() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera3D");
        let id = tree.add_child(root, node).unwrap();

        set_camera_current(&mut tree, id, true);
        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("current"), Variant::Bool(true));
    }

    // -- MeshInstance3D properties -------------------------------------------

    #[test]
    fn mesh_path_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Mesh", "MeshInstance3D");
        let id = tree.add_child(root, node).unwrap();

        assert_eq!(get_mesh_path(&tree, id), None);
        set_mesh_path(&mut tree, id, "res://cube.tres");
        assert_eq!(get_mesh_path(&tree, id), Some("res://cube.tres".into()));
    }

    #[test]
    fn mesh_material_and_shadow() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Mesh", "MeshInstance3D");
        let id = tree.add_child(root, node).unwrap();

        set_material_path(&mut tree, id, "res://mat.tres");
        set_cast_shadow(&mut tree, id, false);

        let n = tree.get_node(id).unwrap();
        assert_eq!(
            n.get_property("material"),
            Variant::String("res://mat.tres".into())
        );
        assert_eq!(n.get_property("cast_shadow"), Variant::Bool(false));
    }

    #[test]
    fn mesh_visibility_range() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Mesh", "MeshInstance3D");
        let id = tree.add_child(root, node).unwrap();

        set_visibility_range_begin(&mut tree, id, 10.0);
        set_visibility_range_end(&mut tree, id, 100.0);

        let n = tree.get_node(id).unwrap();
        assert_eq!(
            n.get_property("visibility_range_begin"),
            Variant::Float(10.0)
        );
        assert_eq!(
            n.get_property("visibility_range_end"),
            Variant::Float(100.0)
        );
    }

    // -- Light3D properties -------------------------------------------------

    #[test]
    fn light_energy_default_and_set() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Light", "OmniLight3D");
        let id = tree.add_child(root, node).unwrap();

        assert!((get_light_energy(&tree, id) - 1.0).abs() < 1e-6);
        set_light_energy(&mut tree, id, 2.5);
        assert!((get_light_energy(&tree, id) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn light_color_default_and_set() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Light", "DirectionalLight3D");
        let id = tree.add_child(root, node).unwrap();

        assert_eq!(get_light_color(&tree, id), Color::WHITE);
        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        set_light_color(&mut tree, id, red);
        assert_eq!(get_light_color(&tree, id), red);
    }

    #[test]
    fn light_shadow_enabled() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Light", "DirectionalLight3D");
        let id = tree.add_child(root, node).unwrap();

        set_shadow_enabled(&mut tree, id, true);
        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("shadow_enabled"), Variant::Bool(true));
    }

    #[test]
    fn directional_light_direction() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Sun", "DirectionalLight3D");
        let id = tree.add_child(root, node).unwrap();

        let dir = Vector3::new(0.0, -1.0, -0.5);
        set_direction(&mut tree, id, dir);
        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("direction"), Variant::Vector3(dir));
    }

    #[test]
    fn omni_light_range_and_attenuation() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Lamp", "OmniLight3D");
        let id = tree.add_child(root, node).unwrap();

        set_range(&mut tree, id, 16.0);
        set_attenuation(&mut tree, id, 2.0);

        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("range"), Variant::Float(16.0));
        assert_eq!(n.get_property("attenuation"), Variant::Float(2.0));
    }
}
