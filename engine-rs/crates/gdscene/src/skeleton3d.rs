//! Skeleton3D node support: bone hierarchy, rest poses, and pose overrides.
//!
//! Follows Godot's `Skeleton3D` API. Bone data is stored as internal
//! properties on the node using `_bones` (Array of bone records) and
//! individual bone properties like `bones/{idx}/name`, `bones/{idx}/rest`, etc.
//!
//! The main API mirrors Godot:
//! - [`add_bone`] / [`get_bone_count`] / [`find_bone`] / [`get_bone_name`]
//! - [`set_bone_parent`] / [`get_bone_parent`]
//! - [`set_bone_rest`] / [`get_bone_rest`]
//! - [`set_bone_pose`] / [`get_bone_pose`]
//! - [`get_bone_global_pose`] — recursively composes rest * pose up the chain

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion, Transform3D};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Internal bone storage key helpers
// ---------------------------------------------------------------------------

fn bone_name_key(idx: usize) -> String {
    format!("bones/{idx}/name")
}

fn bone_parent_key(idx: usize) -> String {
    format!("bones/{idx}/parent")
}

fn bone_rest_key(idx: usize) -> String {
    format!("bones/{idx}/rest")
}

fn bone_pose_key(idx: usize) -> String {
    format!("bones/{idx}/pose")
}

fn bone_pose_position_key(idx: usize) -> String {
    format!("bones/{idx}/pose_position")
}

fn bone_pose_rotation_key(idx: usize) -> String {
    format!("bones/{idx}/pose_rotation")
}

fn bone_pose_scale_key(idx: usize) -> String {
    format!("bones/{idx}/pose_scale")
}

fn bone_enabled_key(idx: usize) -> String {
    format!("bones/{idx}/enabled")
}

const BONE_COUNT_KEY: &str = "_bone_count";

// ===========================================================================
// Bone management
// ===========================================================================

/// Adds a new bone with the given name. Returns the bone index.
///
/// The bone starts with no parent (-1), identity rest pose, and identity pose.
pub fn add_bone(tree: &mut SceneTree, node_id: NodeId, name: &str) -> usize {
    let count = get_bone_count(tree, node_id);
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&bone_name_key(count), Variant::String(name.to_string()));
        node.set_property(&bone_parent_key(count), Variant::Int(-1));
        node.set_property(
            &bone_rest_key(count),
            Variant::Transform3D(Transform3D::IDENTITY),
        );
        node.set_property(
            &bone_pose_key(count),
            Variant::Transform3D(Transform3D::IDENTITY),
        );
        node.set_property(&bone_enabled_key(count), Variant::Bool(true));
        node.set_property(BONE_COUNT_KEY, Variant::Int((count + 1) as i64));
    }
    count
}

/// Returns the number of bones in the skeleton.
///
/// [`add_bone`] maintains an explicit `_bone_count`, which wins when present. A
/// skeleton loaded from a `.tscn` has no such key — Godot writes only the
/// `bones/{idx}/…` records and derives the count from them — so the count is
/// inferred by walking consecutive `bones/{idx}/name` entries (pat-0q2kx.3).
/// Without this, every bone API on a scene-loaded `Skeleton3D` sees zero bones.
pub fn get_bone_count(tree: &SceneTree, node_id: NodeId) -> usize {
    let Some(node) = tree.get_node(node_id) else {
        return 0;
    };
    if node.has_property(BONE_COUNT_KEY) {
        if let Variant::Int(c) = node.get_property(BONE_COUNT_KEY) {
            return c.max(0) as usize;
        }
    }
    // Fast path: no bone records at all (the common case for every non-skeleton
    // node this is asked about).
    if !node.has_property(&bone_name_key(0)) {
        return 0;
    }
    let mut count = 1usize;
    while node.has_property(&bone_name_key(count)) {
        count += 1;
    }
    count
}

/// Materializes the inferred bone count of a scene-loaded skeleton into the
/// explicit `_bone_count` key (pat-0q2kx.3).
///
/// [`get_bone_count`] can infer the count from the `bones/{idx}/…` records a
/// `.tscn` carries, but that scan runs on every bone query. Godot materializes
/// the bone array once at load; this does the same the first time the runtime
/// touches the skeleton, so the per-frame pose walks stay O(1) per lookup.
pub fn cache_bone_count(tree: &mut SceneTree, node_id: NodeId) {
    let already = tree
        .get_node(node_id)
        .map(|n| n.has_property(BONE_COUNT_KEY))
        .unwrap_or(true);
    if already {
        return;
    }
    let count = get_bone_count(tree, node_id);
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(BONE_COUNT_KEY, Variant::Int(count as i64));
    }
}

/// Finds a bone by name, returning its index or `None`.
pub fn find_bone(tree: &SceneTree, node_id: NodeId, name: &str) -> Option<usize> {
    let count = get_bone_count(tree, node_id);
    for i in 0..count {
        if get_bone_name(tree, node_id, i).as_deref() == Some(name) {
            return Some(i);
        }
    }
    None
}

/// Returns the name of bone at `idx`, or `None` if out of range.
pub fn get_bone_name(tree: &SceneTree, node_id: NodeId, idx: usize) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property(&bone_name_key(idx)) {
            Variant::String(s) => Some(s),
            _ => None,
        })
}

/// Sets the name of bone at `idx`.
pub fn set_bone_name(tree: &mut SceneTree, node_id: NodeId, idx: usize, name: &str) {
    if idx < get_bone_count(tree, node_id) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&bone_name_key(idx), Variant::String(name.to_string()));
        }
    }
}

// ===========================================================================
// Parent hierarchy
// ===========================================================================

/// Sets the parent bone index for bone `idx`. Use -1 for root bones.
pub fn set_bone_parent(tree: &mut SceneTree, node_id: NodeId, idx: usize, parent: i64) {
    if idx < get_bone_count(tree, node_id) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&bone_parent_key(idx), Variant::Int(parent));
        }
    }
}

/// Returns the parent bone index (-1 for root bones).
pub fn get_bone_parent(tree: &SceneTree, node_id: NodeId, idx: usize) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_parent_key(idx)) {
            Variant::Int(p) => p,
            _ => -1,
        })
        .unwrap_or(-1)
}

/// Returns the indices of bones whose parent is `parent_idx`.
pub fn get_bone_children(tree: &SceneTree, node_id: NodeId, parent_idx: i64) -> Vec<usize> {
    let count = get_bone_count(tree, node_id);
    let mut children = Vec::new();
    for i in 0..count {
        if get_bone_parent(tree, node_id, i) == parent_idx {
            children.push(i);
        }
    }
    children
}

// ===========================================================================
// Rest pose
// ===========================================================================

/// Sets the rest (bind) pose for bone `idx`.
pub fn set_bone_rest(tree: &mut SceneTree, node_id: NodeId, idx: usize, rest: Transform3D) {
    if idx < get_bone_count(tree, node_id) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&bone_rest_key(idx), Variant::Transform3D(rest));
        }
    }
}

/// Returns the rest (bind) pose for bone `idx`.
pub fn get_bone_rest(tree: &SceneTree, node_id: NodeId, idx: usize) -> Transform3D {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_rest_key(idx)) {
            Variant::Transform3D(t) => t,
            _ => Transform3D::IDENTITY,
        })
        .unwrap_or(Transform3D::IDENTITY)
}

// ===========================================================================
// Pose (animation override)
// ===========================================================================

/// Sets the current pose for bone `idx`.
pub fn set_bone_pose(tree: &mut SceneTree, node_id: NodeId, idx: usize, pose: Transform3D) {
    if idx < get_bone_count(tree, node_id) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&bone_pose_key(idx), Variant::Transform3D(pose));
        }
    }
}

/// Returns the current pose for bone `idx`.
pub fn get_bone_pose(tree: &SceneTree, node_id: NodeId, idx: usize) -> Transform3D {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_pose_key(idx)) {
            Variant::Transform3D(t) => t,
            _ => Transform3D::IDENTITY,
        })
        .unwrap_or(Transform3D::IDENTITY)
}

// ---------------------------------------------------------------------------
// Per-component pose accessors (pat-u3l8c.101)
//
// Godot's `Skeleton3D` exposes the bone pose as separate position / rotation /
// scale components (`set_bone_pose_position`/`_rotation`/`_scale`). We store the
// three components on the node and recompose them into the full `pose`
// Transform3D (basis = rotation × scale, origin = position) so the existing
// `get_bone_pose` / `get_bone_global_pose` path keeps working.
// ---------------------------------------------------------------------------

/// Returns the position component of bone `idx`'s pose (default `ZERO`).
pub fn get_bone_pose_position(tree: &SceneTree, node_id: NodeId, idx: usize) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_pose_position_key(idx)) {
            Variant::Vector3(v) => v,
            _ => Vector3::ZERO,
        })
        .unwrap_or(Vector3::ZERO)
}

/// Returns the rotation component of bone `idx`'s pose (default identity).
pub fn get_bone_pose_rotation(tree: &SceneTree, node_id: NodeId, idx: usize) -> Quaternion {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_pose_rotation_key(idx)) {
            Variant::Quaternion(q) => q,
            _ => Quaternion::IDENTITY,
        })
        .unwrap_or(Quaternion::IDENTITY)
}

/// Returns the scale component of bone `idx`'s pose (default `ONE`).
pub fn get_bone_pose_scale(tree: &SceneTree, node_id: NodeId, idx: usize) -> Vector3 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_pose_scale_key(idx)) {
            Variant::Vector3(v) => v,
            _ => Vector3::ONE,
        })
        .unwrap_or(Vector3::ONE)
}

/// Recomposes bone `idx`'s full pose Transform3D from its stored
/// position / rotation / scale components.
fn recompose_bone_pose(tree: &mut SceneTree, node_id: NodeId, idx: usize) {
    let position = get_bone_pose_position(tree, node_id, idx);
    let rotation = get_bone_pose_rotation(tree, node_id, idx);
    let scale = get_bone_pose_scale(tree, node_id, idx);
    // Scale the rotation basis per-column so `Basis::get_scale` recovers
    // `scale`. `Basis` has no `scaled` method (that lives on `Transform3D`
    // and would also scale the origin, which must stay equal to `position`).
    let rotation_basis = Basis::from_quaternion(rotation);
    let pose = Transform3D {
        basis: Basis {
            x: rotation_basis.x * scale.x,
            y: rotation_basis.y * scale.y,
            z: rotation_basis.z * scale.z,
        },
        origin: position,
    };
    set_bone_pose(tree, node_id, idx, pose);
}

/// Sets the position component of bone `idx`'s pose (Godot
/// `set_bone_pose_position`) and recomposes the full pose.
pub fn set_bone_pose_position(
    tree: &mut SceneTree,
    node_id: NodeId,
    idx: usize,
    position: Vector3,
) {
    if idx >= get_bone_count(tree, node_id) {
        return;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&bone_pose_position_key(idx), Variant::Vector3(position));
    }
    recompose_bone_pose(tree, node_id, idx);
}

