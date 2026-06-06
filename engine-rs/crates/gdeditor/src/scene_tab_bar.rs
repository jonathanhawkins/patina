//! The top-bar scene tab strip.
//!
//! Each open scene is shown as a tab labeled with its name. Selecting a tab
//! makes that scene the active one, switching the editor context to it. This is
//! the editor-context model behind the top bar's `/api/scene/tabs` endpoints —
//! mirroring Godot's scene tab bar.

/// One scene tab in the top bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTab {
    /// Stable identifier for the tab.
    pub id: u32,
    /// Display name shown on the tab (the scene's name).
    pub name: String,
    /// Filesystem path of the scene (empty for an unsaved scene). Used by the
    /// "Copy Scene Path" and "Show in FileSystem" context-menu actions.
    pub path: String,
}

/// An action in a scene tab's right-click context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMenuAction {
    /// Close the targeted tab.
    Close,
    /// Close every tab except the targeted one.
    CloseOthers,
    /// Close all open tabs.
    CloseAll,
    /// Copy the targeted scene's path to the clipboard.
    CopyScenePath,
    /// Reveal the targeted scene in the FileSystem dock.
    ShowInFileSystem,
}

/// The result of invoking a context-menu action on a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabMenuOutcome {
    /// One or more tabs were closed.
    Closed,
    /// A path action produced this scene path (for clipboard / file browser).
    Path(String),
    /// The targeted tab id was not found.
    NotFound,
}

/// The top-bar tab strip of open scenes, tracking which one is active.
#[derive(Debug, Clone, Default)]
pub struct SceneTabBar {
    tabs: Vec<SceneTab>,
    active: usize,
    next_id: u32,
}

impl SceneTabBar {
    /// Creates an empty tab bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a new scene tab labeled `name` (with no saved path) and returns its
    /// id. The newly opened scene becomes the active one (matching Godot's
    /// "open scene").
    pub fn open(&mut self, name: impl Into<String>) -> u32 {
        self.open_with_path(name, "")
    }

    /// Opens a new scene tab labeled `name` backed by `path`, returns its id.
    /// The newly opened scene becomes the active one.
    pub fn open_with_path(&mut self, name: impl Into<String>, path: impl Into<String>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(SceneTab {
            id,
            name: name.into(),
            path: path.into(),
        });
        self.active = self.tabs.len() - 1;
        id
    }

    /// All open tabs, in display order.
    pub fn tabs(&self) -> &[SceneTab] {
        &self.tabs
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether there are no open tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tab labels in display order — what the top bar renders.
    pub fn labels(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.name.as_str()).collect()
    }

    /// The active tab, if any.
    pub fn active_tab(&self) -> Option<&SceneTab> {
        self.tabs.get(self.active)
    }

    /// The active scene's tab id.
    pub fn active_id(&self) -> Option<u32> {
        self.active_tab().map(|t| t.id)
    }

    /// The active scene's name — the current editor context.
    pub fn active_name(&self) -> Option<&str> {
        self.active_tab().map(|t| t.name.as_str())
    }

