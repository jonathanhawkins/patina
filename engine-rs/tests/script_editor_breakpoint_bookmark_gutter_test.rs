//! pat-ladkz: Script editor breakpoint and bookmark gutter.
//!
//! Validates:
//! 1. ScriptGutter: creation, line_count, set_line_count trimming
//! 2. Breakpoints: toggle, set, clear, has, lines, count, clear_all
//! 3. Breakpoints: out-of-range rejection, next/prev with wrapping
//! 4. Bookmarks: toggle, set, clear, has, lines, count, clear_all
//! 5. Bookmarks: out-of-range rejection, next/prev with wrapping
//! 6. Combined: markers_on_line, has_any_marker
//! 7. Line shifting: insert lines, delete lines, zero delta
//! 8. GutterManager: open/close scripts, get/get_mut
//! 9. GutterManager: all_breakpoints across scripts, clear_all_breakpoints
//! 10. GutterManager: reopen resets gutter, open_scripts list
//! 11. Gutter (script_editor): toggle/set/clear breakpoints and bookmarks
//! 12. Gutter: markers_at, next/prev bookmark, clear_all
//! 13. ScriptTab: gutter integration with tab source editing
//! 14. ScriptEditor: gutter per-tab isolation
//! 15. GutterMarker enum variants

use gdeditor::script_gutter::{GutterManager, ScriptGutter};
use gdeditor::script_editor::{Gutter, GutterMarker, ScriptEditor, ScriptTab};

// ── ScriptGutter: creation and defaults ─────────────────────────────

#[test]
fn script_gutter_new_defaults() {
    let g = ScriptGutter::new(100);
    assert_eq!(g.line_count(), 100);
    assert_eq!(g.breakpoint_count(), 0);
    assert_eq!(g.bookmark_count(), 0);
    assert!(g.breakpoint_lines().is_empty());
    assert!(g.bookmark_lines().is_empty());
}

#[test]
fn script_gutter_default_trait() {
    let g = ScriptGutter::default();
    assert_eq!(g.line_count(), 0);
}

// ── ScriptGutter: breakpoints ───────────────────────────────────────

#[test]
fn script_gutter_toggle_breakpoint() {
    let mut g = ScriptGutter::new(50);
    assert!(g.toggle_breakpoint(10));
    assert!(g.has_breakpoint(10));
    assert!(!g.toggle_breakpoint(10));
    assert!(!g.has_breakpoint(10));
}

#[test]
fn script_gutter_toggle_breakpoint_out_of_range() {
    let mut g = ScriptGutter::new(10);
    assert!(!g.toggle_breakpoint(0));
    assert!(!g.toggle_breakpoint(11));
    assert_eq!(g.breakpoint_count(), 0);
}

#[test]
fn script_gutter_set_clear_breakpoint() {
    let mut g = ScriptGutter::new(20);
    assert!(g.set_breakpoint(5));
    assert!(g.set_breakpoint(10));
    assert_eq!(g.breakpoint_count(), 2);
    assert_eq!(g.breakpoint_lines(), vec![5, 10]);

    assert!(g.clear_breakpoint(5));
    assert!(!g.clear_breakpoint(5));
    assert_eq!(g.breakpoint_count(), 1);
}

#[test]
fn script_gutter_set_breakpoint_out_of_range() {
    let mut g = ScriptGutter::new(10);
    assert!(!g.set_breakpoint(0));
    assert!(!g.set_breakpoint(11));
}

#[test]
fn script_gutter_clear_all_breakpoints() {
    let mut g = ScriptGutter::new(50);
    g.set_breakpoint(1);
    g.set_breakpoint(25);
    g.set_breakpoint(50);
    g.clear_all_breakpoints();
    assert_eq!(g.breakpoint_count(), 0);
}

#[test]
fn script_gutter_next_breakpoint_wraps() {
    let mut g = ScriptGutter::new(50);
    g.set_breakpoint(10);
    g.set_breakpoint(30);

    assert_eq!(g.next_breakpoint(5), Some(10));
    assert_eq!(g.next_breakpoint(10), Some(30));
    assert_eq!(g.next_breakpoint(30), Some(10)); // wraps
    assert_eq!(g.next_breakpoint(40), Some(10)); // wraps
}

#[test]
fn script_gutter_next_breakpoint_empty() {
    let g = ScriptGutter::new(50);
    assert_eq!(g.next_breakpoint(1), None);
}

