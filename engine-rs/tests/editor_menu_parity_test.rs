//! pat-vxq4y: Editor menu parity tests.
//!
//! Validates the top-level menu bar (Scene, Edit, Project, Debug, Editor, Help),
//! menu actions, shortcuts, enabled/disabled state, checkable items,
//! and global undo/redo surface integration.

use gdeditor::editor_menu::*;
use gdeditor::{Editor, EditorCommand};
use gdscene::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// Menu bar structure
// ===========================================================================

#[test]
fn menu_bar_has_six_standard_menus() {
    let bar = EditorMenuBar::new();
    assert_eq!(bar.menu_count(), 6);
}

#[test]
fn menu_titles_in_godot_order() {
    let bar = EditorMenuBar::new();
    let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Scene", "Edit", "Project", "Debug", "Editor", "Help"]);
}

#[test]
fn menus_accessible_by_title() {
    let bar = EditorMenuBar::new();
    for title in &["Scene", "Edit", "Project", "Debug", "Editor", "Help"] {
        assert!(
            bar.get_menu_by_title(title).is_some(),
            "menu '{title}' should exist"
        );
    }
}

// ===========================================================================
// Scene menu actions
// ===========================================================================

#[test]
fn scene_menu_new_open_save_close_quit() {
    let bar = EditorMenuBar::new();
    let scene = bar.get_menu_by_title("Scene").unwrap();
    for action in [
        MenuAction::SceneNew,
        MenuAction::SceneOpen,
        MenuAction::SceneSave,
        MenuAction::SceneSaveAs,
        MenuAction::SceneClose,
        MenuAction::SceneQuit,
    ] {
        assert!(
            scene.find_action(action).is_some(),
            "Scene menu should contain {action:?}"
        );
    }
}

#[test]
fn scene_menu_run_stop_actions() {
    let bar = EditorMenuBar::new();
    let scene = bar.get_menu_by_title("Scene").unwrap();
    assert!(scene.find_action(MenuAction::SceneRun).is_some());
    assert!(scene.find_action(MenuAction::SceneStop).is_some());
}

// ===========================================================================
// Scene menu shortcuts
// ===========================================================================

#[test]
fn scene_new_shortcut_ctrl_n() {
    let bar = EditorMenuBar::new();
    let sc = bar.shortcut_for(MenuAction::SceneNew).unwrap();
    assert!(sc.ctrl);
    assert_eq!(sc.key, "N");
}

#[test]
fn scene_save_shortcut_ctrl_s() {
    let bar = EditorMenuBar::new();
    let sc = bar.shortcut_for(MenuAction::SceneSave).unwrap();
    assert!(sc.ctrl);
    assert_eq!(sc.key, "S");
}

#[test]
fn scene_save_as_shortcut_ctrl_shift_s() {
    let bar = EditorMenuBar::new();
    let sc = bar.shortcut_for(MenuAction::SceneSaveAs).unwrap();
    assert!(sc.ctrl);
    assert!(sc.shift);
    assert_eq!(sc.key, "S");
}

#[test]
fn scene_run_shortcut_f5() {
    let bar = EditorMenuBar::new();
    let sc = bar.shortcut_for(MenuAction::SceneRun).unwrap();
    assert!(!sc.ctrl);
    assert_eq!(sc.key, "F5");
}

// ===========================================================================
// Project menu
// ===========================================================================

#[test]
fn project_menu_has_settings_export_reload() {
    let bar = EditorMenuBar::new();
    let project = bar.get_menu_by_title("Project").unwrap();
    assert!(project.find_action(MenuAction::ProjectSettings).is_some());
    assert!(project.find_action(MenuAction::ProjectExport).is_some());
    assert!(project.find_action(MenuAction::ProjectReloadCurrentProject).is_some());
}

// ===========================================================================
// Debug menu
// ===========================================================================

#[test]
fn debug_menu_has_run_and_visibility_toggles() {
    let bar = EditorMenuBar::new();
    let debug = bar.get_menu_by_title("Debug").unwrap();
    assert!(debug.find_action(MenuAction::DebugRunFile).is_some());
    assert!(debug.find_action(MenuAction::DebugNavigation).is_some());
    assert!(debug.find_action(MenuAction::DebugCollisionShapes).is_some());
}

