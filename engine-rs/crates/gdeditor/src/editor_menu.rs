//! Editor menu bar system mirroring Godot 4's top-level menus.
//!
//! Provides the five standard menus (Scene, Project, Debug, Editor, Help)
//! with their actions, keyboard shortcuts, enabled/disabled states, and
//! global undo/redo integration.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Menu action IDs (matching Godot 4 editor menu items)
// ---------------------------------------------------------------------------

/// Identifies a specific menu action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    // -- Scene menu --
    SceneNew,
    SceneOpen,
    SceneOpenRecent,
    SceneSave,
    SceneSaveAs,
    SceneSaveAll,
    SceneClose,
    SceneCloseAll,
    SceneRun,
    SceneRunCustom,
    SceneStop,
    SceneQuit,

    // -- Project menu --
    ProjectSettings,
    ProjectExport,
    ProjectInstallAndroidBuildTemplate,
    ProjectOpenDataFolder,
    ProjectOpenUserFolder,
    ProjectReloadCurrentProject,

    // -- Debug menu --
    DebugRunFile,
    DebugRunSpecific,
    DebugDeployRemote,
    DebugVisible,
    DebugCollisionShapes,
    DebugNavigation,
    DebugShaderOverdraw,

    // -- Editor menu --
    EditorSettings,
    EditorLayout,
    EditorFeatureProfile,
    EditorManageExportTemplates,

    // -- Edit menu --
    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditSelectAll,
    EditDelete,
    EditDuplicate,

    // -- Help menu --
    HelpDocs,
    HelpQA,
    HelpBugTracker,
    HelpCommunity,
    HelpAbout,
}

// ---------------------------------------------------------------------------
// Shortcut
// ---------------------------------------------------------------------------

/// A keyboard shortcut for a menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Display text (e.g. "Ctrl+S", "Cmd+Z").
    pub display: String,
    /// Whether Ctrl (or Cmd on macOS) is required.
    pub ctrl: bool,
    /// Whether Shift is required.
    pub shift: bool,
    /// Whether Alt is required.
    pub alt: bool,
    /// The key name (e.g. "S", "Z", "F5").
    pub key: String,
}

impl Shortcut {
    pub fn new(display: &str, ctrl: bool, shift: bool, alt: bool, key: &str) -> Self {
        Self {
            display: display.to_string(),
            ctrl,
            shift,
            alt,
            key: key.to_string(),
        }
    }

    /// Creates a Ctrl+key shortcut.
    pub fn ctrl(key: &str) -> Self {
        Self::new(&format!("Ctrl+{key}"), true, false, false, key)
    }

    /// Creates a Ctrl+Shift+key shortcut.
    pub fn ctrl_shift(key: &str) -> Self {
        Self::new(&format!("Ctrl+Shift+{key}"), true, true, false, key)
    }

    /// Creates a function key shortcut.
    pub fn fkey(key: &str) -> Self {
        Self::new(key, false, false, false, key)
    }
}

// ---------------------------------------------------------------------------
// MenuItem
// ---------------------------------------------------------------------------

/// A single item in a menu dropdown.
#[derive(Debug, Clone)]
pub enum MenuItem {
    /// A clickable action.
    Action {
        action: MenuAction,
        label: String,
        shortcut: Option<Shortcut>,
        enabled: bool,
        checked: Option<bool>,
    },
    /// A visual separator line.
    Separator,
    /// A submenu group.
    Submenu { label: String, items: Vec<MenuItem> },
}

impl MenuItem {
    /// Creates an action menu item.
    pub fn action(action: MenuAction, label: &str) -> Self {
        Self::Action {
            action,
            label: label.to_string(),
            shortcut: None,
            enabled: true,
            checked: None,
        }
    }

    /// Creates an action with a keyboard shortcut.
    pub fn action_with_shortcut(action: MenuAction, label: &str, shortcut: Shortcut) -> Self {
        Self::Action {
            action,
            label: label.to_string(),
            shortcut: Some(shortcut),
            enabled: true,
            checked: None,
        }
    }

