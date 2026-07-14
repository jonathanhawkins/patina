//! Frontend-wiring regression guard for `Scene > Save Scene` / `Save Scene As`
//! (Godot `EditorNode::_save_scene` + `SCENE_SAVE_AS_SCENE`'s EditorFileDialog).
//!
//! Why this test exists: Save Scene As was wired to a browser `prompt()` — not a
//! real file dialog, and a native prompt also blocks browser automation. This
//! guards that Save As opens the in-editor file-save dialog, that both Save and
//! Save As refuse an empty (rootless) scene, and that the dialog is actually
//! wired to `/api/scene/save`.
//!
//! Inspects the static `EDITOR_HTML` constant directly (no server boot).

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

fn count(needle: &str) -> usize {
    HTML.matches(needle).count()
}

/// Body of a `function <name>(` declaration, up to a heuristic end. Good enough
/// to assert what a specific function does / does not call.
fn func_body(name: &str, len: usize) -> &'static str {
    let sig = format!("function {name}(");
    let start = HTML.find(&sig).unwrap_or_else(|| panic!("{name} is defined"));
    &HTML[start..start + len.min(HTML.len() - start)]
}

#[test]
fn save_as_opens_the_file_dialog_not_a_browser_prompt() {
    // The Save As dialog and its key controls exist.
    for id in [
        "id=\"save-scene-dialog\"",
        "id=\"save-scene-list\"",
        "id=\"save-scene-name\"",
        "id=\"save-scene-confirm\"",
        "id=\"save-scene-cancel\"",
    ] {
        assert!(HTML.contains(id), "the Save As dialog has {id}");
    }
    assert!(HTML.contains("Save Scene As..."), "the dialog is titled like Godot");

    // saveSceneAs opens the dialog and must NOT use a native prompt() anymore.
    let body = func_body("saveSceneAs", 900);
    assert!(
        body.contains("save-scene-dialog"),
        "saveSceneAs opens the file-save dialog: {body:?}"
    );
    assert!(
        !body.contains("prompt("),
        "saveSceneAs must not use a native prompt(): {body:?}"
    );

    // The dialog is initialized at startup.
    assert!(
        count("setupSaveSceneDialog()") >= 2,
        "setupSaveSceneDialog is defined and called from init"
    );
}

#[test]
fn save_dialog_confirm_posts_to_the_save_endpoint() {
    let body = func_body("doSaveSceneConfirm", 900);
    assert!(
        body.contains("/api/scene/save"),
        "the dialog's Save button posts to /api/scene/save: {body:?}"
    );
    // It must ensure a .tscn extension (Godot's save dialog enforces the filter).
    assert!(
        body.contains(".tscn"),
        "the save path is given a .tscn extension"
    );
    // A successful save refreshes the tab bar so the new filename shows.
    assert!(
        body.contains("fetchSceneTabs"),
        "a successful Save As refreshes the scene tabs"
    );
}

#[test]
fn empty_scene_cannot_be_saved() {
    // Godot: "A root node is required to save the scene." Both Save and Save As
    // must guard the empty case rather than writing a bare file.
    let save_as = func_body("saveSceneAs", 700);
    assert!(
        save_as.contains("sceneRootNode()") && save_as.contains("root node is required"),
        "Save As refuses an empty scene with the root-required message: {save_as:?}"
    );
    // btn-save short-circuits an empty scene into the Save As path (which shows
    // the message) instead of saving a rootless scene.
    assert!(
        HTML.contains("if (sceneIsEmpty()) { saveSceneAs(); return; }"),
        "Save (btn-save) guards the empty-scene case"
    );
}

#[test]
fn save_menu_items_are_wired() {
    // Save Scene -> the toolbar Save button; Save Scene As -> saveSceneAs().
    assert!(HTML.contains(r#"data-action="scene-save-as""#));
    let start = HTML.find("case 'scene-save-as':").expect("Save As menu case");
    let seg = &HTML[start..start + 120.min(HTML.len() - start)];
    assert!(
        seg.contains("saveSceneAs()"),
        "the Save Scene As menu item calls saveSceneAs(): {seg:?}"
    );
}
