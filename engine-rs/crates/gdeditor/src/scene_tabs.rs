//! Scene tabs in the editor top bar.
//!
//! Each open scene gets a tab that shows a modified marker while it has unsaved
//! edits. Closing a tab depends on its state: a clean tab closes immediately,
//! while a dirty tab raises a save / discard / cancel prompt and is only removed
//! once that prompt is resolved.

/// A single scene tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTab {
    /// The scene's display name.
    pub name: String,
    /// Whether the scene has unsaved edits.
    pub dirty: bool,
}

/// What happens when the user asks to close a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// The tab was clean and has been removed immediately.
    Closed,
    /// The tab is dirty; a save/discard/cancel prompt should be shown.
    PromptSave,
}

/// The user's response to the unsaved-changes prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePromptChoice {
    /// Save the scene, then close the tab.
    Save,
    /// Discard the edits and close the tab.
    Discard,
    /// Keep the tab open.
    Cancel,
}

/// The ordered set of open scene tabs in the top bar.
#[derive(Debug, Clone, Default)]
pub struct SceneTabs {
    tabs: Vec<SceneTab>,
}

impl SceneTabs {
    /// Creates an empty tab strip.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether no tabs are open.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Opens a scene tab and returns its index.
    pub fn open(&mut self, name: &str) -> usize {
        self.tabs.push(SceneTab {
            name: name.to_string(),
            dirty: false,
        });
        self.tabs.len() - 1
    }

    /// Sets the dirty (unsaved-edits) state of a tab.
    pub fn mark_dirty(&mut self, index: usize, dirty: bool) {
        if let Some(t) = self.tabs.get_mut(index) {
            t.dirty = dirty;
        }
    }

    /// Whether a tab has unsaved edits.
    pub fn is_dirty(&self, index: usize) -> bool {
        self.tabs.get(index).map(|t| t.dirty).unwrap_or(false)
    }

    /// The tab's label, with a `●` modified marker appended while dirty.
    pub fn tab_label(&self, index: usize) -> String {
        match self.tabs.get(index) {
            Some(t) if t.dirty => format!("{} ●", t.name),
            Some(t) => t.name.clone(),
            None => String::new(),
        }
    }

    /// Requests closing a tab. A clean tab is removed immediately and reports
    /// [`CloseOutcome::Closed`]; a dirty tab is kept and reports
    /// [`CloseOutcome::PromptSave`] so the caller can raise the save prompt.
    pub fn request_close(&mut self, index: usize) -> CloseOutcome {
        match self.tabs.get(index) {
            Some(t) if t.dirty => CloseOutcome::PromptSave,
            Some(_) => {
                self.tabs.remove(index);
                CloseOutcome::Closed
            }
            None => CloseOutcome::Closed,
        }
    }

    /// Resolves the save prompt for a dirty tab. `Save` clears the dirty flag
    /// and closes it, `Discard` closes it without saving, and `Cancel` keeps it
    /// open. Returns `true` if the tab was closed.
    pub fn resolve_close(&mut self, index: usize, choice: SavePromptChoice) -> bool {
        match choice {
            SavePromptChoice::Cancel => false,
            SavePromptChoice::Save => {
                if let Some(t) = self.tabs.get_mut(index) {
                    t.dirty = false;
                }
                self.close(index)
            }
            SavePromptChoice::Discard => self.close(index),
        }
    }

    fn close(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-4zsu4): a scene with unsaved edits shows a modified
    /// marker on its tab; closing a dirty tab prompts save/discard/cancel and
    /// closing a clean tab removes it immediately.
    #[test]
    fn top_bar_tab_unsaved_and_close() {
        let mut tabs = SceneTabs::new();
        let main = tabs.open("Main");
        assert!(!tabs.is_dirty(main));
        assert_eq!(tabs.tab_label(main), "Main", "a clean tab shows no marker");

        // Unsaved edits surface a modified marker on the tab.
        tabs.mark_dirty(main, true);
        assert!(tabs.is_dirty(main));
        assert!(
            tabs.tab_label(main).contains('●'),
            "a dirty tab shows the modified marker: {}",
            tabs.tab_label(main)
        );

        // Closing the dirty tab prompts to save — it is not removed yet.
        assert_eq!(tabs.request_close(main), CloseOutcome::PromptSave);
        assert_eq!(tabs.len(), 1, "a dirty tab stays until the prompt is resolved");

        // Cancel keeps the tab open.
        assert!(!tabs.resolve_close(main, SavePromptChoice::Cancel));
        assert_eq!(tabs.len(), 1);
        assert!(tabs.is_dirty(main), "cancel leaves the edits unsaved");

        // Save clears the dirty flag and closes the tab.
        assert!(tabs.resolve_close(main, SavePromptChoice::Save));
        assert!(tabs.is_empty(), "saving closes the tab");

        // A clean tab closes immediately, no prompt.
        let clean = tabs.open("Clean");
        assert_eq!(tabs.request_close(clean), CloseOutcome::Closed);
        assert!(tabs.is_empty(), "a clean tab is removed immediately");

        // Discard closes a dirty tab without saving.
        let dirty = tabs.open("Dirty");
        tabs.mark_dirty(dirty, true);
        assert_eq!(tabs.request_close(dirty), CloseOutcome::PromptSave);
        assert!(tabs.resolve_close(dirty, SavePromptChoice::Discard));
        assert!(tabs.is_empty(), "discard closes the tab");
    }
}
