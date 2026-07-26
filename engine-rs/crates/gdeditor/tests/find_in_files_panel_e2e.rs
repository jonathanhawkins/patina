//! pat-0q2kx.168 — Find in Files: grouped results, Replace in Files, and the
//! results panel markup.
//!
//! Godot's Ctrl+Shift+F opens a dialog (term, folder, extensions, case) and
//! streams results into a bottom panel grouped per file, where each entry opens
//! the file at its line; Replace in Files applies a replacement across matches.
//!
//! Patina had `prompt()` -> `GET /api/search` -> a static text popup. The
//! endpoint returned a flat `{file, line, text}` list with no column and no
//! grouping, hardcoded `.gd`, and matched with a case-sensitive `contains`.
//! There was no replace endpoint at all.

use gdeditor::editor_ui::EDITOR_HTML;
use gdeditor::find_in_files::{find_in_files, SearchOptions};

/// The search core the endpoint delegates to: grouped per file, with columns.
#[test]
fn find_in_files_panel_e2e() {
    // Three fixture "files", two of which contain the term.
    let a = "func ready():\n    var speed = 10\n    return speed\n";
    let b = "# speed tuning\nconst SPEED = 5\n";
    let c = "func idle():\n    pass\n";
    let files = vec![("res://a.gd", a), ("res://b.gd", b), ("res://c.gd", c)];

    let groups = find_in_files(
        &files,
        "speed",
        SearchOptions {
            case_sensitive: true,
        },
    );

    // Grouped per file, and a file with no match is omitted entirely.
    assert_eq!(
        groups.len(),
        2,
        "only the two matching files group: {groups:?}"
    );
    assert_eq!(groups[0].path, "res://a.gd");
    assert_eq!(groups[1].path, "res://b.gd");

    // Line numbers are 1-based and columns point at the match.
    assert_eq!(groups[0].matches.len(), 2, "two `speed` hits in a.gd");
    assert_eq!(groups[0].matches[0].line, 2);
    assert!(
        groups[0].matches[0].column >= 1,
        "columns are 1-based, got {}",
        groups[0].matches[0].column,
    );
    assert!(
        groups[0].matches[0].context.contains("var speed"),
        "the full line text comes back for display, got {:?}",
        groups[0].matches[0].context,
    );

    // Case sensitivity is honoured: `SPEED` only matches when folding is on.
    let folded = find_in_files(
        &files,
        "speed",
        SearchOptions {
            case_sensitive: false,
        },
    );
    let folded_b = folded
        .iter()
        .find(|g| g.path == "res://b.gd")
        .expect("b.gd matches when case-insensitive");
    assert_eq!(
        folded_b.matches.len(),
        2,
        "case-insensitive finds both `speed` and `SPEED` in b.gd",
    );
}

/// The results panel markup and its fetches exist in the served HTML — the
/// server half is useless if nothing renders it.
#[test]
fn find_in_files_panel_markup_present() {
    for needle in [
        // Bottom-panel tab + content pane.
        r#"data-tab="find-in-files""#,
        r#"id="find-in-files-results""#,
        // Per-file grouping and clickable hits carrying file/line/column.
        "fif-file",
        "fif-hit",
        "data-line=",
        "data-column=",
        // Click-to-open and the replace fetch.
        "findInFilesOpen",
        "/api/search/replace_all",
        // Scope controls from Godot's dialog.
        r#"id="fif-ext""#,
        r#"id="fif-case""#,
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "EDITOR_HTML must contain `{needle}` for the Find in Files panel",
        );
    }

    // The handlers are called from inline `onclick`, which only resolves
    // globals — and the editor script is wrapped in an IIFE. Without these
    // window bindings the panel renders and every button is a silent
    // ReferenceError, which still compiles and still serves.
    for binding in [
        "window.scriptFindInFiles",
        "window.findInFilesOpen",
        "window.findInFilesReplaceAll",
    ] {
        assert!(
            EDITOR_HTML.contains(binding),
            "`{binding}` must be exposed globally or the inline onclick is inert",
        );
    }
}
