//! Editor-menu command dispatch.
//!
//! Maps the top-bar **Editor** menu items (Editor Settings, Command Palette,
//! Editor Layout, Toggle Fullscreen, Manage Editor Features) to the
//! editor-level commands they run, and owns the window fullscreen state that
//! Toggle Fullscreen flips.
//!
//! This is the action→command layer for the Editor menu; the visual menu bar
//! lives in [`crate::editor_menu`].

/// An item in the Editor menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuAction {
    /// Open the Editor Settings dialog.
    EditorSettings,
    /// Open the command palette.
    CommandPalette,
    /// Open the editor layout manager.
    EditorLayout,
    /// Toggle the window's fullscreen state.
    ToggleFullscreen,
    /// Open the Manage Editor Features dialog.
    ManageEditorFeatures,
}

/// An editor-level command produced by dispatching an Editor-menu action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    /// Open the Editor Settings dialog.
    OpenEditorSettings,
    /// Open the command palette.
    OpenCommandPalette,
    /// Open the editor layout manager.
    OpenEditorLayout,
    /// Set the window fullscreen state to the given value.
    SetFullscreen(bool),
    /// Open the Manage Editor Features dialog.
    OpenManageFeatures,
}

/// The Editor-menu command dispatcher, owning the fullscreen flag.
#[derive(Debug, Clone, Default)]
pub struct EditorMenuCommands {
    fullscreen: bool,
}

impl EditorMenuCommands {
    /// Creates the dispatcher (windowed by default).
    pub fn new() -> Self {
        Self::default()
    }

    /// The Editor-menu items, in display order.
    pub fn items() -> [EditorMenuAction; 5] {
        [
            EditorMenuAction::EditorSettings,
            EditorMenuAction::CommandPalette,
            EditorMenuAction::EditorLayout,
            EditorMenuAction::ToggleFullscreen,
            EditorMenuAction::ManageEditorFeatures,
        ]
    }

    /// Whether the window is currently fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Dispatches an Editor-menu action to its editor command, applying any
    /// state change (Toggle Fullscreen flips the fullscreen flag).
    pub fn dispatch(&mut self, action: EditorMenuAction) -> EditorCommand {
        match action {
            EditorMenuAction::EditorSettings => EditorCommand::OpenEditorSettings,
            EditorMenuAction::CommandPalette => EditorCommand::OpenCommandPalette,
            EditorMenuAction::EditorLayout => EditorCommand::OpenEditorLayout,
            EditorMenuAction::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                EditorCommand::SetFullscreen(self.fullscreen)
            }
            EditorMenuAction::ManageEditorFeatures => EditorCommand::OpenManageFeatures,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-bx0oi): Editor Settings and Command Palette open from the
    /// Editor menu, and Toggle Fullscreen flips the window state.
    #[test]
    fn menus_editor_actions_dispatch() {
        let mut menu = EditorMenuCommands::new();

        // The menu exposes all five editor items, in order.
        assert_eq!(
            EditorMenuCommands::items(),
            [
                EditorMenuAction::EditorSettings,
                EditorMenuAction::CommandPalette,
                EditorMenuAction::EditorLayout,
                EditorMenuAction::ToggleFullscreen,
                EditorMenuAction::ManageEditorFeatures,
            ]
        );

        // Editor Settings and Command Palette open from the menu.
        assert_eq!(
            menu.dispatch(EditorMenuAction::EditorSettings),
            EditorCommand::OpenEditorSettings
        );
        assert_eq!(
            menu.dispatch(EditorMenuAction::CommandPalette),
            EditorCommand::OpenCommandPalette
        );

        // Toggle Fullscreen flips the window state on each invocation.
        assert!(!menu.is_fullscreen());
        assert_eq!(
            menu.dispatch(EditorMenuAction::ToggleFullscreen),
            EditorCommand::SetFullscreen(true)
        );
        assert!(menu.is_fullscreen(), "toggle enters fullscreen");
        assert_eq!(
            menu.dispatch(EditorMenuAction::ToggleFullscreen),
            EditorCommand::SetFullscreen(false)
        );
        assert!(!menu.is_fullscreen(), "toggle exits fullscreen");

        // The remaining items dispatch to their editor commands.
        assert_eq!(
            menu.dispatch(EditorMenuAction::EditorLayout),
            EditorCommand::OpenEditorLayout
        );
        assert_eq!(
            menu.dispatch(EditorMenuAction::ManageEditorFeatures),
            EditorCommand::OpenManageFeatures
        );
    }
}
