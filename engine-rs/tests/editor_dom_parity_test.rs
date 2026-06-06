//! pat-ml52v: Editor DOM structure parity tests (Layer 2).
//!
//! These tests verify that the editor HTML contains the expected DOM
//! elements, panel layout, IDs, and structural conventions that match
//! Godot's editor UI organization.
//!
//! Coverage:
//!  1. Core layout panels exist (menu-bar, toolbar, main, left-panel, center, inspector)
//!  2. Menu bar has expected menu items (Scene, Edit, View, Help)
//!  3. Toolbar buttons present (add, delete, undo, redo, save, load, play controls)
//!  4. Scene tree panel with search input
//!  5. Inspector panel structure
//!  6. Bottom panel with tabs (Output, Info, Script, Animation, Debugger)
//!  7. Context menu and dialog structures
//!  8. CSS custom properties for theming
//!  9. Play/pause/stop control group
//! 10. Node dock and signal connection dialog

use gdeditor::editor_ui::EDITOR_HTML;

// ===========================================================================
// 1. Core layout structure
// ===========================================================================

#[test]
fn editor_html_is_valid_document() {
    assert!(
        EDITOR_HTML.contains("<!DOCTYPE html>"),
        "must be an HTML5 document"
    );
    assert!(EDITOR_HTML.contains("<html"), "must have html element");
    assert!(EDITOR_HTML.contains("<head>"), "must have head element");
    assert!(EDITOR_HTML.contains("<body"), "must have body element");
    assert!(EDITOR_HTML.contains("</html>"), "must close html element");
}

#[test]
fn editor_has_title() {
    assert!(
        EDITOR_HTML.contains("<title>Patina Editor</title>"),
        "page title must be 'Patina Editor'"
    );
}

#[test]
fn core_layout_panels_exist() {
    let required_ids = [
        "menu-bar",
        "toolbar",
        "main",
        "left-panel",
        "center-area",
        "inspector-panel",
    ];
    for id in &required_ids {
        assert!(
            EDITOR_HTML.contains(&format!("id=\"{id}\"")),
            "must have element with id='{id}'"
        );
    }
}

#[test]
fn left_panel_has_scene_tree_and_filesystem() {
    assert!(
        EDITOR_HTML.contains("id=\"scene-panel\""),
        "left panel must have scene-panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"scene-tree\""),
        "must have scene-tree container"
    );
    assert!(
        EDITOR_HTML.contains("id=\"filesystem-panel\""),
        "left panel must have filesystem-panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"fs-tree\""),
        "must have filesystem tree container"
    );
}

#[test]
fn scene_tree_has_search_input() {
    assert!(
        EDITOR_HTML.contains("id=\"scene-search\""),
        "scene panel must have a search input"
    );
    assert!(
        EDITOR_HTML.contains("Filter nodes"),
        "search placeholder should indicate filtering"
    );
}

// ===========================================================================
// 2. Menu bar structure
// ===========================================================================

#[test]
fn menu_bar_has_brand() {
    assert!(
        EDITOR_HTML.contains("menu-bar-brand"),
        "menu bar must have a brand element"
    );
}

#[test]
fn menu_bar_has_core_menus() {
    // Godot has Scene, Project, Debug, Editor, Help menus
    let expected_menus = ["Scene", "Project", "Debug", "Editor", "Help"];
    for menu in &expected_menus {
        assert!(
            EDITOR_HTML.contains(&format!("data-menu=\"{}\"", menu.to_lowercase())),
            "menu bar must have '{menu}' menu with data-menu attribute"
        );
    }
}

#[test]
fn menu_bar_scene_menu_has_actions() {
    let scene_actions = [
        "scene-new",
        "scene-open",
        "scene-save",
        "scene-save-as",
        "scene-close",
        "scene-quit",
    ];
    for action in &scene_actions {
        assert!(
            EDITOR_HTML.contains(&format!("data-action=\"{action}\"")),
            "Scene menu must have action: {action}"
        );
    }
}

#[test]
fn menu_bar_project_menu_has_actions() {
    let project_actions = ["project-settings", "project-export", "project-refresh"];
    for action in &project_actions {
        assert!(
            EDITOR_HTML.contains(&format!("data-action=\"{action}\"")),
            "Project menu must have action: {action}"
        );
    }
}

