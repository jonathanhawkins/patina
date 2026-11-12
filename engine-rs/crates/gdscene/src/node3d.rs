//! 3D node property helpers for Node3D, Camera3D, MeshInstance3D, and Light3D.
//!
//! Follows the same pattern as [`node2d`](crate::node2d): typed helper
//! functions that read and write well-known properties on nodes via the
//! [`SceneTree`].

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, EulerOrder, Transform3D};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Node3D properties
// ===========================================================================

// ---------------------------------------------------------------------------
// Canonical local-transform state (pat-0q2kx.29)
// ---------------------------------------------------------------------------
//
// Godot's Node3D keeps ONE local transform; `position` / `rotation` / `scale` /
// `transform` are views over it with dirty-flag conversion, so reading any of
// them after writing another always reflects the write (scene/3d/node_3d.cpp).
//
// Patina used to store two competing representations: the `.tscn` parser wrote
// `transform`, the setters wrote separate keys, and `get_local_transform`
// preferred `transform` when present. The consequence was silent and severe —
// on any node loaded with a `transform =` line, a script writing `position`
// changed nothing that rendering or physics could see.
//
// These helpers make every write land on the canonical `transform` while
// keeping the component views in sync, so both directions stay coherent.

/// Splits a possibly-scaled basis into its rotation (Euler) and scale parts.
///
/// `Basis::to_euler` assumes an orthonormal basis — handing it a scaled one
/// yields garbage angles — so the columns are normalised first. A zero-length
/// column (degenerate scale) is left as-is rather than dividing by zero.
fn decompose_basis(basis: Basis) -> (Vector3, Vector3) {
    decompose_basis_ordered(basis, EulerOrder::default())
}

/// [`decompose_basis`] honouring the node's `rotation_order` (pat-mgixk.29).
///
/// 🔑 This is the half that makes the round trip hold: `set_position` reads the
/// transform through [`local_transform_of`] (which COMPOSES with the node's
/// order) and writes it back through [`store_local_transform`] (which
/// DECOMPOSES here). Decomposing with a different order than the composition
/// used would rewrite `rotation` into a different triple that happens to name
/// the same basis — so moving a node would silently change its authored
/// rotation numbers.
fn decompose_basis_ordered(basis: Basis, order: EulerOrder) -> (Vector3, Vector3) {
    let scale = basis.get_scale();
    let norm = |v: Vector3, s: f32| {
        if s.abs() > 1e-6 {
            Vector3::new(v.x / s, v.y / s, v.z / s)
        } else {
            v
        }
    };
    let rotation = Basis {
        x: norm(basis.x, scale.x),
        y: norm(basis.y, scale.y),
        z: norm(basis.z, scale.z),
    };
    (rotation.to_euler_order(order), scale)
}

/// Rebuilds a basis from Euler angles and per-axis scale (Godot's R * S order).
fn compose_basis(euler: Vector3, scale: Vector3) -> Basis {
    compose_basis_ordered(euler, scale, EulerOrder::default())
}

/// [`compose_basis`] honouring the node's `rotation_order` (pat-mgixk.29).
///
/// `EulerOrder::default()` is `Yxz`, which is both Godot's default and the
/// convention Patina's plain `Basis::from_euler` already used — so every
/// existing caller keeps its exact behaviour and only a node that AUTHORS a
/// non-default `rotation_order` takes a different path.
fn compose_basis_ordered(euler: Vector3, scale: Vector3, order: EulerOrder) -> Basis {
    let b = Basis::from_euler_order(euler, order);
    Basis {
        x: b.x * scale.x,
        y: b.y * scale.y,
        z: b.z * scale.z,
    }
}

/// The node's `rotation_order` as an [`EulerOrder`] (pat-mgixk.29).
///
/// ClassDB registers the property with a default of `Int(2)` = `Yxz`, and
/// `from_i64` rejects out-of-range values, so an unauthored or nonsense value
/// falls back to Godot's default rather than silently picking order 0.
fn rotation_order_of(node: &crate::node::Node) -> EulerOrder {
    match node.get_property("rotation_order") {
        Variant::Int(i) => EulerOrder::from_i64(i).unwrap_or_default(),
        _ => EulerOrder::default(),
    }
}

/// The node's canonical local transform: the explicit `transform` when present,
/// otherwise composed from the component keys.
///
/// Public because a caller holding only a `&Node` (an animation or root-motion
/// apply, which never sees the tree) must read the SAME transform the setters
/// write. Reading the raw `position` property instead is how root motion
/// accumulated its delta onto a stale base (pat-mgixk.29).
pub fn local_transform_of(node: &crate::node::Node) -> Transform3D {
    if let Variant::Transform3D(t) = node.get_property("transform") {
        return t;
    }
    let pos = match node.get_property("position") {
        Variant::Vector3(v) => v,
        _ => Vector3::ZERO,
    };
    let rot = match node.get_property("rotation") {
        Variant::Vector3(v) => v,
        _ => Vector3::ZERO,
    };
    let scl = match node.get_property("scale") {
        Variant::Vector3(v) => v,
        _ => Vector3::ONE,
    };
    Transform3D {
        basis: compose_basis_ordered(rot, scl, rotation_order_of(node)),
        origin: pos,
    }
}

/// Stores `t` as the canonical `transform` and refreshes EVERY component view
/// so a raw `get_property` agrees with it whichever representation the caller
/// consults — as in Godot, where `position` / `rotation` / `rotation_degrees` /
/// `quaternion` / `basis` / `scale` are all views of one `data.local_transform`
/// (`scene/3d/node_3d.cpp`).
///
/// ⚠️ `quaternion` and `basis` were MISSING here, and that was a regression I
/// introduced with pat-mgixk.29. Routing animation and tween writes through this
/// funnel was right, but the funnel converted an incoming `quaternion` into the
/// basis and never wrote the `quaternion` KEY back — so a Rotation3D track moved
/// the node correctly while a later raw `get_property("quaternion")` read `Nil`.
/// `gltf_anim_import::gltf_import_animation_clip_plays` is the test that caught
/// it ("the rotation track did not reach the node, got Nil"); my own pat-mgixk.29
/// acceptance asserted on the composed basis instead of the raw property, which
/// is exactly why it did not.
///
/// `get_derived_property` can compute both on demand, but that only serves
/// callers who go through it; a direct `Node::get_property` cannot.
pub(crate) fn store_local_transform(node: &mut crate::node::Node, t: Transform3D) {
    let (euler, scale) = decompose_basis_ordered(t.basis, rotation_order_of(node));
    node.set_property("transform", Variant::Transform3D(t));
    node.set_property("position", Variant::Vector3(t.origin));
    node.set_property("rotation", Variant::Vector3(euler));
    node.set_property("scale", Variant::Vector3(scale));
    node.set_property("basis", Variant::Basis(t.basis));
    node.set_property(
        "quaternion",
        Variant::Quaternion(t.basis.get_rotation_quaternion()),
    );
}

