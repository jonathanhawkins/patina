//! Undo/redo system with full parity for all editor operations.
//!
//! Provides a reusable [`UndoRedoManager`] that wraps the engine's
//! [`EditorCommand`] system into a proper undo/redo controller with:
//!
//! - **Bounded history**: configurable max undo depth with oldest-first eviction.
//! - **Action grouping**: merge multiple commands into a single undo step.
//! - **Dirty tracking**: know whether the document has unsaved changes.
//! - **Action labels**: human-readable descriptions for undo/redo menu items.
//! - **History inspection**: browse the full undo/redo stack for a history panel.

use std::fmt;

// ---------------------------------------------------------------------------
// UndoAction
// ---------------------------------------------------------------------------

/// A single undoable action, which may wrap one or more low-level operations.
#[derive(Debug, Clone)]
pub struct UndoAction {
    /// Human-readable label (e.g., "Rename Node", "Set Property").
    pub label: String,
    /// The operations that make up this action (in execution order).
    /// For undo, these are reversed.
    ops: Vec<UndoOp>,
    /// Monotonically increasing action ID.
    id: u64,
    /// Whether this action can be merged with the next action of the same type.
    pub mergeable: bool,
    /// Optional merge key — actions with the same merge_key can be coalesced.
    pub merge_key: Option<String>,
}

impl UndoAction {
    /// Creates a new single-operation action.
    pub fn new(label: impl Into<String>, op: UndoOp) -> Self {
        Self {
            label: label.into(),
            ops: vec![op],
            id: 0,
            mergeable: false,
            merge_key: None,
        }
    }

    /// Creates a grouped action from multiple operations.
    pub fn group(label: impl Into<String>, ops: Vec<UndoOp>) -> Self {
        Self {
            label: label.into(),
            ops,
            id: 0,
            mergeable: false,
            merge_key: None,
        }
    }

    /// Marks this action as mergeable with the given key.
    pub fn with_merge_key(mut self, key: impl Into<String>) -> Self {
        self.mergeable = true;
        self.merge_key = Some(key.into());
        self
    }

    /// Returns the action ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the operations in this action.
    pub fn ops(&self) -> &[UndoOp] {
        &self.ops
    }

    /// Returns the number of operations.
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// Appends an operation (used for merging).
    fn push_op(&mut self, op: UndoOp) {
        self.ops.push(op);
    }
}

impl fmt::Display for UndoAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

// ---------------------------------------------------------------------------
// UndoOp
// ---------------------------------------------------------------------------

/// A single low-level operation that can be undone/redone.
///
/// This is intentionally a simple data model — the actual execution is
/// delegated to the caller (e.g., the editor server applies these to the
/// scene tree). The undo_redo module only tracks the operations.
#[derive(Debug, Clone)]
pub enum UndoOp {
    /// Set a property on a node.
    SetProperty {
        node_id: u64,
        property: String,
        new_value: String,
        old_value: String,
    },
    /// Add a node.
    AddNode {
        parent_id: u64,
        node_name: String,
        class_name: String,
        created_id: Option<u64>,
    },
    /// Remove a node.
    RemoveNode {
        node_id: u64,
        parent_id: Option<u64>,
        node_name: String,
        class_name: String,
    },
    /// Reparent a node.
    ReparentNode {
        node_id: u64,
        new_parent_id: u64,
        old_parent_id: Option<u64>,
    },
    /// Rename a node.
    RenameNode {
        node_id: u64,
        new_name: String,
        old_name: String,
    },
    /// Move a node to a new child index.
    MoveNode {
        parent_id: u64,
        child_id: u64,
        new_index: usize,
        old_index: usize,
    },
    /// Connect a signal.
    ConnectSignal {
        source_id: u64,
        signal_name: String,
        target_id: u64,
        method: String,
    },
    /// Disconnect a signal.
    DisconnectSignal {
        source_id: u64,
        signal_name: String,
        target_id: u64,
        method: String,
    },
    /// Add a node to a group.
    AddToGroup { node_id: u64, group: String },
    /// Remove a node from a group.
    RemoveFromGroup { node_id: u64, group: String },
    /// A custom/opaque operation described by a string (for extensibility).
    Custom {
        description: String,
        forward_data: String,
        reverse_data: String,
    },
}

