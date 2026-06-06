//! The Debug-menu toggles.
//!
//! The top-bar **Debug** menu exposes five checkable items — Visible Collision
//! Shapes, Visible Navigation, Visible Paths, Synchronize Scene Changes, and
//! Synchronize Script Changes. Each is a persisted checkable flag: toggling one
//! flips its checked state, the state survives across editor sessions (via
//! [`DebugMenuToggles::to_json`] / [`DebugMenuToggles::from_json`]), and the
//! enabled flags are handed to the next play session as launch arguments.

use serde::{Deserialize, Serialize};

/// An item in the Debug menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugToggle {
    /// Draw collision shapes while the game runs.
    VisibleCollisionShapes,
    /// Draw navigation meshes/polygons while the game runs.
    VisibleNavigation,
    /// Draw `Path2D`/`Path3D` curves while the game runs.
    VisiblePaths,
    /// Push scene edits to the running game live.
    SynchronizeSceneChanges,
    /// Push script edits to the running game live.
    SynchronizeScriptChanges,
}

/// The persisted checked-state of every Debug-menu toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DebugMenuToggles {
    /// Visible Collision Shapes is checked.
    pub visible_collision_shapes: bool,
    /// Visible Navigation is checked.
    pub visible_navigation: bool,
    /// Visible Paths is checked.
    pub visible_paths: bool,
    /// Synchronize Scene Changes is checked.
    pub synchronize_scene_changes: bool,
    /// Synchronize Script Changes is checked.
    pub synchronize_script_changes: bool,
}

impl DebugMenuToggles {
    /// Creates the toggle set with every item unchecked.
    pub fn new() -> Self {
        Self::default()
    }

    /// The Debug-menu items, in display order.
    pub fn items() -> [DebugToggle; 5] {
        [
            DebugToggle::VisibleCollisionShapes,
            DebugToggle::VisibleNavigation,
            DebugToggle::VisiblePaths,
            DebugToggle::SynchronizeSceneChanges,
            DebugToggle::SynchronizeScriptChanges,
        ]
    }

    /// Whether `item` is currently checked.
    pub fn is_checked(&self, item: DebugToggle) -> bool {
        match item {
            DebugToggle::VisibleCollisionShapes => self.visible_collision_shapes,
            DebugToggle::VisibleNavigation => self.visible_navigation,
            DebugToggle::VisiblePaths => self.visible_paths,
            DebugToggle::SynchronizeSceneChanges => self.synchronize_scene_changes,
            DebugToggle::SynchronizeScriptChanges => self.synchronize_script_changes,
        }
    }

    /// Flips `item`'s checked state and returns the new value.
    pub fn toggle(&mut self, item: DebugToggle) -> bool {
        let slot = match item {
            DebugToggle::VisibleCollisionShapes => &mut self.visible_collision_shapes,
            DebugToggle::VisibleNavigation => &mut self.visible_navigation,
            DebugToggle::VisiblePaths => &mut self.visible_paths,
            DebugToggle::SynchronizeSceneChanges => &mut self.synchronize_scene_changes,
            DebugToggle::SynchronizeScriptChanges => &mut self.synchronize_script_changes,
        };
        *slot = !*slot;
        *slot
    }

    /// The launch flag for a toggle, e.g. `--debug-collisions`. These are the
    /// arguments passed to the next play session for the checked items.
    pub fn flag(item: DebugToggle) -> &'static str {
        match item {
            DebugToggle::VisibleCollisionShapes => "--debug-collisions",
            DebugToggle::VisibleNavigation => "--debug-navigation",
            DebugToggle::VisiblePaths => "--debug-paths",
            DebugToggle::SynchronizeSceneChanges => "--debug-scene-sync",
            DebugToggle::SynchronizeScriptChanges => "--debug-script-sync",
        }
    }

    /// The launch flags applied to the next play session — one per checked
    /// item, in display order.
    pub fn play_session_flags(&self) -> Vec<&'static str> {
        Self::items()
            .into_iter()
            .filter(|&item| self.is_checked(item))
            .map(Self::flag)
            .collect()
    }

    /// Serializes the toggle state to JSON for persistence across sessions.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Restores the toggle state from JSON produced by [`Self::to_json`].
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-v3auw): toggling a Debug menu item flips its checked
    /// state, persists across a save/reload, and the flag is passed to the next
    /// play session.
    #[test]
    fn menus_debug_toggles_persist_and_apply() {
        let mut toggles = DebugMenuToggles::new();

        // The menu exposes all five debug items, in order.
        assert_eq!(
            DebugMenuToggles::items(),
            [
                DebugToggle::VisibleCollisionShapes,
                DebugToggle::VisibleNavigation,
                DebugToggle::VisiblePaths,
                DebugToggle::SynchronizeSceneChanges,
                DebugToggle::SynchronizeScriptChanges,
            ]
        );

        // Everything starts unchecked; a fresh play session gets no debug flags.
        assert!(!toggles.is_checked(DebugToggle::VisibleCollisionShapes));
        assert!(toggles.play_session_flags().is_empty());

        // Toggling an item flips it to checked and returns the new value.
        assert!(toggles.toggle(DebugToggle::VisibleCollisionShapes));
        assert!(toggles.is_checked(DebugToggle::VisibleCollisionShapes));
        toggles.toggle(DebugToggle::SynchronizeScriptChanges);
        assert!(toggles.is_checked(DebugToggle::SynchronizeScriptChanges));

        // The checked flags are handed to the next play session, in order.
        assert_eq!(
            toggles.play_session_flags(),
            vec!["--debug-collisions", "--debug-script-sync"]
        );

        // The state persists across a save / reload (e.g. a new editor session).
        let persisted = toggles.to_json();
        let reloaded =
            DebugMenuToggles::from_json(&persisted).expect("persisted toggles reload");
        assert_eq!(reloaded, toggles, "checked state survives persistence");
        assert!(reloaded.is_checked(DebugToggle::VisibleCollisionShapes));
        assert!(reloaded.is_checked(DebugToggle::SynchronizeScriptChanges));
        assert_eq!(
            reloaded.play_session_flags(),
            vec!["--debug-collisions", "--debug-script-sync"],
            "the reloaded flags still apply to the next play session"
        );

        // Toggling again flips back to unchecked and drops the flag.
        assert!(!toggles.toggle(DebugToggle::VisibleCollisionShapes));
        assert!(!toggles.is_checked(DebugToggle::VisibleCollisionShapes));
        assert_eq!(toggles.play_session_flags(), vec!["--debug-script-sync"]);
    }

    /// Every toggle has a distinct, stable launch flag.
    #[test]
    fn each_toggle_has_a_distinct_flag() {
        let flags: Vec<&str> = DebugMenuToggles::items()
            .into_iter()
            .map(DebugMenuToggles::flag)
            .collect();
        let mut unique = flags.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), flags.len(), "flags are distinct: {flags:?}");
    }
}
