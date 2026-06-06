//! Project-menu command dispatch, wired to the editor's project subsystems.
//!
//! Maps the top-bar **Project** menu items (Project Settings, Version Control,
//! Export, Reload Current Project, Quit to Project List) to the subsystems they
//! drive: Project Settings and Export raise their respective dialogs, Version
//! Control toggles the VCS panel, Reload Current Project reloads the project,
//! and Quit to Project List returns to the project manager.
//!
//! This is the action→command layer for the Project menu; it mirrors
//! [`crate::scene_menu_commands`] for the Scene menu and owns the dialog
//! subsystems it raises.

use crate::export_dialog::ExportDialog;
use crate::project_settings_dialog::ProjectSettingsDialog;

/// An item in the Project menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMenuAction {
    /// Open the Project Settings dialog.
    ProjectSettings,
    /// Toggle the Version Control panel.
    VersionControl,
    /// Open the Export dialog.
    Export,
    /// Reload the currently open project from disk.
    ReloadCurrentProject,
    /// Close the project and return to the project list.
    QuitToProjectList,
}

/// The outcome of dispatching a Project-menu action against the subsystems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectCommand {
    /// The Project Settings dialog was raised.
    OpenedProjectSettings,
    /// The Version Control panel was toggled to the given visibility.
    ToggledVersionControl(bool),
    /// The Export dialog was raised.
    OpenedExport,
    /// The current project was reloaded (carrying the new reload count).
    ReloadedProject(u32),
    /// The editor was asked to quit to the project list.
    QuitToProjectList,
}

/// The Project menu and the subsystems it drives. Owns the Project Settings and
/// Export dialogs so dispatching their menu items raises them directly.
#[derive(Debug, Default)]
pub struct ProjectMenu {
    settings: ProjectSettingsDialog,
    export: ExportDialog,
    version_control_visible: bool,
    reload_count: u32,
    quit_to_list: bool,
}

impl ProjectMenu {
    /// Creates a Project menu with both dialogs closed and nothing reloaded.
    pub fn new() -> Self {
        Self::default()
    }

    /// The Project-menu items, in display order.
    pub fn items() -> [ProjectMenuAction; 5] {
        [
            ProjectMenuAction::ProjectSettings,
            ProjectMenuAction::VersionControl,
            ProjectMenuAction::Export,
            ProjectMenuAction::ReloadCurrentProject,
            ProjectMenuAction::QuitToProjectList,
        ]
    }

    /// The Project Settings dialog subsystem.
    pub fn settings(&self) -> &ProjectSettingsDialog {
        &self.settings
    }

    /// The Export dialog subsystem.
    pub fn export(&self) -> &ExportDialog {
        &self.export
    }

    /// Whether the Version Control panel is currently shown.
    pub fn is_version_control_visible(&self) -> bool {
        self.version_control_visible
    }

    /// How many times the current project has been reloaded this session.
    pub fn reload_count(&self) -> u32 {
        self.reload_count
    }

    /// Whether Quit to Project List has been requested.
    pub fn quit_requested(&self) -> bool {
        self.quit_to_list
    }

    /// Dispatches a Project-menu action to its subsystem, applying the effect
    /// (raising a dialog, toggling the VCS panel, reloading, or quitting) and
    /// returning the resulting command.
    pub fn dispatch(&mut self, action: ProjectMenuAction) -> ProjectCommand {
        match action {
            ProjectMenuAction::ProjectSettings => {
                self.settings.open();
                ProjectCommand::OpenedProjectSettings
            }
            ProjectMenuAction::VersionControl => {
                self.version_control_visible = !self.version_control_visible;
                ProjectCommand::ToggledVersionControl(self.version_control_visible)
            }
            ProjectMenuAction::Export => {
                self.export.open();
                ProjectCommand::OpenedExport
            }
            ProjectMenuAction::ReloadCurrentProject => {
                self.reload_count += 1;
                ProjectCommand::ReloadedProject(self.reload_count)
            }
            ProjectMenuAction::QuitToProjectList => {
                self.quit_to_list = true;
                ProjectCommand::QuitToProjectList
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-stwms): opening Project Settings and Export from the
    /// Project menu raises their dialogs, and Reload Current Project triggers a
    /// project reload — plus Version Control toggles and Quit to Project List.
    #[test]
    fn menus_project_actions_open_subsystems() {
        let mut menu = ProjectMenu::new();

        // The menu exposes all five project items, in order.
        assert_eq!(
            ProjectMenu::items(),
            [
                ProjectMenuAction::ProjectSettings,
                ProjectMenuAction::VersionControl,
                ProjectMenuAction::Export,
                ProjectMenuAction::ReloadCurrentProject,
                ProjectMenuAction::QuitToProjectList,
            ]
        );

        // Both dialogs start closed and nothing has reloaded.
        assert!(!menu.settings().is_visible());
        assert!(!menu.export().is_visible());
        assert_eq!(menu.reload_count(), 0);

        // Project Settings raises the Project Settings dialog.
        assert_eq!(
            menu.dispatch(ProjectMenuAction::ProjectSettings),
            ProjectCommand::OpenedProjectSettings
        );
        assert!(
            menu.settings().is_visible(),
            "Project Settings menu item raised its dialog"
        );

        // Export raises the Export dialog.
        assert_eq!(
            menu.dispatch(ProjectMenuAction::Export),
            ProjectCommand::OpenedExport
        );
        assert!(
            menu.export().is_visible(),
            "Export menu item raised its dialog"
        );

        // Reload Current Project triggers a project reload.
        assert_eq!(
            menu.dispatch(ProjectMenuAction::ReloadCurrentProject),
            ProjectCommand::ReloadedProject(1)
        );
        assert_eq!(menu.reload_count(), 1, "Reload Current Project reloaded once");
        // Reloading again advances the count.
        assert_eq!(
            menu.dispatch(ProjectMenuAction::ReloadCurrentProject),
            ProjectCommand::ReloadedProject(2)
        );
        assert_eq!(menu.reload_count(), 2);

        // Version Control toggles the VCS panel on, then off.
        assert!(!menu.is_version_control_visible());
        assert_eq!(
            menu.dispatch(ProjectMenuAction::VersionControl),
            ProjectCommand::ToggledVersionControl(true)
        );
        assert!(menu.is_version_control_visible());
        assert_eq!(
            menu.dispatch(ProjectMenuAction::VersionControl),
            ProjectCommand::ToggledVersionControl(false)
        );
        assert!(!menu.is_version_control_visible());

        // Quit to Project List requests returning to the project manager.
        assert!(!menu.quit_requested());
        assert_eq!(
            menu.dispatch(ProjectMenuAction::QuitToProjectList),
            ProjectCommand::QuitToProjectList
        );
        assert!(menu.quit_requested(), "Quit to Project List was requested");
    }
}
