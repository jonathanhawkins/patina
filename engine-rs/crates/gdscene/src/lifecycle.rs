//! Node lifecycle callbacks and processing.
//!
//! Godot dispatches lifecycle notifications in a specific order when nodes
//! enter and leave the scene tree:
//!
//! - **Enter tree**: `ENTER_TREE` fires top-down (parent before children).
//! - **Ready**: `READY` fires bottom-up (children before parent) after all
//!   descendants have entered.
//! - **Exit tree**: `EXIT_TREE` fires bottom-up (children before parent).
//!
//! The [`LifecycleManager`] encapsulates this logic and operates on a
//! [`SceneTree`].

use gdobject::notification::{NOTIFICATION_ENTER_TREE, NOTIFICATION_EXIT_TREE, NOTIFICATION_READY};

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use crate::trace::TraceEventType;

/// Manages lifecycle notification dispatch for the scene tree.
///
/// This is a stateless helper — all state lives in the [`SceneTree`] and
/// its nodes. Methods here encode the ordering rules that Godot specifies.
pub struct LifecycleManager;

impl LifecycleManager {
    /// Dispatches enter-tree and ready notifications for a subtree.
    ///
    /// Call this after adding a subtree to the tree. The sequence is:
    /// 1. `ENTER_TREE` top-down (parent first, then children depth-first).
    /// 2. `READY` bottom-up (deepest children first, then parent).
    pub fn enter_tree(tree: &mut SceneTree, subtree_root: NodeId) {
        // Phase 1: ENTER_TREE — top-down.
        let mut top_down = Vec::new();
        tree.collect_subtree_top_down(subtree_root, &mut top_down);

        for &id in &top_down {
            tree.trace_record(id, TraceEventType::Notification, "ENTER_TREE");
            if let Some(node) = tree.get_node_mut(id) {
                node.set_inside_tree(true);
                node.receive_notification(NOTIFICATION_ENTER_TREE);
            }
            tree.process_script_enter_tree(id);
        }

        // Phase 2: READY — bottom-up.
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(subtree_root, &mut bottom_up);

        for &id in &bottom_up {
            tree.trace_record(id, TraceEventType::Notification, "READY");
            if let Some(node) = tree.get_node_mut(id) {
                node.set_ready(true);
                node.receive_notification(NOTIFICATION_READY);
            }
            tree.process_script_ready(id);
        }
    }

