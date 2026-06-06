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

use gdcore::math3d::Transform3D;
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
pub fn get_bone_count(tree: &SceneTree, node_id: NodeId) -> usize {
    tree.get_node(node_id)
        .map(|n| match n.get_property(BONE_COUNT_KEY) {
            Variant::Int(c) => c.max(0) as usize,
            _ => 0,
        })
        .unwrap_or(0)
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
    tree.get_node(node_id).and_then(|n| {
        match n.get_property(&bone_name_key(idx)) {
            Variant::String(s) => Some(s),
            _ => None,
        }
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
pub fn set_bone_rest(
    tree: &mut SceneTree,
    node_id: NodeId,
    idx: usize,
    rest: Transform3D,
) {
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
pub fn set_bone_pose(
    tree: &mut SceneTree,
    node_id: NodeId,
    idx: usize,
    pose: Transform3D,
) {
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
    tree.get_node(node_id).and_then(|n| {
        match n.get_property("bone_name") {
            Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        }
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

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use gdcore::math::Vector3;
    use gdcore::math3d::Basis;

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
        set_bone_rest(&mut tree, skel_id, hip, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 1.0, 0.0),
        });

        // Spine at Y=0.5 relative to Hip
        let spine = add_bone(&mut tree, skel_id, "Spine");
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_rest(&mut tree, skel_id, spine, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.5, 0.0),
        });

        // Head at Y=0.3 relative to Spine
        let head = add_bone(&mut tree, skel_id, "Head");
        set_bone_parent(&mut tree, skel_id, head, spine as i64);
        set_bone_rest(&mut tree, skel_id, head, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.3, 0.0),
        });

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
        set_bone_rest(&mut tree, skel_id, hip, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 1.0, 0.0),
        });

        let spine = add_bone(&mut tree, skel_id, "Spine");
        set_bone_parent(&mut tree, skel_id, spine, hip as i64);
        set_bone_rest(&mut tree, skel_id, spine, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.5, 0.0),
        });

        // Apply a pose offset to spine: shift X by 1.0
        set_bone_pose(&mut tree, skel_id, spine, Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(1.0, 0.0, 0.0),
        });

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
}
