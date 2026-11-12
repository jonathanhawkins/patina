//! The editor's **bottom panel bar**.
//!
//! Along the bottom of the editor sits a row of buttons for the registered
//! bottom panels — Output, Debugger, Audio, Shader, and so on. The bar behaves
//! like a single-selection toggle group wired to a collapsible dock:
//!
//! - Clicking a panel button **shows that panel and hides the others** (only one
//!   panel is ever active).
//! - Clicking the **already-active** button **collapses the dock** — no panel is
//!   shown and the dock takes no vertical space.
//! - An **expand-to-fill** toggle makes the active panel fill the editor's full
//!   height; collapsing the dock also clears the expanded state.

/// A registered bottom panel and the button that selects it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BottomPanel {
    /// Stable identifier used to select the panel (e.g. `"output"`).
    pub id: String,
    /// Human-readable button label (e.g. `"Output"`).
    pub label: String,
}

/// The bottom panel bar: the registered panels plus which one (if any) is shown
/// and whether the dock is expanded to fill the editor.
#[derive(Debug, Clone, Default)]
pub struct BottomPanelBar {
    panels: Vec<BottomPanel>,
    /// Index into `panels` of the active panel, or `None` when the dock is
    /// collapsed.
    active: Option<usize>,
    /// Whether the active panel is expanded to fill the editor height.
    expanded: bool,
}

/// The default height (in pixels) of the dock when a panel is shown but not
/// expanded to fill.
pub const DEFAULT_DOCK_HEIGHT: f32 = 250.0;

impl BottomPanelBar {
    /// Creates an empty bar with the dock collapsed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a panel button. Returns the index of the registered panel.
    /// Re-registering an existing `id` updates its label and keeps its slot.
    pub fn register(&mut self, id: impl Into<String>, label: impl Into<String>) -> usize {
        let id = id.into();
        if let Some(idx) = self.panels.iter().position(|p| p.id == id) {
            self.panels[idx].label = label.into();
            idx
        } else {
            self.panels.push(BottomPanel {
                id,
                label: label.into(),
            });
            self.panels.len() - 1
        }
    }

    /// The registered panels, in registration order.
    pub fn panels(&self) -> &[BottomPanel] {
        &self.panels
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.panels.iter().position(|p| p.id == id)
    }

    /// Clicks the panel button identified by `id`.
    ///
    /// - If `id` is not the active panel, it becomes active (showing it and
    ///   hiding any other), and the result is `true` (a panel is shown).
    /// - If `id` is already the active panel, the dock collapses (and the
    ///   expanded state is cleared), and the result is `false` (collapsed).
    /// - If `id` is not registered, nothing changes and the result is the
    ///   current visibility of any active panel.
    pub fn click(&mut self, id: &str) -> bool {
        match self.index_of(id) {
            None => self.active.is_some(),
            Some(idx) if self.active == Some(idx) => {
                // Clicking the active panel collapses the dock.
                self.active = None;
                self.expanded = false;
                false
            }
            Some(idx) => {
                self.active = Some(idx);
                true
            }
        }
    }

    /// The id of the currently shown panel, or `None` when collapsed.
    pub fn active_id(&self) -> Option<&str> {
        self.active.map(|idx| self.panels[idx].id.as_str())
    }

    /// Whether the dock is collapsed (no panel shown).
    pub fn is_collapsed(&self) -> bool {
        self.active.is_none()
    }

    /// Whether the panel identified by `id` is the one currently shown.
    pub fn is_panel_visible(&self, id: &str) -> bool {
        match self.active {
            Some(idx) => self.panels[idx].id == id,
            None => false,
        }
    }

    /// Sets whether the active panel is expanded to fill the editor height.
    /// Expanding has no effect while the dock is collapsed.
    pub fn set_expanded(&mut self, expanded: bool) {
        if self.active.is_some() {
            self.expanded = expanded;
        }
    }

    /// Toggles the expand-to-fill state of the active panel. Returns the new
    /// expanded state. No-op (returns `false`) while collapsed.
    pub fn toggle_expand(&mut self) -> bool {
        if self.active.is_none() {
            return false;
        }
        self.expanded = !self.expanded;
        self.expanded
    }

