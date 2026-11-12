//! Enable/disable (and check/uncheck) of menu items based on editor state.
//!
//! Menu items aren't always actionable: **Save** only makes sense when the
//! active scene has unsaved changes, **Revert** only when the scene has actually
//! been saved to disk before, and so on. This module derives each item's
//! enabled (and, where relevant, checked) state from a snapshot of the editor's
//! current state so the menu bar can grey out items that can't run.

/// A snapshot of the editor state that menu enablement depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorMenuState {
    /// Whether a scene is currently open/active.
    pub has_active_scene: bool,
    /// Whether the active scene has unsaved changes.
    pub active_scene_dirty: bool,
    /// Whether the active scene has ever been saved to disk (has a path).
    pub active_scene_has_path: bool,
    /// Whether there is anything on the undo stack.
    pub can_undo: bool,
    /// Whether there is anything on the redo stack.
    pub can_redo: bool,
}

/// A state-dependent menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    /// Save the active scene.
    Save,
    /// Save the active scene to a new path.
    SaveAs,
    /// Save every open scene.
    SaveAll,
    /// Reload the active scene from disk.
    Revert,
    /// Close the active scene.
    CloseScene,
    /// Undo the last action.
    Undo,
    /// Redo the last undone action.
    Redo,
}

/// Computes whether `item` is enabled for the given editor `state`.
///
/// - **Save** is enabled only when the active scene has unsaved changes.
/// - **Revert** is enabled only when the active scene has been saved before
///   (a never-saved scene has nothing on disk to revert to).
/// - **Save As / Save All / Close** require an active scene.
/// - **Undo / Redo** track the undo/redo stacks.
pub fn is_enabled(item: MenuItem, state: &EditorMenuState) -> bool {
    match item {
        MenuItem::Save => state.has_active_scene && state.active_scene_dirty,
        MenuItem::SaveAs => state.has_active_scene,
        MenuItem::SaveAll => state.has_active_scene,
        MenuItem::Revert => state.has_active_scene && state.active_scene_has_path,
        MenuItem::CloseScene => state.has_active_scene,
        MenuItem::Undo => state.can_undo,
        MenuItem::Redo => state.can_redo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-e9l5w): Save is disabled when the active scene has no
    /// changes and becomes enabled after an edit; Revert is disabled for a
    /// never-saved scene.
    #[test]
    fn menus_action_enablement_reflects_state() {
        // No scene open: nothing scene-related is actionable.
        let empty = EditorMenuState::default();
        assert!(!is_enabled(MenuItem::Save, &empty));
        assert!(!is_enabled(MenuItem::SaveAs, &empty));
        assert!(!is_enabled(MenuItem::Revert, &empty));
        assert!(!is_enabled(MenuItem::CloseScene, &empty));

        // A brand-new, never-saved scene with no edits yet.
        let mut state = EditorMenuState {
            has_active_scene: true,
            active_scene_dirty: false,
            active_scene_has_path: false,
            ..Default::default()
        };
        // Save is disabled with no unsaved changes.
        assert!(!is_enabled(MenuItem::Save, &state), "Save off when clean");
        // Revert is disabled for a never-saved scene.
        assert!(
            !is_enabled(MenuItem::Revert, &state),
            "Revert off for an unsaved scene"
        );
        // Save As and Close are available as soon as a scene is open.
        assert!(is_enabled(MenuItem::SaveAs, &state));
        assert!(is_enabled(MenuItem::CloseScene, &state));

        // After an edit, Save becomes enabled.
        state.active_scene_dirty = true;
        assert!(
            is_enabled(MenuItem::Save, &state),
            "Save on after an edit makes the scene dirty"
        );
        // Still never saved, so Revert stays disabled.
        assert!(!is_enabled(MenuItem::Revert, &state));

        // Once the scene has been saved to disk it has a path; Revert enables.
        state.active_scene_has_path = true;
        assert!(
            is_enabled(MenuItem::Revert, &state),
            "Revert on once the scene exists on disk"
        );

        // Saving clears the dirty flag, disabling Save again.
        state.active_scene_dirty = false;
        assert!(
            !is_enabled(MenuItem::Save, &state),
            "Save off again after saving"
        );
        // Revert remains available (the scene still has a path).
        assert!(is_enabled(MenuItem::Revert, &state));
    }

    /// Undo/Redo enablement tracks the undo and redo stacks.
    #[test]
    fn undo_redo_enablement_tracks_stacks() {
        let mut state = EditorMenuState {
            has_active_scene: true,
            ..Default::default()
        };
        assert!(!is_enabled(MenuItem::Undo, &state));
        assert!(!is_enabled(MenuItem::Redo, &state));

        state.can_undo = true;
        assert!(is_enabled(MenuItem::Undo, &state));
        assert!(!is_enabled(MenuItem::Redo, &state));

        state.can_redo = true;
        assert!(is_enabled(MenuItem::Redo, &state));
    }
}