#[test]
fn script_gutter_prev_breakpoint_wraps() {
    let mut g = ScriptGutter::new(50);
    g.set_breakpoint(10);
    g.set_breakpoint(30);

    assert_eq!(g.prev_breakpoint(30), Some(10));
    assert_eq!(g.prev_breakpoint(15), Some(10));
    assert_eq!(g.prev_breakpoint(10), Some(30)); // wraps
    assert_eq!(g.prev_breakpoint(1), Some(30)); // wraps
}

// ── ScriptGutter: bookmarks ─────────────────────────────────────────

#[test]
fn script_gutter_toggle_bookmark() {
    let mut g = ScriptGutter::new(50);
    assert!(g.toggle_bookmark(7));
    assert!(g.has_bookmark(7));
    assert!(!g.toggle_bookmark(7));
    assert!(!g.has_bookmark(7));
}

#[test]
fn script_gutter_toggle_bookmark_out_of_range() {
    let mut g = ScriptGutter::new(10);
    assert!(!g.toggle_bookmark(0));
    assert!(!g.toggle_bookmark(11));
}

#[test]
fn script_gutter_set_clear_bookmark() {
    let mut g = ScriptGutter::new(30);
    assert!(g.set_bookmark(3));
    assert!(g.set_bookmark(15));
    assert_eq!(g.bookmark_count(), 2);
    assert_eq!(g.bookmark_lines(), vec![3, 15]);

    assert!(g.clear_bookmark(3));
    assert!(!g.clear_bookmark(3));
    assert_eq!(g.bookmark_count(), 1);
}

#[test]
fn script_gutter_clear_all_bookmarks() {
    let mut g = ScriptGutter::new(20);
    g.set_bookmark(1);
    g.set_bookmark(10);
    g.clear_all_bookmarks();
    assert_eq!(g.bookmark_count(), 0);
}

#[test]
fn script_gutter_next_bookmark_wraps() {
    let mut g = ScriptGutter::new(40);
    g.set_bookmark(5);
    g.set_bookmark(20);

    assert_eq!(g.next_bookmark(1), Some(5));
    assert_eq!(g.next_bookmark(5), Some(20));
    assert_eq!(g.next_bookmark(20), Some(5)); // wraps
}

#[test]
fn script_gutter_prev_bookmark_wraps() {
    let mut g = ScriptGutter::new(40);
    g.set_bookmark(5);
    g.set_bookmark(20);

    assert_eq!(g.prev_bookmark(20), Some(5));
    assert_eq!(g.prev_bookmark(5), Some(20)); // wraps
}

// ── ScriptGutter: combined queries ──────────────────────────────────

#[test]
fn script_gutter_markers_on_line() {
    let mut g = ScriptGutter::new(20);
    g.set_breakpoint(5);
    g.set_bookmark(5);
    g.set_breakpoint(10);

    let m5 = g.markers_on_line(5);
    assert_eq!(m5.len(), 2);
    assert!(m5.contains(&GutterMarker::Breakpoint));
    assert!(m5.contains(&GutterMarker::Bookmark));

    assert_eq!(g.markers_on_line(10), vec![GutterMarker::Breakpoint]);
    assert!(g.markers_on_line(1).is_empty());
}

#[test]
fn script_gutter_has_any_marker() {
    let mut g = ScriptGutter::new(10);
    assert!(!g.has_any_marker(5));
    g.set_bookmark(5);
    assert!(g.has_any_marker(5));
    g.set_breakpoint(5);
    assert!(g.has_any_marker(5));
}

// ── ScriptGutter: set_line_count ────────────────────────────────────

#[test]
fn script_gutter_set_line_count_trims() {
    let mut g = ScriptGutter::new(50);
    g.set_breakpoint(10);
    g.set_breakpoint(40);
    g.set_bookmark(30);
    g.set_bookmark(50);

    g.set_line_count(35);
    assert_eq!(g.line_count(), 35);
    assert_eq!(g.breakpoint_lines(), vec![10]);
    assert_eq!(g.bookmark_lines(), vec![30]);
}

// ── ScriptGutter: shift_lines ───────────────────────────────────────

#[test]
fn script_gutter_shift_lines_insert() {
    let mut g = ScriptGutter::new(20);
    g.set_breakpoint(5);
    g.set_breakpoint(10);
    g.set_bookmark(8);

    g.shift_lines(7, 3);

    assert_eq!(g.line_count(), 23);
    assert_eq!(g.breakpoint_lines(), vec![5, 13]);
    assert_eq!(g.bookmark_lines(), vec![11]);
}

#[test]
fn script_gutter_shift_lines_delete() {
    let mut g = ScriptGutter::new(30);
    g.set_breakpoint(5);
    g.set_breakpoint(10);
    g.set_breakpoint(20);
    g.set_bookmark(10);

    g.shift_lines(8, -5);

    assert_eq!(g.line_count(), 25);
    assert_eq!(g.breakpoint_lines(), vec![5, 15]);
    assert!(g.bookmark_lines().is_empty());
}

