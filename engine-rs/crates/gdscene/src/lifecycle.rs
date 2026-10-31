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
            // Deliver ENTER_TREE to the script's `_notification` virtual too,
            // alongside the named `_enter_tree` callback (pat-nzgdu.13).
            tree.notify_script(id, NOTIFICATION_ENTER_TREE.code() as i64);
            // Godot emits Node.tree_entered as each node enters the tree
            // (pat-nzgdu.84). Fires here for both add_child auto-enter and any
            // subtree re-entry; nodes with no connections are a no-op.
            tree.emit_signal(id, "tree_entered", &[]);
            // ...and the PARENT announces the arrival (pat-mgixk.64). Godot
            // pairs these: `_propagate_enter_tree` emits `tree_entered` on the
            // node and then `child_entered_tree(node)` on its parent
            // (`node.cpp:358-365`). Registered in our ClassDB but never emitted,
            // so `child_entered_tree.connect(..)` succeeded and never fired.
            //
            // ⚠️ Read the parent BEFORE emitting: `emit_signal` takes `&mut
            // SceneTree`, so the immutable borrow from `get_node` has to end
            // first.
            //
            // Emitted for every node in the subtree, each on its OWN parent —
            // including `subtree_root`, whose parent sits outside the subtree and
            // is the one actually gaining a child.
            let entering_parent = tree.get_node(id).and_then(|n| n.parent());
            if let Some(parent) = entering_parent {
                tree.emit_signal(
                    parent,
                    "child_entered_tree",
                    &[gdvariant::Variant::ObjectId(id.object_id())],
                );
            }
            // A `ShaderGlobalsOverride` pushes its `params/*` values into the
            // global shader parameter store as it enters (pat-s3rrv.209).
            apply_shader_globals_override(tree, id);
        }

        // Phase 2: READY — bottom-up.
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(subtree_root, &mut bottom_up);

        for &id in &bottom_up {
            // 🔑 READY RUNS ONCE PER NODE (pat-mgixk.406). Godot's
            // `Node::_propagate_ready` notifies READY only for a node that has
            // not been readied yet, re-armable through `Node.request_ready()`
            // (`scene/main/node.cpp`); entering the tree again fires
            // EXIT_TREE/ENTER_TREE but **not** READY.
            //
            // Without this guard every re-entry replayed `_ready`, and re-entry
            // is not a rare event:
            //
            // * `SceneTree::reparent` ends by calling this function, so any
            //   reparent re-ran `_ready` for the whole moved subtree.
            // * `SceneTreeAccessor::add_child` routes an ALREADY-PARENTED node
            //   through `reparent`, and `instantiate_packed_scene` parents its
            //   result under the root — so the ordinary GDScript idiom
            //   `var e = scene.instantiate(); add_child(e)` ran the child's
            //   `_ready` twice.
            // * The editor's drag-reparent runs user callbacks a second time too.
            //
            // ⚠️ Guarding HERE rather than special-casing `reparent` is what
            // makes the rule Godot's: it is a property of the node, not of the
            // call path, so every present and future re-entry point inherits it,
            // and `request_ready()` keeps working because it clears the same
            // flag this reads. The two rewind helpers below also clear it
            // deliberately, which is how a premature pass is allowed to re-run.
            if tree.get_node(id).is_some_and(|n| n.is_ready()) {
                continue;
            }
            tree.trace_record(id, TraceEventType::Notification, "READY");
            if let Some(node) = tree.get_node_mut(id) {
                node.set_ready(true);
                node.receive_notification(NOTIFICATION_READY);
            }
            tree.process_script_ready(id);
            tree.notify_script(id, NOTIFICATION_READY.code() as i64);
            // Godot's `_propagate_ready` emits `ready` AFTER the node's own
            // `_ready` has run (`node.cpp:331`), which is what makes
            // `await child.ready` resume at the right moment (pat-mgixk.64).
            // Ordering matters and follows the calls above, not precedes them: a
            // listener woken by `ready` must be able to see whatever `_ready`
            // set up.
            //
            // Bottom-up like the rest of this phase, so a parent's `ready`
            // fires after every descendant's.
            tree.emit_signal(id, "ready", &[]);
        }
    }

    /// Rewinds a premature enter/ready pass so [`enter_tree`](Self::enter_tree)
    /// can deliver the real one exactly once (pat-0q2kx.46).
    ///
    /// [`SceneTree::add_child`] enters a child the moment it is inserted. That is
    /// right for a live tree, but wrong when a subtree is *assembled* under an
    /// already-live root: each node is entered before it has children — so
    /// `READY` fires parent-first instead of child-first — and before its script
    /// is attached, so the script sees neither callback. Callers that assemble
    /// first and dispatch afterwards (the editor's Play flow, autoload
    /// instantiation) call this to drop that pass: `inside_tree` and `ready` are
    /// cleared and the `ENTER_TREE`/`READY` entries are removed from every node's
    /// notification log, leaving exactly one of each once the real dispatch runs.
    ///
    /// Signals are deliberately not rewound. Nothing can be connected to a node
    /// that has not yet run a script callback, so the premature `tree_entered`
    /// emission had no listeners to reach.
    ///
    /// ⚠️ That reasoning was re-checked when `ready` and `child_entered_tree`
    /// were added to the same phases (pat-mgixk.64). It holds for `ready` and
    /// `tree_entered`, which are emitted ON THE ENTERING NODE — that node has no
    /// script and therefore no connections during the premature pass.
    ///
    /// ⚠️ It does NOT hold as cleanly for `child_entered_tree`, and this is the
    /// one place it is written down: that signal is emitted on the **parent**,
    /// which in the assemble-then-dispatch flows (the editor's Play path,
    /// autoload instantiation) is an already-live node that may well have a
    /// script and listeners. Such a parent therefore sees
    /// `child_entered_tree` TWICE for the same child: once from the premature
    /// pass `add_child` triggers, once from the real dispatch after this rewind.
    /// Emissions cannot be un-emitted, so the rewind cannot repair it.
    ///
    /// Left as-is deliberately rather than papered over: the pre-existing
    /// `tree_entered` emission has had the same duplicate-on-rewind shape since
    /// pat-nzgdu.84, suppressing it needs a "premature pass" flag threaded
    /// through `add_child` -> `enter_tree`, and guessing at that without being
    /// able to run the editor Play flow would trade a known, documented
    /// duplicate for an unknown regression. Worth its own bead.
    pub fn rewind_for_redispatch(tree: &mut SceneTree, subtree_root: NodeId) {
        let mut ids = Vec::new();
        tree.collect_subtree_top_down(subtree_root, &mut ids);

        for id in ids {
            if let Some(node) = tree.get_node_mut(id) {
                let kept: Vec<_> = node
                    .notification_log()
                    .iter()
                    .copied()
                    .filter(|n| *n != NOTIFICATION_ENTER_TREE && *n != NOTIFICATION_READY)
                    .collect();
                node.clear_notification_log();
                for n in kept {
                    node.receive_notification(n);
                }
                node.set_inside_tree(false);
                node.set_ready(false);
            }
        }
    }

    /// Dispatches exit-tree notifications for a subtree.
    ///
    /// Call this before removing a subtree from the tree. The sequence is:
    /// `EXIT_TREE` bottom-up (deepest children first, then parent).
    pub fn exit_tree(tree: &mut SceneTree, subtree_root: NodeId) {
        // 🔑 Godot emits `tree_exiting` TOP-DOWN and BEFORE any
        // `NOTIFICATION_EXIT_TREE` is delivered (`node.cpp:423`), pairing it with
        // `child_exiting_tree(node)` on the parent (`:433`). That ordering is the
        // point of the signal: a listener gets to inspect the subtree while it is
        // still intact and still inside the tree. Emitting it alongside the
        // bottom-up EXIT_TREE pass below would hand listeners a subtree whose
        // deeper nodes had already torn themselves down.
        //
        // Hence a separate pre-pass rather than two more lines in the loop.
        let mut top_down = Vec::new();
        tree.collect_subtree_top_down(subtree_root, &mut top_down);
        for &id in &top_down {
            tree.emit_signal(id, "tree_exiting", &[]);
            // Parent read before the emit, as in `enter_tree`: `emit_signal`
            // needs `&mut SceneTree`.
            let exiting_parent = tree.get_node(id).and_then(|n| n.parent());
            if let Some(parent) = exiting_parent {
                tree.emit_signal(
                    parent,
                    "child_exiting_tree",
                    &[gdvariant::Variant::ObjectId(id.object_id())],
                );
            }
        }

        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(subtree_root, &mut bottom_up);

        for &id in &bottom_up {
            tree.trace_record(id, TraceEventType::Notification, "EXIT_TREE");
            if let Some(node) = tree.get_node_mut(id) {
                // 🔑 The ready flag SURVIVES leaving the tree (pat-mgixk.406).
                // Godot's `_propagate_exit_tree` clears `inside_tree` but NOT
                // `data.ready_notified` — only `Node.request_ready()` does
                // (`scene/main/node.cpp`). That is precisely what makes "_ready
                // runs once per node" hold across a re-entry.
                //
                // ⚠️ Clearing it here made the ready-once guard in `enter_tree`
                // inert for the case it exists for: `SceneTree::reparent` runs
                // `exit_tree` and then `enter_tree`, so the flag was false again
                // by the time the guard read it and `_ready` replayed anyway.
                // The two changes are one fix and neither works alone.
                //
                // `rewind_for_redispatch` still clears the flag explicitly —
                // that is a deliberate rewind of a premature pass, not an exit.
                node.set_inside_tree(false);
                node.receive_notification(NOTIFICATION_EXIT_TREE);
            }
            tree.process_script_exit_tree(id);
            tree.notify_script(id, NOTIFICATION_EXIT_TREE.code() as i64);
            // …and drops them again on the way out, restoring whatever the
            // project file or a script last set (pat-s3rrv.209).
            clear_shader_globals_override(tree, id);
        }
    }
}