#[test]
fn debug_visibility_items_are_checkable() {
    let bar = EditorMenuBar::new();
    let debug = bar.get_menu_by_title("Debug").unwrap();
    if let Some(MenuItem::Action { checked, .. }) = debug.find_action(MenuAction::DebugNavigation) {
        assert_eq!(*checked, Some(false), "should default to unchecked");
    } else {
        panic!("DebugNavigation not found or wrong variant");
    }
}

#[test]
fn debug_check_state_can_be_toggled() {
    let mut bar = EditorMenuBar::new();
    bar.set_action_checked(MenuAction::DebugNavigation, true);
    let debug = bar.get_menu_by_title("Debug").unwrap();
    if let Some(MenuItem::Action { checked, .. }) = debug.find_action(MenuAction::DebugNavigation) {
        assert_eq!(*checked, Some(true));
    }
}

// ===========================================================================
// Editor menu
// ===========================================================================

#[test]
fn editor_menu_has_settings_and_profiles() {
    let bar = EditorMenuBar::new();
    let editor = bar.get_menu_by_title("Editor").unwrap();
    assert!(editor.find_action(MenuAction::EditorSettings).is_some());
    assert!(editor.find_action(MenuAction::EditorFeatureProfile).is_some());
}

// ===========================================================================
// Help menu
// ===========================================================================

#[test]
fn help_menu_has_docs_and_about() {
    let bar = EditorMenuBar::new();
    let help = bar.get_menu_by_title("Help").unwrap();
    assert!(help.find_action(MenuAction::HelpDocs).is_some());
    assert!(help.find_action(MenuAction::HelpAbout).is_some());
    assert!(help.find_action(MenuAction::HelpBugTracker).is_some());
}

// ===========================================================================
// Enabled/disabled state
// ===========================================================================

#[test]
fn all_actions_enabled_by_default() {
    let bar = EditorMenuBar::new();
    for menu in bar.menus() {
        for item in &menu.items {
            if let MenuItem::Action { label, enabled, .. } = item {
                assert!(enabled, "action '{label}' should be enabled by default");
            }
        }
    }
}

#[test]
fn disable_save_when_no_scene_open() {
    let mut bar = EditorMenuBar::new();
    bar.set_action_enabled(MenuAction::SceneSave, false);
    bar.set_action_enabled(MenuAction::SceneSaveAs, false);
    let scene = bar.get_menu_by_title("Scene").unwrap();
    assert!(!scene.find_action(MenuAction::SceneSave).unwrap().is_enabled());
    assert!(!scene.find_action(MenuAction::SceneSaveAs).unwrap().is_enabled());
}

// ===========================================================================
// Open/close behavior
// ===========================================================================

#[test]
fn menu_bar_starts_closed() {
    let bar = EditorMenuBar::new();
    assert!(!bar.is_open());
    assert!(bar.open_menu_index().is_none());
}

#[test]
fn open_menu_switches_between_menus() {
    let mut bar = EditorMenuBar::new();
    bar.open_menu(0); // Scene
    assert_eq!(bar.open_menu_index(), Some(0));
    bar.open_menu(3); // Editor
    assert_eq!(bar.open_menu_index(), Some(3));
}

#[test]
fn close_menu_clears_state() {
    let mut bar = EditorMenuBar::new();
    bar.open_menu(1);
    bar.close_menu();
    assert!(!bar.is_open());
}

// ===========================================================================
// Global undo/redo integration
// ===========================================================================