/// Whether this node stores a 3D transform, judged from the property types it
/// already holds rather than from ClassDB.
///
/// Used only where the written value cannot discriminate on its own — a
/// sub-path like `position:x` carries a bare `Float` whether the node is 2D or
/// 3D. The whole-property cases need no guard, because `Vector3`, `Quaternion`,
/// `Basis` and `Transform3D` are 3D-only payloads: a Node2D writes `position` as
/// a `Vector2` and so falls through untouched.
pub(crate) fn looks_like_node3d(node: &crate::node::Node) -> bool {
    matches!(node.get_property("transform"), Variant::Transform3D(_))
        || matches!(node.get_property("position"), Variant::Vector3(_))
        || matches!(node.get_property("scale"), Variant::Vector3(_))
        || matches!(node.get_property("rotation"), Variant::Vector3(_))
}

/// The node's local Euler rotation, derived from the canonical transform.
///
/// The companion read to [`local_transform_of`] for a caller holding only a
/// `&Node`: reading the raw `rotation` property can disagree with the explicit
/// `transform` a `.tscn` node carries, which is the whole defect pat-mgixk.29
/// fixes.
pub fn local_rotation_of(node: &crate::node::Node) -> Vector3 {
    decompose_basis(local_transform_of(node).basis).0
}

/// Writes ONE transform property through the canonical local transform, the way
/// Godot's `Node3D` setters all funnel into `data.local_transform`
/// (`scene/3d/node_3d.cpp`).
///
/// Returns `false` when `prop` is not a transform property, so a caller can fall
/// back to a raw `set_property` for everything else.
///
/// ⚠️ This exists because every non-script writer bypassed the component
/// setters. Tweens, animation value tracks, 3D rotation tracks and root motion
/// all called `Node::set_property` directly, which has no transform hook — so on
/// a `.tscn`-loaded Node3D (which keeps an explicit `transform =`)
/// `local_transform_of` returned that stale transform and ignored the animated
/// `position` entirely. Nothing moved on screen (pat-mgixk.29).
///
/// `quaternion` and `basis` were worse than stale: `local_transform_of` never
/// read them at all, so an animated `quaternion` — which is exactly what the
/// glTF importer emits for a Rotation3D track — was inert on EVERY node,
/// `.tscn` or not.
pub fn set_transform_component(node: &mut crate::node::Node, prop: &str, value: &Variant) -> bool {
    // A sub-path writes one axis of a vector component (`position:x`), which is
    // how an animation track addresses a single channel. The other two axes have
    // to be read back off the current transform, not defaulted, or animating one
    // axis would silently zero the others.
    if let Some((base, axis)) = prop.split_once(':') {
        // ⚠️ `position:x` is a bare Float on a Node2D as well, so unlike the
        // whole-property cases below the payload cannot discriminate 2D from 3D
        // — and treating a Node2D's Vector2 position as 3D here would rewrite it
        // as a Vector3 and corrupt it.
        //
        // Deliberately NOT `is_class("Node3D")`: ClassDB is not guaranteed to be
        // populated in every runtime or test context, and a ClassDB miss would
        // silently disable this routing exactly where it is hardest to notice.
        // The node's own stored types are always available, so they decide.
        if !looks_like_node3d(node) {
            return false;
        }
        let component = match value {
            Variant::Float(f) => *f as f32,
            Variant::Int(i) => *i as f32,
            _ => return false,
        };
        let current = local_transform_of(node);
        let (euler, scale) = decompose_basis(current.basis);
        let mut vec = match base {
            "position" => current.origin,
            "rotation" => euler,
            "rotation_degrees" => Vector3::new(
                euler.x.to_degrees(),
                euler.y.to_degrees(),
                euler.z.to_degrees(),
            ),
            "scale" => scale,
            _ => return false,
        };
        match axis {
            "x" => vec.x = component,
            "y" => vec.y = component,
            "z" => vec.z = component,
            _ => return false,
        }
        return set_transform_component(node, base, &Variant::Vector3(vec));
    }

    let current = local_transform_of(node);
    let (euler, scale) = decompose_basis(current.basis);
    let updated = match (prop, value) {
        ("position", Variant::Vector3(v)) => Transform3D {
            basis: current.basis,
            origin: *v,
        },
        ("rotation", Variant::Vector3(v)) => Transform3D {
            basis: compose_basis(*v, scale),
            origin: current.origin,
        },
        // Godot stores rotation in radians and exposes `rotation_degrees` as a
        // view, so this converts rather than storing a second representation.
        ("rotation_degrees", Variant::Vector3(v)) => Transform3D {
            basis: compose_basis(
                Vector3::new(v.x.to_radians(), v.y.to_radians(), v.z.to_radians()),
                scale,
            ),
            origin: current.origin,
        },
        ("scale", Variant::Vector3(v)) => Transform3D {
            basis: compose_basis(euler, *v),
            origin: current.origin,
        },
        // `from_quaternion_scale` keeps the node's existing scale: a rotation
        // track must not reset it to one.
        ("quaternion", Variant::Quaternion(q)) => Transform3D {
            basis: gdcore::math3d::Basis::from_quaternion_scale(*q, scale),
            origin: current.origin,
        },
        // A basis carries its own scale, so it replaces the rotation AND scale
        // and only the origin is preserved.
        ("basis", Variant::Basis(b)) => Transform3D {
            basis: *b,
            origin: current.origin,
        },
        ("transform", Variant::Transform3D(t)) => *t,
        _ => return false,
    };
    store_local_transform(node, updated);
    true
}

/// Sets the node's local position, preserving its rotation and scale.
pub fn set_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        let mut t = local_transform_of(node);
        t.origin = pos;
        store_local_transform(node, t);
    }
}

/// Reads the node's local position, defaulting to [`Vector3::ZERO`].
///
/// Prefers the `position` view (always in sync after any setter) and falls back
/// to the canonical `transform`, so a `.tscn`-loaded node that only carries
/// `transform =` still reports its real origin.
pub fn get_position(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    let Some(node) = tree.get_node(node_id) else {
        return Vector3::ZERO;
    };
    match node.get_property("position") {
        Variant::Vector3(v) => v,
        _ => local_transform_of(node).origin,
    }
}

/// Sets the node's local rotation (Euler radians), preserving position and scale.
pub fn set_rotation(tree: &mut SceneTree, node_id: NodeId, euler: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        let mut t = local_transform_of(node);
        let (_, scale) = decompose_basis(t.basis);
        t.basis = compose_basis(euler, scale);
        store_local_transform(node, t);
    }
}