impl UndoOp {
    /// Returns the reverse of this operation (for undo).
    pub fn reversed(&self) -> Self {
        match self {
            Self::SetProperty {
                node_id,
                property,
                new_value,
                old_value,
            } => Self::SetProperty {
                node_id: *node_id,
                property: property.clone(),
                new_value: old_value.clone(),
                old_value: new_value.clone(),
            },
            Self::AddNode {
                parent_id,
                node_name,
                class_name,
                created_id,
            } => Self::RemoveNode {
                node_id: created_id.unwrap_or(0),
                parent_id: Some(*parent_id),
                node_name: node_name.clone(),
                class_name: class_name.clone(),
            },
            Self::RemoveNode {
                node_id: _,
                parent_id,
                node_name,
                class_name,
            } => Self::AddNode {
                parent_id: parent_id.unwrap_or(0),
                node_name: node_name.clone(),
                class_name: class_name.clone(),
                created_id: None,
            },
            Self::ReparentNode {
                node_id,
                new_parent_id,
                old_parent_id,
            } => Self::ReparentNode {
                node_id: *node_id,
                new_parent_id: old_parent_id.unwrap_or(0),
                old_parent_id: Some(*new_parent_id),
            },
            Self::RenameNode {
                node_id,
                new_name,
                old_name,
            } => Self::RenameNode {
                node_id: *node_id,
                new_name: old_name.clone(),
                old_name: new_name.clone(),
            },
            Self::MoveNode {
                parent_id,
                child_id,
                new_index,
                old_index,
            } => Self::MoveNode {
                parent_id: *parent_id,
                child_id: *child_id,
                new_index: *old_index,
                old_index: *new_index,
            },
            Self::ConnectSignal {
                source_id,
                signal_name,
                target_id,
                method,
            } => Self::DisconnectSignal {
                source_id: *source_id,
                signal_name: signal_name.clone(),
                target_id: *target_id,
                method: method.clone(),
            },
            Self::DisconnectSignal {
                source_id,
                signal_name,
                target_id,
                method,
            } => Self::ConnectSignal {
                source_id: *source_id,
                signal_name: signal_name.clone(),
                target_id: *target_id,
                method: method.clone(),
            },
            Self::AddToGroup { node_id, group } => Self::RemoveFromGroup {
                node_id: *node_id,
                group: group.clone(),
            },
            Self::RemoveFromGroup { node_id, group } => Self::AddToGroup {
                node_id: *node_id,
                group: group.clone(),
            },
            Self::Custom {
                description,
                forward_data,
                reverse_data,
            } => Self::Custom {
                description: format!("Undo: {}", description),
                forward_data: reverse_data.clone(),
                reverse_data: forward_data.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// UndoRedoManager
// ---------------------------------------------------------------------------

/// The undo/redo manager — tracks action history with bounded depth.
#[derive(Debug)]
pub struct UndoRedoManager {
    /// Undo stack (most recent on top).
    undo_stack: Vec<UndoAction>,
    /// Redo stack (most recent on top).
    redo_stack: Vec<UndoAction>,
    /// Maximum undo depth.
    max_depth: usize,
    /// Next action ID.
    next_id: u64,
    /// The action ID at the last save point (for dirty tracking).
    save_point_id: Option<u64>,
    /// Whether a group is currently being recorded.
    group_depth: u32,
    /// Pending operations for the current group.
    group_ops: Vec<UndoOp>,
    /// Label for the current group.
    group_label: String,
}

impl Default for UndoRedoManager {
    fn default() -> Self {
        Self::new(256)
    }
}

impl UndoRedoManager {
    /// Creates a new manager with the given max undo depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth,
            next_id: 1,
            save_point_id: None, // Clean state at start — no actions yet.
            group_depth: 0,
            group_ops: Vec::new(),
            group_label: String::new(),
        }
    }

    /// Pushes a new action onto the undo stack.
    ///
    /// Clears the redo stack (new action invalidates redo history).
    /// If a group is active, the ops are collected instead.
    pub fn push(&mut self, mut action: UndoAction) {
        if self.group_depth > 0 {
            self.group_ops.extend(action.ops.drain(..));
            if self.group_label.is_empty() {
                self.group_label = action.label;
            }
            return;
        }

        // Try to merge with the top of the undo stack.
        if action.mergeable {
            if let Some(top) = self.undo_stack.last_mut() {
                if top.mergeable && top.merge_key == action.merge_key {
                    for op in action.ops {
                        top.push_op(op);
                    }
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        action.id = self.next_id;
        self.next_id += 1;

        self.undo_stack.push(action);
        self.redo_stack.clear();

        // Enforce max depth.
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    /// Pops the most recent action from the undo stack and moves it to redo.
    ///
    /// Returns the action that should be undone, or `None` if nothing to undo.
    pub fn undo(&mut self) -> Option<&UndoAction> {
        let action = self.undo_stack.pop()?;
        self.redo_stack.push(action);
        self.redo_stack.last()
    }

    /// Pops the most recent action from the redo stack and moves it to undo.
    ///
    /// Returns the action that should be redone, or `None` if nothing to redo.
    pub fn redo(&mut self) -> Option<&UndoAction> {
        let action = self.redo_stack.pop()?;
        self.undo_stack.push(action);
        self.undo_stack.last()
    }

    /// Returns true if there are actions to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns true if there are actions to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Returns the label of the next action to undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|a| a.label.as_str())
    }

    /// Returns the label of the next action to redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|a| a.label.as_str())
    }

    /// Returns the number of actions on the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Returns the number of actions on the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Returns the maximum undo depth.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Clears all undo and redo history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.save_point_id = None;
    }

    /// Marks the current state as the save point (clean).
    pub fn mark_saved(&mut self) {
        self.save_point_id = self.undo_stack.last().map(|a| a.id);
    }

    /// Returns whether the document is dirty (has unsaved changes).
    pub fn is_dirty(&self) -> bool {
        let current_id = self.undo_stack.last().map(|a| a.id);
        current_id != self.save_point_id
    }

    /// Begins a group — all subsequent `push()` calls are collected
    /// into a single compound action until `end_group()` is called.
    pub fn begin_group(&mut self, label: impl Into<String>) {
        if self.group_depth == 0 {
            self.group_label = label.into();
            self.group_ops.clear();
        }
        self.group_depth += 1;
    }

    /// Ends the current group and pushes the compound action.
    pub fn end_group(&mut self) {
        if self.group_depth == 0 {
            return;
        }
        self.group_depth -= 1;
        if self.group_depth == 0 && !self.group_ops.is_empty() {
            let label = std::mem::take(&mut self.group_label);
            let ops = std::mem::take(&mut self.group_ops);
            let action = UndoAction::group(label, ops);
            self.push(action);
        }
    }

    /// Returns whether a group is currently being recorded.
    pub fn is_grouping(&self) -> bool {
        self.group_depth > 0
    }

    /// Returns the full undo history (oldest first) for a history panel.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let mut entries: Vec<HistoryEntry> = self
            .undo_stack
            .iter()
            .enumerate()
            .map(|(i, a)| HistoryEntry {
                index: i,
                label: a.label.clone(),
                id: a.id,
                is_current: false,
                op_count: a.ops.len(),
            })
            .collect();

        // Mark the top of the undo stack as current.
        if let Some(last) = entries.last_mut() {
            last.is_current = true;
        }

        // Add redo entries (grayed out / future).
        for (i, a) in self.redo_stack.iter().rev().enumerate() {
            entries.push(HistoryEntry {
                index: entries.len(),
                label: a.label.clone(),
                id: a.id,
                is_current: false,
                op_count: a.ops.len(),
            });
            let _ = i; // suppress unused warning
        }

        entries
    }

    /// Undoes multiple actions at once, returning the count undone.
    pub fn undo_n(&mut self, n: usize) -> usize {
        let mut count = 0;
        for _ in 0..n {
            if self.undo().is_some() {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Redoes multiple actions at once, returning the count redone.
    pub fn redo_n(&mut self, n: usize) -> usize {
        let mut count = 0;
        for _ in 0..n {
            if self.redo().is_some() {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Returns a reference to the undo stack.
    pub fn undo_stack(&self) -> &[UndoAction] {
        &self.undo_stack
    }

    /// Returns a reference to the redo stack.
    pub fn redo_stack(&self) -> &[UndoAction] {
        &self.redo_stack
    }
}

// ---------------------------------------------------------------------------
// HistoryEntry
// ---------------------------------------------------------------------------

/// An entry in the undo/redo history panel.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Position in the history list.
    pub index: usize,
    /// Human-readable label.
    pub label: String,
    /// Action ID.
    pub id: u64,
    /// Whether this is the current state.
    pub is_current: bool,
    /// Number of operations in this action.
    pub op_count: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_set_prop(node: u64, prop: &str, new_val: &str, old_val: &str) -> UndoOp {
        UndoOp::SetProperty {
            node_id: node,
            property: prop.to_string(),
            new_value: new_val.to_string(),
            old_value: old_val.to_string(),
        }
    }

    fn make_action(label: &str, node: u64) -> UndoAction {
        UndoAction::new(label, make_set_prop(node, "position", "10,20", "0,0"))
    }

    // ── UndoOp reversed ─────────────────────────────────────────────

    #[test]
    fn set_property_reversal() {
        let op = make_set_prop(1, "x", "new", "old");
        let rev = op.reversed();
        match rev {
            UndoOp::SetProperty {
                new_value,
                old_value,
                ..
            } => {
                assert_eq!(new_value, "old");
                assert_eq!(old_value, "new");
            }
            _ => panic!("expected SetProperty"),
        }
    }

    #[test]
    fn add_node_reversal_becomes_remove() {
        let op = UndoOp::AddNode {
            parent_id: 1,
            node_name: "Sprite".to_string(),
            class_name: "Sprite2D".to_string(),
            created_id: Some(42),
        };
        let rev = op.reversed();
        match rev {
            UndoOp::RemoveNode { node_id, .. } => assert_eq!(node_id, 42),
            _ => panic!("expected RemoveNode"),
        }
    }

    #[test]
    fn remove_node_reversal_becomes_add() {
        let op = UndoOp::RemoveNode {
            node_id: 5,
            parent_id: Some(1),
            node_name: "Node".to_string(),
            class_name: "Node2D".to_string(),
        };
        let rev = op.reversed();
        match rev {
            UndoOp::AddNode { parent_id, .. } => assert_eq!(parent_id, 1),
            _ => panic!("expected AddNode"),
        }
    }

    #[test]
    fn rename_reversal() {
        let op = UndoOp::RenameNode {
            node_id: 1,
            new_name: "NewName".to_string(),
            old_name: "OldName".to_string(),
        };
        let rev = op.reversed();
        match rev {
            UndoOp::RenameNode {
                new_name, old_name, ..
            } => {
                assert_eq!(new_name, "OldName");
                assert_eq!(old_name, "NewName");
            }
            _ => panic!("expected RenameNode"),
        }
    }

    #[test]
    fn connect_reversal_becomes_disconnect() {
        let op = UndoOp::ConnectSignal {
            source_id: 1,
            signal_name: "pressed".to_string(),
            target_id: 2,
            method: "_on_pressed".to_string(),
        };
        let rev = op.reversed();
        assert!(matches!(rev, UndoOp::DisconnectSignal { .. }));
    }

    #[test]
    fn group_reversal() {
        let op = UndoOp::AddToGroup {
            node_id: 1,
            group: "enemies".to_string(),
        };
        let rev = op.reversed();
        assert!(matches!(rev, UndoOp::RemoveFromGroup { .. }));
    }

    #[test]
    fn custom_op_reversal() {
        let op = UndoOp::Custom {
            description: "paint".to_string(),
            forward_data: "fwd".to_string(),
            reverse_data: "rev".to_string(),
        };
        let rev = op.reversed();
        match rev {
            UndoOp::Custom {
                forward_data,
                reverse_data,
                ..
            } => {
                assert_eq!(forward_data, "rev");
                assert_eq!(reverse_data, "fwd");
            }
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn move_node_reversal() {
        let op = UndoOp::MoveNode {
            parent_id: 1,
            child_id: 2,
            new_index: 5,
            old_index: 3,
        };
        let rev = op.reversed();
        match rev {
            UndoOp::MoveNode {
                new_index,
                old_index,
                ..
            } => {
                assert_eq!(new_index, 3);
                assert_eq!(old_index, 5);
            }
            _ => panic!("expected MoveNode"),
        }
    }

    // ── UndoAction ──────────────────────────────────────────────────

    #[test]
    fn action_single_op() {
        let a = make_action("Set Position", 1);
        assert_eq!(a.label, "Set Position");
        assert_eq!(a.op_count(), 1);
        assert!(!a.mergeable);
    }

    #[test]
    fn action_group() {
        let ops = vec![
            make_set_prop(1, "x", "1", "0"),
            make_set_prop(1, "y", "2", "0"),
        ];
        let a = UndoAction::group("Move Node", ops);
        assert_eq!(a.op_count(), 2);
    }

    #[test]
    fn action_with_merge_key() {
        let a = make_action("Set X", 1).with_merge_key("node_1_x");
        assert!(a.mergeable);
        assert_eq!(a.merge_key, Some("node_1_x".to_string()));
    }

    // ── UndoRedoManager basics ──────────────────────────────────────

    #[test]
    fn manager_default() {
        let mgr = UndoRedoManager::default();
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
        assert_eq!(mgr.max_depth(), 256);
        assert!(!mgr.is_dirty());
    }

    #[test]
    fn push_and_undo() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));

        assert!(mgr.can_undo());
        assert_eq!(mgr.undo_count(), 2);
        assert_eq!(mgr.undo_label(), Some("B"));

        let undone = mgr.undo().unwrap();
        assert_eq!(undone.label, "B");
        assert_eq!(mgr.undo_count(), 1);
        assert!(mgr.can_redo());
        assert_eq!(mgr.redo_label(), Some("B"));
    }

    #[test]
    fn undo_and_redo() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));

        mgr.undo();
        assert!(!mgr.can_undo());
        assert!(mgr.can_redo());

        let redone = mgr.redo().unwrap();
        assert_eq!(redone.label, "A");
        assert!(mgr.can_undo());
        assert!(!mgr.can_redo());
    }

    #[test]
    fn push_clears_redo() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));

        mgr.undo(); // Undo B
        assert!(mgr.can_redo());

        mgr.push(make_action("C", 3)); // New action clears redo
        assert!(!mgr.can_redo());
        assert_eq!(mgr.undo_count(), 2); // A, C
    }

