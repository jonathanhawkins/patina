//! The new-scene control.
//!
//! Activating the editor's "new scene" control creates a fresh untitled scene,
//! appends its tab to the scene workspace, and focuses it as the active scene.
//! Each new untitled scene gets a distinct placeholder title.

/// A scene tab in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEntry {
    /// The tab's display title.
    pub title: String,
    /// Whether the scene is an unsaved/untitled placeholder.
    pub untitled: bool,
}

/// The set of open scene tabs and which one is active.
#[derive(Debug, Clone, Default)]
pub struct SceneWorkspace {
    tabs: Vec<SceneEntry>,
    active: Option<usize>,
    untitled_count: u32,
}

impl SceneWorkspace {
    /// Creates an empty workspace (no scenes open).
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open scene tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The index of the active scene tab, if any.
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    /// The active scene's title, if any.
    pub fn active_title(&self) -> Option<&str> {
        self.active
            .and_then(|i| self.tabs.get(i))
            .map(|e| e.title.as_str())
    }

    /// The tab at `index`, if any.
    pub fn tab(&self, index: usize) -> Option<&SceneEntry> {
        self.tabs.get(index)
    }

    /// Activates the new-scene control: creates a fresh untitled scene, appends
    /// its tab, focuses it, and returns its index.
    pub fn new_scene(&mut self) -> usize {
        self.untitled_count += 1;
        let title = if self.untitled_count == 1 {
            "untitled".to_string()
        } else {
            format!("untitled ({})", self.untitled_count)
        };
        self.tabs.push(SceneEntry {
            title,
            untitled: true,
        });
        let index = self.tabs.len() - 1;
        self.active = Some(index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-ajp4b): activating the new-scene control creates an
    /// untitled scene, adds its tab, and makes it the active scene.
    #[test]
    fn top_bar_open_new_scene() {
        let mut workspace = SceneWorkspace::new();
        assert_eq!(workspace.tab_count(), 0);
        assert_eq!(workspace.active_index(), None);

        // Activating new-scene adds an untitled tab and focuses it.
        let first = workspace.new_scene();
        assert_eq!(workspace.tab_count(), 1, "a tab was added");
        assert_eq!(workspace.active_index(), Some(first), "the new scene is active");
        let tab = workspace.tab(first).expect("new tab exists");
        assert!(tab.untitled, "the new scene is untitled");
        assert_eq!(workspace.active_title(), Some("untitled"));

        // A second new-scene gets a distinct title and takes focus from the first.
        let second = workspace.new_scene();
        assert_eq!(workspace.tab_count(), 2);
        assert_eq!(
            workspace.active_index(),
            Some(second),
            "focus moves to the newest scene"
        );
        assert_ne!(first, second);
        assert_eq!(workspace.active_title(), Some("untitled (2)"));
        // The first scene is still present, just no longer active.
        assert!(workspace.tab(first).unwrap().untitled);
    }
}