/// Sets the rotation component of bone `idx`'s pose (Godot
/// `set_bone_pose_rotation`) and recomposes the full pose.
pub fn set_bone_pose_rotation(
    tree: &mut SceneTree,
    node_id: NodeId,
    idx: usize,
    rotation: Quaternion,
) {
    if idx >= get_bone_count(tree, node_id) {
        return;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&bone_pose_rotation_key(idx), Variant::Quaternion(rotation));
    }
    recompose_bone_pose(tree, node_id, idx);
}

/// Sets the scale component of bone `idx`'s pose (Godot `set_bone_pose_scale`)
/// and recomposes the full pose.
pub fn set_bone_pose_scale(tree: &mut SceneTree, node_id: NodeId, idx: usize, scale: Vector3) {
    if idx >= get_bone_count(tree, node_id) {
        return;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&bone_pose_scale_key(idx), Variant::Vector3(scale));
    }
    recompose_bone_pose(tree, node_id, idx);
}

/// Sets whether bone `idx` is enabled.
pub fn set_bone_enabled(tree: &mut SceneTree, node_id: NodeId, idx: usize, enabled: bool) {
    if idx < get_bone_count(tree, node_id) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&bone_enabled_key(idx), Variant::Bool(enabled));
        }
    }
}

/// Returns whether bone `idx` is enabled (defaults to `true`).
pub fn is_bone_enabled(tree: &SceneTree, node_id: NodeId, idx: usize) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property(&bone_enabled_key(idx)) {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(true)
}

// ===========================================================================
// Global (skeleton-space) pose
// ===========================================================================

/// Computes the global pose of bone `idx` within the skeleton.
///
/// This walks the bone parent chain, composing `rest * pose` at each level,
/// matching Godot's `Skeleton3D.get_bone_global_pose()`.
pub fn get_bone_global_pose(tree: &SceneTree, node_id: NodeId, idx: usize) -> Transform3D {
    let count = get_bone_count(tree, node_id);
    if idx >= count {
        return Transform3D::IDENTITY;
    }

    // Build the chain from root to idx.
    let mut chain = Vec::new();
    let mut current = idx as i64;
    while current >= 0 && (current as usize) < count {
        chain.push(current as usize);
        current = get_bone_parent(tree, node_id, current as usize);
    }
    chain.reverse();

    let mut global = Transform3D::IDENTITY;
    for bone_idx in chain {
        let rest = get_bone_rest(tree, node_id, bone_idx);
        let pose = get_bone_pose(tree, node_id, bone_idx);
        global = global * rest * pose;
    }
    global
}

/// Sets bone `idx`'s **global** pose, by solving for the local pose that
/// produces it (pat-0q2kx.203).
///
/// [`get_bone_global_pose`] accumulates `parent_global * rest * pose` down the
/// chain, so the local pose that lands a bone at `global` is
/// `(parent_global * rest)⁻¹ * global`. This is the writeback direction a
/// ragdoll needs: the simulation produces world-space bone transforms, while
/// the skeleton only stores local poses.
pub fn set_bone_global_pose(
    tree: &mut SceneTree,
    node_id: NodeId,
    idx: usize,
    global: Transform3D,
) {
    if idx >= get_bone_count(tree, node_id) {
        return;
    }
    let parent = get_bone_parent(tree, node_id, idx);
    let parent_global = if parent >= 0 {
        get_bone_global_pose(tree, node_id, parent as usize)
    } else {
        Transform3D::IDENTITY
    };
    let rest = get_bone_rest(tree, node_id, idx);

    let pose = (parent_global * rest).inverse() * global;
    set_bone_pose(tree, node_id, idx, pose);
}

/// Gathers the global pose of every bone into a flat array indexed by bone
/// index (`0..get_bone_count`).
///
/// This is the bridge from `Skeleton3D` to the mesh-skinning pipeline: the
/// returned array is exactly what `gdserver3d::skin::skin_mesh` expects as its
/// `bone_global_poses` argument (a `Skin`'s binds reference these bone
/// indices). It is what actually *consumes* the per-bone global poses computed
/// by [`get_bone_global_pose`].
pub fn bone_pose_array(tree: &SceneTree, node_id: NodeId) -> Vec<Transform3D> {
    let count = get_bone_count(tree, node_id);
    (0..count)
        .map(|i| get_bone_global_pose(tree, node_id, i))
        .collect()
}

/// Composes bone `idx`'s **rest** transform up the parent chain, ignoring any
/// pose override.
///
/// This is `get_bone_global_pose` evaluated at the bind pose — Godot's
/// `Skeleton3D.get_bone_global_rest()`. It is what a default `Skin` is built
/// from: each bind's inverse bind matrix is the inverse of this transform.
pub fn get_bone_global_rest(tree: &SceneTree, node_id: NodeId, idx: usize) -> Transform3D {
    let count = get_bone_count(tree, node_id);
    if idx >= count {
        return Transform3D::IDENTITY;
    }
    let mut chain = Vec::new();
    let mut current = idx as i64;
    while current >= 0 && (current as usize) < count {
        chain.push(current as usize);
        current = get_bone_parent(tree, node_id, current as usize);
    }
    chain.reverse();

    let mut global = Transform3D::IDENTITY;
    for bone_idx in chain {
        global = global * get_bone_rest(tree, node_id, bone_idx);
    }
    global
}

/// Gathers the global **rest** transform of every bone, indexed by bone index.
///
/// Mirrors [`bone_pose_array`] for the bind pose. Godot's
/// `Skeleton3D.create_skin_from_rest_transforms()` builds its `Skin` from
/// exactly this array.
pub fn bone_rest_array(tree: &SceneTree, node_id: NodeId) -> Vec<Transform3D> {
    let count = get_bone_count(tree, node_id);
    (0..count)
        .map(|i| get_bone_global_rest(tree, node_id, i))
        .collect()
}

/// Resets all bone poses to identity (clears animation overrides).
pub fn clear_bones_pose(tree: &mut SceneTree, node_id: NodeId) {
    let count = get_bone_count(tree, node_id);
    for i in 0..count {
        set_bone_pose(tree, node_id, i, Transform3D::IDENTITY);
    }
}

// ===========================================================================
// BoneAttachment3D helpers
// ===========================================================================

/// Sets the `"bone_name"` property on a BoneAttachment3D node.
pub fn set_bone_attachment_bone_name(tree: &mut SceneTree, node_id: NodeId, name: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("bone_name", Variant::String(name.to_string()));
    }
}

/// Reads the `"bone_name"` property from a BoneAttachment3D node.
pub fn get_bone_attachment_bone_name(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("bone_name") {
            Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
}

/// Sets the `"bone_idx"` property on a BoneAttachment3D node.
pub fn set_bone_attachment_bone_idx(tree: &mut SceneTree, node_id: NodeId, idx: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("bone_idx", Variant::Int(idx));
    }
}

/// Reads the `"bone_idx"` property from a BoneAttachment3D node.
pub fn get_bone_attachment_bone_idx(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("bone_idx") {
            Variant::Int(i) => i,
            _ => -1,
        })
        .unwrap_or(-1)
}

/// Places a BoneAttachment3D node at the global pose of its target bone.
///
/// The target bone is resolved from the attachment's `bone_idx` when set
/// (`>= 0`), otherwise from its `bone_name` via [`find_bone`]. The bone's
/// global pose is written to the attachment node's `"transform"` property so it
/// follows the (possibly animated) bone. Returns the applied transform, or
/// `None` when the bone can't be resolved.
pub fn update_bone_attachment(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    attachment_id: NodeId,
) -> Option<Transform3D> {
    let by_idx = get_bone_attachment_bone_idx(tree, attachment_id);
    let idx = if by_idx >= 0 {
        by_idx as usize
    } else {
        let name = get_bone_attachment_bone_name(tree, attachment_id)?;
        find_bone(tree, skeleton_id, &name)?
    };
    if idx >= get_bone_count(tree, skeleton_id) {
        return None;
    }
    let pose = get_bone_global_pose(tree, skeleton_id, idx);
    if let Some(node) = tree.get_node_mut(attachment_id) {
        node.set_property("transform", Variant::Transform3D(pose));
    }
    Some(pose)
}

// ===========================================================================
// SkeletonModifier3D / IKModifier3D (pat-nzgdu.151)
// ===========================================================================

/// `SkeletonModifier3D.active` — when false the modifier is skipped entirely.
pub const MODIFIER_ACTIVE_KEY: &str = "active";
/// `SkeletonModifier3D.influence` — how strongly the modifier applies, in [0, 1].
pub const MODIFIER_INFLUENCE_KEY: &str = "influence";
/// `IKModifier3D` target position, in the skeleton's space.
pub const IK_TARGET_POSITION_KEY: &str = "target_position";
/// First bone of the chain the solver may rotate.
pub const IK_ROOT_BONE_KEY: &str = "root_bone";
/// Last bone of the chain — the one driven toward the target.
pub const IK_TIP_BONE_KEY: &str = "tip_bone";
/// CCD passes per update.
pub const IK_ITERATIONS_KEY: &str = "iterations";

/// Godot's `SkeletonModifier3D` class name.
pub const SKELETON_MODIFIER_CLASS: &str = "SkeletonModifier3D";
/// Godot's `IKModifier3D` class name.
pub const IK_MODIFIER_CLASS: &str = "IKModifier3D";
/// Godot's `FABRIK3D` class name — an `IterateIK3D` whose solver is FABRIK
/// (pat-nzgdu.183).
pub const FABRIK_CLASS: &str = "FABRIK3D";
/// Godot's `CCDIK3D` — the `IterateIK3D` sibling whose solver is CCD
/// (pat-0q2kx.156).
pub const CCDIK_CLASS: &str = "CCDIK3D";
/// Godot's `JacobianIK3D` — the third `IterateIK3D` sibling (pat-0q2kx.156).
pub const JACOBIAN_IK_CLASS: &str = "JacobianIK3D";
/// Godot's `LookAtModifier3D` — aims one bone at a target (pat-0q2kx.156).
pub const LOOK_AT_CLASS: &str = "LookAtModifier3D";

/// Below this the chain is treated as already aligned and left alone, so a
/// solved chain doesn't jitter.
const IK_EPSILON: f32 = 1e-6;

