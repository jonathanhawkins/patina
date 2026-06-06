//! The editor's multi-document model: the set of open documents (scenes and
//! scripts) plus which one is active.
//!
//! Godot's editor keeps several documents open at once as tabs and tracks a
//! single active document. [`EditorState`](crate::editor_server::EditorState)
//! embeds a [`MultiDocModel`] to back the scene/script tab bar:
//!
//! - **open** focuses the document, re-using an already-open tab when the path
//!   matches rather than duplicating it;
//! - **switch** changes which tab is active;
//! - **close** removes a tab while keeping a sensible neighbour focused.

/// What kind of document a tab holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// A scene document (e.g. `.tscn`).
    Scene,
    /// A script document (e.g. `.gd`).
    Script,
}

/// The 2D viewport's view transform (pan offset + zoom), saved per document so
/// each open scene keeps its own camera framing across tab switches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportState {
    /// Horizontal pan offset, in world units.
    pub pan_x: f32,
    /// Vertical pan offset, in world units.
    pub pan_y: f32,
    /// Zoom factor (`1.0` = 100%).
    pub zoom: f32,
}

impl Default for ViewportState {
    /// The default framing: centred at the origin at 100% zoom.
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

/// The per-document editing state that swaps when the active tab changes: which
/// nodes are selected in that document and where its viewport is framed.
///
/// Each open document carries its own, so switching tabs restores *that*
/// document's selection and view rather than leaking the previous tab's state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentState {
    /// The selected node ids in this document (empty = nothing selected).
    pub selection: Vec<u64>,
    /// This document's saved viewport framing.
    pub viewport: ViewportState,
}

/// A single open document (one tab) in the editor.
///
/// `state` is the document's own active editing state (selection + viewport);
/// it is what makes switching tabs swap the active selection and view. (No
/// `Eq`: the viewport's `f32` framing is only `PartialEq`.)
#[derive(Debug, Clone, PartialEq)]
pub struct OpenDocument {
    /// Display title shown on the tab.
    pub title: String,
    /// Resource path on disk, or `None` for an unsaved/untitled document.
    pub path: Option<String>,
    /// What kind of document this is.
    pub kind: DocumentKind,
    /// Whether the document has unsaved changes.
    pub dirty: bool,
    /// This document's own selection + viewport, restored when it is focused.
    pub state: DocumentState,
}

impl OpenDocument {
    /// Creates a document with the given title, path, and kind (initially not
    /// dirty, with an empty selection and a default viewport).
    pub fn new(title: impl Into<String>, path: Option<String>, kind: DocumentKind) -> Self {
        Self {
            title: title.into(),
            path,
            kind,
            dirty: false,
            state: DocumentState::default(),
        }
    }
}

/// The result of requesting that a document be closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// The document was clean and has been closed immediately.
    Closed,
    /// The document has unsaved changes and is kept open; the caller should
    /// raise a save / discard / cancel prompt and then call
    /// [`MultiDocModel::resolve_close`].
    NeedsSavePrompt,
}

/// The user's response to the unsaved-changes prompt raised when closing a
/// dirty document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePromptChoice {
    /// Save the document, then close it.
    Save,
    /// Discard the unsaved changes and close the document.
    Discard,
    /// Keep the document open (abort the close).
    Cancel,
}

/// The set of documents open in the editor plus the active selection.
///
/// Invariant: `active` is `Some(i)` with `i < docs.len()` whenever there is at
/// least one open document, and `None` exactly when no documents are open.
#[derive(Debug, Clone, Default)]
pub struct MultiDocModel {
    docs: Vec<OpenDocument>,
    active: Option<usize>,
}

impl MultiDocModel {
    /// Creates an empty model (no documents open).
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether no documents are open.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The index of the active document, if any.
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    /// The active document, if any.
    pub fn active(&self) -> Option<&OpenDocument> {
        self.active.and_then(|i| self.docs.get(i))
    }