#[test]
fn menu_bar_debug_menu_has_actions() {
    let debug_actions = [
        "debug-run",
        "debug-run-current",
        "debug-pause",
        "debug-stop",
        "debug-step",
        "debug-break",
    ];
    for action in &debug_actions {
        assert!(
            EDITOR_HTML.contains(&format!("data-action=\"{action}\"")),
            "Debug menu must have action: {action}"
        );
    }
}

#[test]
fn menu_bar_editor_menu_has_actions() {
    let editor_actions = [
        "editor-settings",
        "editor-layout-save",
        "editor-layout-default",
        "editor-toggle-fullscreen",
        "editor-toggle-console",
    ];
    for action in &editor_actions {
        assert!(
            EDITOR_HTML.contains(&format!("data-action=\"{action}\"")),
            "Editor menu must have action: {action}"
        );
    }
}

#[test]
fn menu_bar_help_menu_has_actions() {
    let help_actions = ["help-docs", "help-issues", "help-about"];
    for action in &help_actions {
        assert!(
            EDITOR_HTML.contains(&format!("data-action=\"{action}\"")),
            "Help menu must have action: {action}"
        );
    }
}

#[test]
fn menu_bar_has_keyboard_shortcuts() {
    // Key menu actions should display keyboard shortcuts
    let shortcuts = [
        "Ctrl+N", "Ctrl+O", "Ctrl+S", "Ctrl+Q", "F5", "F6", "F7", "F8", "F11",
    ];
    for shortcut in &shortcuts {
        assert!(
            EDITOR_HTML.contains(shortcut),
            "menu must display keyboard shortcut: {shortcut}"
        );
    }
}

#[test]
fn menu_bar_has_menu_dropdown_structure() {
    // Each menu item should have a dropdown with menu-action items
    assert!(
        EDITOR_HTML.contains("class=\"menu-dropdown\""),
        "menus must use menu-dropdown class"
    );
    assert!(
        EDITOR_HTML.contains("class=\"menu-action\""),
        "menu items must use menu-action class"
    );
    assert!(
        EDITOR_HTML.contains("class=\"menu-sep\""),
        "menus must have separator elements"
    );
}

#[test]
fn menu_bar_has_handle_menu_action_js() {
    assert!(
        EDITOR_HTML.contains("function handleMenuAction"),
        "must have handleMenuAction JavaScript function"
    );
    assert!(
        EDITOR_HTML.contains("function setupMenuBar"),
        "must have setupMenuBar JavaScript function"
    );
}

// ===========================================================================
// 3. Toolbar buttons
// ===========================================================================

#[test]
fn toolbar_has_node_management_buttons() {
    assert!(
        EDITOR_HTML.contains("id=\"btn-add\""),
        "toolbar must have Add button"
    );
    assert!(
        EDITOR_HTML.contains("id=\"btn-delete\""),
        "toolbar must have Delete button"
    );
}

#[test]
fn toolbar_has_undo_redo() {
    assert!(
        EDITOR_HTML.contains("id=\"btn-undo\""),
        "toolbar must have Undo button"
    );
    assert!(
        EDITOR_HTML.contains("id=\"btn-redo\""),
        "toolbar must have Redo button"
    );
}

#[test]
fn toolbar_has_save_load() {
    assert!(
        EDITOR_HTML.contains("id=\"btn-save\""),
        "toolbar must have Save button"
    );
    assert!(
        EDITOR_HTML.contains("id=\"btn-load\""),
        "toolbar must have Load button"
    );
}

#[test]
fn toolbar_has_settings_button() {
    assert!(
        EDITOR_HTML.contains("id=\"btn-settings\""),
        "toolbar must have Settings button"
    );
}

// ===========================================================================
// 4. Play controls (Godot parity: F5/F6/F7/F8)
// ===========================================================================

#[test]
fn play_controls_exist() {
    let play_ids = ["btn-play", "btn-pause", "btn-stop", "btn-play-current"];
    for id in &play_ids {
        assert!(
            EDITOR_HTML.contains(&format!("id=\"{id}\"")),
            "must have play control button: {id}"
        );
    }
}