/// Whether `id` is a `ShaderGlobalsOverride` node.
fn is_shader_globals_override(tree: &SceneTree, id: NodeId) -> bool {
    tree.get_node(id)
        .map(|n| n.is_class("ShaderGlobalsOverride"))
        .unwrap_or(false)
}

/// The node's `params/<name>` properties, stripped to `<name>`.
///
/// Godot's `ShaderGlobalsOverride` exposes one `params/<name>` property per
/// declared project global; only the ones actually set are pushed.
fn shader_globals_override_params(
    tree: &SceneTree,
    id: NodeId,
) -> Vec<(String, gdvariant::Variant)> {
    match tree.get_node(id) {
        Some(node) => node
            .properties()
            .filter_map(|(key, value)| {
                key.strip_prefix("params/")
                    .map(|name| (name.to_string(), value.clone()))
            })
            .collect(),
        None => Vec::new(),
    }
}

/// The `ShaderGlobalsOverride` currently in the tree, ignoring `exclude`.
///
/// ⚠️ Godot's rule is that exactly **one** override is active: the first in the
/// tree wins and any other warns rather than fighting it. Resolved by walking
/// the tree rather than caching a "current override" somewhere, so there is no
/// stale global to leak between scenes — or, in this codebase, between tests
/// sharing one process.
fn active_shader_globals_override(tree: &SceneTree, exclude: Option<NodeId>) -> Option<NodeId> {
    let mut all = Vec::new();
    tree.collect_subtree_top_down(tree.root_id(), &mut all);
    all.into_iter().find(|&id| {
        Some(id) != exclude
            && tree
                .get_node(id)
                .map(|n| n.is_class("ShaderGlobalsOverride") && n.is_inside_tree())
                .unwrap_or(false)
    })
}