/// Reads a modifier's `influence`, clamped to Godot's [0, 1]. Missing = 1.0
/// (fully applied), matching Godot's default.
fn modifier_influence(tree: &SceneTree, modifier_id: NodeId) -> f32 {
    tree.get_node(modifier_id)
        .map(|n| match n.get_property(MODIFIER_INFLUENCE_KEY) {
            Variant::Float(f) => (f as f32).clamp(0.0, 1.0),
            Variant::Int(i) => (i as f32).clamp(0.0, 1.0),
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// Reads a modifier's `active`. Missing = true (Godot's default).
fn modifier_active(tree: &SceneTree, modifier_id: NodeId) -> bool {
    tree.get_node(modifier_id)
        .map(|n| match n.get_property(MODIFIER_ACTIVE_KEY) {
            Variant::Bool(b) => b,
            _ => true,
        })
        .unwrap_or(false)
}

/// The bone chain from `root` down to `tip`, inclusive.
///
/// Returns an empty vec when `tip` does not actually descend from `root`, so a
/// mis-wired modifier is a no-op rather than mangling unrelated bones.
fn bone_chain(tree: &SceneTree, skeleton_id: NodeId, root: usize, tip: usize) -> Vec<usize> {
    let count = get_bone_count(tree, skeleton_id);
    if root >= count || tip >= count {
        return Vec::new();
    }
    let mut chain = Vec::new();
    let mut current = tip as i64;
    while current >= 0 && (current as usize) < count {
        chain.push(current as usize);
        if current as usize == root {
            chain.reverse();
            return chain;
        }
        current = get_bone_parent(tree, skeleton_id, current as usize);
    }
    Vec::new()
}

/// Runs a skeleton's modifier stack (pat-nzgdu.151).
///
/// Mirrors Godot, where `SkeletonModifier3D` children process **after** the
/// AnimationPlayer/Tree has written poses — so IK runs on top of animation
/// rather than being overwritten by it. Call this once per frame, after the
/// pose update. Children are processed in tree order; inactive ones are skipped.
pub fn process_modifiers(tree: &mut SceneTree, skeleton_id: NodeId) {
    let children = tree
        .get_node(skeleton_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();

    for child in children {
        let class = match tree.get_node(child) {
            Some(n) => n.class_name().to_string(),
            None => continue,
        };
        // Godot picks the algorithm by CLASS: FABRIK3D, CCDIK3D and
        // JacobianIK3D are siblings under IterateIK3D that share every knob and
        // differ only in how they solve (pat-nzgdu.183).
        // Not an IK solver — it aims a single bone, so it does not go through
        // `IkSolverKind`/`apply_ik_modifier` at all.
        if class == LOOK_AT_CLASS {
            if modifier_active(tree, child) {
                apply_look_at_modifier(tree, skeleton_id, child);
            }
            continue;
        }
        // BoneConstraint3D children — not IK solvers either: they write an apply
        // bone's pose from a reference bone rather than solving a chain
        // (pat-0q2kx.239).
        if class == COPY_TRANSFORM_CLASS {
            if modifier_active(tree, child) {
                apply_copy_transform_modifier(tree, skeleton_id, child);
            }
            continue;
        }
        if class == CONVERT_TRANSFORM_CLASS {
            if modifier_active(tree, child) {
                apply_convert_transform_modifier(tree, skeleton_id, child);
            }
            continue;
        }
        let solver = match class.as_str() {
            IK_MODIFIER_CLASS => IkSolverKind::Ccd,
            FABRIK_CLASS => IkSolverKind::Fabrik,
            CCDIK_CLASS => IkSolverKind::Ccd,
            // ⚠️ Godot's JacobianIK3D solves by Jacobian transpose/DLS, which
            // Patina has no solver for. Routing it to CCD makes the class reach
            // its target — both are iterative — but the convergence path is not
            // Godot's. This is a deliberate placeholder, not parity; a real
            // Jacobian solver belongs in `ik_solver.rs`.
            JACOBIAN_IK_CLASS => IkSolverKind::Ccd,
            _ => continue,
        };
        if !modifier_active(tree, child) {
            continue;
        }
        apply_ik_modifier(tree, skeleton_id, child, solver);
    }
}

/// Maps Godot's `SkeletonModifier3D.BoneAxis` to a unit direction in the bone's
/// local frame: `0=+X 1=-X 2=+Y 3=-Y 4=+Z 5=-Z`.
///
/// `4` (`BONE_AXIS_PLUS_Z`) is `LookAtModifier3D`'s default, so an unset or
/// unrecognised value must fall back to +Z — **not** to axis 0 (+X), which is
/// what treating enum-zero as the default would give.
fn bone_axis_vector(axis: i64) -> Vector3 {
    match axis {
        0 => Vector3::new(1.0, 0.0, 0.0),
        1 => Vector3::new(-1.0, 0.0, 0.0),
        2 => Vector3::new(0.0, 1.0, 0.0),
        3 => Vector3::new(0.0, -1.0, 0.0),
        5 => Vector3::new(0.0, 0.0, -1.0),
        _ => Vector3::new(0.0, 0.0, 1.0),
    }
}

// ---------------------------------------------------------------------------
// BoneConstraint3D family: Copy / Convert transform modifiers (pat-0q2kx.239)
//
// Godot ground truth: `scene/3d/copy_transform_modifier_3d.cpp`,
// `scene/3d/convert_transform_modifier_3d.cpp`, `scene/3d/bone_constraint_3d.cpp`.
// Both are *pose-pure* — a function of the current pose with no delta and no
// state carried between calls — which is why they fit `process_modifiers` as it
// stands, while a time-integrating modifier like SpringBoneSimulator3D cannot.
// ---------------------------------------------------------------------------

/// `CopyTransformModifier3D`'s class name.
pub const COPY_TRANSFORM_CLASS: &str = "CopyTransformModifier3D";
/// `ConvertTransformModifier3D`'s class name.
pub const CONVERT_TRANSFORM_CLASS: &str = "ConvertTransformModifier3D";

/// `TRANSFORM_FLAG_ALL` / `AXIS_FLAG_ALL` — Godot's **defaults** for
/// `copy_flags` and `axis_flags` (`copy_transform_modifier_3d.h:54-55`).
///
/// ⚠️ Reading a missing flag property as enum-zero would mask every component
/// and axis away, turning the modifier into a silent no-op. The default is
/// all-bits-set, not zero.
const FLAG_ALL: i64 = 7;

/// Reads `settings/<index>/<key>` off a modifier node.
fn setting_prop(tree: &SceneTree, modifier_id: NodeId, index: usize, key: &str) -> Variant {
    tree.get_node(modifier_id)
        .map(|n| n.get_property(&format!("settings/{index}/{key}")))
        .unwrap_or(Variant::Nil)
}

fn setting_int(tree: &SceneTree, m: NodeId, i: usize, key: &str, default: i64) -> i64 {
    match setting_prop(tree, m, i, key) {
        Variant::Int(v) => v,
        Variant::Float(f) => f as i64,
        _ => default,
    }
}

fn setting_f32(tree: &SceneTree, m: NodeId, i: usize, key: &str, default: f32) -> f32 {
    match setting_prop(tree, m, i, key) {
        Variant::Float(f) => f as f32,
        Variant::Int(v) => v as f32,
        _ => default,
    }
}

fn setting_bool(tree: &SceneTree, m: NodeId, i: usize, key: &str, default: bool) -> bool {
    match setting_prop(tree, m, i, key) {
        Variant::Bool(b) => b,
        _ => default,
    }
}

/// How many settings a constraint modifier carries.
///
/// Prefers the authored `setting_count`, but falls back to probing
/// `settings/<n>/…` so a hand-written fixture that omits the count still works
/// (a `.tscn` from Godot always writes it).
fn constraint_setting_count(tree: &SceneTree, modifier_id: NodeId) -> usize {
    if let Some(n) = tree.get_node(modifier_id) {
        match n.get_property("setting_count") {
            Variant::Int(v) if v > 0 => return v as usize,
            Variant::Float(f) if f > 0.0 => return f as usize,
            _ => {}
        }
    }
    let mut count = 0usize;
    while count < 64 {
        let has_any = [
            "apply_bone",
            "apply_bone_name",
            "reference_bone",
            "reference_bone_name",
        ]
        .iter()
        .any(|k| !matches!(setting_prop(tree, modifier_id, count, k), Variant::Nil));
        if !has_any {
            break;
        }
        count += 1;
    }
    count
}

/// Resolves a setting's bone index from either the integer or the name form.
fn setting_bone(
    tree: &SceneTree,
    skeleton_id: NodeId,
    modifier_id: NodeId,
    index: usize,
    which: &str,
) -> Option<usize> {
    match setting_prop(tree, modifier_id, index, &format!("{which}_bone")) {
        Variant::Int(v) if v >= 0 => return Some(v as usize),
        _ => {}
    }
    let name = match setting_prop(tree, modifier_id, index, &format!("{which}_bone_name")) {
        Variant::String(s) if !s.is_empty() => s,
        _ => return None,
    };
    find_bone(tree, skeleton_id, &name)
}

/// The component of `v` along Godot's `Vector3::Axis` (`0=X 1=Y 2=Z`).
fn axis_component(v: Vector3, axis: i64) -> f32 {
    match axis {
        1 => v.y,
        2 => v.z,
        _ => v.x,
    }
}

/// `v` with its `axis` component replaced.
fn with_axis_component(v: Vector3, axis: i64, value: f32) -> Vector3 {
    match axis {
        1 => Vector3::new(v.x, value, v.z),
        2 => Vector3::new(v.x, v.y, value),
        _ => Vector3::new(value, v.y, v.z),
    }
}

/// Godot's `SkeletonModifier3D::get_roll_angle` — the signed twist of `rot`
/// about `axis` (`skeleton_modifier_3d.cpp:346`).
///
/// Projects the quaternion's vector part onto the axis, rebuilds a quaternion
/// from that component plus the original `w`, and reads its angle. A degenerate
/// (zero-length) projection is 0 rather than NaN.
fn roll_angle(rot: Quaternion, axis: Vector3) -> f32 {
    let a = axis.normalized();
    let dot = rot.x * a.x + rot.y * a.y + rot.z * a.z;
    let (rx, ry, rz, rw) = (a.x * dot, a.y * dot, a.z * dot, rot.w);
    let length = (rx * rx + ry * ry + rz * rz + rw * rw).sqrt();
    if length <= 1e-6 {
        return 0.0;
    }
    let (rx, ry, rz, rw) = (rx / length, ry / length, rz / length, rw / length);
    let angle = 2.0 * rw.clamp(-1.0, 1.0).acos();
    let direction = if rx * a.x + ry * a.y + rz * a.z > 0.0 {
        1.0
    } else {
        -1.0
    };
    angle * direction
}

/// Builds a pose from position/rotation/scale.
///
/// Scales the rotation basis per column, matching `recompose_bone_pose`:
/// `Basis` has no `scaled`, and `Transform3D::scaled` would also scale the
/// origin, which must stay exactly `position`.
fn compose_pose(position: Vector3, rotation: Quaternion, scale: Vector3) -> Transform3D {
    let r = Basis::from_quaternion(rotation);
    Transform3D {
        basis: Basis {
            x: r.x * scale.x,
            y: r.y * scale.y,
            z: r.z * scale.z,
        },
        origin: position,
    }
}

/// Applies Godot's rotation axis mask (`_process_copy`, the `axis_flags` switch).
///
/// A single axis keeps only the twist about it; **two** axes remove the twist
/// about the *third* one (not "keep those two"); all three pass through; none
/// gives identity.
fn mask_rotation(rot: Quaternion, axis_flags: i64) -> Quaternion {
    let twist = |axis: Vector3| Quaternion::from_axis_angle(axis, roll_angle(rot, axis));
    let x = Vector3::new(1.0, 0.0, 0.0);
    let y = Vector3::new(0.0, 1.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    match axis_flags & 7 {
        0 => Quaternion::IDENTITY,
        1 => twist(x),
        2 => twist(y),
        4 => twist(z),
        3 => rot * twist(z).inverse(),
        6 => rot * twist(x).inverse(),
        5 => rot * twist(y).inverse(),
        _ => rot,
    }
}

/// `CopyTransformModifier3D`: writes a reference bone's transform onto an apply
/// bone, per setting (pat-0q2kx.239).
pub fn apply_copy_transform_modifier(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    modifier_id: NodeId,
) {
    let count = constraint_setting_count(tree, modifier_id);
    for i in 0..count {
        let Some(apply) = setting_bone(tree, skeleton_id, modifier_id, i, "apply") else {
            continue;
        };
        let Some(reference) = setting_bone(tree, skeleton_id, modifier_id, i, "reference") else {
            continue;
        };
        let bones = get_bone_count(tree, skeleton_id);
        if apply >= bones || reference >= bones {
            continue;
        }

        let amount = setting_f32(tree, modifier_id, i, "amount", 1.0);
        let copy_flags = setting_int(tree, modifier_id, i, "copy_flags", FLAG_ALL);
        let axis_flags = setting_int(tree, modifier_id, i, "axis_flags", FLAG_ALL);
        let invert_flags = setting_int(tree, modifier_id, i, "invert_flags", 0);
        let additive = setting_bool(tree, modifier_id, i, "additive", false);
        let relative = setting_bool(tree, modifier_id, i, "relative", false);

        // 1. Start from the reference pose, optionally made relative to its rest.
        let mut dest = get_bone_pose(tree, skeleton_id, reference);
        if relative {
            let rest = get_bone_rest(tree, skeleton_id, reference);
            let rest_scale = rest.basis.get_scale();
            let dest_scale = dest.basis.get_scale();
            // A zero rest scale has no ratio to take; keep the reference value
            // rather than producing an infinity.
            let ratio = |d: f32, r: f32| if r != 0.0 { d / r } else { d };
            let rel_scale = Vector3::new(
                ratio(dest_scale.x, rest_scale.x),
                ratio(dest_scale.y, rest_scale.y),
                ratio(dest_scale.z, rest_scale.z),
            );
            let rel_rot = rest.basis.get_rotation_quaternion().inverse()
                * dest.basis.get_rotation_quaternion();
            dest = compose_pose(dest.origin - rest.origin, rel_rot, rel_scale);
        }

        // 2/3. Split, then mask position and scale per axis.
        let mut pos = dest.origin;
        let mut scl = dest.basis.get_scale();
        for axis in 0..3i64 {
            if axis_flags & (1 << axis) == 0 {
                pos = with_axis_component(pos, axis, 0.0);
                scl = with_axis_component(scl, axis, 1.0);
            }
        }
        // 4. Mask rotation by the flag *combination*.
        let mut rot = mask_rotation(dest.basis.get_rotation_quaternion(), axis_flags);

        // 5. Invert the axes that are both enabled and inverted.
        for axis in 0..3i64 {
            let bit = 1 << axis;
            if axis_flags & bit != 0 && invert_flags & bit != 0 {
                pos = with_axis_component(pos, axis, -axis_component(pos, axis));
                let s = axis_component(scl, axis);
                scl = with_axis_component(scl, axis, if s != 0.0 { 1.0 / s } else { s });
                rot = match axis {
                    1 => Quaternion { y: -rot.y, ..rot },
                    2 => Quaternion { z: -rot.z, ..rot },
                    _ => Quaternion { x: -rot.x, ..rot },
                };
            }
        }
        rot = rot.normalized();

        // 6. Recompose against the apply bone's pose (additive), its rest
        //    (relative), or absolutely.
        let target = if additive {
            let base_pos = get_bone_pose_position(tree, skeleton_id, apply);
            let base_rot = get_bone_pose_rotation(tree, skeleton_id, apply);
            let base_scl = get_bone_pose_scale(tree, skeleton_id, apply);
            compose_pose(
                base_pos + pos,
                base_rot * rot,
                Vector3::new(base_scl.x * scl.x, base_scl.y * scl.y, base_scl.z * scl.z),
            )
        } else if relative {
            let rest = get_bone_rest(tree, skeleton_id, apply);
            let rest_scl = rest.basis.get_scale();
            compose_pose(
                rest.origin + pos,
                rest.basis.get_rotation_quaternion() * rot,
                Vector3::new(rest_scl.x * scl.x, rest_scl.y * scl.y, rest_scl.z * scl.z),
            )
        } else {
            compose_pose(pos, rot, scl)
        };

        // 7. Blend by `amount` — Godot interpolates the whole transform.
        let blended = get_bone_pose(tree, skeleton_id, apply).interpolate_with(target, amount);

        // 8. Write only the components `copy_flags` selects.
        if copy_flags & 1 != 0 {
            set_bone_pose_position(tree, skeleton_id, apply, blended.origin);
        }
        if copy_flags & 2 != 0 {
            set_bone_pose_rotation(
                tree,
                skeleton_id,
                apply,
                blended.basis.get_rotation_quaternion(),
            );
        }
        if copy_flags & 4 != 0 {
            set_bone_pose_scale(tree, skeleton_id, apply, blended.basis.get_scale());
        }
    }
}

/// `ConvertTransformModifier3D`: maps one transform component of a reference
/// bone onto a component of an apply bone, through a range remap
/// (pat-0q2kx.239).
pub fn apply_convert_transform_modifier(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    modifier_id: NodeId,
) {
    let count = constraint_setting_count(tree, modifier_id);
    for i in 0..count {
        let Some(apply) = setting_bone(tree, skeleton_id, modifier_id, i, "apply") else {
            continue;
        };
        let Some(reference) = setting_bone(tree, skeleton_id, modifier_id, i, "reference") else {
            continue;
        };
        let bones = get_bone_count(tree, skeleton_id);
        if apply >= bones || reference >= bones {
            continue;
        }

        let amount = setting_f32(tree, modifier_id, i, "amount", 1.0);
        let ref_mode = setting_int(tree, modifier_id, i, "reference_transform_mode", 0);
        let ref_axis = setting_int(tree, modifier_id, i, "reference_axis", 0);
        let ref_min = setting_f32(tree, modifier_id, i, "reference_range_min", 0.0);
        let ref_max = setting_f32(tree, modifier_id, i, "reference_range_max", 0.0);
        let apply_mode = setting_int(tree, modifier_id, i, "apply_transform_mode", 0);
        let apply_axis = setting_int(tree, modifier_id, i, "apply_axis", 0);
        let apply_min = setting_f32(tree, modifier_id, i, "apply_range_min", 0.0);
        let apply_max = setting_f32(tree, modifier_id, i, "apply_range_max", 0.0);
        // Godot's ConvertTransform3DSetting defaults `relative` to TRUE.
        let relative = setting_bool(tree, modifier_id, i, "relative", true);
        let additive = setting_bool(tree, modifier_id, i, "additive", false);

        // Extract the scalar the reference contributes.
        let src = get_bone_pose(tree, skeleton_id, reference);
        let mut point = match ref_mode {
            1 => {
                let axis = match ref_axis {
                    1 => Vector3::new(0.0, 1.0, 0.0),
                    2 => Vector3::new(0.0, 0.0, 1.0),
                    _ => Vector3::new(1.0, 0.0, 0.0),
                };
                roll_angle(src.basis.get_rotation_quaternion(), axis)
            }
            2 => axis_component(src.basis.get_scale(), ref_axis),
            _ => axis_component(src.origin, ref_axis),
        };

        // Remap. ⚠️ A degenerate reference range is a STEP, not a division:
        // Godot yields 0 at or below the bound and 1 above it.
        point = if (ref_max - ref_min).abs() <= 1e-6 {
            if point <= ref_min {
                0.0
            } else {
                1.0
            }
        } else {
            (point - ref_min) / (ref_max - ref_min)
        };
        point = apply_min + (apply_max - apply_min) * point.clamp(0.0, 1.0);

        // Write the mapped scalar into the apply component.
        let current = get_bone_pose(tree, skeleton_id, apply);
        let target = match apply_mode {
            1 => {
                let axis = match apply_axis {
                    1 => Vector3::new(0.0, 1.0, 0.0),
                    2 => Vector3::new(0.0, 0.0, 1.0),
                    _ => Vector3::new(1.0, 0.0, 0.0),
                };
                let base = if additive {
                    roll_angle(get_bone_pose_rotation(tree, skeleton_id, apply), axis)
                } else if relative {
                    roll_angle(
                        get_bone_rest(tree, skeleton_id, apply)
                            .basis
                            .get_rotation_quaternion(),
                        axis,
                    )
                } else {
                    0.0
                };
                compose_pose(
                    current.origin,
                    Quaternion::from_axis_angle(axis, base + point),
                    current.basis.get_scale(),
                )
            }
            2 => {
                let base = if additive {
                    axis_component(get_bone_pose_scale(tree, skeleton_id, apply), apply_axis)
                } else if relative {
                    axis_component(
                        get_bone_rest(tree, skeleton_id, apply).basis.get_scale(),
                        apply_axis,
                    )
                } else {
                    0.0
                };
                compose_pose(
                    current.origin,
                    current.basis.get_rotation_quaternion(),
                    with_axis_component(current.basis.get_scale(), apply_axis, base + point),
                )
            }
            _ => {
                let base = if additive {
                    axis_component(get_bone_pose_position(tree, skeleton_id, apply), apply_axis)
                } else if relative {
                    axis_component(get_bone_rest(tree, skeleton_id, apply).origin, apply_axis)
                } else {
                    0.0
                };
                compose_pose(
                    with_axis_component(current.origin, apply_axis, base + point),
                    current.basis.get_rotation_quaternion(),
                    current.basis.get_scale(),
                )
            }
        };

        let blended = current.interpolate_with(target, amount);
        set_bone_pose_position(tree, skeleton_id, apply, blended.origin);
        set_bone_pose_rotation(
            tree,
            skeleton_id,
            apply,
            blended.basis.get_rotation_quaternion(),
        );
        set_bone_pose_scale(tree, skeleton_id, apply, blended.basis.get_scale());
    }
}

/// Aims one bone's forward axis at a target — Godot's `LookAtModifier3D`
/// (pat-0q2kx.156).
///
/// The target is `target_node` (a NodePath resolved relative to the modifier,
/// which is Godot's API), falling back to a literal `target_position` so the
/// modifier is usable without a scene node — the same key the IK modifiers in
/// this file already accept.
///
/// Rotation is built as an explicit axis/angle rather than through
/// [`crate::ik_solver::aim_rotation`] because `use_angle_limitation` has to clamp
/// the **angle** before it becomes a rotation, and a quaternion would have to be
/// decomposed again to do that. The frame conversion at the end is the same one
/// `solve_fabrik_chain` and `solve_ccd` use: poses are stored in the bone's
/// parent frame, so a skeleton-space delta must be conjugated into it.
///
/// Not yet modelled: `duration`/`transition_type`/`ease_type` time-based
/// interpolation, `use_secondary_rotation`, the per-axis positive/negative limit
/// split, damp thresholds, and `origin_from`/`origin_offset`. Those properties
/// are registered but do not affect the result.
fn apply_look_at_modifier(tree: &mut SceneTree, skeleton_id: NodeId, modifier_id: NodeId) {
    // Collect every read before mutating, so the immutable borrows end here.
    let (bone, forward_axis, use_limit, limit, target_path, literal_target) = {
        let node = match tree.get_node(modifier_id) {
            Some(n) => n,
            None => return,
        };
        // `bone` defaults to -1 ("unset"); Godot aims nothing in that case.
        let bone = match node.get_property("bone") {
            Variant::Int(i) if i >= 0 => i as usize,
            _ => return,
        };
        let forward_axis = match node.get_property("forward_axis") {
            Variant::Int(i) => i,
            _ => 4,
        };
        let use_limit = matches!(
            node.get_property("use_angle_limitation"),
            Variant::Bool(true)
        );
        // Godot's default is TAU — unlimited. A missing value must not read as 0,
        // which would forbid rotation entirely.
        let limit = match node.get_property("primary_limit_angle") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => std::f32::consts::TAU,
        };
        let target_path = match node.get_property("target_node") {
            Variant::NodePath(p) if !p.is_empty() => Some(p.to_string()),
            Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        };
        let literal_target = match node.get_property(IK_TARGET_POSITION_KEY) {
            Variant::Vector3(v) => Some(v),
            _ => None,
        };
        (
            bone,
            forward_axis,
            use_limit,
            limit,
            target_path,
            literal_target,
        )
    };

    let target = match target_path
        .and_then(|p| tree.get_node_or_null(modifier_id, &p))
        .map(|id| crate::node3d::get_global_transform(tree, id).origin)
        .or(literal_target)
    {
        Some(t) => t,
        None => return,
    };

    let bone_global = get_bone_global_pose(tree, skeleton_id, bone);
    let to_vec = target - bone_global.origin;
    // `Basis` has no `Mul<Vector3>`; `xform` is the vector-transform API here.
    let forward_world = bone_global.basis.xform(bone_axis_vector(forward_axis));
    if to_vec.length() < IK_EPSILON || forward_world.length() < IK_EPSILON {
        return;
    }

    let from = forward_world.normalized();
    let to = to_vec.normalized();
    let mut axis = from.cross(to);
    if axis.length() < IK_EPSILON {
        // Degenerate: the cross product vanishes both when already aimed and when
        // the target sits exactly BEHIND the bone. Those need opposite handling.
        if from.dot(to) > 0.0 {
            return; // already aimed — nothing to do
        }
        // Exactly opposed. Any perpendicular axis gives a valid 180° turn, which
        // is what `ik_solver::aim_rotation` does for the same case. Returning
        // early here instead would make a target directly behind the bone
        // permanently unreachable — the bone would sit at its old aim forever.
        axis = crate::ik_solver::any_perpendicular(from);
    }
    let mut angle = from.dot(to).clamp(-1.0, 1.0).acos();
    if use_limit {
        angle = angle.min(limit);
    }
    angle *= modifier_influence(tree, modifier_id);
    if angle.abs() < IK_EPSILON {
        return;
    }

    let delta = Basis::from_quaternion(Quaternion::from_axis_angle(axis.normalized(), angle));
    let parent = get_bone_parent(tree, skeleton_id, bone);
    let parent_basis = if parent >= 0 {
        get_bone_global_pose(tree, skeleton_id, parent as usize).basis
    } else {
        Basis::IDENTITY
    };
    let rest = get_bone_rest(tree, skeleton_id, bone);
    let pose = get_bone_pose(tree, skeleton_id, bone);
    let frame = parent_basis * rest.basis;
    let mut new_pose = pose;
    new_pose.basis = frame.inverse() * delta * frame * pose.basis;
    set_bone_pose(tree, skeleton_id, bone, new_pose);
}

/// Which algorithm a modifier's class selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IkSolverKind {
    /// Cyclic Coordinate Descent — `IKModifier3D` (pat-nzgdu.151).
    Ccd,
    /// Forward And Backward Reaching IK — `FABRIK3D` (pat-nzgdu.183).
    Fabrik,
}

/// Applies one IK modifier to its skeleton, using the algorithm its class picks.
fn apply_ik_modifier(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    modifier_id: NodeId,
    solver: IkSolverKind,
) {
    let node = match tree.get_node(modifier_id) {
        Some(n) => n,
        None => return,
    };
    let target = match node.get_property(IK_TARGET_POSITION_KEY) {
        Variant::Vector3(v) => v,
        _ => return, // no target: nothing to solve toward
    };
    let root = match node.get_property(IK_ROOT_BONE_KEY) {
        Variant::Int(i) if i >= 0 => i as usize,
        _ => return,
    };
    let tip = match node.get_property(IK_TIP_BONE_KEY) {
        Variant::Int(i) if i >= 0 => i as usize,
        _ => return,
    };
    let iterations = match node.get_property(IK_ITERATIONS_KEY) {
        Variant::Int(i) => i.clamp(0, 64) as u32,
        _ => 10,
    };
    let influence = modifier_influence(tree, modifier_id);
    if influence <= 0.0 {
        // Godot: influence 0 means the modifier contributes nothing at all.
        return;
    }

    match solver {
        IkSolverKind::Ccd => solve_ccd(tree, skeleton_id, root, tip, target, iterations, influence),
        IkSolverKind::Fabrik => {
            solve_fabrik_chain(tree, skeleton_id, root, tip, target, iterations, influence)
        }
    }
}

/// Solves the chain with FABRIK and writes the result into the bone poses
/// (pat-nzgdu.183).
///
/// The math core `ik_solver::solve_fabrik` already existed with its own tests —
/// but it works in **world positions**, while a skeleton stores **poses**. This
/// is the missing half: run FABRIK on the chain's joint positions, then turn the
/// solved positions back into per-bone rotations.
///
/// Unlike CCD, FABRIK converges in one pass over the chain, so the iteration
/// budget is spent inside `solve_fabrik` rather than re-walking the joints here.
///
/// `influence` scales each correction angle, matching `solve_ccd`: `0` leaves
/// the pose untouched, `1` is a full solve.
#[allow(clippy::too_many_arguments)]
fn solve_fabrik_chain(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    root: usize,
    tip: usize,
    target: Vector3,
    iterations: u32,
    influence: f32,
) {
    let chain = bone_chain(tree, skeleton_id, root, tip);
    if chain.len() < 2 {
        return; // nothing to rotate (or tip does not descend from root)
    }

    // FABRIK works on the chain's joint positions in skeleton space.
    let joints: Vec<Vector3> = chain
        .iter()
        .map(|&b| get_bone_global_pose(tree, skeleton_id, b).origin)
        .collect();

    // NOTE: the IterateIK3D properties registered in ClassDB (`max_iterations`,
    // `min_distance`, `angular_delta_limit`, `deterministic`) are NOT consumed
    // here — this reads pat-nzgdu.151's flat `iterations` property and a fixed
    // tolerance instead. Most visibly, `angular_delta_limit` (Godot's 2°/frame
    // rotation cap) is unimplemented: Godot eases a big swing over many frames
    // where this arrives in one. Wiring those knobs is follow-up; see the bead.
    let solved = crate::ik_solver::solve_fabrik(&joints, target, iterations as usize, IK_EPSILON);

    // Turn solved positions back into rotations, root → tip. Each bone rotates
    // about its OWN origin, and rotating it carries its whole subtree with it —
    // so the current positions are re-read every step rather than reused from
    // `joints`, which went stale the moment the parent moved.
    for i in 0..chain.len() - 1 {
        let joint = chain[i];
        let child = chain[i + 1];

        let joint_pos = get_bone_global_pose(tree, skeleton_id, joint).origin;
        let child_pos = get_bone_global_pose(tree, skeleton_id, child).origin;

        let from_vec = child_pos - joint_pos;
        // Aim at where FABRIK put the child, measured from where this joint
        // actually is now.
        let to_vec = solved[i + 1] - joint_pos;
        if from_vec.length() < IK_EPSILON || to_vec.length() < IK_EPSILON {
            continue;
        }
        let from = from_vec.normalized();
        let to = to_vec.normalized();

        let axis = from.cross(to);
        if axis.length() < IK_EPSILON {
            continue; // already aligned (or exactly opposed — no unique axis)
        }
        let angle = from.dot(to).clamp(-1.0, 1.0).acos() * influence;
        if angle.abs() < IK_EPSILON {
            continue;
        }
        let delta = Basis::from_quaternion(Quaternion::from_axis_angle(axis.normalized(), angle));

        // Same frame conversion as solve_ccd: `delta` is a rotation in skeleton
        // space, but poses are stored in the bone's parent frame.
        let parent = get_bone_parent(tree, skeleton_id, joint);
        let parent_basis = if parent >= 0 {
            get_bone_global_pose(tree, skeleton_id, parent as usize).basis
        } else {
            Basis::IDENTITY
        };
        let rest = get_bone_rest(tree, skeleton_id, joint);
        let pose = get_bone_pose(tree, skeleton_id, joint);

        let frame = parent_basis * rest.basis;
        let mut new_pose = pose;
        new_pose.basis = frame.inverse() * delta * frame * pose.basis;
        set_bone_pose(tree, skeleton_id, joint, new_pose);
    }
}

/// Cyclic Coordinate Descent: rotate each joint in turn so the tip swings
/// toward `target` (pat-nzgdu.151).
///
/// Only bone **poses** are written, and a bone's global pose is composed from
/// its parents, so the chain stays connected by construction — intermediate
/// bones cannot detach.
///
/// `influence` scales each correction angle, which gives Godot's blend
/// behaviour at the ends that matter: `0` leaves the pose untouched, `1` is a
/// full solve, and values between move the tip partway.
#[allow(clippy::too_many_arguments)]
fn solve_ccd(
    tree: &mut SceneTree,
    skeleton_id: NodeId,
    root: usize,
    tip: usize,
    target: Vector3,
    iterations: u32,
    influence: f32,
) {
    let chain = bone_chain(tree, skeleton_id, root, tip);
    if chain.len() < 2 {
        return; // nothing to rotate (or tip does not descend from root)
    }

    for _ in 0..iterations {
        // Walk joints from the one nearest the tip back to the root — the
        // "cyclic" part; near-tip joints make the fine corrections.
        for &joint in chain.iter().rev().skip(1) {
            let joint_pos = get_bone_global_pose(tree, skeleton_id, joint).origin;
            let tip_pos = get_bone_global_pose(tree, skeleton_id, tip).origin;

            let to_tip = tip_pos - joint_pos;
            let to_target = target - joint_pos;
            if to_tip.length() < IK_EPSILON || to_target.length() < IK_EPSILON {
                continue;
            }
            let from = to_tip.normalized();
            let to = to_target.normalized();

            let axis = from.cross(to);
            if axis.length() < IK_EPSILON {
                continue; // already aligned (or exactly opposed — no unique axis)
            }
            let angle = from.dot(to).clamp(-1.0, 1.0).acos() * influence;
            if angle.abs() < IK_EPSILON {
                continue;
            }
            let delta =
                Basis::from_quaternion(Quaternion::from_axis_angle(axis.normalized(), angle));

            // `delta` is a rotation in skeleton space, but poses are stored in
            // the bone's parent frame. global(j) = parent_global * rest * pose,
            // so to get global' = delta * global we need
            //   pose' = (parent_global.basis * rest.basis)^-1
            //           * delta * (parent_global.basis * rest.basis) * pose
            let parent = get_bone_parent(tree, skeleton_id, joint);
            let parent_basis = if parent >= 0 {
                get_bone_global_pose(tree, skeleton_id, parent as usize).basis
            } else {
                Basis::IDENTITY
            };
            let rest = get_bone_rest(tree, skeleton_id, joint);
            let pose = get_bone_pose(tree, skeleton_id, joint);

            let frame = parent_basis * rest.basis;
            let mut new_pose = pose;
            new_pose.basis = frame.inverse() * delta * frame * pose.basis;
            set_bone_pose(tree, skeleton_id, joint, new_pose);
        }
    }
}

// ===========================================================================
// Per-frame runtime pass (pat-0q2kx.3)
// ===========================================================================

/// Godot's `Skeleton3D` class name.
pub const SKELETON_CLASS: &str = "Skeleton3D";
/// Godot's `BoneAttachment3D` class name.
pub const BONE_ATTACHMENT_CLASS: &str = "BoneAttachment3D";

/// Returns every `Skeleton3D` node in the tree, in tree order.
pub fn skeletons_in_tree(tree: &SceneTree) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| {
            tree.get_node(id)
                .map(|n| n.class_name() == SKELETON_CLASS)
                .unwrap_or(false)
        })
        .collect()
}

/// Routes `AnimationPlayer` 3D bone tracks onto `Skeleton3D` bone poses
/// (pat-0q2kx.3).
///
/// In Godot a skeletal clip keys `Skeleton3D:BoneName` with `position_3d` /
/// `rotation_3d` / `scale_3d` tracks; the mixer writes them through
/// `set_bone_pose_position/_rotation/_scale` rather than as node properties,
/// which is why a keyframed rig deforms at all. The generic property-track path
/// cannot do this — it has no notion of a bone — so this pass pairs each sampled
/// value with its track type (`AnimationPlayer::current_track_types`) and writes
/// the bone components for any track whose target resolves to a `Skeleton3D`
/// whose bone names include the track's sub-path.
///
/// Runs *before* [`process_modifiers`], matching Godot's order: modifiers (IK)
/// layer on top of the animated pose rather than being overwritten by it.
///
/// NOTE: `SceneTree::process_animations` runs first and, having no bone concept,
/// also drops each bone track's value into a same-named node property on the
/// `Skeleton3D` (`"Tip"`, etc.). That stray property is inert — nothing reads
/// it — but suppressing it belongs in the property-track loop itself and is left
/// as a follow-up.
pub fn apply_animation_bone_tracks(tree: &mut SceneTree) {
    use crate::animation::TrackType;
    use std::collections::HashMap;

    fn is_bone_track(t: TrackType) -> bool {
        matches!(
            t,
            TrackType::Position3D | TrackType::Rotation3D | TrackType::Scale3D
        )
    }

    // Gather while the tree is only borrowed immutably, then apply.
    let mut writes: Vec<(NodeId, usize, TrackType, Variant)> = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let player = match tree.get_animation_player(id) {
            Some(p) => p,
            None => continue,
        };
        let types: HashMap<(String, String), TrackType> = player
            .current_track_types()
            .into_iter()
            .filter(|(_, _, t)| is_bone_track(*t))
            .map(|(n, p, t)| ((n, p), t))
            .collect();
        if types.is_empty() {
            continue;
        }
        // Same base-node resolution the property-track path uses: the player's
        // `root_node`, relative to the node the player is attached to.
        let root_node = player.root_node.clone();
        let base = if root_node.is_empty() || root_node == "." {
            id
        } else {
            tree.get_node_or_null(id, &root_node).unwrap_or(id)
        };

        for (track_node, prop, value) in player.get_current_values_targeted() {
            let track_type = match types.get(&(track_node.clone(), prop.clone())) {
                Some(t) => *t,
                None => continue,
            };
            let target = if track_node.is_empty() || track_node == "." {
                base
            } else {
                match tree.get_node_or_null(base, &track_node) {
                    Some(t) => t,
                    None => continue,
                }
            };
            if tree.get_node(target).map(|n| n.class_name()) != Some(SKELETON_CLASS) {
                continue;
            }
            // The track's sub-path names the bone (`Skeleton3D:BoneName`).
            let bone = match find_bone(tree, target, &prop) {
                Some(b) => b,
                None => continue,
            };
            writes.push((target, bone, track_type, value));
        }
    }

    for (skeleton, bone, track_type, value) in writes {
        match (track_type, value) {
            (TrackType::Position3D, Variant::Vector3(v)) => {
                set_bone_pose_position(tree, skeleton, bone, v)
            }
            (TrackType::Rotation3D, Variant::Quaternion(q)) => {
                set_bone_pose_rotation(tree, skeleton, bone, q)
            }
            (TrackType::Scale3D, Variant::Vector3(v)) => {
                set_bone_pose_scale(tree, skeleton, bone, v)
            }
            _ => {}
        }
    }
}

