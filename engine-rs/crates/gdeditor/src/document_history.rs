//! Per-document undo/redo histories.
//!
//! In a multi-document editor each open document (scene or script) keeps its
//! own undo/redo stack: editing and undoing in one tab must never disturb the
//! history of another. [`DocumentHistories`] holds one independent
//! [`UndoRedoManager`] per open document and routes undo/redo to the requested
//! document, keeping the histories fully isolated. It mirrors the document list
//! in [`MultiDocModel`](crate::multi_document::MultiDocModel): opening a
//! document adds a fresh history, closing one removes it.

use crate::undo_redo::{UndoAction, UndoRedoManager};

/// One independent undo/redo history per open document.
///
/// Histories are stored in document order (parallel to the document tabs).
/// Every mutating call is scoped to a single document index, so an action,
/// undo, or redo in one document is invisible to all others.
#[derive(Debug, Default)]
pub struct DocumentHistories {
    histories: Vec<UndoRedoManager>,
}

impl DocumentHistories {
    /// Creates an empty set of histories (no documents open).
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of documents with a history.
    pub fn len(&self) -> usize {
        self.histories.len()
    }

    /// Whether no documents have a history.
    pub fn is_empty(&self) -> bool {
        self.histories.is_empty()
    }

    /// Adds a fresh, empty history for a newly opened document and returns its
    /// index (the position in document order).
    pub fn open(&mut self) -> usize {
        self.histories.push(UndoRedoManager::default());
        self.histories.len() - 1
    }

    /// Closes the history for the document at `doc`, removing it. Histories of
    /// later documents shift down by one index (matching tab removal). No-op if
    /// `doc` is out of range.
    pub fn close(&mut self, doc: usize) {
        if doc < self.histories.len() {
            self.histories.remove(doc);
        }
    }

    /// Borrows the history for `doc`, if it exists.
    pub fn get(&self, doc: usize) -> Option<&UndoRedoManager> {
        self.histories.get(doc)
    }

    /// Mutably borrows the history for `doc`, if it exists.
    pub fn get_mut(&mut self, doc: usize) -> Option<&mut UndoRedoManager> {
        self.histories.get_mut(doc)
    }

    /// Records `action` in `doc`'s history only. No-op if `doc` is out of range.
    pub fn push(&mut self, doc: usize, action: UndoAction) {
        if let Some(history) = self.histories.get_mut(doc) {
            history.push(action);
        }
    }

    /// Undoes the most recent action in `doc`'s history. Returns `true` if an
    /// action was undone, `false` if there was nothing to undo (or `doc` is out
    /// of range). Other documents are unaffected.
    pub fn undo(&mut self, doc: usize) -> bool {
        self.histories
            .get_mut(doc)
            .map(|h| h.undo().is_some())
            .unwrap_or(false)
    }

    /// Redoes the most recently undone action in `doc`'s history. Returns `true`
    /// if an action was redone. Other documents are unaffected.
    pub fn redo(&mut self, doc: usize) -> bool {
        self.histories
            .get_mut(doc)
            .map(|h| h.redo().is_some())
            .unwrap_or(false)
    }

    /// Whether `doc` has any actions to undo.
    pub fn can_undo(&self, doc: usize) -> bool {
        self.histories.get(doc).map(|h| h.can_undo()).unwrap_or(false)
    }

    /// Whether `doc` has any actions to redo.
    pub fn can_redo(&self, doc: usize) -> bool {
        self.histories.get(doc).map(|h| h.can_redo()).unwrap_or(false)
    }

    /// The number of undoable actions in `doc`'s history (0 if out of range).
    pub fn undo_count(&self, doc: usize) -> usize {
        self.histories.get(doc).map(|h| h.undo_count()).unwrap_or(0)
    }

    /// The number of redoable actions in `doc`'s history (0 if out of range).
    pub fn redo_count(&self, doc: usize) -> usize {
        self.histories.get(doc).map(|h| h.redo_count()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo_redo::UndoOp;

    fn edit(label: &str, node: u64) -> UndoAction {
        UndoAction::new(
            label,
            UndoOp::SetProperty {
                node_id: node,
                property: "position".to_string(),
                new_value: "10,20".to_string(),
                old_value: "0,0".to_string(),
            },
        )
    }

    /// Acceptance (pat-w1ub4.5): each document owns an isolated undo/redo stack —
    /// pushing, undoing, and redoing in one document never affects another.
    #[test]
    fn editor_per_document_undo_redo_isolated() {
        let mut hist = DocumentHistories::new();
        assert!(hist.is_empty());

        // Open two documents, each with its own empty history.
        let a = hist.open();
        let b = hist.open();
        assert_eq!(hist.len(), 2);
        assert!(!hist.can_undo(a));
        assert!(!hist.can_undo(b));

        // Two edits in A, one edit in B.
        hist.push(a, edit("A1", 1));
        hist.push(a, edit("A2", 1));
        hist.push(b, edit("B1", 2));
        assert_eq!(hist.undo_count(a), 2);
        assert_eq!(hist.undo_count(b), 1, "B's history is independent of A's");

        // Undoing in A leaves B completely untouched.
        assert!(hist.undo(a));
        assert_eq!(hist.undo_count(a), 1);
        assert_eq!(hist.redo_count(a), 1);
        assert_eq!(hist.undo_count(b), 1, "undo in A does not affect B");
        assert_eq!(hist.redo_count(b), 0);

        // Drain A's history entirely; B still has its action.
        assert!(hist.undo(a));
        assert!(!hist.can_undo(a));
        assert!(hist.can_undo(b), "B keeps its undo history when A is empty");
        assert_eq!(hist.undo_count(b), 1);

        // Redo is per-document too: redoing in A doesn't touch B.
        assert!(hist.redo(a));
        assert_eq!(hist.undo_count(a), 1);
        assert!(hist.can_undo(b));
        assert!(!hist.can_redo(b));

        // Undo on B affects only B.
        assert!(hist.undo(b));
        assert!(!hist.can_undo(b));
        assert!(hist.can_redo(b));
        assert!(hist.can_undo(a), "B's undo left A intact");

        // Closing A drops its history; B's history survives at its new index.
        hist.close(a);
        assert_eq!(hist.len(), 1);
        // B is now at index 0 and still has its redo pending.
        assert!(hist.can_redo(0));
        assert_eq!(hist.undo_count(0), 0);

        // Operations on an out-of-range document are safe no-ops.
        assert!(!hist.undo(99));
        assert!(!hist.can_undo(99));
        assert_eq!(hist.undo_count(99), 0);
    }
}