    /// Switches the active scene to the tab with `id`, returning `true` if a
    /// matching tab was found. Clicking a tab in the top bar calls this, making
    /// it the active scene and switching the editor context to it. Switching to
    /// an unknown id is a no-op that returns `false`.
    pub fn switch_to(&mut self, id: u32) -> bool {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
            self.active = idx;
            true
        } else {
            false
        }
    }

    /// The right-click context menu for the tab with `id`: the actions exposed
    /// when a tab is right-clicked. Returns an empty list for an unknown id.
    pub fn context_menu(&self, id: u32) -> Vec<TabMenuAction> {
        if self.tabs.iter().any(|t| t.id == id) {
            vec![
                TabMenuAction::Close,
                TabMenuAction::CloseOthers,
                TabMenuAction::CloseAll,
                TabMenuAction::CopyScenePath,
                TabMenuAction::ShowInFileSystem,
            ]
        } else {
            Vec::new()
        }
    }

    /// Invokes a context-menu `action` on the tab with `id`. Close actions
    /// mutate the tab set (keeping the active index valid); path actions return
    /// the targeted scene's path. An unknown id yields `NotFound`.
    pub fn apply_menu(&mut self, action: TabMenuAction, id: u32) -> TabMenuOutcome {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else {
            return TabMenuOutcome::NotFound;
        };
        match action {
            TabMenuAction::Close => {
                self.tabs.remove(idx);
                self.clamp_active(idx);
                TabMenuOutcome::Closed
            }
            TabMenuAction::CloseOthers => {
                self.tabs.retain(|t| t.id == id);
                self.active = 0;
                TabMenuOutcome::Closed
            }
            TabMenuAction::CloseAll => {
                self.tabs.clear();
                self.active = 0;
                TabMenuOutcome::Closed
            }
            TabMenuAction::CopyScenePath | TabMenuAction::ShowInFileSystem => {
                TabMenuOutcome::Path(self.tabs[idx].path.clone())
            }
        }
    }

    /// Keeps `active` pointing at a valid tab after `removed_idx` was closed.
    fn clamp_active(&mut self, removed_idx: usize) {
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active > removed_idx || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
    }

    /// Reorders the tab strip by moving the tab at index `from` to index `to`
    /// (drag-and-drop), preserving which scene is active. Returns `false` if
    /// either index is out of range.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        if from == to {
            return true;
        }
        // Remember the active scene so the selection follows the reorder.
        let active_id = self.active_id();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        // Restore the active index to wherever the active scene landed.
        if let Some(id) = active_id {
            if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                self.active = idx;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-2674w): each open scene renders a tab labeled with its
    /// name, and selecting a tab switches the active scene and editor context.
    #[test]
    fn top_bar_scene_tabs_switch() {
        let mut bar = SceneTabBar::new();
        assert!(bar.is_empty(), "no tabs before any scene is opened");

        // Open three scenes — each appears as a tab labeled with its name.
        let a = bar.open("Main.tscn");
        let b = bar.open("Player.tscn");
        let c = bar.open("Enemy.tscn");
        assert_eq!(bar.len(), 3);
        assert_eq!(
            bar.labels(),
            vec!["Main.tscn", "Player.tscn", "Enemy.tscn"],
            "each open scene renders a tab labeled with its name"
        );

        // Opening the last scene leaves it active (editor context = Enemy).
        assert_eq!(bar.active_id(), Some(c));
        assert_eq!(bar.active_name(), Some("Enemy.tscn"));

        // Clicking the first tab switches the active scene / editor context.
        assert!(bar.switch_to(a), "switching to an open tab succeeds");
        assert_eq!(bar.active_id(), Some(a));
        assert_eq!(
            bar.active_name(),
            Some("Main.tscn"),
            "selecting a tab switches the active scene context to it"
        );

        // Clicking another tab switches again.
        assert!(bar.switch_to(b));
        assert_eq!(bar.active_name(), Some("Player.tscn"));

        // Switching to a non-existent tab is a no-op and reports false.
        assert!(!bar.switch_to(9999), "unknown tab id is rejected");
        assert_eq!(
            bar.active_name(),
            Some("Player.tscn"),
            "a rejected switch leaves the active scene unchanged"
        );

        // Tab ids are stable and distinct.
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    /// Acceptance (pat-s8p4r): right-clicking a tab exposes Close, Close Other
    /// Tabs, Close All, Copy Scene Path, and Show in FileSystem, each acting on
    /// the targeted tab.
    #[test]
    fn top_bar_tab_context_menu() {
        let mut bar = SceneTabBar::new();
        let a = bar.open_with_path("Main.tscn", "res://Main.tscn");
        let b = bar.open_with_path("Player.tscn", "res://actors/Player.tscn");
        let c = bar.open_with_path("Enemy.tscn", "res://actors/Enemy.tscn");

        // Right-clicking a tab exposes all five actions, in order.
        assert_eq!(
            bar.context_menu(b),
            vec![
                TabMenuAction::Close,
                TabMenuAction::CloseOthers,
                TabMenuAction::CloseAll,
                TabMenuAction::CopyScenePath,
                TabMenuAction::ShowInFileSystem,
            ],
            "all five context-menu actions are exposed"
        );
        // An unknown tab has no menu.
        assert!(bar.context_menu(9999).is_empty());

        // Copy Scene Path / Show in FileSystem act on the TARGETED tab's path.
        assert_eq!(
            bar.apply_menu(TabMenuAction::CopyScenePath, b),
            TabMenuOutcome::Path("res://actors/Player.tscn".to_string()),
            "Copy Scene Path returns the targeted tab's path"
        );
        assert_eq!(
            bar.apply_menu(TabMenuAction::ShowInFileSystem, a),
            TabMenuOutcome::Path("res://Main.tscn".to_string()),
            "Show in FileSystem reveals the targeted tab's path"
        );
        // Path actions don't close anything.
        assert_eq!(bar.len(), 3);

        // Close acts on the targeted tab only.
        assert_eq!(bar.apply_menu(TabMenuAction::Close, b), TabMenuOutcome::Closed);
        assert_eq!(bar.len(), 2);
        assert_eq!(bar.labels(), vec!["Main.tscn", "Enemy.tscn"]);

        // Close Other Tabs keeps only the targeted tab.
        assert_eq!(
            bar.apply_menu(TabMenuAction::CloseOthers, c),
            TabMenuOutcome::Closed
        );
        assert_eq!(bar.len(), 1);
        assert_eq!(bar.active_name(), Some("Enemy.tscn"));
        assert_eq!(bar.active_id(), Some(c));

        // Close All clears every tab.
        assert_eq!(
            bar.apply_menu(TabMenuAction::CloseAll, c),
            TabMenuOutcome::Closed
        );
        assert!(bar.is_empty());

        // Acting on a missing tab reports NotFound.
        assert_eq!(
            bar.apply_menu(TabMenuAction::Close, 1234),
            TabMenuOutcome::NotFound
        );

        // Closing the active tab keeps the active index valid.
        let mut bar2 = SceneTabBar::new();
        let x = bar2.open("A");
        let y = bar2.open("B");
        let z = bar2.open("C"); // C is active
        assert_eq!(bar2.active_id(), Some(z));
        assert_eq!(bar2.apply_menu(TabMenuAction::Close, z), TabMenuOutcome::Closed);
        // Active falls back to a valid remaining tab.
        assert!(bar2.active_id() == Some(y) || bar2.active_id() == Some(x));
        assert!(bar2.active_tab().is_some(), "active index stays valid after close");
    }

    /// Acceptance (pat-8iwh0): dragging a tab to a new position reorders the tab
    /// strip and preserves the active selection.
    #[test]
    fn top_bar_tab_reorder() {
        let mut bar = SceneTabBar::new();
        let _a = bar.open("A");
        let _b = bar.open("B");
        let c = bar.open("C"); // C is active (opened last)
        assert_eq!(bar.labels(), vec!["A", "B", "C"]);
        assert_eq!(bar.active_id(), Some(c));

        // Drag the first tab (A) to the end.
        assert!(bar.reorder(0, 2));
        assert_eq!(bar.labels(), vec!["B", "C", "A"], "tab strip reordered");
        // The active scene (C) is preserved across the reorder.
        assert_eq!(bar.active_id(), Some(c));
        assert_eq!(bar.active_name(), Some("C"));

        // Drag C (now at index 1) to the front.
        assert!(bar.reorder(1, 0));
        assert_eq!(bar.labels(), vec!["C", "B", "A"]);
        assert_eq!(bar.active_id(), Some(c), "active selection still preserved");

        // Out-of-range indices are rejected and leave the strip unchanged.
        assert!(!bar.reorder(0, 9));
        assert!(!bar.reorder(9, 0));
        assert_eq!(bar.labels(), vec!["C", "B", "A"]);

        // A no-op reorder (same index) is allowed and changes nothing.
        assert!(bar.reorder(1, 1));
        assert_eq!(bar.labels(), vec!["C", "B", "A"]);
        assert_eq!(bar.active_id(), Some(c));
    }
}