/// Runs the per-frame `Skeleton3D` update for every skeleton in the tree
/// (pat-0q2kx.3).
///
/// This is the runtime caller Godot has as `NOTIFICATION_UPDATE_SKELETON`
/// (`upstream/godot/scene/3d/skeleton_3d.cpp`) and Patina previously lacked, so
/// [`process_modifiers`] and [`update_bone_attachment`] never ran outside tests
/// and a rigged scene stayed frozen at its rest pose. Per skeleton, in Godot's
/// order:
///
/// 1. animated bone poses ([`apply_animation_bone_tracks`], run once for the
///    whole tree since one player can drive several skeletons),
/// 2. the modifier stack ([`process_modifiers`] — IK on top of animation),
/// 3. `BoneAttachment3D` children, which snap to their bone's final global pose.
///
/// The resulting poses are what the render bridge reads via [`bone_pose_array`]
/// to skin bound meshes.
pub fn process_skeletons(tree: &mut SceneTree) {
    let skeletons = skeletons_in_tree(tree);
    for &skeleton in &skeletons {
        cache_bone_count(tree, skeleton);
    }

    apply_animation_bone_tracks(tree);

    for skeleton in skeletons {
        process_modifiers(tree, skeleton);

        // Godot's BoneAttachment3D reads its bone from its parent Skeleton3D.
        let children = tree
            .get_node(skeleton)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();
        for child in children {
            let is_attachment = tree
                .get_node(child)
                .map(|n| n.class_name() == BONE_ATTACHMENT_CLASS)
                .unwrap_or(false);
            if is_attachment {
                update_bone_attachment(tree, skeleton, child);
            }
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
    use gdcore::math::Vector3;
    use gdcore::math3d::{Basis, Quaternion};

    fn make_tree() -> SceneTree {
        SceneTree::new()
    }

    // -- Basic bone management -----------------------------------------------

    #[test]
    fn add_bones_and_count() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        assert_eq!(get_bone_count(&tree, skel_id), 0);

        let hip = add_bone(&mut tree, skel_id, "Hip");
        assert_eq!(hip, 0);
        assert_eq!(get_bone_count(&tree, skel_id), 1);

        let spine = add_bone(&mut tree, skel_id, "Spine");
        assert_eq!(spine, 1);
        assert_eq!(get_bone_count(&tree, skel_id), 2);
    }

    #[test]
    fn find_bone_by_name() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Hip");
        add_bone(&mut tree, skel_id, "Spine");
        add_bone(&mut tree, skel_id, "Head");

        assert_eq!(find_bone(&tree, skel_id, "Spine"), Some(1));
        assert_eq!(find_bone(&tree, skel_id, "Head"), Some(2));
        assert_eq!(find_bone(&tree, skel_id, "Missing"), None);
    }

    #[test]
    fn bone_name_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "OldName");
        assert_eq!(get_bone_name(&tree, skel_id, 0), Some("OldName".into()));

        set_bone_name(&mut tree, skel_id, 0, "NewName");
        assert_eq!(get_bone_name(&tree, skel_id, 0), Some("NewName".into()));
    }

    #[test]
    fn bone_name_out_of_range() {
        let tree = make_tree();
        let root = tree.root_id();
        assert_eq!(get_bone_name(&tree, root, 99), None);
    }

    // -- Parent hierarchy ----------------------------------------------------

    #[test]
    fn bone_parent_default_is_negative_one() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        assert_eq!(get_bone_parent(&tree, skel_id, 0), -1);
    }

    #[test]
    fn set_bone_parent_builds_hierarchy() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        let hip = add_bone(&mut tree, skel_id, "Hip");
        let spine = add_bone(&mut tree, skel_id, "Spine");
        let head = add_bone(&mut tree, skel_id, "Head");

        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_parent(&mut tree, skel_id, head, spine as i64);

        assert_eq!(get_bone_parent(&tree, skel_id, hip), -1);
        assert_eq!(get_bone_parent(&tree, skel_id, spine), 0);
        assert_eq!(get_bone_parent(&tree, skel_id, head), 1);
    }

    #[test]
    fn get_bone_children_lists_children() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        let hip = add_bone(&mut tree, skel_id, "Hip");
        let left_leg = add_bone(&mut tree, skel_id, "LeftLeg");
        let right_leg = add_bone(&mut tree, skel_id, "RightLeg");
        let spine = add_bone(&mut tree, skel_id, "Spine");

        set_bone_parent(&mut tree, skel_id, left_leg, hip as i64);
        set_bone_parent(&mut tree, skel_id, right_leg, hip as i64);
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);

        let children = get_bone_children(&tree, skel_id, hip as i64);
        assert_eq!(children, vec![1, 2, 3]);

        // Root bones (parent -1) — only Hip
        let roots = get_bone_children(&tree, skel_id, -1);
        assert_eq!(roots, vec![0]);
    }

    // -- Rest pose -----------------------------------------------------------

    #[test]
    fn bone_rest_default_is_identity() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        assert_eq!(get_bone_rest(&tree, skel_id, 0), Transform3D::IDENTITY);
    }

    #[test]
    fn set_bone_rest_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Hip");
        let rest = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 1.0, 0.0),
        };
        set_bone_rest(&mut tree, skel_id, 0, rest);
        assert_eq!(get_bone_rest(&tree, skel_id, 0), rest);
    }

    // -- Pose override -------------------------------------------------------

    #[test]
    fn bone_pose_default_is_identity() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        assert_eq!(get_bone_pose(&tree, skel_id, 0), Transform3D::IDENTITY);
    }

    #[test]
    fn set_bone_pose_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        let pose = Transform3D {
            basis: Basis::from_euler(Vector3::new(0.0, 0.5, 0.0)),
            origin: Vector3::ZERO,
        };
        set_bone_pose(&mut tree, skel_id, 0, pose);
        assert_eq!(get_bone_pose(&tree, skel_id, 0), pose);
    }

    // -- Bone enabled --------------------------------------------------------

    #[test]
    fn bone_enabled_default_is_true() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        assert!(is_bone_enabled(&tree, skel_id, 0));
    }

    #[test]
    fn disable_and_reenable_bone() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Root");
        set_bone_enabled(&mut tree, skel_id, 0, false);
        assert!(!is_bone_enabled(&tree, skel_id, 0));

        set_bone_enabled(&mut tree, skel_id, 0, true);
        assert!(is_bone_enabled(&tree, skel_id, 0));
    }

    // -- Global pose ---------------------------------------------------------

    #[test]
    fn global_pose_single_bone() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "Hip");
        let rest = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 1.0, 0.0),
        };
        set_bone_rest(&mut tree, skel_id, 0, rest);

        let global = get_bone_global_pose(&tree, skel_id, 0);
        assert_eq!(global.origin, Vector3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn global_pose_chain() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        // Hip at Y=1
        let hip = add_bone(&mut tree, skel_id, "Hip");
        set_bone_rest(
            &mut tree,
            skel_id,
            hip,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 1.0, 0.0),
            },
        );

        // Spine at Y=0.5 relative to Hip
        let spine = add_bone(&mut tree, skel_id, "Spine");
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_rest(
            &mut tree,
            skel_id,
            spine,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.5, 0.0),
            },
        );

        // Head at Y=0.3 relative to Spine
        let head = add_bone(&mut tree, skel_id, "Head");
        set_bone_parent(&mut tree, skel_id, head, spine as i64);
        set_bone_rest(
            &mut tree,
            skel_id,
            head,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.3, 0.0),
            },
        );

        let hip_global = get_bone_global_pose(&tree, skel_id, hip);
        let spine_global = get_bone_global_pose(&tree, skel_id, spine);
        let head_global = get_bone_global_pose(&tree, skel_id, head);

        assert!((hip_global.origin.y - 1.0).abs() < 1e-5);
        assert!((spine_global.origin.y - 1.5).abs() < 1e-5);
        assert!((head_global.origin.y - 1.8).abs() < 1e-5);
    }

    #[test]
    fn global_pose_with_animation_override() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        let hip = add_bone(&mut tree, skel_id, "Hip");
        set_bone_rest(
            &mut tree,
            skel_id,
            hip,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 1.0, 0.0),
            },
        );

        let spine = add_bone(&mut tree, skel_id, "Spine");
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_rest(
            &mut tree,
            skel_id,
            spine,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.5, 0.0),
            },
        );

        // Apply a pose offset to spine: shift X by 1.0
        set_bone_pose(
            &mut tree,
            skel_id,
            spine,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(1.0, 0.0, 0.0),
            },
        );

        let spine_global = get_bone_global_pose(&tree, skel_id, spine);
        // Hip rest origin (0,1,0) + spine rest origin (0,0.5,0) + spine pose (1,0,0)
        assert!((spine_global.origin.x - 1.0).abs() < 1e-5);
        assert!((spine_global.origin.y - 1.5).abs() < 1e-5);
    }

    #[test]
    fn global_pose_out_of_range_returns_identity() {
        let tree = make_tree();
        let root = tree.root_id();
        assert_eq!(get_bone_global_pose(&tree, root, 99), Transform3D::IDENTITY);
    }

    // -- Clear poses ---------------------------------------------------------

    #[test]
    fn clear_bones_pose_resets_all() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        add_bone(&mut tree, skel_id, "A");
        add_bone(&mut tree, skel_id, "B");

        let offset = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(5.0, 0.0, 0.0),
        };
        set_bone_pose(&mut tree, skel_id, 0, offset);
        set_bone_pose(&mut tree, skel_id, 1, offset);

        clear_bones_pose(&mut tree, skel_id);
        assert_eq!(get_bone_pose(&tree, skel_id, 0), Transform3D::IDENTITY);
        assert_eq!(get_bone_pose(&tree, skel_id, 1), Transform3D::IDENTITY);
    }

    // -- Pose array + skinning bridge ----------------------------------------

    #[test]
    fn bone_pose_array_matches_individual_poses() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        let hip = add_bone(&mut tree, skel_id, "Hip");
        set_bone_rest(
            &mut tree,
            skel_id,
            hip,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 1.0, 0.0),
            },
        );
        let spine = add_bone(&mut tree, skel_id, "Spine");
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_rest(
            &mut tree,
            skel_id,
            spine,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.5, 0.0),
            },
        );

        let poses = bone_pose_array(&tree, skel_id);
        assert_eq!(poses.len(), 2);
        assert_eq!(poses[0], get_bone_global_pose(&tree, skel_id, 0));
        assert_eq!(poses[1], get_bone_global_pose(&tree, skel_id, 1));
        // Spine global Y = hip(1.0) + spine(0.5).
        assert!((poses[1].origin.y - 1.5).abs() < 1e-5);
    }

    /// End-to-end: `Skeleton3D` global poses, gathered via [`bone_pose_array`],
    /// drive `gdserver3d::skin::skin_mesh` to deform a rigged mesh. This closes
    /// the "skeleton3d computes poses nothing consumes" gap.
    #[test]
    fn skeleton_poses_drive_mesh_skinning() {
        use gdserver3d::mesh::{Mesh3D, PrimitiveType};
        use gdserver3d::skin::{skin_mesh, Skin};

        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        // bone0 at origin, bone1 (child) resting at Y=1 (a "joint").
        let b0 = add_bone(&mut tree, skel_id, "Root");
        let b1 = add_bone(&mut tree, skel_id, "Tip");
        set_bone_parent(&mut tree, skel_id, b1, b0 as i64);
        set_bone_rest(
            &mut tree,
            skel_id,
            b1,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 1.0, 0.0),
            },
        );

        // Register a Skin whose binds invert each bone's rest global pose.
        let mut skin = Skin::new();
        skin.add_bind_from_rest(b0, get_bone_global_pose(&tree, skel_id, b0));
        skin.add_bind_from_rest(b1, get_bone_global_pose(&tree, skel_id, b1));

        // Vertex 0 at the root joint -> bind 0; vertex 1 above the tip joint
        // (Y=2) -> bind 1, so a rotation of bone1 about its joint swings it.
        let mut mesh = Mesh3D::new(PrimitiveType::Points);
        mesh.vertices = vec![Vector3::ZERO, Vector3::new(0.0, 2.0, 0.0)];
        mesh.indices = vec![0, 1];
        mesh.bone_weights = vec![[1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
        mesh.bone_indices = vec![[0, 0, 0, 0], [1, 0, 0, 0]];
        assert!(mesh.is_skinned());

        // At rest the skin is a no-op.
        let rest_poses = bone_pose_array(&tree, skel_id);
        let rest_skinned = skin_mesh(&mesh, &skin, &rest_poses);
        assert!((rest_skinned.vertices[1] - Vector3::new(0.0, 2.0, 0.0)).length() < 1e-4);

        // Rotate bone1 90° about Z at its joint; the tip vertex swings sideways.
        set_bone_pose(
            &mut tree,
            skel_id,
            b1,
            Transform3D {
                basis: Basis::from_euler(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2)),
                origin: Vector3::ZERO,
            },
        );
        let posed = bone_pose_array(&tree, skel_id);
        let posed_skinned = skin_mesh(&mesh, &skin, &posed);

        let moved = (posed_skinned.vertices[1] - rest_skinned.vertices[1]).length();
        assert!(
            moved > 0.5,
            "posed skin should move the tip vertex ({moved})"
        );
        // Root vertex (bind 0, unposed) stays put.
        assert!((posed_skinned.vertices[0] - Vector3::ZERO).length() < 1e-4);
    }

    // -- BoneAttachment3D helpers --------------------------------------------

    #[test]
    fn bone_attachment_name_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let att = Node::new("Attachment", "BoneAttachment3D");
        let att_id = tree.add_child(root, att).unwrap();

        assert_eq!(get_bone_attachment_bone_name(&tree, att_id), None);
        set_bone_attachment_bone_name(&mut tree, att_id, "Head");
        assert_eq!(
            get_bone_attachment_bone_name(&tree, att_id),
            Some("Head".into())
        );
    }

    #[test]
    fn bone_attachment_idx_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let att = Node::new("Attachment", "BoneAttachment3D");
        let att_id = tree.add_child(root, att).unwrap();

        assert_eq!(get_bone_attachment_bone_idx(&tree, att_id), -1);
        set_bone_attachment_bone_idx(&mut tree, att_id, 3);
        assert_eq!(get_bone_attachment_bone_idx(&tree, att_id), 3);
    }

    // -- Out-of-range writes are silently ignored ----------------------------

    #[test]
    fn set_bone_rest_out_of_range_noop() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        // No bones added — set should be a no-op.
        set_bone_rest(&mut tree, skel_id, 0, Transform3D::IDENTITY);
        assert_eq!(get_bone_count(&tree, skel_id), 0);
    }

    #[test]
    fn set_bone_pose_out_of_range_noop() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        set_bone_pose(&mut tree, skel_id, 5, Transform3D::IDENTITY);
        assert_eq!(get_bone_count(&tree, skel_id), 0);
    }

    #[test]
    fn set_bone_parent_out_of_range_noop() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = Node::new("Skeleton", "Skeleton3D");
        let skel_id = tree.add_child(root, skel).unwrap();

        set_bone_parent(&mut tree, skel_id, 0, -1);
        assert_eq!(get_bone_count(&tree, skel_id), 0);
    }

    #[test]
    fn per_component_pose_accessors_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = tree
            .add_child(root, Node::new("Skeleton", "Skeleton3D"))
            .unwrap();
        let hip = add_bone(&mut tree, skel, "Hip");

        // Position.
        let pos = Vector3::new(1.0, 2.0, 3.0);
        set_bone_pose_position(&mut tree, skel, hip, pos);
        assert_eq!(get_bone_pose_position(&tree, skel, hip), pos);

        // Rotation (90° about Y).
        let rot = Quaternion::new(0.0, 0.707_106_77, 0.0, 0.707_106_77);
        set_bone_pose_rotation(&mut tree, skel, hip, rot);
        let got = get_bone_pose_rotation(&tree, skel, hip);
        assert!(
            (got.x - rot.x).abs() < 1e-6
                && (got.y - rot.y).abs() < 1e-6
                && (got.z - rot.z).abs() < 1e-6
                && (got.w - rot.w).abs() < 1e-6,
            "rotation round-trip: {got:?}"
        );

        // Scale.
        let scale = Vector3::new(2.0, 2.0, 2.0);
        set_bone_pose_scale(&mut tree, skel, hip, scale);
        assert_eq!(get_bone_pose_scale(&tree, skel, hip), scale);

        // The recomposed pose Transform3D carries the position as its origin.
        assert_eq!(get_bone_pose(&tree, skel, hip).origin, pos);

        // A fresh bone has identity/default components.
        let spine = add_bone(&mut tree, skel, "Spine");
        assert_eq!(get_bone_pose_position(&tree, skel, spine), Vector3::ZERO);
        assert_eq!(get_bone_pose_scale(&tree, skel, spine), Vector3::ONE);
        assert!((get_bone_pose_rotation(&tree, skel, spine).w - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bone_attachment_follows_bone_global_pose() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = tree
            .add_child(root, Node::new("Skeleton", "Skeleton3D"))
            .unwrap();
        let hip = add_bone(&mut tree, skel, "Hip");

        // Animate the bone by moving its pose.
        let pos = Vector3::new(5.0, 0.0, 0.0);
        set_bone_pose_position(&mut tree, skel, hip, pos);

        let attach = tree
            .add_child(root, Node::new("Attach", "BoneAttachment3D"))
            .unwrap();
        set_bone_attachment_bone_idx(&mut tree, attach, hip as i64);

        let applied =
            update_bone_attachment(&mut tree, skel, attach).expect("attachment tracks the bone");
        assert_eq!(applied.origin, pos);

        // The attachment node's transform property now equals the bone pose.
        match tree.get_node(attach).unwrap().get_property("transform") {
            Variant::Transform3D(t) => assert_eq!(t.origin, pos),
            other => panic!("attachment transform not set: {other:?}"),
        }
    }

    #[test]
    fn bone_attachment_resolves_by_name() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let skel = tree
            .add_child(root, Node::new("Skeleton", "Skeleton3D"))
            .unwrap();
        let _hip = add_bone(&mut tree, skel, "Hip");
        let head = add_bone(&mut tree, skel, "Head");
        let pos = Vector3::new(0.0, 7.0, 0.0);
        set_bone_pose_position(&mut tree, skel, head, pos);

        let attach = tree
            .add_child(root, Node::new("Attach", "BoneAttachment3D"))
            .unwrap();
        // No bone_idx set (-1) → resolve via bone_name.
        set_bone_attachment_bone_name(&mut tree, attach, "Head");

        let applied = update_bone_attachment(&mut tree, skel, attach)
            .expect("attachment resolves bone by name");
        assert_eq!(applied.origin, pos);
    }

    // -- Per-frame runtime pass (pat-0q2kx.3) --------------------------------

    /// Builds a two-bone skeleton the way a `.tscn` does: `bones/{idx}/…`
    /// records with **no** `_bone_count` key.
    fn scene_style_skeleton(tree: &mut SceneTree) -> NodeId {
        let root = tree.root_id();
        let skel = tree
            .add_child(root, Node::new("Skeleton3D", "Skeleton3D"))
            .unwrap();
        let node = tree.get_node_mut(skel).unwrap();
        node.set_property("bones/0/name", Variant::String("Root".into()));
        node.set_property("bones/0/parent", Variant::Int(-1));
        node.set_property("bones/0/rest", Variant::Transform3D(Transform3D::IDENTITY));
        node.set_property("bones/1/name", Variant::String("Tip".into()));
        node.set_property("bones/1/parent", Variant::Int(0));
        node.set_property(
            "bones/1/rest",
            Variant::Transform3D(Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 2.0, 0.0),
            }),
        );
        skel
    }

    #[test]
    fn bone_count_infers_from_scene_loaded_records() {
        // A `.tscn` Skeleton3D carries no `_bone_count`; without inference every
        // bone API on a scene-loaded rig sees zero bones.
        let mut tree = make_tree();
        let skel = scene_style_skeleton(&mut tree);
        assert_eq!(get_bone_count(&tree, skel), 2);
        assert_eq!(get_bone_name(&tree, skel, 1).as_deref(), Some("Tip"));
        assert_eq!(find_bone(&tree, skel, "Tip"), Some(1));
        assert_eq!(get_bone_parent(&tree, skel, 1), 0);
    }

    #[test]
    fn explicit_bone_count_wins_over_inference() {
        let mut tree = make_tree();
        let skel = scene_style_skeleton(&mut tree);
        // add_bone maintains the explicit key; it must remain authoritative.
        let idx = add_bone(&mut tree, skel, "Extra");
        assert_eq!(idx, 2, "next index continues past the inferred bones");
        assert_eq!(get_bone_count(&tree, skel), 3);
    }

    #[test]
    fn global_rest_ignores_pose_overrides() {
        let mut tree = make_tree();
        let skel = scene_style_skeleton(&mut tree);
        set_bone_pose_position(&mut tree, skel, 1, Vector3::new(9.0, 9.0, 9.0));

        // The pose moves the global *pose* but must not touch the global rest.
        assert_ne!(
            get_bone_global_pose(&tree, skel, 1).origin,
            Vector3::new(0.0, 2.0, 0.0)
        );
        assert_eq!(
            get_bone_global_rest(&tree, skel, 1).origin,
            Vector3::new(0.0, 2.0, 0.0)
        );
        let rests = bone_rest_array(&tree, skel);
        assert_eq!(rests.len(), 2);
        assert_eq!(rests[0].origin, Vector3::ZERO);
        assert_eq!(rests[1].origin, Vector3::new(0.0, 2.0, 0.0));
    }

    #[test]
    fn process_skeletons_runs_modifiers_and_attachments() {
        let mut tree = make_tree();
        let skel = scene_style_skeleton(&mut tree);

        // A FABRIK3D aimed sideways, plus an attachment riding the tip bone.
        let ik = tree.add_child(skel, Node::new("IK", "FABRIK3D")).unwrap();
        {
            let n = tree.get_node_mut(ik).unwrap();
            n.set_property(IK_ROOT_BONE_KEY, Variant::Int(0));
            n.set_property(IK_TIP_BONE_KEY, Variant::Int(1));
            n.set_property(IK_ITERATIONS_KEY, Variant::Int(10));
            n.set_property(
                IK_TARGET_POSITION_KEY,
                Variant::Vector3(Vector3::new(2.0, 0.0, 0.0)),
            );
        }
        let attach = tree
            .add_child(skel, Node::new("Hat", "BoneAttachment3D"))
            .unwrap();
        set_bone_attachment_bone_name(&mut tree, attach, "Tip");

        let before = get_bone_global_pose(&tree, skel, 1).origin;
        process_skeletons(&mut tree);
        let after = get_bone_global_pose(&tree, skel, 1).origin;

        assert_ne!(
            before, after,
            "the modifier stack must run without a manual process_modifiers call"
        );
        // The attachment followed the bone to its solved pose in the same pass.
        match tree.get_node(attach).unwrap().get_property("transform") {
            Variant::Transform3D(t) => assert_eq!(t.origin, after),
            other => panic!("attachment transform not updated: {other:?}"),
        }
    }

    #[test]
    fn process_skeletons_skips_non_skeleton_nodes() {
        // A BoneAttachment3D parented to something that is not a Skeleton3D, and
        // a modifier under a plain Node3D, must both be left alone.
        let mut tree = make_tree();
        let root = tree.root_id();
        let plain = tree.add_child(root, Node::new("Plain", "Node3D")).unwrap();
        let attach = tree
            .add_child(plain, Node::new("Hat", "BoneAttachment3D"))
            .unwrap();
        set_bone_attachment_bone_name(&mut tree, attach, "Tip");

        process_skeletons(&mut tree);

        assert_eq!(
            tree.get_node(attach).unwrap().get_property("transform"),
            Variant::Nil,
            "a BoneAttachment3D outside a Skeleton3D must not be repositioned"
        );
    }

    #[test]
    fn animation_bone_tracks_write_bone_poses() {
        use crate::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};

        let mut tree = make_tree();
        let skel = scene_style_skeleton(&mut tree);
        let root = tree.root_id();
        let player_node = tree
            .add_child(root, Node::new("AnimationPlayer", "AnimationPlayer"))
            .unwrap();

        // Godot keys a skeletal clip as `Skeleton3D:BoneName` with a
        // position_3d / rotation_3d / scale_3d track.
        let mut anim = Animation::new("pose", 1.0);
        let mut pos = AnimationTrack::with_type("Skeleton3D", "Tip", TrackType::Position3D);
        pos.add_keyframe(KeyFrame::linear(
            0.0,
            Variant::Vector3(Vector3::new(3.0, 0.0, 0.0)),
        ));
        anim.tracks.push(pos);

        let mut player = AnimationPlayer::new();
        player.root_node = "..".into();
        player.add_animation(anim);
        player.play("pose");
        tree.attach_animation_player(player_node, player).unwrap();

        // Before: the bone sits at its rest pose.
        assert_eq!(get_bone_pose_position(&tree, skel, 1), Vector3::ZERO);

        apply_animation_bone_tracks(&mut tree);

        assert_eq!(
            get_bone_pose_position(&tree, skel, 1),
            Vector3::new(3.0, 0.0, 0.0),
            "a position_3d track targeting Skeleton3D:Tip must write the bone pose"
        );
        assert_eq!(
            get_bone_global_pose(&tree, skel, 1).origin,
            Vector3::new(3.0, 2.0, 0.0),
            "the animated pose must compose with the bone's rest"
        );
    }
}