#[test]
fn play_buttons_have_keyboard_shortcut_titles() {
    assert!(
        EDITOR_HTML.contains("title=\"Play (F5)\""),
        "Play button must show F5 shortcut"
    );
    assert!(
        EDITOR_HTML.contains("title=\"Pause (F7)\""),
        "Pause button must show F7 shortcut"
    );
    assert!(
        EDITOR_HTML.contains("title=\"Stop (F8)\""),
        "Stop button must show F8 shortcut"
    );
}

// ===========================================================================
// 5. Inspector panel
// ===========================================================================

#[test]
fn inspector_panel_has_content_area() {
    assert!(
        EDITOR_HTML.contains("id=\"inspector-content\""),
        "inspector panel must have content area"
    );
    assert!(
        EDITOR_HTML.contains("id=\"inspector\""),
        "must have inspector element"
    );
}

#[test]
fn inspector_has_node_dock_tab() {
    assert!(
        EDITOR_HTML.contains("id=\"node-dock-content\""),
        "must have node dock content area"
    );
    assert!(
        EDITOR_HTML.contains("id=\"node-dock\""),
        "must have node-dock element"
    );
}

// ===========================================================================
// 6. Bottom panel with tabs
// ===========================================================================

#[test]
fn bottom_panel_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"bottom-panel\""),
        "must have bottom panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"bottom-panel-header\""),
        "bottom panel must have header"
    );
    assert!(
        EDITOR_HTML.contains("id=\"bottom-panel-content\""),
        "bottom panel must have content area"
    );
}

#[test]
fn bottom_panel_has_output_tab() {
    assert!(
        EDITOR_HTML.contains("id=\"output-log\""),
        "bottom panel must have output log"
    );
}

#[test]
fn bottom_panel_has_script_tab() {
    assert!(
        EDITOR_HTML.contains("id=\"script-panel\""),
        "bottom panel must have script panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"script-tab-btn\""),
        "must have script tab button"
    );
}

#[test]
fn bottom_panel_has_animation_panel() {
    assert!(
        EDITOR_HTML.contains("id=\"animation-panel\""),
        "bottom panel must have animation panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"anim-timeline-canvas\""),
        "animation panel must have timeline canvas"
    );
}

#[test]
fn bottom_panel_has_debugger() {
    assert!(
        EDITOR_HTML.contains("id=\"debugger-panel\""),
        "bottom panel must have debugger panel"
    );
}

#[test]
fn debugger_panel_has_step_controls() {
    let controls = [
        "debug-btn-continue",
        "debug-btn-step-in",
        "debug-btn-step-over",
        "debug-btn-step-out",
    ];
    for id in &controls {
        assert!(
            EDITOR_HTML.contains(&format!("id=\"{id}\"")),
            "debugger panel must have step control: {id}"
        );
    }
}

#[test]
fn debugger_panel_has_status_indicator() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-status\""),
        "debugger panel must have status indicator"
    );
}

#[test]
fn debugger_panel_has_stack_frames() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-stack-frames\""),
        "debugger panel must have stack frames container"
    );
}

#[test]
fn debugger_panel_has_breakpoints_list() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-breakpoints-list\""),
        "debugger panel must have breakpoints list"
    );
}

#[test]
fn debugger_panel_has_variable_inspection() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-variables\""),
        "debugger panel must have locals variable inspector"
    );
    assert!(
        EDITOR_HTML.contains("id=\"debug-globals\""),
        "debugger panel must have globals variable inspector"
    );
}

#[test]
fn debugger_panel_has_toolbar() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-toolbar\""),
        "debugger panel must have toolbar"
    );
}

#[test]
fn debugger_panel_has_setup_js() {
    assert!(
        EDITOR_HTML.contains("function setupDebuggerPanel"),
        "must have setupDebuggerPanel JavaScript function"
    );
    assert!(
        EDITOR_HTML.contains("function fetchDebugData"),
        "must have fetchDebugData JavaScript function"
    );
    assert!(
        EDITOR_HTML.contains("function renderBreakpointList"),
        "must have renderBreakpointList JavaScript function"
    );
    assert!(
        EDITOR_HTML.contains("function renderVariables"),
        "must have renderVariables JavaScript function"
    );
}