    /// The active document's per-document state (selection + viewport), if any.
    /// This is the state currently shown in the editor; it swaps to the target
    /// document's state on [`switch_to`](Self::switch_to).
    pub fn active_state(&self) -> Option<&DocumentState> {
        self.active().map(|d| &d.state)
    }

    /// Mutable access to the active document's state, for updating its selection
    /// or viewport as the user edits the active tab. The edits stay with that
    /// document and are restored when it is focused again.
    pub fn active_state_mut(&mut self) -> Option<&mut DocumentState> {
        let i = self.active?;
        self.docs.get_mut(i).map(|d| &mut d.state)
    }

    /// The document at `index`, if any.
    pub fn get(&self, index: usize) -> Option<&OpenDocument> {
        self.docs.get(index)
    }

    /// The index of the open document with the given path, if one is open.
    pub fn index_of_path(&self, path: &str) -> Option<usize> {
        self.docs.iter().position(|d| d.path.as_deref() == Some(path))
    }

    /// Opens `doc`, focusing it and returning its index.
    ///
    /// If a document with the same (`Some`) path is already open, no duplicate
    /// tab is created — the existing tab is focused and its index returned.
    /// Untitled documents (`path == None`) always open a fresh tab.
    pub fn open(&mut self, doc: OpenDocument) -> usize {
        if let Some(path) = doc.path.as_deref() {
            if let Some(existing) = self.index_of_path(path) {
                self.active = Some(existing);
                return existing;
            }
        }
        self.docs.push(doc);
        let index = self.docs.len() - 1;
        self.active = Some(index);
        index
    }

    /// Creates a new empty, untitled scene document, opens it as a fresh tab,
    /// and focuses it — backing the editor's `Scene > New Scene` action. The new
    /// document has no path (unsaved) and starts clean; each invocation adds its
    /// own active tab, and distinct untitled scenes get distinct titles
    /// (`Untitled`, then `Untitled 2`, `Untitled 3`, …). Returns the new
    /// document's index.
    pub fn new_scene(&mut self) -> usize {
        let title = self.unique_untitled_title();
        self.open(OpenDocument::new(title, None, DocumentKind::Scene))
    }

    /// Picks an unused `Untitled` title for a new untitled document: `Untitled`
    /// first, then `Untitled 2`, `Untitled 3`, … skipping any title already
    /// taken by an open document.
    fn unique_untitled_title(&self) -> String {
        let taken = |t: &str| self.docs.iter().any(|d| d.title.as_str() == t);
        if !taken("Untitled") {
            return "Untitled".to_string();
        }
        let mut i = 2;
        loop {
            let candidate = format!("Untitled {i}");
            if !taken(&candidate) {
                return candidate;
            }
            i += 1;
        }
    }