    /// Whether the active panel is expanded to fill the editor.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// The height the dock occupies given the editor's total height.
    ///
    /// - Collapsed: `0.0` (the dock takes no space).
    /// - Expanded: the full `editor_height`.
    /// - Shown but not expanded: [`DEFAULT_DOCK_HEIGHT`].
    pub fn dock_height(&self, editor_height: f32) -> f32 {
        if self.active.is_none() {
            0.0
        } else if self.expanded {
            editor_height
        } else {
            DEFAULT_DOCK_HEIGHT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> BottomPanelBar {
        let mut bar = BottomPanelBar::new();
        bar.register("output", "Output");
        bar.register("debugger", "Debugger");
        bar.register("audio", "Audio");
        bar.register("shader", "Shader");
        bar
    }

    /// Acceptance (pat-2e2s0): clicking a panel button shows that panel and
    /// hides the others, clicking the active button collapses the dock, and
    /// expanding fills the editor height.
    #[test]
    fn bottom_panel_bar_toggles_active_panel() {
        let editor_height = 1000.0;
        let mut bar = bar();

        // All registered panels are listed, in order.
        let ids: Vec<&str> = bar.panels().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["output", "debugger", "audio", "shader"]);

        // Starts collapsed: nothing visible, the dock takes no space.
        assert!(bar.is_collapsed());
        assert!(bar.active_id().is_none());
        assert_eq!(bar.dock_height(editor_height), 0.0);

        // Clicking a panel shows it and hides the others.
        assert!(bar.click("output"));
        assert!(!bar.is_collapsed());
        assert_eq!(bar.active_id(), Some("output"));
        assert!(bar.is_panel_visible("output"));
        assert!(!bar.is_panel_visible("debugger"));
        assert_eq!(bar.dock_height(editor_height), DEFAULT_DOCK_HEIGHT);

        // Clicking a different panel switches the active one (others hidden).
        assert!(bar.click("debugger"));
        assert_eq!(bar.active_id(), Some("debugger"));
        assert!(bar.is_panel_visible("debugger"));
        assert!(!bar.is_panel_visible("output"));

        // Clicking the active button collapses the dock.
        assert!(!bar.click("debugger"));
        assert!(bar.is_collapsed());
        assert!(bar.active_id().is_none());
        assert!(!bar.is_panel_visible("debugger"));
        assert_eq!(bar.dock_height(editor_height), 0.0);

        // Show a panel and expand it to fill the editor height.
        assert!(bar.click("audio"));
        assert!(!bar.is_expanded());
        assert_eq!(bar.dock_height(editor_height), DEFAULT_DOCK_HEIGHT);
        assert!(bar.toggle_expand(), "expand turns on");
        assert!(bar.is_expanded());
        assert_eq!(
            bar.dock_height(editor_height),
            editor_height,
            "expanded dock fills the editor height"
        );

        // Toggling expand off returns to the default dock height.
        assert!(!bar.toggle_expand(), "expand turns off");
        assert!(!bar.is_expanded());
        assert_eq!(bar.dock_height(editor_height), DEFAULT_DOCK_HEIGHT);

        // Collapsing the dock also clears the expanded state.
        bar.set_expanded(true);
        assert!(bar.is_expanded());
        assert!(!bar.click("audio"), "clicking active collapses");
        assert!(bar.is_collapsed());
        assert!(!bar.is_expanded(), "collapse clears expanded");
        assert_eq!(bar.dock_height(editor_height), 0.0);
    }

    /// Clicking an unregistered id does nothing; expand is a no-op while
    /// collapsed.
    #[test]
    fn unknown_panel_and_expand_while_collapsed_are_noops() {
        let mut bar = bar();

        // Unknown id while collapsed: stays collapsed.
        assert!(!bar.click("does-not-exist"));
        assert!(bar.is_collapsed());

        // Expanding while collapsed has no effect.
        bar.set_expanded(true);
        assert!(!bar.is_expanded());
        assert!(!bar.toggle_expand());

        // With a panel shown, an unknown id leaves the active panel intact.
        assert!(bar.click("shader"));
        assert!(bar.click("does-not-exist"));
        assert_eq!(bar.active_id(), Some("shader"));
    }
}