    #[test]
    fn undo_empty_returns_none() {
        let mut mgr = UndoRedoManager::new(10);
        assert!(mgr.undo().is_none());
    }

    #[test]
    fn redo_empty_returns_none() {
        let mut mgr = UndoRedoManager::new(10);
        assert!(mgr.redo().is_none());
    }

    // ── Max depth ───────────────────────────────────────────────────

    #[test]
    fn max_depth_evicts_oldest() {
        let mut mgr = UndoRedoManager::new(3);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.push(make_action("C", 3));
        mgr.push(make_action("D", 4));

        assert_eq!(mgr.undo_count(), 3);
        // A was evicted; oldest remaining is B.
        let history = mgr.history();
        assert_eq!(history[0].label, "B");
    }

    // ── Dirty tracking ──────────────────────────────────────────────

    #[test]
    fn dirty_after_push() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.mark_saved();
        assert!(!mgr.is_dirty());

        mgr.push(make_action("A", 1));
        assert!(mgr.is_dirty());
    }

    #[test]
    fn clean_after_save() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.mark_saved();
        assert!(!mgr.is_dirty());
    }

    #[test]
    fn dirty_after_undo_past_save() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.mark_saved();
        assert!(!mgr.is_dirty());

        mgr.undo();
        assert!(mgr.is_dirty());

        mgr.redo();
        assert!(!mgr.is_dirty());
    }

    // ── Grouping ────────────────────────────────────────────────────

    #[test]
    fn group_creates_single_action() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.begin_group("Move Node");
        mgr.push(make_action("Set X", 1));
        mgr.push(make_action("Set Y", 1));
        mgr.end_group();

        assert_eq!(mgr.undo_count(), 1);
        let action = &mgr.undo_stack()[0];
        assert_eq!(action.label, "Move Node");
        assert_eq!(action.op_count(), 2);
    }

    #[test]
    fn nested_groups() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.begin_group("Outer");
        mgr.push(make_action("A", 1));
        mgr.begin_group("Inner");
        mgr.push(make_action("B", 2));
        mgr.end_group(); // Inner — still collecting for Outer
        mgr.push(make_action("C", 3));
        mgr.end_group(); // Outer — all 3 ops in one action

        assert_eq!(mgr.undo_count(), 1);
        assert_eq!(mgr.undo_stack()[0].op_count(), 3);
    }

    #[test]
    fn empty_group_produces_nothing() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.begin_group("Empty");
        mgr.end_group();

        assert_eq!(mgr.undo_count(), 0);
    }

    #[test]
    fn is_grouping() {
        let mut mgr = UndoRedoManager::new(10);
        assert!(!mgr.is_grouping());
        mgr.begin_group("G");
        assert!(mgr.is_grouping());
        mgr.end_group();
        assert!(!mgr.is_grouping());
    }

    // ── Merging ─────────────────────────────────────────────────────

    #[test]
    fn mergeable_actions_coalesce() {
        let mut mgr = UndoRedoManager::new(10);

        let a1 = make_action("Set X", 1).with_merge_key("pos_x");
        let a2 = make_action("Set X", 1).with_merge_key("pos_x");

        mgr.push(a1);
        mgr.push(a2);

        assert_eq!(mgr.undo_count(), 1);
        assert_eq!(mgr.undo_stack()[0].op_count(), 2);
    }

    #[test]
    fn different_merge_keys_dont_merge() {
        let mut mgr = UndoRedoManager::new(10);

        let a1 = make_action("Set X", 1).with_merge_key("pos_x");
        let a2 = make_action("Set Y", 1).with_merge_key("pos_y");

        mgr.push(a1);
        mgr.push(a2);

        assert_eq!(mgr.undo_count(), 2);
    }

    #[test]
    fn non_mergeable_doesnt_merge() {
        let mut mgr = UndoRedoManager::new(10);

        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));

        assert_eq!(mgr.undo_count(), 2);
    }

    // ── History ─────────────────────────────────────────────────────

    #[test]
    fn history_includes_undo_and_redo() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.push(make_action("C", 3));

        mgr.undo(); // Undo C

        let history = mgr.history();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].label, "A");
        assert_eq!(history[1].label, "B");
        assert!(history[1].is_current); // B is top of undo
        assert_eq!(history[2].label, "C"); // C is in redo
        assert!(!history[2].is_current);
    }

    #[test]
    fn history_empty() {
        let mgr = UndoRedoManager::new(10);
        assert!(mgr.history().is_empty());
    }

    // ── Batch undo/redo ─────────────────────────────────────────────

    #[test]
    fn undo_n() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.push(make_action("C", 3));

        let undone = mgr.undo_n(2);
        assert_eq!(undone, 2);
        assert_eq!(mgr.undo_count(), 1);
        assert_eq!(mgr.redo_count(), 2);
    }

    #[test]
    fn undo_n_more_than_available() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));

        let undone = mgr.undo_n(5);
        assert_eq!(undone, 1);
    }

    #[test]
    fn redo_n() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.undo_n(2);

        let redone = mgr.redo_n(2);
        assert_eq!(redone, 2);
        assert_eq!(mgr.undo_count(), 2);
    }

    // ── Clear ───────────────────────────────────────────────────────

    #[test]
    fn clear_resets_everything() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.undo();

        mgr.clear();
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
        assert_eq!(mgr.undo_count(), 0);
        assert_eq!(mgr.redo_count(), 0);
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn end_group_without_begin_is_safe() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.end_group(); // Should not panic.
        assert_eq!(mgr.undo_count(), 0);
    }

    #[test]
    fn action_ids_are_sequential() {
        let mut mgr = UndoRedoManager::new(10);
        mgr.push(make_action("A", 1));
        mgr.push(make_action("B", 2));
        mgr.push(make_action("C", 3));

        assert_eq!(mgr.undo_stack()[0].id(), 1);
        assert_eq!(mgr.undo_stack()[1].id(), 2);
        assert_eq!(mgr.undo_stack()[2].id(), 3);
    }

    #[test]
    fn reparent_reversal() {
        let op = UndoOp::ReparentNode {
            node_id: 5,
            new_parent_id: 10,
            old_parent_id: Some(3),
        };
        let rev = op.reversed();
        match rev {
            UndoOp::ReparentNode {
                new_parent_id,
                old_parent_id,
                ..
            } => {
                assert_eq!(new_parent_id, 3);
                assert_eq!(old_parent_id, Some(10));
            }
            _ => panic!("expected ReparentNode"),
        }
    }
}