#[test]
fn script_gutter_shift_lines_zero_noop() {
    let mut g = ScriptGutter::new(10);
    g.set_breakpoint(5);
    g.shift_lines(3, 0);
    assert_eq!(g.breakpoint_lines(), vec![5]);
    assert_eq!(g.line_count(), 10);
}

// ── GutterManager ───────────────────────────────────────────────────

#[test]
fn gutter_manager_open_close() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://main.gd", 100);
    mgr.open_script("res://player.gd", 50);
    assert_eq!(mgr.script_count(), 2);

    assert!(mgr.close_script("res://main.gd"));
    assert!(!mgr.close_script("res://main.gd"));
    assert_eq!(mgr.script_count(), 1);
}

#[test]
fn gutter_manager_get() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://test.gd", 30);
    assert_eq!(mgr.get("res://test.gd").unwrap().line_count(), 30);
    assert!(mgr.get("res://nonexistent.gd").is_none());
}

#[test]
fn gutter_manager_get_mut_toggle() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://test.gd", 20);
    let g = mgr.get_mut("res://test.gd").unwrap();
    g.toggle_breakpoint(5);
    g.toggle_breakpoint(15);
    assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 2);
}

#[test]
fn gutter_manager_all_breakpoints() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://a.gd", 20);
    mgr.open_script("res://b.gd", 30);

    mgr.get_mut("res://a.gd").unwrap().set_breakpoint(5);
    mgr.get_mut("res://a.gd").unwrap().set_breakpoint(10);
    mgr.get_mut("res://b.gd").unwrap().set_breakpoint(3);

    let all = mgr.all_breakpoints();
    assert_eq!(all.len(), 3);
    assert!(all.contains(&("res://a.gd", 5)));
    assert!(all.contains(&("res://a.gd", 10)));
    assert!(all.contains(&("res://b.gd", 3)));
}

#[test]
fn gutter_manager_clear_all_breakpoints() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://a.gd", 20);
    mgr.open_script("res://b.gd", 30);
    mgr.get_mut("res://a.gd").unwrap().set_breakpoint(5);
    mgr.get_mut("res://b.gd").unwrap().set_breakpoint(3);

    mgr.clear_all_breakpoints();
    assert!(mgr.all_breakpoints().is_empty());
}

#[test]
fn gutter_manager_open_scripts() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://x.gd", 10);
    mgr.open_script("res://y.gd", 20);
    let mut scripts = mgr.open_scripts();
    scripts.sort();
    assert_eq!(scripts, vec!["res://x.gd", "res://y.gd"]);
}

#[test]
fn gutter_manager_reopen_resets() {
    let mut mgr = GutterManager::new();
    mgr.open_script("res://test.gd", 50);
    mgr.get_mut("res://test.gd").unwrap().set_breakpoint(10);
    assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 1);

    mgr.open_script("res://test.gd", 100);
    assert_eq!(mgr.get("res://test.gd").unwrap().line_count(), 100);
    assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 0);
}

#[test]
fn gutter_manager_default_trait() {
    let mgr = GutterManager::default();
    assert_eq!(mgr.script_count(), 0);
}

// ── Gutter (script_editor) ──────────────────────────────────────────

#[test]
fn gutter_new_empty() {
    let g = Gutter::new();
    assert_eq!(g.breakpoint_count(), 0);
    assert_eq!(g.bookmark_count(), 0);
}

#[test]
fn gutter_toggle_breakpoint() {
    let mut g = Gutter::new();
    assert!(g.toggle_breakpoint(5));
    assert!(g.has_breakpoint(5));
    assert!(!g.toggle_breakpoint(5));
    assert!(!g.has_breakpoint(5));
}

#[test]
fn gutter_set_clear_breakpoint() {
    let mut g = Gutter::new();
    assert!(g.set_breakpoint(10));
    assert!(!g.set_breakpoint(10)); // already set
    assert_eq!(g.breakpoints(), vec![10]);
    assert!(g.clear_breakpoint(10));
    assert!(!g.clear_breakpoint(10));
}

#[test]
fn gutter_toggle_bookmark() {
    let mut g = Gutter::new();
    assert!(g.toggle_bookmark(3));
    assert!(g.has_bookmark(3));
    assert!(!g.toggle_bookmark(3));
    assert!(!g.has_bookmark(3));
}

