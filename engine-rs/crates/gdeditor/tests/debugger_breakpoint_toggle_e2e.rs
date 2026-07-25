//! pat-0q2kx.10 — the script editor's breakpoint gutter is wired.
//!
//! Godot ground truth: `editor/script/script_text_editor.cpp` toggles a
//! breakpoint when you click the gutter or press F9, and the debugger's
//! breakpoint list shows it.
//!
//! Patina had the server half (`POST /api/script/breakpoint/toggle`,
//! `GET /api/script/breakpoints`) and the *remove* half of the debugger panel,
//! but no way to ADD a breakpoint: the gutter was a single text node with
//! `pointer-events: none`, so nothing was clickable and the pre-existing
//! `.script-line-numbers .breakpoint` CSS rule could never match an element.
//! Worse, the two stores behind those routes were disconnected — the gutter
//! wrote `EditorState::breakpoints` while the debugger panel and
//! `GET /api/debug/state` read `EditorState::debug_breakpoints`.
//!
//! This guards both halves:
//!  1. Static — `EDITOR_HTML` renders per-line gutter rows and wires click + F9
//!     to the toggle route.
//!  2. Functional — a toggle lands in *both* stores, so a breakpoint set from
//!     the gutter is the same one the debugger reports.
//!
//! NOT covered, because it is not implemented anywhere in the engine: actually
//! *pausing* a running scene at the breakpoint. `Debugger::should_break` has no
//! production caller (the GDScript interpreter never consults breakpoints while
//! executing), and `ENGINE_DEBUGGER` is `thread_local!` while the editor steps
//! `run_main_loop` on a background thread. See the completion report.

use gdeditor::editor_server::{
    remove_breakpoint_in_state, toggle_breakpoint_in_state, EditorState,
};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_the_breakpoint_gutter() {
    assert!(
        EDITOR_HTML.contains("/api/script/breakpoint/toggle"),
        "EDITOR_HTML must POST the toggle route to add a breakpoint"
    );
    assert!(
        EDITOR_HTML.contains("/api/script/breakpoints?path="),
        "EDITOR_HTML must GET the breakpoint list for the open script"
    );
    assert!(
        EDITOR_HTML.contains("toggleBreakpoint"),
        "EDITOR_HTML must define the gutter toggle handler"
    );
    assert!(
        EDITOR_HTML.contains("refreshBreakpoints"),
        "EDITOR_HTML must refresh markers from the server after a toggle"
    );
    assert!(
        EDITOR_HTML.contains("'F9'"),
        "EDITOR_HTML must bind F9 to toggle the breakpoint on the caret line"
    );
    assert!(
        EDITOR_HTML.contains("currentScriptLine"),
        "F9 needs the caret's line number"
    );
}

/// The gutter must render one element per line — a single text node has nothing
/// to click and nothing to hang the `.breakpoint` class on.
#[test]
fn editor_html_renders_clickable_per_line_gutter_rows() {
    assert!(
        EDITOR_HTML.contains("className = 'ln'"),
        "each gutter line must be its own element"
    );
    assert!(
        EDITOR_HTML.contains("data-line"),
        "gutter rows must carry their line number for the click handler"
    );
    assert!(
        EDITOR_HTML.contains(".script-line-numbers .ln { display: block; cursor: pointer; }"),
        "gutter rows must be styled as clickable block rows"
    );
    // The gutter used to be click-through, which is why no click handler could
    // ever have fired.
    assert!(
        !EDITOR_HTML.contains(
            "border-right: 1px solid var(--border); pointer-events: none; white-space: pre;"
        ),
        "the line-number gutter must no longer be pointer-events:none"
    );
    assert!(
        EDITOR_HTML.contains("applyBreakpointMarkers"),
        "EDITOR_HTML must paint the .breakpoint class onto gutter rows"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional guard — the two stores stay in step.
// ---------------------------------------------------------------------------

fn state() -> EditorState {
    EditorState::new(SceneTree::new())
}

#[test]
fn debugger_breakpoint_toggle_e2e() {
    let mut s = state();
    let script = "res://main.gd";

    // Setting a breakpoint from the gutter.
    assert!(
        toggle_breakpoint_in_state(&mut s, script, 12),
        "first toggle must ADD the breakpoint"
    );

    // ...is visible to GET /api/script/breakpoints (the gutter's store)...
    assert_eq!(
        s.breakpoints.get(script).cloned().unwrap_or_default(),
        vec![12],
        "the script-editor breakpoint list must contain the toggled line"
    );
    // ...and to GET /api/debug/state (the debugger's store). This is the link
    // that did not exist: a gutter breakpoint the debugger could not see.
    assert!(
        s.debug_breakpoints.contains(&(script.to_string(), 12)),
        "the debugger's breakpoint list must contain the toggled line, got {:?}",
        s.debug_breakpoints
    );

    // A second toggle on the same line clears it from both stores.
    assert!(
        !toggle_breakpoint_in_state(&mut s, script, 12),
        "second toggle must REMOVE the breakpoint"
    );
    assert!(
        s.breakpoints
            .get(script)
            .cloned()
            .unwrap_or_default()
            .is_empty(),
        "the script-editor list must be empty after the second toggle"
    );
    assert!(
        !s.debug_breakpoints.contains(&(script.to_string(), 12)),
        "the debugger list must be empty after the second toggle"
    );
}

/// Removing from the debugger panel must clear the gutter marker too, or the
/// script editor would keep showing a breakpoint the debugger no longer has.
#[test]
fn removing_from_the_debugger_panel_clears_the_gutter_marker() {
    let mut s = state();
    let script = "res://main.gd";
    toggle_breakpoint_in_state(&mut s, script, 12);
    toggle_breakpoint_in_state(&mut s, script, 40);

    remove_breakpoint_in_state(&mut s, script, 12);

    assert!(
        !s.debug_breakpoints.contains(&(script.to_string(), 12)),
        "removed line must leave the debugger list"
    );
    assert_eq!(
        s.breakpoints.get(script).cloned().unwrap_or_default(),
        vec![40],
        "removed line must also leave the gutter's list, leaving the others"
    );
}

#[test]
fn breakpoints_are_per_script_and_sorted() {
    let mut s = state();
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 30);
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 4);
    toggle_breakpoint_in_state(&mut s, "res://b.gd", 7);

    // Sorted, so the gutter and the panel list agree on order.
    assert_eq!(s.breakpoints.get("res://a.gd").unwrap(), &vec![4, 30]);
    assert_eq!(s.breakpoints.get("res://b.gd").unwrap(), &vec![7]);
    // Toggling one script's line must not disturb another's.
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 4);
    assert_eq!(s.breakpoints.get("res://a.gd").unwrap(), &vec![30]);
    assert_eq!(s.breakpoints.get("res://b.gd").unwrap(), &vec![7]);
    assert!(s.debug_breakpoints.contains(&("res://b.gd".to_string(), 7)));
}

#[test]
fn toggling_the_same_line_twice_leaves_no_duplicate_in_the_debug_list() {
    // Guards the mirror specifically: pushing into a Vec (unlike the sorted
    // insert on the other side) would happily duplicate.
    let mut s = state();
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 9);
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 9);
    toggle_breakpoint_in_state(&mut s, "res://a.gd", 9);
    let hits = s
        .debug_breakpoints
        .iter()
        .filter(|(sc, ln)| sc == "res://a.gd" && *ln == 9)
        .count();
    assert_eq!(
        hits, 1,
        "expected exactly one entry, got {:?}",
        s.debug_breakpoints
    );
}