/// Reads the node's local rotation as Euler angles, defaulting to [`Vector3::ZERO`].
pub fn get_rotation(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    let Some(node) = tree.get_node(node_id) else {
        return Vector3::ZERO;
    };
    match node.get_property("rotation") {
        Variant::Vector3(v) => v,
        _ => decompose_basis(local_transform_of(node).basis).0,
    }
}

/// Sets the node's local scale, preserving position and rotation.
pub fn set_scale(tree: &mut SceneTree, node_id: NodeId, scale: Vector3) {
    if let Some(node) = tree.get_node_mut(node_id) {
        let mut t = local_transform_of(node);
        let (euler, _) = decompose_basis(t.basis);
        t.basis = compose_basis(euler, scale);
        store_local_transform(node, t);
    }
}

/// Reads the node's local scale, defaulting to [`Vector3::ONE`].
pub fn get_scale(tree: &SceneTree, node_id: NodeId) -> Vector3 {
    let Some(node) = tree.get_node(node_id) else {
        return Vector3::ONE;
    };
    match node.get_property("scale") {
        Variant::Vector3(v) => v,
        _ => decompose_basis(local_transform_of(node).basis).1,
    }
}

/// Returns the local [`Transform3D`] for a node.
///
/// If the node has an explicit `"transform"` property (as stored by `.tscn`
/// files using `transform = Transform3D(…)`), that value is returned directly.
/// Otherwise the transform is composed from separate `position`, `rotation`,
/// and `scale` properties (composition order matches Godot: T * R * S).
pub fn get_local_transform(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    // Single source of truth for both the "explicit transform wins" rule and the
    // T * R * S composition — see `local_transform_of`. Keeping a second copy of
    // the composition here is how the two representations drifted apart in the
    // first place (pat-0q2kx.29).
    tree.get_node(node_id)
        .map(local_transform_of)
        .unwrap_or(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::ZERO,
        })
}

