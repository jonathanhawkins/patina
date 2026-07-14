//! Frontend-wiring regression guard for `Scene > Close Scene` (Godot
//! `EditorNode::_scene_tab_closed`).
//!
//! Why this test exists — the bug it guards: the `Close Scene` menu item was
//! wired to `POST /api/scene/load {path:""}` (the same dead empty-path call that
//! broke `New Scene`), so it did nothing. The real endpoint is
//! `POST /api/scene/tabs/close`. This guards that the menu — and each tab's ×
//! button — route through the shared `closeSceneTab()` path that actually closes
//! the active tab, prompts before discarding unsaved changes, and refreshes the
//! displayed scene (closing the active tab changes which scene is shown).
//!
//! Inspects the static `EDITOR_HTML` constant directly (no server boot).

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

fn count(needle: &str) -> usize {
    HTML.matches(needle).count()
}

/// The body of the `scene-close` menu case, up to the next `break;`.
fn close_case_body() -> &'static str {
    let start = HTML
        .find("case 'scene-close':")
        .expect("the Scene menu has a Close Scene action");
    let rest = &HTML[start..];
    let end = rest.find("break;").expect("the case ends with break;");
    &rest[..end]
}

#[test]
fn close_scene_menu_is_wired_to_the_close_endpoint() {
    let body = close_case_body();
    assert!(
        body.contains("closeCurrentScene()"),
        "Close Scene must call closeCurrentScene(); got: {body:?}"
    );
    // The old bug wired Close Scene to /api/scene/load (empty path -> no-op).
    assert!(
        !body.contains("/api/scene/load"),
        "Close Scene must NOT call /api/scene/load; got: {body:?}"
    );

    assert!(
        HTML.contains("function closeCurrentScene"),
        "closeCurrentScene() is defined"
    );
    assert!(
        HTML.contains("function closeSceneTab"),
        "the shared closeSceneTab() path is defined"
    );
    assert!(
        HTML.contains("'/api/scene/tabs/close'") || HTML.contains("\"/api/scene/tabs/close\""),
        "the close path posts to /api/scene/tabs/close (Godot _scene_tab_closed)"
    );
}

#[test]
fn closing_the_active_tab_refreshes_the_displayed_scene() {
    // Closing the active tab changes which scene is shown, so the close path
    // must refresh the scene tree, not just the tab bar.
    let start = HTML.find("function closeSceneTab").unwrap();
    let func = &HTML[start..start + 700.min(HTML.len() - start)];
    assert!(
        func.contains("fetchScene()"),
        "closeSceneTab() refreshes the scene tree after closing"
    );
    assert!(
        func.contains("fetchSceneTabs"),
        "closeSceneTab() also refreshes the tab bar"
    );
}

#[test]
fn unsaved_scene_prompts_before_closing() {
    // Godot pops a save/discard confirmation for an unsaved scene before closing;
    // a clean scene closes immediately. closeSceneTab() branches on `modified`.
    let start = HTML.find("function closeSceneTab").unwrap();
    let func = &HTML[start..start + 900.min(HTML.len() - start)];
    assert!(
        func.contains("modified") && func.contains("showConfirm"),
        "closeSceneTab() confirms before discarding a modified scene"
    );
}

#[test]
fn tab_close_button_no_longer_refuses_the_last_tab() {
    // Godot lets you close the last tab (it opens a fresh empty scene). The old
    // UI bailed out with `if (sceneTabs.length <= 1) return;` on the × button.
    assert_eq!(
        count("sceneTabs.length <= 1) return"),
        0,
        "the tab × button must not early-return on the last tab anymore"
    );
    // Both entry points funnel through the shared helper.
    assert!(
        count("closeSceneTab(") >= 2,
        "both the menu and the tab × button call the shared closeSceneTab()"
    );
}