#[test]
fn gutter_set_clear_bookmark() {
    let mut g = Gutter::new();
    assert!(g.set_bookmark(7));
    assert_eq!(g.bookmarks(), vec![7]);
    assert!(g.clear_bookmark(7));
    assert_eq!(g.bookmark_count(), 0);
}

#[test]
fn gutter_markers_at() {
    let mut g = Gutter::new();
    g.set_breakpoint(5);
    g.set_bookmark(5);

    let m = g.markers_at(5);
    assert_eq!(m.len(), 2);
    assert!(m.contains(&GutterMarker::Breakpoint));
    assert!(m.contains(&GutterMarker::Bookmark));
    assert!(g.markers_at(1).is_empty());
}

#[test]
fn gutter_next_prev_bookmark() {
    let mut g = Gutter::new();
    g.set_bookmark(5);
    g.set_bookmark(15);
    g.set_bookmark(25);

    assert_eq!(g.next_bookmark(5), Some(15));
    assert_eq!(g.next_bookmark(25), Some(5)); // wraps
    assert_eq!(g.prev_bookmark(15), Some(5));
    assert_eq!(g.prev_bookmark(5), Some(25)); // wraps
}

#[test]
fn gutter_next_bookmark_empty() {
    let g = Gutter::new();
    assert_eq!(g.next_bookmark(1), None);
    assert_eq!(g.prev_bookmark(1), None);
}

#[test]
fn gutter_clear_all() {
    let mut g = Gutter::new();
    g.set_breakpoint(1);
    g.set_breakpoint(10);
    g.set_bookmark(5);
    g.clear_all();
    assert_eq!(g.breakpoint_count(), 0);
    assert_eq!(g.bookmark_count(), 0);
}

#[test]
fn gutter_clear_all_breakpoints_only() {
    let mut g = Gutter::new();
    g.set_breakpoint(1);
    g.set_bookmark(5);
    g.clear_all_breakpoints();
    assert_eq!(g.breakpoint_count(), 0);
    assert_eq!(g.bookmark_count(), 1); // bookmarks preserved
}

#[test]
fn gutter_clear_all_bookmarks_only() {
    let mut g = Gutter::new();
    g.set_breakpoint(1);
    g.set_bookmark(5);
    g.clear_all_bookmarks();
    assert_eq!(g.bookmark_count(), 0);
    assert_eq!(g.breakpoint_count(), 1); // breakpoints preserved
}

// ── ScriptTab gutter integration ────────────────────────────────────

#[test]
fn script_tab_has_gutter() {
    let mut tab = ScriptTab::new("res://test.gd", "extends Node\nfunc _ready():\n\tpass");
    tab.gutter.set_breakpoint(2);
    tab.gutter.set_bookmark(1);
    assert!(tab.gutter.has_breakpoint(2));
    assert!(tab.gutter.has_bookmark(1));
    assert_eq!(tab.line_count(), 3);
}

#[test]
fn script_tab_gutter_survives_edit() {
    let mut tab = ScriptTab::new("res://test.gd", "line1\nline2\nline3");
    tab.gutter.set_breakpoint(2);
    tab.set_source("line1\nline2\nline3\nline4");
    // Gutter state persists through edits
    assert!(tab.gutter.has_breakpoint(2));
    assert_eq!(tab.line_count(), 4);
}

// ── ScriptEditor per-tab gutter isolation ───────────────────────────

#[test]
fn script_editor_per_tab_gutters() {
    let mut editor = ScriptEditor::new();
    editor.open("res://a.gd", "extends Node\nfunc _ready():\n\tpass");
    editor.open("res://b.gd", "extends Node2D\nvar speed = 10");

    // Set breakpoint on tab 0
    editor.set_active_tab(0);
    editor.tab_mut(0).unwrap().gutter.set_breakpoint(2);

    // Set bookmark on tab 1
    editor.tab_mut(1).unwrap().gutter.set_bookmark(1);

    // Verify isolation
    assert!(editor.tab(0).unwrap().gutter.has_breakpoint(2));
    assert!(!editor.tab(0).unwrap().gutter.has_bookmark(1));
    assert!(editor.tab(1).unwrap().gutter.has_bookmark(1));
    assert!(!editor.tab(1).unwrap().gutter.has_breakpoint(2));
}

// ── GutterMarker enum ──────────────────────────────────────────────

#[test]
fn gutter_marker_variants() {
    let bp = GutterMarker::Breakpoint;
    let bm = GutterMarker::Bookmark;
    assert_ne!(bp, bm);
    assert_eq!(bp, GutterMarker::Breakpoint);
    assert_eq!(bm, GutterMarker::Bookmark);
}