/// Computes the global transform by walking the parent chain and multiplying
/// local transforms from the root downward, stopping at a `top_level` node.
///
/// 🔑 The `top_level` check used to live in a SEPARATE
/// [`get_global_transform_top_level`], which had no caller outside this module
/// — so all 77 call sites of this function, the 3D render list among them,
/// composed a `top_level` node's transform with its parent's anyway. Godot's
/// `set_as_top_level` makes a node's transform independent of its parent
/// (`scene/3d/node_3d.cpp`), and a node that opts out of its parent and moves
/// with it regardless is the whole bug (pat-mgixk.32).
///
/// Folded in here rather than switched per call site for the reason the split
/// caused the bug in the first place: two functions that differ by one
/// condition get chosen by whoever is writing the call, and the one without the
/// condition is shorter to type.
pub fn get_global_transform(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    let mut chain = Vec::new();
    let mut current = node_id;
    loop {
        chain.push(current);
        // A top-level node ignores its parent chain entirely, so the walk ends
        // with its own local transform as the base.
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

/// Sets the node's local position such that its global position equals `pos`.
///
/// ⚠️ Through [`effective_parent_global`], so a `top_level` node's local
/// position IS its global one (pat-v3co1). This used to divide by the parent's
/// global transform even for a top-level node — whose global transform then
/// ignored that parent again on read — so `look_at_from_position` /
/// `global_position = p` on a top-level node landed it at the wrong place,
/// offset by the parent. The official 3D platformer's follow camera
/// (`set_as_top_level(true)` + `look_at_from_position`) never followed the
/// player because of it.
pub fn set_global_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector3) {
    let parent_global = effective_parent_global(tree, node_id);
    let local_pos = parent_global.inverse().xform(pos);
    set_position(tree, node_id, local_pos);
}

/// The node's WORLD orientation, scale discarded.
pub fn get_global_rotation_quaternion(
    tree: &SceneTree,
    node_id: NodeId,
) -> gdcore::math3d::Quaternion {
    get_global_transform(tree, node_id)
        .basis
        .get_rotation_quaternion()
}

/// Sets the node's local rotation so that its WORLD orientation is `q`,
/// keeping its local position and scale (pat-qg3nu). The physics servers
/// simulate world orientations, so a body under a rotated parent (truck_town's
/// car under `InstancePos`, turned 90 degrees) must be de-parented on the way
/// back, exactly as its position is by [`set_global_position`].
pub fn set_global_rotation_quaternion(
    tree: &mut SceneTree,
    node_id: NodeId,
    q: gdcore::math3d::Quaternion,
) {
    let parent = effective_parent_global(tree, node_id)
        .basis
        .get_rotation_quaternion();
    let local = (parent.inverse() * q).normalized();
    set_rotation(tree, node_id, local.to_euler());
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
/// ⚠️ Now identical to [`get_global_transform`], which honours `top_level`
/// itself (pat-mgixk.32). Kept as a name rather than deleted because it reads
/// at a call site as "and I did mean with top_level", and because deleting it
/// would churn every existing caller for no behavioural gain. Delegating
/// rather than duplicating is the point: the two bodies differing by one
/// condition is exactly how the render list ended up on the wrong one.
pub fn get_global_transform_top_level(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    get_global_transform(tree, node_id)
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
        let Some(parent_id) = tree.get_node(current).and_then(|n| n.parent()) else {
            return true;
        };
        // ⚠️ The walk STOPS at a non-Node3D ancestor, and that is Godot's rule,
        // not a shortcut. `Node3D::is_visible_in_tree` climbs `data.parent`,
        // which is `Object::cast_to<Node3D>(get_parent())` and therefore null
        // the moment the parent is not a Node3D — so a hidden `Node` or
        // `CanvasItem` above a 3D subtree does not hide it. Walking through
        // would couple the two visibility families, and a hidden Control
        // ancestor would blank the 3D scene under it.
        //
        // 🔑 BOTH tests, because each covers the other's blind spot and a miss
        // here fails SILENTLY in the permissive direction (stop early => a
        // hidden parent renders its children again, which is the very bug this
        // function exists to fix):
        //   * `is_class` compares the node's OWN class name before touching
        //     ClassDB, so a bare `Node3D` group parent resolves even with an
        //     empty ClassDB — but a `MeshInstance3D` parent needs the ClassDB
        //     ancestor walk, and `set_transform_component` above documents that
        //     ClassDB is not populated in every runtime or test context.
        //   * `looks_like_node3d` reads stored property TYPES and so needs no
        //     ClassDB at all — but a transform-less Node3D used purely as a
        //     visibility group stores none of them.
        // A node has to miss both — a ClassDB miss AND no stored transform — to
        // stop the walk, and then it stops one level early rather than
        // disabling propagation outright.
        let parent_is_node3d = tree
            .get_node(parent_id)
            .map(|n| n.is_class("Node3D") || looks_like_node3d(n))
            .unwrap_or(false);
        if !parent_is_node3d {
            return true;
        }
        current = parent_id;
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
///
/// Godot semantics: making a camera current clears every other Camera3D's
/// flag (one current camera per viewport — `Camera3D::make_current`). This
/// helper used to set only this camera's flag, so a second camera made current
/// alongside the auto-current first one never rendered.
pub fn set_camera_current(tree: &mut SceneTree, node_id: NodeId, current: bool) {
    if current {
        crate::camera3d::make_current(tree, node_id);
    } else if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current", Variant::Bool(false));
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
            // `mesh = ExtResource("id")`: the raw property is the reference
            // LITERAL, which no loader can open; `packed_scene` parks the
            // file's path in the `__extres__mesh` companion. Without this an
            // ArrayMesh `.res`/`.tres` on a MeshInstance3D could never load
            // (pat-33wmr). Only while `mesh` still IS that reference: a
            // script that reassigns `mesh` must not be shadowed by the stale
            // companion.
            _ => match n.get_property("mesh") {
                Variant::String(s)
                    if s.starts_with("ExtResource(") || s.starts_with("ExtResource:") =>
                {
                    match n.get_property("__extres__mesh") {
                        Variant::String(path) => Some(path),
                        _ => Some(s),
                    }
                }
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

/// Floats per instance in a `MULTIMESH_TRANSFORM_3D` buffer: a row-major 3x4
/// matrix, each basis row followed by one origin component
/// (`servers/rendering/renderer_rd/storage_rd/mesh_storage.cpp:1896-1907`).
const MULTIMESH_XFORM_3D_FLOATS: usize = 12;
/// Floats per instance in a `MULTIMESH_TRANSFORM_2D` buffer (a 2x4 matrix).
const MULTIMESH_XFORM_2D_FLOATS: usize = 8;
/// The per-instance color block that follows the transform when `use_colors`.
const MULTIMESH_COLOR_FLOATS: usize = 4;

/// The authored MultiMesh buffer copied onto the node by
/// `PackedScene`'s `__multimesh__*` flattening, if the node has one.
fn multimesh_buffer(tree: &SceneTree, node_id: NodeId) -> Option<Vec<f32>> {
    match tree.get_node(node_id)?.get_property("__multimesh__buffer") {
        Variant::PackedFloat32Array(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// `(per-instance stride, transform floats)` for the node's MultiMesh buffer.
///
/// ⚠️ Godot's `transform_format` defaults to `TRANSFORM_2D` (`MultiMesh.xml`
/// gives `default="0"`), so a 3D scene has to say `transform_format = 1`.
/// Assuming 3D because the node is a MultiMeshInstance3D would mis-stride an
/// authored 2D buffer and read each instance from the wrong offset.
fn multimesh_stride(tree: &SceneTree, node_id: NodeId) -> Option<(usize, usize)> {
    let node = tree.get_node(node_id)?;
    let xform = match node.get_property("__multimesh__transform_format") {
        Variant::Int(1) => MULTIMESH_XFORM_3D_FLOATS,
        _ => MULTIMESH_XFORM_2D_FLOATS,
    };
    let mut stride = xform;
    if matches!(
        node.get_property("__multimesh__use_colors"),
        Variant::Bool(true)
    ) {
        stride += MULTIMESH_COLOR_FLOATS;
    }
    Some((stride, xform))
}

/// Decodes instance `index`'s transform out of the authored buffer.
fn multimesh_buffer_transform(
    tree: &SceneTree,
    node_id: NodeId,
    index: usize,
) -> Option<Transform3D> {
    let (stride, xform) = multimesh_stride(tree, node_id)?;
    // A 2D buffer carries no 3D basis; fall back rather than invent one.
    if xform != MULTIMESH_XFORM_3D_FLOATS {
        return None;
    }
    let buffer = multimesh_buffer(tree, node_id)?;
    let base = index.checked_mul(stride)?;
    let f = buffer.get(base..base.checked_add(MULTIMESH_XFORM_3D_FLOATS)?)?;

    // 🔑 Godot writes the basis ROWS (`basis.rows[0][0..2]`, then `origin.x`,
    // and so on); our `Basis` stores COLUMNS in `x`/`y`/`z`. So each of our
    // columns is strided out of the buffer, not read contiguously. Reading
    // `f[0..3]` into `basis.x` is the natural-looking version and it is the
    // transpose — invisible for a pure translation, wrong for any rotation.
    Some(Transform3D {
        basis: Basis {
            x: Vector3::new(f[0], f[4], f[8]),
            y: Vector3::new(f[1], f[5], f[9]),
            z: Vector3::new(f[2], f[6], f[10]),
        },
        origin: Vector3::new(f[3], f[7], f[11]),
    })
}

/// Decodes instance `index`'s color, which sits directly after its transform.
fn multimesh_buffer_color(tree: &SceneTree, node_id: NodeId, index: usize) -> Option<Color> {
    if !matches!(
        tree.get_node(node_id)?
            .get_property("__multimesh__use_colors"),
        Variant::Bool(true)
    ) {
        return None;
    }
    let (stride, xform) = multimesh_stride(tree, node_id)?;
    let buffer = multimesh_buffer(tree, node_id)?;
    let base = index.checked_mul(stride)?.checked_add(xform)?;
    let f = buffer.get(base..base.checked_add(MULTIMESH_COLOR_FLOATS)?)?;
    Some(Color::new(f[0], f[1], f[2], f[3]))
}

/// Returns the number of instances in a MultiMeshInstance3D.
pub fn get_multimesh_instance_count(tree: &SceneTree, node_id: NodeId) -> u32 {
    tree.get_node(node_id)
        .and_then(|n| {
            // The authored sub-resource wins over the node-level property.
            match n.get_property("__multimesh__instance_count") {
                Variant::Int(v) => Some(v as u32),
                _ => match n.get_property("instance_count") {
                    Variant::Int(v) => Some(v as u32),
                    _ => None,
                },
            }
        })
        .unwrap_or(0)
}

/// Returns the per-instance transform for the given instance index.
///
/// Prefers the authored MultiMesh buffer, so the N expanded instances of a
/// MultiMeshInstance3D land where the scene placed them. Falls back to the
/// node's single `instance_transform` property when no buffer was authored —
/// which is all this used to do, stacking every instance at one position
/// (pat-s3rrv.112).
pub fn get_multimesh_instance_transform(
    tree: &SceneTree,
    node_id: NodeId,
    index: usize,
) -> Transform3D {
    if let Some(t) = multimesh_buffer_transform(tree, node_id, index) {
        return t;
    }
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("instance_transform") {
            Variant::Transform3D(t) => Some(t),
            _ => None,
        })
        .unwrap_or(Transform3D::IDENTITY)
}

/// Returns the per-instance color for the given instance index.
pub fn get_multimesh_instance_color(tree: &SceneTree, node_id: NodeId, index: usize) -> Color {
    if let Some(c) = multimesh_buffer_color(tree, node_id, index) {
        return c;
    }
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("instance_color") {
            Variant::Color(c) => Some(c),
            _ => None,
        })
        .unwrap_or(Color::WHITE)
}

/// How many of a MultiMeshInstance3D's instances are actually drawn.
///
/// Godot's `MultiMesh.visible_instance_count` is a PREFIX count with `-1` as the
/// "draw all" sentinel, so a buffer of 3 with the count at 2 draws the first two
/// and leaves the third authored but unrendered.
///
/// ⚠️ The clamping rule is defined once, by
/// [`gdserver3d::multimesh::MultiMesh3D::visible_count`]: negative means all,
/// otherwise `min(count, instance_count)`. This reader returns the RAW authored
/// value so callers apply that one rule rather than each inventing a clamp —
/// the sync path ignored the property entirely and drew every instance
/// (pat-mgixk.50).
pub fn get_multimesh_visible_instance_count(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .and_then(|n| {
            // The authored sub-resource wins over the node-level property, as
            // for `instance_count`.
            match n.get_property("__multimesh__visible_instance_count") {
                Variant::Int(v) => Some(v),
                _ => match n.get_property("visible_instance_count") {
                    Variant::Int(v) => Some(v),
                    _ => None,
                },
            }
        })
        .unwrap_or(-1)
}

/// The number of instances to draw: [`get_multimesh_instance_count`] capped by
/// [`get_multimesh_visible_instance_count`], applying Godot's `-1` sentinel.
///
/// The single place that rule lives on this path, so the editor list and the
/// render-server adapter cannot disagree about how many blobs a scene has.
pub fn multimesh_visible_count(tree: &SceneTree, node_id: NodeId) -> usize {
    let total = get_multimesh_instance_count(tree, node_id) as usize;
    let visible = get_multimesh_visible_instance_count(tree, node_id);
    if visible < 0 {
        total
    } else {
        (visible as usize).min(total)
    }
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
// RemoteTransform3D
// ===========================================================================

/// Which parts of its transform a `RemoteTransform3D` pushes onto its target.
///
/// Mirrors Godot's `update_remote_position` / `update_remote_rotation` /
/// `update_remote_scale` properties, which all default to `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteTransformFlags {
    /// Push the origin.
    pub position: bool,
    /// Push the basis' orientation.
    pub rotation: bool,
    /// Push the basis' scale (column lengths).
    pub scale: bool,
}

impl Default for RemoteTransformFlags {
    fn default() -> Self {
        Self {
            position: true,
            rotation: true,
            scale: true,
        }
    }
}

/// Resolves what a `RemoteTransform3D` writes onto its target, given its own
/// transform `ours` and the target's current transform `target`.
///
/// 1:1 port of Godot's `RemoteTransform3D::_update_remote`
/// (`upstream/godot/scene/3d/remote_transform_3d.cpp`). Both transforms must be
/// in the SAME space: global when `use_global_coordinates` is set, local
/// otherwise — Godot reads and writes both ends in whichever space is selected,
/// so the choice is the caller's and this function stays space-agnostic.
///
/// The partial-flag cases are the subtle part, and are easy to get backwards:
///
/// - **rotation only** keeps each of the target's basis column *lengths* (its
///   scale) and takes only our column *directions*.
/// - **scale only** is the mirror: it keeps the target's column *directions*
///   and takes only our column *lengths*.
///
/// With all three flags set the whole transform is copied verbatim, which is
/// the common case and the one Godot special-cases first.
pub fn remote_target_transform(
    ours: Transform3D,
    target: Transform3D,
    flags: RemoteTransformFlags,
) -> Transform3D {
    // Godot's fast path: everything is pushed, so the target simply becomes us.
    if flags.position && flags.rotation && flags.scale {
        return ours;
    }

    let mut result = target;

    if flags.rotation && flags.scale {
        result.basis = ours.basis;
    } else if flags.rotation {
        // our direction, target's length — per basis column.
        result.basis = Basis {
            x: ours.basis.x.normalized() * target.basis.x.length(),
            y: ours.basis.y.normalized() * target.basis.y.length(),
            z: ours.basis.z.normalized() * target.basis.z.length(),
        };
    } else if flags.scale {
        // target's direction, our length — per basis column.
        result.basis = Basis {
            x: target.basis.x.normalized() * ours.basis.x.length(),
            y: target.basis.y.normalized() * ours.basis.y.length(),
            z: target.basis.z.normalized() * ours.basis.z.length(),
        };
    }

    if flags.position {
        result.origin = ours.origin;
    }

    result
}

/// Sets the node's local transform such that its **global** transform equals
/// `t`, by peeling off the parent chain.
///
/// Writes through [`store_local_transform`], which updates `transform`,
/// `position`, `rotation` and `scale` together. That matters here: the
/// module-private `set_local_transform` above writes *only* the `"transform"`
/// property, and while `get_local_transform` prefers that key, `get_position`
/// prefers the separate `position` property — so a transform-only write would
/// leave `get_position` reporting a stale value for a node this pushes to.
///
/// ⚠️ A `top_level` node has no effective parent, so its local transform is
/// the global one (pat-v3co1) — see [`set_global_position`].
pub fn set_global_transform(tree: &mut SceneTree, node_id: NodeId, t: Transform3D) {
    let parent_global = effective_parent_global(tree, node_id);
    let local = parent_global.inverse() * t;
    if let Some(node) = tree.get_node_mut(node_id) {
        store_local_transform(node, local);
    }
}

/// Forwards every `RemoteTransform3D`'s transform onto the node at its
/// `remote_path` (pat-0q2kx.197).
///
/// Godot pushes on `NOTIFICATION_TRANSFORM_CHANGED`; a per-frame phase is the
/// equivalent for a frame-stepped tree. [`remote_target_transform`] already had
/// the per-channel masking logic — and every one of its callers lived in
/// `#[cfg(test)]`, so the node was inert in a real scene.
pub fn process_remote_transforms_3d(tree: &mut SceneTree) {
    struct Push {
        target: NodeId,
        value: Transform3D,
        global: bool,
    }
    // Gathered before any write: a remote target can itself be another
    // RemoteTransform3D's source, and mutating mid-walk would let this frame's
    // result depend on tree order.
    let mut pushes: Vec<Push> = Vec::new();

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "RemoteTransform3D" {
            continue;
        }

        let path = match node.get_property("remote_path") {
            Variant::NodePath(p) => p.to_string(),
            Variant::String(s) => s,
            _ => continue,
        };
        // Clearing remote_path stops the forwarding, as in Godot.
        if path.is_empty() {
            continue;
        }

        let target = if path.starts_with('/') {
            tree.get_node_by_path(&path)
        } else {
            tree.get_node_relative(id, &path)
        };
        let Some(target) = target else {
            continue;
        };
        // Pushing onto yourself would feed the transform back every frame.
        if target == id {
            continue;
        }

        // All four flags default to true in Godot.
        let flags = RemoteTransformFlags {
            position: !matches!(node.get_property("update_position"), Variant::Bool(false)),
            rotation: !matches!(node.get_property("update_rotation"), Variant::Bool(false)),
            scale: !matches!(node.get_property("update_scale"), Variant::Bool(false)),
        };
        let global = !matches!(
            node.get_property("use_global_coordinates"),
            Variant::Bool(false)
        );

        // The target's own transform is an input, not just a destination: the
        // partial-flag cases keep the channels that are NOT being pushed.
        let (ours, theirs) = if global {
            (
                get_global_transform(tree, id),
                get_global_transform(tree, target),
            )
        } else {
            (
                get_local_transform(tree, id),
                get_local_transform(tree, target),
            )
        };

        pushes.push(Push {
            target,
            value: remote_target_transform(ours, theirs, flags),
            global,
        });
    }

    for push in pushes {
        if push.global {
            set_global_transform(tree, push.target, push.value);
        } else if let Some(node) = tree.get_node_mut(push.target) {
            // Same reasoning as `set_global_transform`: go through
            // `store_local_transform` so `position`/`rotation`/`scale` track
            // the pushed transform, not just the `"transform"` key.
            store_local_transform(node, push.value);
        }
    }
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

    // -- RemoteTransform3D ---------------------------------------------------

    /// A transform whose basis columns are axis-aligned but scaled, so a
    /// column's direction and its length can be checked independently.
    fn scaled_at(origin: Vector3, scale: Vector3) -> Transform3D {
        Transform3D {
            basis: Basis {
                x: Vector3::new(scale.x, 0.0, 0.0),
                y: Vector3::new(0.0, scale.y, 0.0),
                z: Vector3::new(0.0, 0.0, scale.z),
            },
            origin,
        }
    }

    /// The default (all flags on) copies our transform wholesale — Godot's
    /// fast path, and the bead's headline behaviour.
    #[test]
    fn remote_transform_pushes_whole_transform_by_default() {
        let ours = scaled_at(Vector3::new(1.0, 2.0, 3.0), Vector3::new(2.0, 2.0, 2.0));
        let target = scaled_at(Vector3::new(-9.0, -9.0, -9.0), Vector3::new(1.0, 1.0, 1.0));

        let out = remote_target_transform(ours, target, RemoteTransformFlags::default());

        assert_eq!(out, ours, "target becomes an exact copy of the remote");
    }

    /// Position-only leaves the target's basis completely alone.
    #[test]
    fn remote_transform_position_only_keeps_target_basis() {
        let ours = scaled_at(Vector3::new(1.0, 2.0, 3.0), Vector3::new(5.0, 5.0, 5.0));
        let target = scaled_at(Vector3::new(-9.0, -9.0, -9.0), Vector3::new(1.0, 1.0, 1.0));

        let out = remote_target_transform(
            ours,
            target,
            RemoteTransformFlags {
                position: true,
                rotation: false,
                scale: false,
            },
        );

        assert_eq!(out.origin, ours.origin, "origin is pushed");
        assert_eq!(out.basis, target.basis, "basis is untouched");
    }

    /// Rotation+scale replaces the whole basis but keeps the target's origin.
    #[test]
    fn remote_transform_rotation_and_scale_keeps_target_origin() {
        let ours = scaled_at(Vector3::new(1.0, 2.0, 3.0), Vector3::new(2.0, 3.0, 4.0));
        let target = scaled_at(Vector3::new(-9.0, -9.0, -9.0), Vector3::new(1.0, 1.0, 1.0));

        let out = remote_target_transform(
            ours,
            target,
            RemoteTransformFlags {
                position: false,
                rotation: true,
                scale: true,
            },
        );

        assert_eq!(out.basis, ours.basis, "basis is pushed");
        assert_eq!(out.origin, target.origin, "origin is untouched");
    }

    /// Rotation-only: our column DIRECTIONS, the target's column LENGTHS.
    /// Getting this backwards silently rescales the target every frame.
    #[test]
    fn remote_transform_rotation_only_keeps_target_scale() {
        // Ours points +X on the x column but is 10 long; target is 3 long.
        let ours = scaled_at(Vector3::ZERO, Vector3::new(10.0, 10.0, 10.0));
        let target = scaled_at(Vector3::ZERO, Vector3::new(3.0, 3.0, 3.0));

        let out = remote_target_transform(
            ours,
            target,
            RemoteTransformFlags {
                position: false,
                rotation: true,
                scale: false,
            },
        );

        assert_eq!(
            out.basis.x.length(),
            3.0,
            "target keeps ITS scale under a rotation-only push"
        );
        assert_eq!(
            out.basis.x.normalized(),
            ours.basis.x.normalized(),
            "but takes OUR direction"
        );
    }

    /// Scale-only is the mirror: the target's DIRECTIONS, our LENGTHS.
    #[test]
    fn remote_transform_scale_only_keeps_target_rotation() {
        // Our x column points +X (length 10); the target's x column points +Y.
        let ours = scaled_at(Vector3::ZERO, Vector3::new(10.0, 10.0, 10.0));
        let target = Transform3D {
            basis: Basis {
                x: Vector3::new(0.0, 3.0, 0.0),
                y: Vector3::new(3.0, 0.0, 0.0),
                z: Vector3::new(0.0, 0.0, 3.0),
            },
            origin: Vector3::ZERO,
        };

        let out = remote_target_transform(
            ours,
            target,
            RemoteTransformFlags {
                position: false,
                rotation: false,
                scale: true,
            },
        );

        assert_eq!(
            out.basis.x.normalized(),
            target.basis.x.normalized(),
            "target keeps ITS orientation under a scale-only push"
        );
        assert_eq!(
            out.basis.x.length(),
            10.0,
            "but takes OUR column length (scale)"
        );
    }

    /// All flags off is a no-op — the target must not drift.
    #[test]
    fn remote_transform_with_no_flags_is_a_no_op() {
        let ours = scaled_at(Vector3::new(1.0, 2.0, 3.0), Vector3::new(2.0, 2.0, 2.0));
        let target = scaled_at(Vector3::new(-9.0, -9.0, -9.0), Vector3::new(1.0, 1.0, 1.0));

        let out = remote_target_transform(
            ours,
            target,
            RemoteTransformFlags {
                position: false,
                rotation: false,
                scale: false,
            },
        );

        assert_eq!(out, target, "nothing is pushed");
    }
}

// ---------------------------------------------------------------------------
// Script surface: derived properties + the Node3D method table (pat-mgixk.30)
// ---------------------------------------------------------------------------

/// Radians per degree, for the `*_degrees` property pair.
const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;

/// Whether `node_id` is a Node3D, so the 3D surface never answers for a 2D node.
///
/// ⚠️ `is_class` rather than a name equality test: every 3D node worth asking
/// (Camera3D, CharacterBody3D, a scripted subclass) inherits Node3D, and
/// `class_name() == "Node3D"` would answer only for the bare base.
///
/// The gate is load-bearing in the other direction too: `global_position` is a
/// Node2D property as well, already served for 2D in `scene_tree.rs`. Without
/// this check the 3D reader would capture a Sprite2D's read and hand back a
/// `Vector3` where the script expects a `Vector2`.
fn is_node3d(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("Node3D"))
        .unwrap_or(false)
}

/// The transform a global write is divided by: the parent's global transform,
/// or identity at the top of the chain OR for a `top_level` node, whose global
/// transform does not include its parent (pat-v3co1).
fn parent_global(tree: &SceneTree, node_id: NodeId) -> Transform3D {
    effective_parent_global(tree, node_id)
}

/// Reads one of Node3D's COMPUTED properties, or `None` for a name it does not
/// serve (pat-mgixk.30).
///
/// Godot exposes `global_position`, `global_transform`, `global_basis`,
/// `global_rotation(_degrees)`, `basis`, `quaternion` and `rotation_degrees` as
/// real properties on Node3D (`doc/classes/Node3D.xml`). Patina stored only
/// `transform`/`position`/`rotation`/`scale`, and `get_node_property` fell
/// through to the raw bag — so `self.global_position` on a Node3D was an
/// `UndefinedVariable`, the single most ordinary thing a 3D script does.
///
/// `global_*` uses [`get_global_transform_top_level`], which stops the parent
/// walk at a `top_level` node, so a node that opted out of inheriting its
/// parent's transform reports the position it is actually drawn at.
pub fn get_derived_property(tree: &SceneTree, node_id: NodeId, prop: &str) -> Option<Variant> {
    if !is_node3d(tree, node_id) {
        return None;
    }
    let global = || get_global_transform_top_level(tree, node_id);
    match prop {
        "global_position" => Some(Variant::Vector3(global().origin)),
        "global_transform" => Some(Variant::Transform3D(global())),
        "global_basis" => Some(Variant::Basis(global().basis)),
        // Rotation only: `to_euler` on a basis that also carries scale mixes the
        // two, so the rotation is extracted first.
        "global_rotation" => Some(Variant::Vector3(
            global().basis.get_rotation_basis().to_euler(),
        )),
        "global_rotation_degrees" => {
            let e = global().basis.get_rotation_basis().to_euler();
            Some(Variant::Vector3(Vector3::new(
                e.x / DEG_TO_RAD,
                e.y / DEG_TO_RAD,
                e.z / DEG_TO_RAD,
            )))
        }
        "basis" => Some(Variant::Basis(get_local_transform(tree, node_id).basis)),
        "quaternion" => Some(Variant::Quaternion(
            get_local_transform(tree, node_id)
                .basis
                .get_rotation_quaternion(),
        )),
        "rotation_degrees" => {
            let r = get_rotation(tree, node_id);
            Some(Variant::Vector3(Vector3::new(
                r.x / DEG_TO_RAD,
                r.y / DEG_TO_RAD,
                r.z / DEG_TO_RAD,
            )))
        }
        _ => None,
    }
}

/// Writes one of Node3D's computed properties. Returns whether it was handled.
///
/// Each global write is converted into the LOCAL transform the node actually
/// stores, by composing against the parent's global — the same arithmetic
/// [`set_global_position`] already used. Writing the global value straight into
/// the local slot is the bug this avoids: under a translated parent the node
/// would land at parent + target instead of at target.
pub fn set_derived_property(
    tree: &mut SceneTree,
    node_id: NodeId,
    prop: &str,
    value: &Variant,
) -> bool {
    if !is_node3d(tree, node_id) {
        return false;
    }
    match (prop, value) {
        ("global_position", Variant::Vector3(v)) => {
            set_global_position(tree, node_id, *v);
            true
        }
        ("global_transform", Variant::Transform3D(t)) => {
            let local = parent_global(tree, node_id).inverse() * *t;
            if let Some(node) = tree.get_node_mut(node_id) {
                store_local_transform(node, local);
            }
            true
        }
        ("global_basis", Variant::Basis(b)) => {
            let mut target = get_global_transform(tree, node_id);
            target.basis = *b;
            let local = parent_global(tree, node_id).inverse() * target;
            if let Some(node) = tree.get_node_mut(node_id) {
                store_local_transform(node, local);
            }
            true
        }
        ("global_rotation", Variant::Vector3(e)) => {
            set_global_rotation_euler(tree, node_id, *e);
            true
        }
        ("global_rotation_degrees", Variant::Vector3(d)) => {
            set_global_rotation_euler(
                tree,
                node_id,
                Vector3::new(d.x * DEG_TO_RAD, d.y * DEG_TO_RAD, d.z * DEG_TO_RAD),
            );
            true
        }
        ("basis", Variant::Basis(b)) => {
            if let Some(node) = tree.get_node_mut(node_id) {
                let mut t = local_transform_of(node);
                t.basis = *b;
                store_local_transform(node, t);
            }
            true
        }
        ("quaternion", Variant::Quaternion(q)) => {
            if let Some(node) = tree.get_node_mut(node_id) {
                let mut t = local_transform_of(node);
                // Scale is preserved: Godot's `quaternion` setter replaces the
                // ROTATION, and rebuilding the basis from the quaternion alone
                // would silently reset a scaled node to 1.
                let scale = t.basis.get_scale();
                t.basis = Basis::from_quaternion_scale(*q, scale);
                store_local_transform(node, t);
            }
            true
        }
        ("rotation_degrees", Variant::Vector3(d)) => {
            set_rotation(
                tree,
                node_id,
                Vector3::new(d.x * DEG_TO_RAD, d.y * DEG_TO_RAD, d.z * DEG_TO_RAD),
            );
            true
        }
        _ => false,
    }
}

/// Sets the node's local rotation so its GLOBAL rotation is `euler`.
fn set_global_rotation_euler(tree: &mut SceneTree, node_id: NodeId, euler: Vector3) {
    let mut target = get_global_transform(tree, node_id);
    let scale = target.basis.get_scale();
    target.basis = Basis::from_quaternion_scale(Basis::from_euler(euler).to_quaternion(), scale);
    let local = parent_global(tree, node_id).inverse() * target;
    if let Some(node) = tree.get_node_mut(node_id) {
        store_local_transform(node, local);
    }
}

/// Dispatches a Node3D method, or `None` for a name it does not serve.
///
/// ⚠️ Carries ONLY names with no arm anywhere else. This runs before
/// `scripting.rs`'s own method table, so an implementation of a name the table
/// already serves would SHADOW it rather than fill a gap — the same warning the
/// `pat-mgixk.8` block beside it carries. Verified absent from `scripting.rs`
/// before being added here; `hide`, `show`, `set_visible` and `is_visible` are
/// deliberately NOT here, because that table already dispatches them.
pub fn call_node3d_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !is_node3d(tree, node_id) {
        return None;
    }
    let vec3 = |i: usize| match args.get(i) {
        Some(Variant::Vector3(v)) => Some(*v),
        _ => None,
    };
    let float = |i: usize| match args.get(i) {
        Some(Variant::Float(f)) => Some(*f as f32),
        Some(Variant::Int(n)) => Some(*n as f32),
        _ => None,
    };
    match method {
        "is_visible_in_tree" => Some(Variant::Bool(is_visible_in_tree(tree, node_id))),
        "get_global_transform" => Some(Variant::Transform3D(get_global_transform_top_level(
            tree, node_id,
        ))),
        "set_global_transform" => match args.first() {
            Some(v @ Variant::Transform3D(_)) => {
                set_derived_property(tree, node_id, "global_transform", v);
                Some(Variant::Nil)
            }
            _ => None,
        },
        "get_global_position" => Some(Variant::Vector3(
            get_global_transform_top_level(tree, node_id).origin,
        )),
        "set_global_position" => {
            let target = vec3(0)?;
            set_global_position(tree, node_id, target);
            Some(Variant::Nil)
        }
        "get_quaternion" | "get_basis" | "get_rotation_degrees" => {
            // One arm: these are the getter spellings of properties
            // `get_derived_property` already computes, so routing them keeps a
            // single implementation instead of a second that can drift.
            let prop = method.trim_start_matches("get_");
            get_derived_property(tree, node_id, prop)
        }
        "set_quaternion" | "set_basis" | "set_rotation_degrees" => {
            let prop = method.trim_start_matches("set_");
            let value = args.first()?;
            set_derived_property(tree, node_id, prop, value).then_some(Variant::Nil)
        }
        // Godot rotates about a GLOBAL axis through the node's own origin, so
        // the basis is rebuilt in global space and converted back.
        "global_rotate" => {
            let axis = vec3(0)?;
            let angle = float(1)?;
            let mut target = get_global_transform(tree, node_id);
            target.basis = target.basis.rotated(axis, angle);
            set_derived_property(
                tree,
                node_id,
                "global_transform",
                &Variant::Transform3D(target),
            );
            Some(Variant::Nil)
        }
        "global_translate" => {
            let offset = vec3(0)?;
            let current = get_global_transform_top_level(tree, node_id).origin;
            set_global_position(
                tree,
                node_id,
                Vector3::new(
                    current.x + offset.x,
                    current.y + offset.y,
                    current.z + offset.z,
                ),
            );
            Some(Variant::Nil)
        }
        // "object local" means about the node's OWN axes, i.e. post-multiplied
        // into the local basis rather than pre-multiplied like `rotate`.
        "rotate_object_local" => {
            let axis = vec3(0)?;
            let angle = float(1)?;
            if let Some(node) = tree.get_node_mut(node_id) {
                let mut t = local_transform_of(node);
                let local_axis = t.basis.xform(axis);
                t.basis = t.basis.rotated(local_axis, angle);
                store_local_transform(node, t);
            }
            Some(Variant::Nil)
        }
        "translate_object_local" => {
            let offset = vec3(0)?;
            if let Some(node) = tree.get_node_mut(node_id) {
                let mut t = local_transform_of(node);
                t.origin = t.origin + t.basis.xform(offset);
                store_local_transform(node, t);
            }
            Some(Variant::Nil)
        }
        "orthonormalize" => {
            if let Some(node) = tree.get_node_mut(node_id) {
                let mut t = local_transform_of(node);
                t.basis = t.basis.orthonormalized();
                store_local_transform(node, t);
            }
            Some(Variant::Nil)
        }
        "set_identity" => {
            if let Some(node) = tree.get_node_mut(node_id) {
                store_local_transform(node, Transform3D::IDENTITY);
            }
            Some(Variant::Nil)
        }
        "look_at_from_position" => {
            let from = vec3(0)?;
            let target = vec3(1)?;
            let up = vec3(2).unwrap_or(Vector3::new(0.0, 1.0, 0.0));
            set_global_position(tree, node_id, from);
            look_at(tree, node_id, target, up);
            Some(Variant::Nil)
        }
        // Godot returns the nearest Node3D ANCESTOR, not merely the parent: a
        // Node3D parented under a plain `Node` still has one above it.
        "get_parent_node_3d" => {
            let mut cursor = tree.get_node(node_id).and_then(|n| n.parent());
            while let Some(id) = cursor {
                if is_node3d(tree, id) {
                    return Some(Variant::ObjectId(id.object_id()));
                }
                cursor = tree.get_node(id).and_then(|n| n.parent());
            }
            Some(Variant::Nil)
        }
        "set_as_top_level" => {
            let enabled = matches!(args.first(), Some(Variant::Bool(true)));
            // 🔑 Godot KEEPS the node where it is on screen when the flag
            // flips (`Node3D::set_as_top_level`: enabling stores
            // `get_global_transform()` as the new local transform; disabling
            // stores `parent_global⁻¹ * global`). Flipping only the flag made
            // the old LOCAL transform read as a world one, so the official 3D
            // platformer's follow camera — `set_as_top_level(true)` in `_ready`
            // — jumped from behind the player to near the world origin, its
            // first follow step swung it round to the far side, and the
            // player then walked "forward" in the wrong direction.
            if enabled != is_top_level(tree, node_id) {
                let global = get_global_transform(tree, node_id);
                set_top_level(tree, node_id, enabled);
                let parent_global = effective_parent_global(tree, node_id);
                let local = parent_global.inverse() * global;
                if let Some(node) = tree.get_node_mut(node_id) {
                    store_local_transform(node, local);
                }
            }
            Some(Variant::Nil)
        }
        "is_set_as_top_level" => Some(Variant::Bool(is_top_level(tree, node_id))),
        // Patina recomputes transforms on read, so there is no deferred queue to
        // flush. Answered rather than declined because a script calling it
        // expects a no-op, not an UndefinedFunction.
        "force_update_transform" => Some(Variant::Nil),
        _ => None,
    }
}