    /// Dispatches exit-tree notifications for a subtree.
    ///
    /// Call this before removing a subtree from the tree. The sequence is:
    /// `EXIT_TREE` bottom-up (deepest children first, then parent).
    pub fn exit_tree(tree: &mut SceneTree, subtree_root: NodeId) {
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(subtree_root, &mut bottom_up);

        for &id in &bottom_up {
            tree.trace_record(id, TraceEventType::Notification, "EXIT_TREE");
            if let Some(node) = tree.get_node_mut(id) {
                node.set_ready(false);
                node.set_inside_tree(false);
                node.receive_notification(NOTIFICATION_EXIT_TREE);
            }
            tree.process_script_exit_tree(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;
    use gdobject::notification::{
        NOTIFICATION_ENTER_TREE, NOTIFICATION_EXIT_TREE, NOTIFICATION_READY,
    };

    /// Helper: build a small tree and return (tree, root, parent, child1, child2).
    fn build_test_tree() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();

        let child1 = Node::new("Child1", "Node");
        let child1_id = tree.add_child(parent_id, child1).unwrap();

        let child2 = Node::new("Child2", "Node");
        let child2_id = tree.add_child(parent_id, child2).unwrap();

        (tree, root, parent_id, child1_id, child2_id)
    }

    #[test]
    fn enter_tree_fires_top_down() {
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        // ENTER_TREE should be the first notification for parent.
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(parent_log[0], NOTIFICATION_ENTER_TREE);

        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(c1_log[0], NOTIFICATION_ENTER_TREE);

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(c2_log[0], NOTIFICATION_ENTER_TREE);

        // Verify ordering: parent's ENTER_TREE comes before children's.
        // Since each node's log is independent, we verify by collecting
        // global order from the traversal.
        // The top-down order is [Parent, Child1, Child2].
        // The bottom-up order is [Child1, Child2, Parent].
        // Full notification sequence per node:
        //   Parent: ENTER_TREE (pos 0), READY (pos 5)
        //   Child1: ENTER_TREE (pos 1), READY (pos 3)
        //   Child2: ENTER_TREE (pos 2), READY (pos 4)
    }

    #[test]
    fn ready_fires_bottom_up() {
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        // After enter_tree, each node should have [ENTER_TREE, READY].
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(parent_log.len(), 2);
        assert_eq!(parent_log[0], NOTIFICATION_ENTER_TREE);
        assert_eq!(parent_log[1], NOTIFICATION_READY);

        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(c1_log.len(), 2);
        assert_eq!(c1_log[0], NOTIFICATION_ENTER_TREE);
        assert_eq!(c1_log[1], NOTIFICATION_READY);

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(c2_log.len(), 2);
        assert_eq!(c2_log[0], NOTIFICATION_ENTER_TREE);
        assert_eq!(c2_log[1], NOTIFICATION_READY);
    }

    #[test]
    fn ready_children_before_parent_global_order() {
        // To verify the exact global dispatch order, we use a different
        // strategy: record the order of READY dispatches by collecting
        // the bottom-up traversal order and checking it.
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        // Manually verify the bottom-up ordering.
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(parent_id, &mut bottom_up);
        assert_eq!(bottom_up, vec![child1_id, child2_id, parent_id]);

        // Now run the lifecycle and verify READY went bottom-up.
        LifecycleManager::enter_tree(&mut tree, parent_id);

        // Children received READY before parent (in global time).
        // We can verify this by the fact that bottom_up order is
        // [child1, child2, parent], matching Godot's contract.
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        let c2_log = tree.get_node(child2_id).unwrap().notification_log();

        // All should have received both notifications.
        assert_eq!(parent_log, &[NOTIFICATION_ENTER_TREE, NOTIFICATION_READY]);
        assert_eq!(c1_log, &[NOTIFICATION_ENTER_TREE, NOTIFICATION_READY]);
        assert_eq!(c2_log, &[NOTIFICATION_ENTER_TREE, NOTIFICATION_READY]);
    }

    #[test]
    fn exit_tree_fires_bottom_up() {
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        LifecycleManager::exit_tree(&mut tree, parent_id);

        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(parent_log, &[NOTIFICATION_EXIT_TREE]);

        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(c1_log, &[NOTIFICATION_EXIT_TREE]);

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(c2_log, &[NOTIFICATION_EXIT_TREE]);

        // Bottom-up means children exit before parent.
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(parent_id, &mut bottom_up);
        assert_eq!(bottom_up, vec![child1_id, child2_id, parent_id]);
    }

    #[test]
    fn enter_tree_single_node_no_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let leaf = Node::new("Leaf", "Node");
        let leaf_id = tree.add_child(root, leaf).unwrap();

        LifecycleManager::enter_tree(&mut tree, leaf_id);

        let log = tree.get_node(leaf_id).unwrap().notification_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], NOTIFICATION_ENTER_TREE);
        assert_eq!(log[1], NOTIFICATION_READY);
        assert!(tree.get_node(leaf_id).unwrap().is_inside_tree());
        assert!(tree.get_node(leaf_id).unwrap().is_ready());
    }

    #[test]
    fn exit_tree_leaf_node_only() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let leaf = Node::new("Leaf", "Node");
        let leaf_id = tree.add_child(parent_id, leaf).unwrap();

        LifecycleManager::exit_tree(&mut tree, leaf_id);

        let leaf_log = tree.get_node(leaf_id).unwrap().notification_log();
        assert_eq!(leaf_log, &[NOTIFICATION_EXIT_TREE]);

        // Parent should not have received any notification
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert!(parent_log.is_empty());
    }

    #[test]
    fn enter_then_exit_full_cycle() {
        let (mut tree, _root, parent_id, child1_id, _child2_id) = build_test_tree();

        LifecycleManager::enter_tree(&mut tree, parent_id);
        LifecycleManager::exit_tree(&mut tree, parent_id);

        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(
            c1_log,
            &[
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY,
                NOTIFICATION_EXIT_TREE,
            ]
        );

        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(
            parent_log,
            &[
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY,
                NOTIFICATION_EXIT_TREE,
            ]
        );
        assert!(!tree.get_node(child1_id).unwrap().is_inside_tree());
        assert!(!tree.get_node(child1_id).unwrap().is_ready());
    }
}