#[test]
fn undo_redo_state_syncs_with_editor() {
    let tree = SceneTree::new();
    let mut editor = Editor::new(tree);
    let mut bar = EditorMenuBar::new();

    // Undo/Redo are now in the Edit menu by default.
    // Initially: nothing to undo or redo.
    sync_undo_redo_state(&mut bar, editor.undo_depth() > 0, editor.redo_depth() > 0);
    let edit = bar.get_menu_by_title("Edit").unwrap();
    assert!(!edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
    assert!(!edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());

    // Execute a command: undo becomes available.
    let root = editor.tree().root_id();
    editor.execute(EditorCommand::SetProperty {
        node_id: root,
        property: "name".to_string(),
        old_value: Variant::from("root"),
        new_value: Variant::from("Root"),
    }).unwrap();

    sync_undo_redo_state(&mut bar, editor.undo_depth() > 0, editor.redo_depth() > 0);
    let edit = bar.get_menu_by_title("Edit").unwrap();
    assert!(edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
    assert!(!edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());

    // Undo: redo becomes available.
    editor.undo().unwrap();
    sync_undo_redo_state(&mut bar, editor.undo_depth() > 0, editor.redo_depth() > 0);
    let edit = bar.get_menu_by_title("Edit").unwrap();
    assert!(!edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
    assert!(edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());
}

// ===========================================================================
// Custom menu extensibility
// ===========================================================================

#[test]
fn custom_menu_can_be_added() {
    let mut bar = EditorMenuBar::new();
    let custom = TopMenu::new("Plugins", vec![
        MenuItem::action(MenuAction::HelpDocs, "My Plugin Action"),
    ]);
    bar.add_menu(custom);
    assert_eq!(bar.menu_count(), 7);
    assert!(bar.get_menu_by_title("Plugins").is_some());
}

// ===========================================================================
// Action count
// ===========================================================================

#[test]
fn total_actions_at_least_30() {
    let bar = EditorMenuBar::new();
    let count = bar.total_action_count();
    assert!(count >= 30, "expected >=30 total actions, got {count}");
}

// ===========================================================================
// Edit menu parity
// ===========================================================================

#[test]
fn edit_menu_has_all_standard_actions() {
    let bar = EditorMenuBar::new();
    let edit = bar.get_menu_by_title("Edit").unwrap();
    for action in [
        MenuAction::EditUndo,
        MenuAction::EditRedo,
        MenuAction::EditCut,
        MenuAction::EditCopy,
        MenuAction::EditPaste,
        MenuAction::EditSelectAll,
        MenuAction::EditDelete,
        MenuAction::EditDuplicate,
    ] {
        assert!(
            edit.find_action(action).is_some(),
            "Edit menu should contain {action:?}"
        );
    }
}

#[test]
fn edit_menu_shortcuts_match_godot() {
    let bar = EditorMenuBar::new();
    // Undo: Ctrl+Z
    let sc = bar.shortcut_for(MenuAction::EditUndo).unwrap();
    assert!(sc.ctrl && !sc.shift && sc.key == "Z");
    // Redo: Ctrl+Shift+Z
    let sc = bar.shortcut_for(MenuAction::EditRedo).unwrap();
    assert!(sc.ctrl && sc.shift && sc.key == "Z");
    // Cut: Ctrl+X
    let sc = bar.shortcut_for(MenuAction::EditCut).unwrap();
    assert!(sc.ctrl && sc.key == "X");
    // Copy: Ctrl+C
    let sc = bar.shortcut_for(MenuAction::EditCopy).unwrap();
    assert!(sc.ctrl && sc.key == "C");
    // Paste: Ctrl+V
    let sc = bar.shortcut_for(MenuAction::EditPaste).unwrap();
    assert!(sc.ctrl && sc.key == "V");
    // Select All: Ctrl+A
    let sc = bar.shortcut_for(MenuAction::EditSelectAll).unwrap();
    assert!(sc.ctrl && sc.key == "A");
    // Duplicate: Ctrl+D
    let sc = bar.shortcut_for(MenuAction::EditDuplicate).unwrap();
    assert!(sc.ctrl && sc.key == "D");
}

#[test]
fn edit_menu_is_second_in_order() {
    let bar = EditorMenuBar::new();
    assert_eq!(bar.get_menu(1).unwrap().title, "Edit");
}

// ===========================================================================
// Action dispatch
// ===========================================================================

#[test]
fn action_dispatch_undo_redo() {
    let mut bar = EditorMenuBar::new();
    assert_eq!(bar.handle_action(MenuAction::EditUndo), MenuActionResult::Undo);
    assert_eq!(bar.handle_action(MenuAction::EditRedo), MenuActionResult::Redo);
}

#[test]
fn action_dispatch_closes_menu() {
    let mut bar = EditorMenuBar::new();
    bar.open_menu(0);
    assert!(bar.is_open());
    bar.handle_action(MenuAction::SceneNew);
    assert!(!bar.is_open(), "menu should close after handling an action");
}

#[test]
fn action_dispatch_debug_toggles() {
    let mut bar = EditorMenuBar::new();
    // First toggle: off → on
    let result = bar.handle_action(MenuAction::DebugNavigation);
    assert_eq!(result, MenuActionResult::ToggleDebugFlag(MenuAction::DebugNavigation, true));
    assert!(bar.is_action_checked(MenuAction::DebugNavigation));
    // Second toggle: on → off
    let result = bar.handle_action(MenuAction::DebugNavigation);
    assert_eq!(result, MenuActionResult::ToggleDebugFlag(MenuAction::DebugNavigation, false));
    assert!(!bar.is_action_checked(MenuAction::DebugNavigation));
}

#[test]
fn action_dispatch_scene_operations() {
    let mut bar = EditorMenuBar::new();
    assert_eq!(bar.handle_action(MenuAction::SceneNew), MenuActionResult::NewScene);
    assert_eq!(bar.handle_action(MenuAction::SceneSave), MenuActionResult::SaveScene);
    assert_eq!(bar.handle_action(MenuAction::SceneClose), MenuActionResult::CloseScene);
    assert_eq!(bar.handle_action(MenuAction::SceneRun), MenuActionResult::RunScene);
    assert_eq!(bar.handle_action(MenuAction::SceneStop), MenuActionResult::StopScene);
    assert_eq!(bar.handle_action(MenuAction::SceneQuit), MenuActionResult::QuitEditor);
}

#[test]
fn action_dispatch_project_operations() {
    let mut bar = EditorMenuBar::new();
    if let MenuActionResult::OpenDialog(name) = bar.handle_action(MenuAction::ProjectSettings) {
        assert_eq!(name, "Project Settings");
    } else {
        panic!("expected OpenDialog");
    }
    assert_eq!(bar.handle_action(MenuAction::ProjectReloadCurrentProject), MenuActionResult::ReloadProject);
}

#[test]
fn action_dispatch_help_opens_urls() {
    let mut bar = EditorMenuBar::new();
    if let MenuActionResult::OpenUrl(url) = bar.handle_action(MenuAction::HelpDocs) {
        assert!(url.starts_with("https://"));
    } else {
        panic!("expected OpenUrl");
    }
    if let MenuActionResult::OpenDialog(name) = bar.handle_action(MenuAction::HelpAbout) {
        assert_eq!(name, "About Patina Engine");
    } else {
        panic!("expected OpenDialog for About");
    }
}

#[test]
fn action_dispatch_edit_clipboard() {
    let mut bar = EditorMenuBar::new();
    assert_eq!(bar.handle_action(MenuAction::EditCut), MenuActionResult::Cut);
    assert_eq!(bar.handle_action(MenuAction::EditCopy), MenuActionResult::Copy);
    assert_eq!(bar.handle_action(MenuAction::EditPaste), MenuActionResult::Paste);
    assert_eq!(bar.handle_action(MenuAction::EditSelectAll), MenuActionResult::SelectAll);
    assert_eq!(bar.handle_action(MenuAction::EditDelete), MenuActionResult::DeleteSelected);
    assert_eq!(bar.handle_action(MenuAction::EditDuplicate), MenuActionResult::DuplicateSelected);
}

// ===========================================================================
// Undo/redo integration with action dispatch
// ===========================================================================

#[test]
fn undo_redo_dispatch_integrates_with_editor() {
    let tree = SceneTree::new();
    let mut editor = Editor::new(tree);
    let mut bar = EditorMenuBar::new();

    // Execute a command.
    let root = editor.tree().root_id();
    editor.execute(EditorCommand::SetProperty {
        node_id: root,
        property: "name".to_string(),
        old_value: Variant::from("root"),
        new_value: Variant::from("Root"),
    }).unwrap();

    // Dispatch undo action.
    let result = bar.handle_action(MenuAction::EditUndo);
    assert_eq!(result, MenuActionResult::Undo);

    // The caller would execute editor.undo() based on this result.
    editor.undo().unwrap();

    // Update menu state.
    sync_undo_redo_state(&mut bar, editor.undo_depth() > 0, editor.redo_depth() > 0);
    let edit = bar.get_menu_by_title("Edit").unwrap();
    assert!(!edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
    assert!(edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());

    // Dispatch redo action.
    let result = bar.handle_action(MenuAction::EditRedo);
    assert_eq!(result, MenuActionResult::Redo);
    editor.redo().unwrap();

    sync_undo_redo_state(&mut bar, editor.undo_depth() > 0, editor.redo_depth() > 0);
    let edit = bar.get_menu_by_title("Edit").unwrap();
    assert!(edit.find_action(MenuAction::EditUndo).unwrap().is_enabled());
    assert!(!edit.find_action(MenuAction::EditRedo).unwrap().is_enabled());
}