    /// Creates a checkable action item.
    pub fn checkable(action: MenuAction, label: &str, checked: bool) -> Self {
        Self::Action {
            action,
            label: label.to_string(),
            shortcut: None,
            enabled: true,
            checked: Some(checked),
        }
    }

    /// Returns the action ID if this is an action item.
    pub fn action_id(&self) -> Option<MenuAction> {
        match self {
            Self::Action { action, .. } => Some(*action),
            _ => None,
        }
    }

    /// Returns the label text.
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Action { label, .. } | Self::Submenu { label, .. } => Some(label),
            Self::Separator => None,
        }
    }

    /// Returns true if this item is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Action { enabled, .. } => *enabled,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// TopMenu
// ---------------------------------------------------------------------------

/// A top-level menu (e.g. "Scene", "Project").
#[derive(Debug, Clone)]
pub struct TopMenu {
    /// Menu title displayed in the menu bar.
    pub title: String,
    /// Items in the dropdown.
    pub items: Vec<MenuItem>,
}

impl TopMenu {
    pub fn new(title: &str, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.to_string(),
            items,
        }
    }

    /// Returns the number of action items (excluding separators and submenus).
    pub fn action_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, MenuItem::Action { .. }))
            .count()
    }

    /// Finds an item by action ID.
    pub fn find_action(&self, action: MenuAction) -> Option<&MenuItem> {
        self.items
            .iter()
            .find(|item| item.action_id() == Some(action))
    }

    /// Finds a mutable reference to an item by action ID.
    pub fn find_action_mut(&mut self, action: MenuAction) -> Option<&mut MenuItem> {
        self.items
            .iter_mut()
            .find(|item| item.action_id() == Some(action))
    }
}

// ---------------------------------------------------------------------------
// MenuActionResult
// ---------------------------------------------------------------------------