#[test]
fn debugger_panel_has_two_column_layout() {
    assert!(
        EDITOR_HTML.contains("id=\"debug-left\""),
        "debugger panel must have left column"
    );
    assert!(
        EDITOR_HTML.contains("id=\"debug-right\""),
        "debugger panel must have right column"
    );
}

#[test]
fn bottom_panel_has_performance_monitors() {
    assert!(
        EDITOR_HTML.contains("id=\"monitors-panel\""),
        "bottom panel must have monitors panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"monitor-fps\""),
        "monitors must show FPS"
    );
    assert!(
        EDITOR_HTML.contains("id=\"profiler-panel\""),
        "bottom panel must have profiler panel"
    );
}

// ===========================================================================
// 7. Dialogs
// ===========================================================================

#[test]
fn add_node_dialog_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"add-node-dialog\""),
        "must have add-node dialog"
    );
    assert!(
        EDITOR_HTML.contains("id=\"add-node-search\""),
        "add-node dialog must have search input"
    );
    assert!(
        EDITOR_HTML.contains("id=\"add-node-list\""),
        "add-node dialog must have node list"
    );
    assert!(
        EDITOR_HTML.contains("id=\"add-node-create\""),
        "add-node dialog must have create button"
    );
}

#[test]
fn context_menu_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"context-menu\""),
        "must have context menu element"
    );
}

#[test]
fn help_dialog_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"help-dialog\""),
        "must have help dialog"
    );
}

#[test]
fn signal_connect_dialog_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"connect-dialog-overlay\""),
        "must have signal connection dialog overlay"
    );
    assert!(
        EDITOR_HTML.contains("id=\"connect-signal-name\""),
        "connect dialog must have signal name input"
    );
    assert!(
        EDITOR_HTML.contains("id=\"connect-method-name\""),
        "connect dialog must have method name input"
    );
}

// ===========================================================================
// 8. Theming (CSS custom properties)
// ===========================================================================

#[test]
fn css_has_theme_variables() {
    let required_vars = [
        "--bg",
        "--panel",
        "--border",
        "--text",
        "--text-dim",
        "--accent",
        "--selected",
        "--hover",
        "--error",
    ];
    for var in &required_vars {
        assert!(
            EDITOR_HTML.contains(var),
            "CSS must define custom property: {var}"
        );
    }
}

#[test]
fn css_has_node_icon_colors() {
    let icon_vars = [
        "--icon-node",
        "--icon-node2d",
        "--icon-sprite2d",
        "--icon-camera2d",
        "--icon-node3d",
    ];
    for var in &icon_vars {
        assert!(
            EDITOR_HTML.contains(var),
            "CSS must define node icon color: {var}"
        );
    }
}

#[test]
fn supports_light_theme() {
    assert!(
        EDITOR_HTML.contains("body.light"),
        "must have light theme CSS rules"
    );
}

// ===========================================================================
// 9. Viewport
// ===========================================================================

#[test]
fn viewport_panel_exists() {
    assert!(
        EDITOR_HTML.contains("id=\"viewport-panel\""),
        "must have viewport panel"
    );
    assert!(
        EDITOR_HTML.contains("id=\"viewport-container\""),
        "must have viewport container"
    );
}

#[test]
fn scene_tabs_exist() {
    assert!(
        EDITOR_HTML.contains("id=\"scene-tabs\""),
        "must have scene tabs container"
    );
}

// ===========================================================================
// 10. Animation controls (Godot parity)
// ===========================================================================

#[test]
fn animation_controls_exist() {
    let anim_ids = [
        "anim-select",
        "anim-new-btn",
        "anim-play-btn",
        "anim-stop-btn",
        "anim-record-btn",
        "anim-time-display",
        "anim-tracks",
        "anim-timeline",
        "anim-playhead",
        "anim-add-track-btn",
    ];
    for id in &anim_ids {
        assert!(
            EDITOR_HTML.contains(&format!("id=\"{id}\"")),
            "animation panel must have element: {id}"
        );
    }
}

#[test]
fn animation_has_blend_controls() {
    assert!(
        EDITOR_HTML.contains("id=\"anim-blend-toolbar\""),
        "animation must have blend toolbar"
    );
    assert!(
        EDITOR_HTML.contains("id=\"anim-blend-slider\""),
        "animation must have blend slider"
    );
}