    /// Switches the active document to `index`. Returns `true` if `index` is a
    /// valid tab (now active); `false` otherwise, leaving the active tab
    /// unchanged.
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.docs.len() {
            self.active = Some(index);
            true
        } else {
            false
        }
    }

    /// Closes the document at `index`, returning the removed document (or `None`
    /// if `index` is out of range).
    ///
    /// The active selection follows Godot's behaviour: closing a tab *before*
    /// the active one shifts the active index down; closing the active tab
    /// focuses the tab that takes its place (or the new last tab if the active
    /// tab was last); closing the last remaining tab clears the selection.
    pub fn close(&mut self, index: usize) -> Option<OpenDocument> {
        if index >= self.docs.len() {
            return None;
        }
        let removed = self.docs.remove(index);
        self.active = if self.docs.is_empty() {
            None
        } else {
            match self.active {
                // Active tab was after the closed one: shift down.
                Some(a) if a > index => Some(a - 1),
                // The active tab itself was closed: focus its successor, or the
                // new last tab if it had been the last.
                Some(a) if a == index => Some(a.min(self.docs.len() - 1)),
                // Active tab was before the closed one (or unset): unchanged.
                other => other,
            }
        };
        Some(removed)
    }

    /// Sets the dirty (unsaved-changes) flag of the document at `index` (a no-op
    /// if `index` is out of range). The editor calls this as the active document
    /// is edited so [`request_close`](Self::request_close) can guard it.
    pub fn set_dirty(&mut self, index: usize, dirty: bool) {
        if let Some(d) = self.docs.get_mut(index) {
            d.dirty = dirty;
        }
    }

    /// Requests closing the document at `index`, guarding unsaved changes.
    ///
    /// A clean document is closed immediately and reports
    /// [`CloseOutcome::Closed`]; a dirty one is **kept open** and reports
    /// [`CloseOutcome::NeedsSavePrompt`] so the caller can raise the
    /// save/discard/cancel prompt. An out-of-range index is a harmless no-op
    /// that reports `Closed`. Closing the last document leaves an empty editor
    /// (see [`close`](Self::close)).
    pub fn request_close(&mut self, index: usize) -> CloseOutcome {
        match self.docs.get(index).map(|d| d.dirty) {
            Some(true) => CloseOutcome::NeedsSavePrompt,
            Some(false) => {
                self.close(index);
                CloseOutcome::Closed
            }
            None => CloseOutcome::Closed,
        }
    }

    /// Resolves the save prompt for the dirty document at `index`. `Save` clears
    /// the dirty flag and closes it, `Discard` closes it without saving, and
    /// `Cancel` keeps it open. Returns `true` if the document was closed.
    pub fn resolve_close(&mut self, index: usize, choice: SavePromptChoice) -> bool {
        match choice {
            SavePromptChoice::Cancel => false,
            SavePromptChoice::Save => {
                if let Some(d) = self.docs.get_mut(index) {
                    d.dirty = false;
                }
                self.close(index).is_some()
            }
            SavePromptChoice::Discard => self.close(index).is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(title: &str, path: Option<&str>) -> OpenDocument {
        OpenDocument::new(title, path.map(|p| p.to_string()), DocumentKind::Scene)
    }

    /// Acceptance (pat-w1ub4.1): the multi-document model supports opening,
    /// switching, and closing documents while correctly tracking the active
    /// index across each operation.
    #[test]
    fn editor_multidoc_open_switch_close_model() {
        let mut docs = MultiDocModel::new();
        assert!(docs.is_empty());
        assert_eq!(docs.active_index(), None);

        // Opening focuses the newly opened document.
        let a = docs.open(scene("Main", Some("res://main.tscn")));
        let b = docs.open(scene("Level", Some("res://level.tscn")));
        let c = docs.open(OpenDocument::new(
            "player.gd",
            Some("res://player.gd".to_string()),
            DocumentKind::Script,
        ));
        assert_eq!(docs.len(), 3);
        assert_eq!(docs.active_index(), Some(c));
        assert_eq!(docs.active().unwrap().title, "player.gd");
        assert_eq!(docs.active().unwrap().kind, DocumentKind::Script);

        // Re-opening an already-open path switches to it — no duplicate tab.
        let a_again = docs.open(scene("Main", Some("res://main.tscn")));
        assert_eq!(a_again, a, "re-open returns the existing index");
        assert_eq!(docs.len(), 3, "no duplicate tab created");
        assert_eq!(docs.active_index(), Some(a), "re-open focuses the existing doc");

        // Switching changes the active tab; an out-of-range switch is rejected.
        assert!(docs.switch_to(b));
        assert_eq!(docs.active_index(), Some(b));
        assert!(!docs.switch_to(99));
        assert_eq!(
            docs.active_index(),
            Some(b),
            "an invalid switch leaves the active tab unchanged"
        );

        // Closing a tab before the active one shifts the active index down.
        // docs: [Main(0), Level(1), player(2)], active = Level(1).
        let removed = docs.close(a).expect("closed Main");
        assert_eq!(removed.title, "Main");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs.active_index(), Some(0), "active index shifts down");
        assert_eq!(docs.active().unwrap().title, "Level");

        // Closing the active (last) tab falls back to the new last tab.
        // docs: [Level(0), player(1)]; focus player(1) then close it.
        assert!(docs.switch_to(1));
        let removed = docs.close(1).expect("closed player");
        assert_eq!(removed.kind, DocumentKind::Script);
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs.active_index(),
            Some(0),
            "focus falls back to the remaining tab"
        );
        assert_eq!(docs.active().unwrap().title, "Level");

        // Closing the last remaining tab clears the active selection.
        docs.close(0).expect("closed Level");
        assert!(docs.is_empty());
        assert_eq!(docs.active_index(), None);

        // Closing out of range returns None.
        assert!(docs.close(0).is_none());
    }

    /// Acceptance (pat-w1ub4.2): `Scene > New Scene` creates an empty, untitled
    /// scene document and makes it the active tab. Each invocation adds its own
    /// distinct active tab.
    #[test]
    fn editor_new_scene_creates_empty_active_document() {
        let mut docs = MultiDocModel::new();
        assert!(docs.is_empty());
        assert_eq!(docs.active_index(), None);

        // New Scene adds an empty active document + tab.
        let i = docs.new_scene();
        assert_eq!(docs.len(), 1, "a new tab was added");
        assert_eq!(docs.active_index(), Some(i), "the new scene is the active tab");
        let doc = docs.active().expect("active document present");
        assert_eq!(doc.kind, DocumentKind::Scene, "New Scene creates a scene document");
        assert_eq!(doc.path, None, "an empty new scene is unsaved (no path)");
        assert!(!doc.dirty, "a freshly created scene starts clean");
        assert!(
            doc.title.starts_with("Untitled"),
            "the new scene is labeled Untitled: {}",
            doc.title
        );

        // A second New Scene adds another distinct active tab.
        let j = docs.new_scene();
        assert_ne!(i, j, "each New Scene is a distinct tab");
        assert_eq!(docs.len(), 2, "each New Scene adds its own tab");
        assert_eq!(docs.active_index(), Some(j), "the latest new scene is active");
        assert_eq!(docs.get(j).unwrap().path, None, "the second new scene is also empty");
        // Distinct untitled scenes get distinct titles.
        assert_ne!(
            docs.get(i).unwrap().title,
            docs.get(j).unwrap().title,
            "untitled scenes are uniquely titled"
        );

        // With a real (saved) scene already open, New Scene still creates a
        // fresh empty active document rather than reusing an existing tab.
        docs.open(OpenDocument::new(
            "Main",
            Some("res://main.tscn".to_string()),
            DocumentKind::Scene,
        ));
        let before = docs.len();
        let k = docs.new_scene();
        assert_eq!(docs.len(), before + 1, "New Scene opens a brand-new tab");
        assert_eq!(docs.active_index(), Some(k), "the new empty scene is active");
        assert_eq!(docs.active().unwrap().path, None, "the new scene is empty (unsaved)");
    }

    /// Acceptance (pat-w1ub4.4): switching tabs swaps the active document's
    /// tree, selection, and viewport. Each open document keeps its own
    /// selection and viewport framing, restored when it becomes active again —
    /// switching never leaks one tab's state into another.
    #[test]
    fn editor_switch_document_swaps_active_state() {
        let mut docs = MultiDocModel::new();

        // Open scene A and give it a selection + viewport while it is active.
        let a = docs.open(scene("Main", Some("res://main.tscn")));
        {
            let st = docs.active_state_mut().expect("A is active");
            st.selection = vec![10, 11];
            st.viewport = ViewportState {
                pan_x: 100.0,
                pan_y: 50.0,
                zoom: 2.0,
            };
        }

        // Open scene B: a freshly opened document starts with default state.
        let b = docs.open(scene("Level", Some("res://level.tscn")));
        {
            let st = docs.active_state().expect("B is active");
            assert!(st.selection.is_empty(), "a new document starts with no selection");
            assert_eq!(
                st.viewport,
                ViewportState::default(),
                "and a default viewport framing"
            );
        }
        // Give scene B its own distinct selection + viewport.
        {
            let st = docs.active_state_mut().expect("B is active");
            st.selection = vec![42];
            st.viewport = ViewportState {
                pan_x: -20.0,
                pan_y: 0.0,
                zoom: 0.5,
            };
        }

        // Switching to A swaps the active tree and restores A's state.
        assert!(docs.switch_to(a));
        assert_eq!(docs.active().unwrap().title, "Main", "active tree is scene A");
        let sa = docs.active_state().unwrap();
        assert_eq!(sa.selection, vec![10, 11], "A's selection is restored");
        assert_eq!(
            sa.viewport,
            ViewportState { pan_x: 100.0, pan_y: 50.0, zoom: 2.0 },
            "A's viewport framing is restored"
        );

        // Switching to B swaps to B's tree and its own selection + viewport.
        assert!(docs.switch_to(b));
        assert_eq!(docs.active().unwrap().title, "Level", "active tree is scene B");
        let sb = docs.active_state().unwrap();
        assert_eq!(sb.selection, vec![42], "B's selection is restored");
        assert_eq!(
            sb.viewport,
            ViewportState { pan_x: -20.0, pan_y: 0.0, zoom: 0.5 },
            "B's viewport framing is restored"
        );

        // The two documents' states are independent — no leakage on switch.
        assert_ne!(
            docs.get(a).unwrap().state.selection,
            docs.get(b).unwrap().state.selection,
            "each document keeps its own selection"
        );
    }

    /// Acceptance (pat-w1ub4.6): closing a scene guards unsaved changes — a
    /// clean document closes immediately, a dirty one prompts save/discard/
    /// cancel and only closes once resolved, and closing the last document
    /// leaves an empty editor (no documents, no active selection).
    #[test]
    fn editor_close_document_dirty_guard() {
        let mut docs = MultiDocModel::new();
        let a = docs.open(scene("Main", Some("res://main.tscn")));
        let _b = docs.open(scene("Level", Some("res://level.tscn")));
        assert_eq!(docs.len(), 2);

        // A clean document closes immediately, no prompt.
        assert_eq!(docs.request_close(a), CloseOutcome::Closed);
        assert_eq!(docs.len(), 1, "a clean document is removed immediately");

        // Mark the remaining document (now at index 0) dirty.
        let idx = docs.active_index().expect("a document remains");
        docs.set_dirty(idx, true);

        // Requesting close on a dirty document prompts to save; it stays open.
        assert_eq!(docs.request_close(idx), CloseOutcome::NeedsSavePrompt);
        assert_eq!(
            docs.len(),
            1,
            "a dirty document is not closed until the prompt resolves"
        );

        // Cancel keeps it open and still dirty.
        assert!(!docs.resolve_close(idx, SavePromptChoice::Cancel));
        assert_eq!(docs.len(), 1);
        assert!(docs.get(idx).unwrap().dirty, "cancel leaves the edits unsaved");

        // Save clears the dirty flag and closes it — the last close leaves an
        // empty editor.
        assert!(docs.resolve_close(idx, SavePromptChoice::Save));
        assert!(docs.is_empty(), "closing the last document leaves an empty editor");
        assert_eq!(docs.active_index(), None);

        // Discard closes a dirty document without saving; the editor is empty
        // again afterwards.
        let c = docs.open(scene("Enemy", Some("res://enemy.tscn")));
        docs.set_dirty(c, true);
        assert_eq!(docs.request_close(c), CloseOutcome::NeedsSavePrompt);
        assert!(docs.resolve_close(c, SavePromptChoice::Discard));
        assert!(docs.is_empty(), "discard-closing the last document empties the editor");
        assert_eq!(docs.active_index(), None);

        // Requesting close of an out-of-range index is a harmless no-op.
        assert_eq!(docs.request_close(99), CloseOutcome::Closed);
    }
}