/// The result of handling a menu action, telling the caller what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuActionResult {
    /// Undo the last editor operation.
    Undo,
    /// Redo the last undone operation.
    Redo,
    /// Cut the current selection to clipboard.
    Cut,
    /// Copy the current selection to clipboard.
    Copy,
    /// Paste from clipboard.
    Paste,
    /// Select all nodes in the current scope.
    SelectAll,
    /// Delete the currently selected nodes.
    DeleteSelected,
    /// Duplicate the currently selected nodes.
    DuplicateSelected,
    /// Create a new empty scene.
    NewScene,
    /// Save the current scene.
    SaveScene,
    /// Save all open scenes.
    SaveAllScenes,
    /// Close the current scene.
    CloseScene,
    /// Close all open scenes.
    CloseAllScenes,
    /// Run the main scene (F5).
    RunScene,
    /// Run the current scene (F6).
    RunCurrentScene,
    /// Stop the running scene (F8).
    StopScene,
    /// Quit the editor.
    QuitEditor,
    /// Reload the current project.
    ReloadProject,
    /// Deploy with remote debug.
    DeployRemote,
    /// Toggle a debug visibility flag (action, new state).
    ToggleDebugFlag(MenuAction, bool),
    /// Open a dialog by name (e.g., "Project Settings", "Export").
    OpenDialog(&'static str),
    /// Open a submenu by name.
    OpenSubmenu(&'static str),
    /// Open a URL in the default browser.
    OpenUrl(&'static str),
    /// Open a system folder by key ("project_data", "user_data").
    OpenFolder(&'static str),
}

// ---------------------------------------------------------------------------
// EditorMenuBar
// ---------------------------------------------------------------------------

/// The full editor menu bar with all top-level menus.
#[derive(Debug, Clone)]
pub struct EditorMenuBar {
    /// Top-level menus in display order.
    menus: Vec<TopMenu>,
    /// Currently open menu index (None if all closed).
    open_menu: Option<usize>,
    /// Custom action handlers registered by name.
    custom_actions: HashMap<String, MenuAction>,
}

impl Default for EditorMenuBar {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorMenuBar {
    /// Creates the standard editor menu bar with all six menus.
    ///
    /// Menu order matches Godot 4: Scene, Edit, Project, Debug, Editor, Help.
    pub fn new() -> Self {
        Self {
            menus: vec![
                Self::build_scene_menu(),
                Self::build_edit_menu(),
                Self::build_project_menu(),
                Self::build_debug_menu(),
                Self::build_editor_menu(),
                Self::build_help_menu(),
            ],
            open_menu: None,
            custom_actions: HashMap::new(),
        }
    }

    /// Returns all top-level menus.
    pub fn menus(&self) -> &[TopMenu] {
        &self.menus
    }

    /// Returns the number of top-level menus.
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// Returns a menu by index.
    pub fn get_menu(&self, index: usize) -> Option<&TopMenu> {
        self.menus.get(index)
    }

    /// Returns a menu by title.
    pub fn get_menu_by_title(&self, title: &str) -> Option<&TopMenu> {
        self.menus.iter().find(|m| m.title == title)
    }

    /// Returns a mutable menu by title.
    pub fn get_menu_by_title_mut(&mut self, title: &str) -> Option<&mut TopMenu> {
        self.menus.iter_mut().find(|m| m.title == title)
    }

    /// Opens a menu by index. Closes any previously open menu.
    pub fn open_menu(&mut self, index: usize) {
        if index < self.menus.len() {
            self.open_menu = Some(index);
        }
    }

    /// Closes the currently open menu.
    pub fn close_menu(&mut self) {
        self.open_menu = None;
    }

    /// Returns the currently open menu index.
    pub fn open_menu_index(&self) -> Option<usize> {
        self.open_menu
    }

    /// Returns true if any menu is currently open.
    pub fn is_open(&self) -> bool {
        self.open_menu.is_some()
    }

    /// Sets the enabled state of a menu action across all menus.
    pub fn set_action_enabled(&mut self, action: MenuAction, enabled: bool) {
        for menu in &mut self.menus {
            if let Some(item) = menu.find_action_mut(action) {
                if let MenuItem::Action {
                    enabled: ref mut e, ..
                } = item
                {
                    *e = enabled;
                }
            }
        }
    }

    /// Sets the checked state of a checkable menu action.
    pub fn set_action_checked(&mut self, action: MenuAction, checked: bool) {
        for menu in &mut self.menus {
            if let Some(item) = menu.find_action_mut(action) {
                if let MenuItem::Action {
                    checked: ref mut c, ..
                } = item
                {
                    *c = Some(checked);
                }
            }
        }
    }

    /// Finds the shortcut for a given action.
    pub fn shortcut_for(&self, action: MenuAction) -> Option<&Shortcut> {
        for menu in &self.menus {
            if let Some(MenuItem::Action { shortcut, .. }) = menu.find_action(action) {
                return shortcut.as_ref();
            }
        }
        None
    }

    /// Returns the total number of action items across all menus.
    pub fn total_action_count(&self) -> usize {
        self.menus.iter().map(|m| m.action_count()).sum()
    }

    /// Adds a custom top-level menu.
    pub fn add_menu(&mut self, menu: TopMenu) {
        self.menus.push(menu);
    }

    /// Handles a triggered menu action, returning what the editor should do.
    ///
    /// The caller is responsible for executing the returned action result
    /// against the editor state (e.g., calling undo, opening a dialog, etc.).
    pub fn handle_action(&mut self, action: MenuAction) -> MenuActionResult {
        // Close the menu after any action is triggered.
        self.close_menu();

        // Toggle checkable items in-place.
        match action {
            MenuAction::DebugVisible
            | MenuAction::DebugCollisionShapes
            | MenuAction::DebugNavigation
            | MenuAction::DebugShaderOverdraw => {
                let current = self.is_action_checked(action);
                self.set_action_checked(action, !current);
                return MenuActionResult::ToggleDebugFlag(action, !current);
            }
            _ => {}
        }

        match action {
            // Edit actions
            MenuAction::EditUndo => MenuActionResult::Undo,
            MenuAction::EditRedo => MenuActionResult::Redo,
            MenuAction::EditCut => MenuActionResult::Cut,
            MenuAction::EditCopy => MenuActionResult::Copy,
            MenuAction::EditPaste => MenuActionResult::Paste,
            MenuAction::EditSelectAll => MenuActionResult::SelectAll,
            MenuAction::EditDelete => MenuActionResult::DeleteSelected,
            MenuAction::EditDuplicate => MenuActionResult::DuplicateSelected,

            // Scene actions
            MenuAction::SceneNew => MenuActionResult::NewScene,
            MenuAction::SceneOpen => MenuActionResult::OpenDialog("Open Scene"),
            MenuAction::SceneOpenRecent => MenuActionResult::OpenSubmenu("Recent Scenes"),
            MenuAction::SceneSave => MenuActionResult::SaveScene,
            MenuAction::SceneSaveAs => MenuActionResult::OpenDialog("Save Scene As"),
            MenuAction::SceneSaveAll => MenuActionResult::SaveAllScenes,
            MenuAction::SceneClose => MenuActionResult::CloseScene,
            MenuAction::SceneCloseAll => MenuActionResult::CloseAllScenes,
            MenuAction::SceneRun => MenuActionResult::RunScene,
            MenuAction::SceneRunCustom => MenuActionResult::OpenDialog("Run Custom Scene"),
            MenuAction::SceneStop => MenuActionResult::StopScene,
            MenuAction::SceneQuit => MenuActionResult::QuitEditor,

            // Project actions
            MenuAction::ProjectSettings => MenuActionResult::OpenDialog("Project Settings"),
            MenuAction::ProjectExport => MenuActionResult::OpenDialog("Export"),
            MenuAction::ProjectInstallAndroidBuildTemplate => {
                MenuActionResult::OpenDialog("Install Android Build Template")
            }
            MenuAction::ProjectOpenDataFolder => MenuActionResult::OpenFolder("project_data"),
            MenuAction::ProjectOpenUserFolder => MenuActionResult::OpenFolder("user_data"),
            MenuAction::ProjectReloadCurrentProject => MenuActionResult::ReloadProject,

            // Debug actions
            MenuAction::DebugRunFile => MenuActionResult::RunCurrentScene,
            MenuAction::DebugRunSpecific => MenuActionResult::OpenDialog("Run Specific Scene"),
            MenuAction::DebugDeployRemote => MenuActionResult::DeployRemote,

            // Editor actions
            MenuAction::EditorSettings => MenuActionResult::OpenDialog("Editor Settings"),
            MenuAction::EditorLayout => MenuActionResult::OpenSubmenu("Editor Layout"),
            MenuAction::EditorFeatureProfile => {
                MenuActionResult::OpenDialog("Manage Feature Profiles")
            }
            MenuAction::EditorManageExportTemplates => {
                MenuActionResult::OpenDialog("Manage Export Templates")
            }

            // Help actions
            MenuAction::HelpDocs => MenuActionResult::OpenUrl("https://docs.patinaengine.com"),
            MenuAction::HelpQA => MenuActionResult::OpenUrl("https://qa.patinaengine.com"),
            MenuAction::HelpBugTracker => {
                MenuActionResult::OpenUrl("https://github.com/patinaengine/patina/issues")
            }
            MenuAction::HelpCommunity => {
                MenuActionResult::OpenUrl("https://community.patinaengine.com")
            }
            MenuAction::HelpAbout => MenuActionResult::OpenDialog("About Patina Engine"),

            // Debug toggles already handled above.
            MenuAction::DebugVisible
            | MenuAction::DebugCollisionShapes
            | MenuAction::DebugNavigation
            | MenuAction::DebugShaderOverdraw => unreachable!(),
        }
    }

    /// Returns the checked state of a checkable action.
    pub fn is_action_checked(&self, action: MenuAction) -> bool {
        for menu in &self.menus {
            if let Some(MenuItem::Action {
                checked: Some(c), ..
            }) = menu.find_action(action)
            {
                return *c;
            }
        }
        false
    }

    // -- Standard menu builders ---------------------------------------------

    fn build_scene_menu() -> TopMenu {
        TopMenu::new(
            "Scene",
            vec![
                MenuItem::action_with_shortcut(
                    MenuAction::SceneNew,
                    "New Scene",
                    Shortcut::ctrl("N"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::SceneOpen,
                    "Open Scene...",
                    Shortcut::ctrl("O"),
                ),
                MenuItem::action(MenuAction::SceneOpenRecent, "Open Recent"),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(
                    MenuAction::SceneSave,
                    "Save Scene",
                    Shortcut::ctrl("S"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::SceneSaveAs,
                    "Save Scene As...",
                    Shortcut::ctrl_shift("S"),
                ),
                MenuItem::action(MenuAction::SceneSaveAll, "Save All Scenes"),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(
                    MenuAction::SceneClose,
                    "Close Scene",
                    Shortcut::ctrl("W"),
                ),
                MenuItem::action(MenuAction::SceneCloseAll, "Close All Scenes"),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(
                    MenuAction::SceneRun,
                    "Run Scene",
                    Shortcut::fkey("F5"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::SceneRunCustom,
                    "Run Custom Scene...",
                    Shortcut::ctrl_shift("F5"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::SceneStop,
                    "Stop Scene",
                    Shortcut::fkey("F8"),
                ),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(MenuAction::SceneQuit, "Quit", Shortcut::ctrl("Q")),
            ],
        )
    }

    fn build_edit_menu() -> TopMenu {
        TopMenu::new(
            "Edit",
            vec![
                MenuItem::action_with_shortcut(MenuAction::EditUndo, "Undo", Shortcut::ctrl("Z")),
                MenuItem::action_with_shortcut(
                    MenuAction::EditRedo,
                    "Redo",
                    Shortcut::ctrl_shift("Z"),
                ),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(MenuAction::EditCut, "Cut", Shortcut::ctrl("X")),
                MenuItem::action_with_shortcut(MenuAction::EditCopy, "Copy", Shortcut::ctrl("C")),
                MenuItem::action_with_shortcut(MenuAction::EditPaste, "Paste", Shortcut::ctrl("V")),
                MenuItem::Separator,
                MenuItem::action_with_shortcut(
                    MenuAction::EditSelectAll,
                    "Select All",
                    Shortcut::ctrl("A"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::EditDelete,
                    "Delete",
                    Shortcut::fkey("Delete"),
                ),
                MenuItem::action_with_shortcut(
                    MenuAction::EditDuplicate,
                    "Duplicate",
                    Shortcut::ctrl("D"),
                ),
            ],
        )
    }

    fn build_project_menu() -> TopMenu {
        TopMenu::new(
            "Project",
            vec![
                MenuItem::action(MenuAction::ProjectSettings, "Project Settings..."),
                MenuItem::Separator,
                MenuItem::action(MenuAction::ProjectExport, "Export..."),
                MenuItem::action(
                    MenuAction::ProjectInstallAndroidBuildTemplate,
                    "Install Android Build Template...",
                ),
                MenuItem::Separator,
                MenuItem::action(
                    MenuAction::ProjectOpenDataFolder,
                    "Open Project Data Folder",
                ),
                MenuItem::action(MenuAction::ProjectOpenUserFolder, "Open User Data Folder"),
                MenuItem::Separator,
                MenuItem::action(
                    MenuAction::ProjectReloadCurrentProject,
                    "Reload Current Project",
                ),
            ],
        )
    }

    fn build_debug_menu() -> TopMenu {
        TopMenu::new(
            "Debug",
            vec![
                MenuItem::action_with_shortcut(
                    MenuAction::DebugRunFile,
                    "Run Current Scene",
                    Shortcut::fkey("F6"),
                ),
                MenuItem::action(MenuAction::DebugRunSpecific, "Run Specific Scene..."),
                MenuItem::action(MenuAction::DebugDeployRemote, "Deploy with Remote Debug"),
                MenuItem::Separator,
                MenuItem::checkable(MenuAction::DebugVisible, "Visible Collision Shapes", false),
                MenuItem::checkable(
                    MenuAction::DebugCollisionShapes,
                    "Visible Collision Shapes (3D)",
                    false,
                ),
                MenuItem::checkable(MenuAction::DebugNavigation, "Visible Navigation", false),
                MenuItem::checkable(MenuAction::DebugShaderOverdraw, "Shader Overdraw", false),
            ],
        )
    }

    fn build_editor_menu() -> TopMenu {
        TopMenu::new(
            "Editor",
            vec![
                MenuItem::action(MenuAction::EditorSettings, "Editor Settings..."),
                MenuItem::action(MenuAction::EditorLayout, "Editor Layout"),
                MenuItem::action(
                    MenuAction::EditorFeatureProfile,
                    "Manage Feature Profiles...",
                ),
                MenuItem::Separator,
                MenuItem::action(
                    MenuAction::EditorManageExportTemplates,
                    "Manage Export Templates...",
                ),
            ],
        )
    }

    fn build_help_menu() -> TopMenu {
        TopMenu::new(
            "Help",
            vec![
                MenuItem::action(MenuAction::HelpDocs, "Online Documentation"),
                MenuItem::action(MenuAction::HelpQA, "Questions & Answers"),
                MenuItem::action(MenuAction::HelpBugTracker, "Report a Bug"),
                MenuItem::action(MenuAction::HelpCommunity, "Community"),
                MenuItem::Separator,
                MenuItem::action(MenuAction::HelpAbout, "About Patina Engine"),
            ],
        )
    }
}

// ---------------------------------------------------------------------------
// UndoRedoMenuState: helper to sync undo/redo menu items with editor state
// ---------------------------------------------------------------------------

/// Updates undo/redo menu items based on editor state.
pub fn sync_undo_redo_state(menu_bar: &mut EditorMenuBar, can_undo: bool, can_redo: bool) {
    menu_bar.set_action_enabled(MenuAction::EditUndo, can_undo);
    menu_bar.set_action_enabled(MenuAction::EditRedo, can_redo);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_has_six_top_level_menus() {
        let bar = EditorMenuBar::new();
        assert_eq!(bar.menu_count(), 6);
    }

    #[test]
    fn menu_titles_match_godot_order() {
        let bar = EditorMenuBar::new();
        let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Scene", "Edit", "Project", "Debug", "Editor", "Help"]
        );
    }

    #[test]
    fn scene_menu_has_expected_actions() {
        let bar = EditorMenuBar::new();
        let scene = bar.get_menu_by_title("Scene").unwrap();
        assert!(scene.find_action(MenuAction::SceneNew).is_some());
        assert!(scene.find_action(MenuAction::SceneOpen).is_some());
        assert!(scene.find_action(MenuAction::SceneSave).is_some());
        assert!(scene.find_action(MenuAction::SceneQuit).is_some());
    }

    #[test]
    fn scene_menu_shortcuts() {
        let bar = EditorMenuBar::new();
        let shortcut = bar.shortcut_for(MenuAction::SceneSave).unwrap();
        assert!(shortcut.ctrl);
        assert_eq!(shortcut.key, "S");
        assert_eq!(shortcut.display, "Ctrl+S");
    }

    #[test]
    fn project_menu_has_settings_and_export() {
        let bar = EditorMenuBar::new();
        let project = bar.get_menu_by_title("Project").unwrap();
        assert!(project.find_action(MenuAction::ProjectSettings).is_some());
        assert!(project.find_action(MenuAction::ProjectExport).is_some());
    }

    #[test]
    fn debug_menu_has_checkable_items() {
        let bar = EditorMenuBar::new();
        let debug = bar.get_menu_by_title("Debug").unwrap();
        if let Some(MenuItem::Action { checked, .. }) =
            debug.find_action(MenuAction::DebugNavigation)
        {
            assert_eq!(*checked, Some(false));
        } else {
            panic!("DebugNavigation not found");
        }
    }

    #[test]
    fn editor_menu_has_settings() {
        let bar = EditorMenuBar::new();
        let editor = bar.get_menu_by_title("Editor").unwrap();
        assert!(editor.find_action(MenuAction::EditorSettings).is_some());
    }

    #[test]
    fn help_menu_has_about() {
        let bar = EditorMenuBar::new();
        let help = bar.get_menu_by_title("Help").unwrap();
        assert!(help.find_action(MenuAction::HelpAbout).is_some());
    }

    #[test]
    fn total_action_count() {
        let bar = EditorMenuBar::new();
        // Scene: 12, Project: 6, Debug: 6, Editor: 4, Help: 5 = 33
        assert!(
            bar.total_action_count() >= 30,
            "expected >=30 actions, got {}",
            bar.total_action_count()
        );
    }

    #[test]
    fn open_close_menu() {
        let mut bar = EditorMenuBar::new();
        assert!(!bar.is_open());
        bar.open_menu(0);
        assert!(bar.is_open());
        assert_eq!(bar.open_menu_index(), Some(0));
        bar.open_menu(2);
        assert_eq!(bar.open_menu_index(), Some(2));
        bar.close_menu();
        assert!(!bar.is_open());
    }

    #[test]
    fn set_action_enabled() {
        let mut bar = EditorMenuBar::new();
        let scene = bar.get_menu_by_title("Scene").unwrap();
        assert!(scene
            .find_action(MenuAction::SceneSave)
            .unwrap()
            .is_enabled());

        bar.set_action_enabled(MenuAction::SceneSave, false);
        let scene = bar.get_menu_by_title("Scene").unwrap();
        assert!(!scene
            .find_action(MenuAction::SceneSave)
            .unwrap()
            .is_enabled());
    }

    #[test]
    fn set_action_checked() {
        let mut bar = EditorMenuBar::new();
        bar.set_action_checked(MenuAction::DebugNavigation, true);
        let debug = bar.get_menu_by_title("Debug").unwrap();
        if let Some(MenuItem::Action { checked, .. }) =
            debug.find_action(MenuAction::DebugNavigation)
        {
            assert_eq!(*checked, Some(true));
        } else {
            panic!("not found");
        }
    }

    #[test]
    fn custom_menu_can_be_added() {
        let mut bar = EditorMenuBar::new();
        let custom = TopMenu::new(
            "Custom",
            vec![MenuItem::action(MenuAction::HelpDocs, "Custom Action")],
        );
        bar.add_menu(custom);
        assert_eq!(bar.menu_count(), 7);
        assert!(bar.get_menu_by_title("Custom").is_some());
    }

    #[test]
    fn menu_item_label() {
        let item = MenuItem::action(MenuAction::SceneNew, "New Scene");
        assert_eq!(item.label(), Some("New Scene"));
        assert_eq!(MenuItem::Separator.label(), None);
    }

    #[test]
    fn sync_undo_redo_enables_disables() {
        let mut bar = EditorMenuBar::new();
        // Undo/Redo are now in the Edit menu by default.
        sync_undo_redo_state(&mut bar, false, false);
        let edit = bar.get_menu_by_title("Edit").unwrap();
        assert!(!edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
        assert!(!edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());

        sync_undo_redo_state(&mut bar, true, true);
        let edit = bar.get_menu_by_title("Edit").unwrap();
        assert!(edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
        assert!(edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());
    }

    #[test]
    fn edit_menu_has_undo_redo_and_clipboard() {
        let bar = EditorMenuBar::new();
        let edit = bar.get_menu_by_title("Edit").unwrap();
        assert!(edit.find_action(MenuAction::EditUndo).is_some());
        assert!(edit.find_action(MenuAction::EditRedo).is_some());
        assert!(edit.find_action(MenuAction::EditCut).is_some());
        assert!(edit.find_action(MenuAction::EditCopy).is_some());
        assert!(edit.find_action(MenuAction::EditPaste).is_some());
        assert!(edit.find_action(MenuAction::EditSelectAll).is_some());
        assert!(edit.find_action(MenuAction::EditDelete).is_some());
        assert!(edit.find_action(MenuAction::EditDuplicate).is_some());
    }

    #[test]
    fn edit_undo_shortcut_ctrl_z() {
        let bar = EditorMenuBar::new();
        let sc = bar.shortcut_for(MenuAction::EditUndo).unwrap();
        assert!(sc.ctrl);
        assert!(!sc.shift);
        assert_eq!(sc.key, "Z");
    }

    #[test]
    fn edit_redo_shortcut_ctrl_shift_z() {
        let bar = EditorMenuBar::new();
        let sc = bar.shortcut_for(MenuAction::EditRedo).unwrap();
        assert!(sc.ctrl);
        assert!(sc.shift);
        assert_eq!(sc.key, "Z");
    }

    #[test]
    fn handle_action_returns_undo() {
        let mut bar = EditorMenuBar::new();
        bar.open_menu(1); // Edit
        let result = bar.handle_action(MenuAction::EditUndo);
        assert_eq!(result, MenuActionResult::Undo);
        assert!(!bar.is_open(), "menu should close after action");
    }

    #[test]
    fn handle_action_debug_toggle() {
        let mut bar = EditorMenuBar::new();
        assert!(!bar.is_action_checked(MenuAction::DebugNavigation));
        let result = bar.handle_action(MenuAction::DebugNavigation);
        assert_eq!(
            result,
            MenuActionResult::ToggleDebugFlag(MenuAction::DebugNavigation, true)
        );
        assert!(bar.is_action_checked(MenuAction::DebugNavigation));
    }

    #[test]
    fn handle_action_scene_operations() {
        let mut bar = EditorMenuBar::new();
        assert_eq!(
            bar.handle_action(MenuAction::SceneNew),
            MenuActionResult::NewScene
        );
        assert_eq!(
            bar.handle_action(MenuAction::SceneSave),
            MenuActionResult::SaveScene
        );
        assert_eq!(
            bar.handle_action(MenuAction::SceneRun),
            MenuActionResult::RunScene
        );
        assert_eq!(
            bar.handle_action(MenuAction::SceneStop),
            MenuActionResult::StopScene
        );
        assert_eq!(
            bar.handle_action(MenuAction::SceneQuit),
            MenuActionResult::QuitEditor
        );
    }

    #[test]
    fn handle_action_edit_clipboard() {
        let mut bar = EditorMenuBar::new();
        assert_eq!(
            bar.handle_action(MenuAction::EditCut),
            MenuActionResult::Cut
        );
        assert_eq!(
            bar.handle_action(MenuAction::EditCopy),
            MenuActionResult::Copy
        );
        assert_eq!(
            bar.handle_action(MenuAction::EditPaste),
            MenuActionResult::Paste
        );
        assert_eq!(
            bar.handle_action(MenuAction::EditSelectAll),
            MenuActionResult::SelectAll
        );
        assert_eq!(
            bar.handle_action(MenuAction::EditDelete),
            MenuActionResult::DeleteSelected
        );
        assert_eq!(
            bar.handle_action(MenuAction::EditDuplicate),
            MenuActionResult::DuplicateSelected
        );
    }

    #[test]
    fn handle_action_help_opens_urls() {
        let mut bar = EditorMenuBar::new();
        if let MenuActionResult::OpenUrl(url) = bar.handle_action(MenuAction::HelpDocs) {
            assert!(url.starts_with("https://"));
        } else {
            panic!("HelpDocs should return OpenUrl");
        }
    }

    #[test]
    fn is_action_checked_default_false() {
        let bar = EditorMenuBar::new();
        assert!(!bar.is_action_checked(MenuAction::DebugVisible));
        assert!(!bar.is_action_checked(MenuAction::DebugNavigation));
    }

    #[test]
    fn shortcut_constructors() {
        let ctrl_s = Shortcut::ctrl("S");
        assert!(ctrl_s.ctrl);
        assert!(!ctrl_s.shift);
        assert_eq!(ctrl_s.display, "Ctrl+S");

        let ctrl_shift_s = Shortcut::ctrl_shift("S");
        assert!(ctrl_shift_s.ctrl);
        assert!(ctrl_shift_s.shift);

        let f5 = Shortcut::fkey("F5");
        assert!(!f5.ctrl);
        assert_eq!(f5.key, "F5");
    }
}