/// Pushes a `ShaderGlobalsOverride`'s params into the global store.
///
/// A no-op for every other class, so this costs one `is_class` per node
/// entering the tree.
fn apply_shader_globals_override(tree: &mut SceneTree, id: NodeId) {
    if !is_shader_globals_override(tree, id) {
        return;
    }
    // ⚠️ First in the tree wins. ENTER_TREE runs top-down, so an override
    // earlier in the scene is already `inside_tree` here and this one must
    // stand down — it reports a configuration warning instead
    // (`SceneTree::node_configuration_warnings`).
    if active_shader_globals_override(tree, Some(id)).is_some() {
        return;
    }
    for (name, value) in shader_globals_override_params(tree, id) {
        gdvariant::shader_globals::set_override(&name, Some(value));
    }
}

/// Drops a `ShaderGlobalsOverride`'s params, then hands control to whatever
/// override is still in the tree.
fn clear_shader_globals_override(tree: &mut SceneTree, id: NodeId) {
    if !is_shader_globals_override(tree, id) {
        return;
    }
    for (name, _) in shader_globals_override_params(tree, id) {
        gdvariant::shader_globals::set_override(&name, None);
    }
    // ⚠️ Re-apply whoever remains. Two things need this: the active override
    // leaving should promote the next one, and an INACTIVE override leaving
    // would otherwise clear a param the active one owns, because both nodes
    // name the same global. Re-applying makes that self-correcting rather than
    // order-dependent.
    if let Some(next) = active_shader_globals_override(tree, Some(id)) {
        for (name, value) in shader_globals_override_params(tree, next) {
            gdvariant::shader_globals::set_override(&name, Some(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;
    use gdobject::notification::{
        NOTIFICATION_CHILD_ORDER_CHANGED, NOTIFICATION_ENTER_TREE, NOTIFICATION_EXIT_TREE,
        NOTIFICATION_PARENTED, NOTIFICATION_POSTINITIALIZE, NOTIFICATION_READY,
        NOTIFICATION_UNPARENTED,
    };

    /// Helper: build a small tree with root NOT inside_tree, so lifecycle
    /// tests can manually control when enter_tree / exit_tree fires.
    fn build_test_tree() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        // Disable auto-lifecycle so tests control it manually.
        tree.get_node_mut(root).unwrap().set_inside_tree(false);

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

        // Parent: POSTINITIALIZE + PARENTED (add to root) + 2x CHILD_ORDER_CHANGED + ENTER_TREE + READY
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(parent_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(parent_log[1], NOTIFICATION_PARENTED);
        assert_eq!(parent_log[2], NOTIFICATION_CHILD_ORDER_CHANGED);
        assert_eq!(parent_log[3], NOTIFICATION_CHILD_ORDER_CHANGED);
        assert_eq!(parent_log[4], NOTIFICATION_ENTER_TREE);

        // Children: POSTINITIALIZE + PARENTED + ENTER_TREE
        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(c1_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(c1_log[1], NOTIFICATION_PARENTED);
        assert_eq!(c1_log[2], NOTIFICATION_ENTER_TREE);

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(c2_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(c2_log[1], NOTIFICATION_PARENTED);
        assert_eq!(c2_log[2], NOTIFICATION_ENTER_TREE);
    }

    #[test]
    fn ready_fires_bottom_up() {
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        LifecycleManager::enter_tree(&mut tree, parent_id);

        // Parent: POSTINITIALIZE + PARENTED + 2x CHILD_ORDER_CHANGED + ENTER_TREE + READY = 6
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(parent_log.len(), 6);
        assert_eq!(parent_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(parent_log[1], NOTIFICATION_PARENTED);
        assert_eq!(parent_log[2], NOTIFICATION_CHILD_ORDER_CHANGED);
        assert_eq!(parent_log[3], NOTIFICATION_CHILD_ORDER_CHANGED);
        assert_eq!(parent_log[4], NOTIFICATION_ENTER_TREE);
        assert_eq!(parent_log[5], NOTIFICATION_READY);

        // Children: POSTINITIALIZE + PARENTED + ENTER_TREE + READY = 4
        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(c1_log.len(), 4);
        assert_eq!(c1_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(c1_log[1], NOTIFICATION_PARENTED);
        assert_eq!(c1_log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(c1_log[3], NOTIFICATION_READY);

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(c2_log.len(), 4);
        assert_eq!(c2_log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(c2_log[1], NOTIFICATION_PARENTED);
        assert_eq!(c2_log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(c2_log[3], NOTIFICATION_READY);
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

        // Parent: POSTINITIALIZE + PARENTED + 2x CHILD_ORDER_CHANGED + ENTER_TREE + READY
        assert_eq!(
            parent_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY
            ]
        );
        // Children: POSTINITIALIZE + PARENTED + ENTER_TREE + READY
        assert_eq!(
            c1_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY
            ]
        );
        assert_eq!(
            c2_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY
            ]
        );
    }

    #[test]
    fn exit_tree_fires_bottom_up() {
        let (mut tree, _root, parent_id, child1_id, child2_id) = build_test_tree();

        LifecycleManager::exit_tree(&mut tree, parent_id);

        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(
            parent_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_EXIT_TREE
            ]
        );

        let c1_log = tree.get_node(child1_id).unwrap().notification_log();
        assert_eq!(
            c1_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_EXIT_TREE
            ]
        );

        let c2_log = tree.get_node(child2_id).unwrap().notification_log();
        assert_eq!(
            c2_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_EXIT_TREE
            ]
        );

        // Bottom-up means children exit before parent.
        let mut bottom_up = Vec::new();
        tree.collect_subtree_bottom_up(parent_id, &mut bottom_up);
        assert_eq!(bottom_up, vec![child1_id, child2_id, parent_id]);
    }

    #[test]
    fn enter_tree_single_node_no_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(false);
        let leaf = Node::new("Leaf", "Node");
        let leaf_id = tree.add_child(root, leaf).unwrap();

        LifecycleManager::enter_tree(&mut tree, leaf_id);

        let log = tree.get_node(leaf_id).unwrap().notification_log();
        assert_eq!(log.len(), 4);
        assert_eq!(log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(log[1], NOTIFICATION_PARENTED);
        assert_eq!(log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(log[3], NOTIFICATION_READY);
        assert!(tree.get_node(leaf_id).unwrap().is_inside_tree());
        assert!(tree.get_node(leaf_id).unwrap().is_ready());
    }

    #[test]
    fn exit_tree_leaf_node_only() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(false);
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let leaf = Node::new("Leaf", "Node");
        let leaf_id = tree.add_child(parent_id, leaf).unwrap();

        LifecycleManager::exit_tree(&mut tree, leaf_id);

        let leaf_log = tree.get_node(leaf_id).unwrap().notification_log();
        assert_eq!(
            leaf_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_EXIT_TREE
            ]
        );

        // Parent should have POSTINITIALIZE + PARENTED + CHILD_ORDER_CHANGED
        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(
            parent_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_CHILD_ORDER_CHANGED
            ]
        );
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
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY,
                NOTIFICATION_EXIT_TREE,
            ]
        );

        let parent_log = tree.get_node(parent_id).unwrap().notification_log();
        assert_eq!(
            parent_log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_CHILD_ORDER_CHANGED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_READY,
                NOTIFICATION_EXIT_TREE,
            ]
        );
        assert!(!tree.get_node(child1_id).unwrap().is_inside_tree());
        // ⚠️ REVISED for pat-mgixk.406: this asserted `!is_ready()`, which
        // encoded Patina's old behaviour of clearing the flag on EXIT_TREE.
        // Godot keeps `ready_notified` set after a node leaves the tree —
        // `Node.is_node_ready()` keeps reporting true, and only
        // `request_ready()` re-arms it. Keeping the flag is what stops a
        // reparent replaying `_ready`.
        assert!(
            tree.get_node(child1_id).unwrap().is_ready(),
            "the ready flag must survive EXIT_TREE: _ready has run for this \
             node and only request_ready() may undo that"
        );
    }
}
