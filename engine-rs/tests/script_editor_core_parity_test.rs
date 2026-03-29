//! pat-1flw7: Script editor parity — core editing features.
//!
//! Validates Godot editor parity for lane 13:
//!   - Syntax highlighting covers all GDScript token categories
//!   - Code completion provides keywords, classes, functions, and source identifiers
//!   - Gutter markers (breakpoints + bookmarks) with navigation
//!   - Code folding detects functions, classes, conditionals, loops, match, regions
//!   - Multi-caret editing (add/remove cursors, selection, select-all)
//!   - Minimap state (viewport fraction, scroll fraction, click-to-line)
//!   - Diagnostics (errors, warnings, hints) with navigation
//!   - Method outline extraction from GDScript source
//!   - Script list panel builds from editor state
//!   - Status bar displays cursor/diagnostic info

use gdeditor::script_editor::{
    Caret, CodeFolding, CompletionKind, CompletionProvider, Diagnostic, DiagnosticList,
    DiagnosticSeverity, FoldKind, GutterMarker, HighlightKind, Minimap, MultiCaret, ScriptEditor,
    ScriptStatusBar, Selection, SyntaxHighlighter,
    build_script_list, detect_fold_regions, extract_outline,
};
use gdeditor::script_editor::{OutlineKind, FindReplace, FindOptions};

// ── Sample GDScript sources for testing ─────────────────────────────

const PLAYER_GD: &str = r#"extends CharacterBody2D

const MAX_SPEED: float = 200.0

signal health_changed(new_hp: int)

enum Direction { LEFT, RIGHT, UP, DOWN }

@export var health: int = 100
var speed: float = 100.0
var velocity_dir := Vector2.ZERO

func _ready():
    print("Player ready")
    connect("health_changed", _on_health_changed)

func _process(delta):
    var input = Input.get_vector("left", "right", "up", "down")
    velocity = input * speed
    move_and_slide()

func take_damage(amount: int):
    health -= amount
    if health <= 0:
        queue_free()
    else:
        emit_signal("health_changed", health)

func _on_health_changed(hp: int):
    print("Health: ", hp)

class InnerState:
    var active: bool = false
    func activate():
        active = true
"#;

const FOLDING_GD: &str = r#"func top_level():
    pass
    pass

class MyClass:
    func inner():
        pass

if condition:
    do_something()
elif other:
    do_other()
else:
    fallback()

for i in range(10):
    process(i)

while running:
    step()

match state:
    "idle":
        idle()
    "run":
        run()

#region Utilities
func helper():
    pass
#endregion
"#;

// ===========================================================================
// 1. Syntax Highlighting — all token categories
// ===========================================================================

#[test]
fn highlight_covers_all_token_categories() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight(PLAYER_GD).unwrap();

    assert!(!spans.is_empty(), "should produce spans");

    let kinds: Vec<HighlightKind> = spans.iter().map(|s| s.kind).collect();

    assert!(kinds.contains(&HighlightKind::Keyword), "must have keywords");
    assert!(kinds.contains(&HighlightKind::Identifier), "must have identifiers");
    assert!(kinds.contains(&HighlightKind::StringLiteral), "must have strings");
    assert!(kinds.contains(&HighlightKind::NumberLiteral), "must have numbers");
    assert!(kinds.contains(&HighlightKind::Operator), "must have operators");
    assert!(kinds.contains(&HighlightKind::Punctuation), "must have punctuation");
    assert!(kinds.contains(&HighlightKind::Annotation), "must have annotations");
}

#[test]
fn highlight_line_filters_correctly() {
    let hl = SyntaxHighlighter::new();
    // Line 1 is "extends CharacterBody2D"
    let line1 = hl.highlight_line(PLAYER_GD, 1).unwrap();
    assert!(!line1.is_empty());
    assert!(line1.iter().all(|s| s.line == 1), "all spans must be on line 1");
    assert!(
        line1.iter().any(|s| s.kind == HighlightKind::Keyword && s.text == "extends"),
        "line 1 should contain 'extends' keyword"
    );
}

#[test]
fn highlight_used_kinds_deduplicates() {
    let hl = SyntaxHighlighter::new();
    let kinds = hl.used_kinds(PLAYER_GD).unwrap();
    let mut deduped = kinds.clone();
    deduped.dedup();
    assert_eq!(kinds, deduped, "used_kinds should be deduplicated");
    assert!(kinds.len() >= 5, "should have at least 5 distinct kinds");
}

#[test]
fn highlight_constant_literals() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("var x = true\nvar y = null").unwrap();
    assert!(
        spans.iter().any(|s| s.kind == HighlightKind::ConstantLiteral && s.text == "true"),
        "should highlight 'true' as constant"
    );
    assert!(
        spans.iter().any(|s| s.kind == HighlightKind::ConstantLiteral && s.text == "null"),
        "should highlight 'null' as constant"
    );
}

// ===========================================================================
// 2. Code Completion — keywords, classes, functions, source identifiers
// ===========================================================================

#[test]
fn completion_empty_prefix_returns_nothing() {
    let provider = CompletionProvider::new();
    let items = provider.complete("", PLAYER_GD);
    assert!(items.is_empty(), "empty prefix should return no completions");
}

#[test]
fn completion_keyword_prefix() {
    let provider = CompletionProvider::new();
    let items = provider.complete("re", PLAYER_GD);
    assert!(
        items.iter().any(|i| i.label == "return" && i.kind == CompletionKind::Keyword),
        "should suggest 'return' for prefix 're'"
    );
}

#[test]
fn completion_class_prefix() {
    let provider = CompletionProvider::new();
    let items = provider.complete("Vec", PLAYER_GD);
    assert!(
        items.iter().any(|i| i.kind == CompletionKind::Class && i.label.starts_with("Vec")),
        "should suggest Vector classes for prefix 'Vec'"
    );
}

#[test]
fn completion_builtin_function() {
    let provider = CompletionProvider::new();
    let items = provider.complete("pri", PLAYER_GD);
    assert!(
        items.iter().any(|i| i.kind == CompletionKind::Function && i.label.contains("print")),
        "should suggest print() for prefix 'pri'"
    );
}

#[test]
fn completion_source_identifiers() {
    let provider = CompletionProvider::new();
    let items = provider.complete("hea", PLAYER_GD);
    assert!(
        items.iter().any(|i| i.label == "health" || i.label == "health_changed"),
        "should suggest identifiers from source for prefix 'hea'"
    );
}

#[test]
fn completion_is_case_insensitive() {
    let provider = CompletionProvider::new();
    let items = provider.complete("VEC", PLAYER_GD);
    assert!(
        items.iter().any(|i| i.label.starts_with("Vec")),
        "completion should be case-insensitive"
    );
}

#[test]
fn completion_deduplicates_results() {
    let provider = CompletionProvider::new();
    let items = provider.complete("pr", PLAYER_GD);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(labels.len(), unique.len(), "completions should be deduplicated");
}

// ===========================================================================
// 3. Code Folding — region detection and state management
// ===========================================================================

#[test]
fn fold_regions_detect_functions() {
    let regions = detect_fold_regions(FOLDING_GD);
    let funcs: Vec<_> = regions.iter().filter(|r| r.kind == FoldKind::Function).collect();
    assert!(funcs.len() >= 2, "should detect at least 2 functions, got {}", funcs.len());
}

#[test]
fn fold_regions_detect_class() {
    let regions = detect_fold_regions(FOLDING_GD);
    assert!(
        regions.iter().any(|r| r.kind == FoldKind::Class),
        "should detect class block"
    );
}

#[test]
fn fold_regions_detect_conditionals() {
    let regions = detect_fold_regions(FOLDING_GD);
    let conds: Vec<_> = regions.iter().filter(|r| r.kind == FoldKind::Conditional).collect();
    assert!(conds.len() >= 2, "should detect if/elif/else blocks");
}

#[test]
fn fold_regions_detect_loops() {
    let regions = detect_fold_regions(FOLDING_GD);
    let loops: Vec<_> = regions.iter().filter(|r| r.kind == FoldKind::Loop).collect();
    assert!(loops.len() >= 2, "should detect for and while loops");
}

#[test]
fn fold_regions_detect_match() {
    let regions = detect_fold_regions(FOLDING_GD);
    assert!(
        regions.iter().any(|r| r.kind == FoldKind::Match),
        "should detect match block"
    );
}

#[test]
fn fold_regions_detect_region_comments() {
    let regions = detect_fold_regions(FOLDING_GD);
    assert!(
        regions.iter().any(|r| r.kind == FoldKind::Region),
        "should detect #region/#endregion blocks"
    );
}

#[test]
fn code_folding_toggle_fold_unfold() {
    let mut folding = CodeFolding::new();
    assert_eq!(folding.folded_count(), 0);

    // Toggle fold on.
    assert!(folding.toggle(5));
    assert!(folding.is_folded(5));
    assert_eq!(folding.folded_count(), 1);

    // Toggle fold off.
    assert!(!folding.toggle(5));
    assert!(!folding.is_folded(5));
    assert_eq!(folding.folded_count(), 0);
}

#[test]
fn code_folding_fold_all_unfold_all() {
    let regions = detect_fold_regions(FOLDING_GD);
    let mut folding = CodeFolding::new();

    folding.fold_all(&regions);
    assert_eq!(folding.folded_count(), regions.len());

    folding.unfold_all();
    assert_eq!(folding.folded_count(), 0);
}

#[test]
fn fold_region_line_ranges_valid() {
    let regions = detect_fold_regions(FOLDING_GD);
    for r in &regions {
        assert!(r.start_line < r.end_line, "fold region start {} must be before end {}", r.start_line, r.end_line);
        assert!(r.start_line >= 1, "line numbers are 1-based");
    }
}

// ===========================================================================
// 4. Multi-Caret Editing
// ===========================================================================

#[test]
fn multi_caret_starts_with_single() {
    let mc = MultiCaret::new();
    assert_eq!(mc.caret_count(), 1);
    assert_eq!(mc.primary().caret, Caret::new(1, 1));
}

#[test]
fn multi_caret_add_and_remove() {
    let mut mc = MultiCaret::new();
    mc.add_cursor(3, 5);
    mc.add_cursor(7, 1);
    assert_eq!(mc.caret_count(), 3);

    // Duplicate add is idempotent.
    mc.add_cursor(3, 5);
    assert_eq!(mc.caret_count(), 3);

    // Remove one.
    assert!(mc.remove_cursor(3, 5));
    assert_eq!(mc.caret_count(), 2);

    // Can't remove last cursor.
    mc.remove_cursor(7, 1);
    assert_eq!(mc.caret_count(), 1);
    assert!(!mc.remove_cursor(1, 1));
}

#[test]
fn multi_caret_selections_sorted() {
    let mut mc = MultiCaret::new();
    mc.add_cursor(10, 1);
    mc.add_cursor(5, 3);
    mc.add_cursor(1, 1); // already exists

    let sels = mc.selections();
    for i in 1..sels.len() {
        assert!(sels[i - 1].caret <= sels[i].caret, "selections must be sorted by position");
    }
}

#[test]
fn multi_caret_set_selection_range() {
    let mut mc = MultiCaret::new();
    mc.set_selection(1, 1, 3, 10);
    assert!(mc.has_selection());

    let primary = mc.primary();
    assert_eq!(primary.anchor, Caret::new(1, 1));
    assert_eq!(primary.caret, Caret::new(3, 10));
}

#[test]
fn multi_caret_select_all() {
    let mut mc = MultiCaret::new();
    mc.select_all(50);
    let (start, end) = mc.primary().ordered();
    assert_eq!(start.line, 1);
    assert_eq!(end.line, 50);
}

#[test]
fn multi_caret_go_to_line() {
    let mut mc = MultiCaret::new();
    mc.add_cursor(5, 5);
    mc.go_to_line(42);
    assert_eq!(mc.caret_count(), 1, "go_to_line should clear multi-cursor");
    assert_eq!(mc.primary().caret.line, 42);
}

#[test]
fn multi_caret_clear_selections_keeps_carets() {
    let mut mc = MultiCaret::new();
    mc.set_selection(1, 1, 5, 10);
    assert!(mc.has_selection());
    mc.clear_selections();
    assert!(!mc.has_selection());
    assert_eq!(mc.primary().caret, Caret::new(5, 10));
}

#[test]
fn selection_ordered_handles_reverse() {
    let sel = Selection::range(10, 5, 3, 1);
    let (start, end) = sel.ordered();
    assert_eq!(start, Caret::new(3, 1));
    assert_eq!(end, Caret::new(10, 5));
}

#[test]
fn selection_is_empty_when_cursor_only() {
    let sel = Selection::cursor_only(5, 3);
    assert!(sel.is_empty());
    let sel2 = Selection::range(1, 1, 5, 3);
    assert!(!sel2.is_empty());
}

// ===========================================================================
// 5. Minimap
// ===========================================================================

#[test]
fn minimap_defaults() {
    let mm = Minimap::new();
    assert!(mm.visible);
    assert_eq!(mm.width, 80);
}

#[test]
fn minimap_viewport_fraction() {
    let mut mm = Minimap::new();
    mm.update(1, 40, 200);
    let frac = mm.viewport_fraction();
    assert!((frac - 0.2).abs() < 0.01, "40/200 = 0.2, got {frac}");
}

#[test]
fn minimap_viewport_fraction_empty_doc() {
    let mut mm = Minimap::new();
    mm.update(1, 1, 0);
    assert_eq!(mm.viewport_fraction(), 1.0, "empty doc should show full viewport");
}

#[test]
fn minimap_scroll_fraction() {
    let mut mm = Minimap::new();
    mm.update(100, 140, 200);
    let frac = mm.scroll_fraction();
    // (100 - 1) / (200 - 1) ≈ 0.497
    assert!(frac > 0.49 && frac < 0.51, "scroll should be ~0.5, got {frac}");
}

#[test]
fn minimap_click_to_line() {
    let mut mm = Minimap::new();
    mm.update(1, 40, 100);

    assert_eq!(mm.click_to_line(0.0), 1, "top click = line 1");
    assert_eq!(mm.click_to_line(1.0), 100, "bottom click = last line");
    assert_eq!(mm.click_to_line(0.5), 50, "middle click = line 50");
}

#[test]
fn minimap_toggle() {
    let mut mm = Minimap::new();
    assert!(mm.visible);
    assert!(!mm.toggle());
    assert!(!mm.visible);
    assert!(mm.toggle());
    assert!(mm.visible);
}

// ===========================================================================
// 6. Diagnostics
// ===========================================================================

fn make_diag(sev: DiagnosticSeverity, line: usize, msg: &str) -> Diagnostic {
    Diagnostic {
        severity: sev,
        line,
        col: 1,
        message: msg.into(),
        code: None,
    }
}

#[test]
fn diagnostics_set_and_query() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Error, 5, "undefined variable"),
        make_diag(DiagnosticSeverity::Warning, 10, "unused variable"),
        make_diag(DiagnosticSeverity::Hint, 15, "consider using const"),
    ]);
    assert_eq!(diags.count(), 3);
    assert_eq!(diags.error_count(), 1);
    assert_eq!(diags.warning_count(), 1);
    assert!(diags.has_errors());
}

#[test]
fn diagnostics_at_line() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Error, 5, "error A"),
        make_diag(DiagnosticSeverity::Warning, 5, "warning B"),
        make_diag(DiagnosticSeverity::Error, 10, "error C"),
    ]);
    let line5 = diags.at_line(5);
    assert_eq!(line5.len(), 2, "two diagnostics on line 5");
    let line10 = diags.at_line(10);
    assert_eq!(line10.len(), 1);
    let line1 = diags.at_line(1);
    assert!(line1.is_empty());
}

#[test]
fn diagnostics_errors_and_warnings_filter() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Error, 1, "e1"),
        make_diag(DiagnosticSeverity::Error, 3, "e2"),
        make_diag(DiagnosticSeverity::Warning, 2, "w1"),
        make_diag(DiagnosticSeverity::Hint, 4, "h1"),
    ]);
    assert_eq!(diags.errors().len(), 2);
    assert_eq!(diags.warnings().len(), 1);
}

#[test]
fn diagnostics_navigation() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Error, 5, "first"),
        make_diag(DiagnosticSeverity::Warning, 15, "second"),
        make_diag(DiagnosticSeverity::Error, 25, "third"),
    ]);

    let next = diags.next_diagnostic(5).unwrap();
    assert_eq!(next.line, 15);

    let next2 = diags.next_diagnostic(15).unwrap();
    assert_eq!(next2.line, 25);

    assert!(diags.next_diagnostic(25).is_none());

    let prev = diags.prev_diagnostic(25).unwrap();
    assert_eq!(prev.line, 15);

    assert!(diags.prev_diagnostic(5).is_none());
}

#[test]
fn diagnostics_clear() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![make_diag(DiagnosticSeverity::Error, 1, "err")]);
    assert_eq!(diags.count(), 1);
    diags.clear();
    assert_eq!(diags.count(), 0);
    assert!(!diags.has_errors());
}

#[test]
fn diagnostics_sorted_by_position() {
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Warning, 20, "late"),
        make_diag(DiagnosticSeverity::Error, 1, "early"),
        make_diag(DiagnosticSeverity::Hint, 10, "mid"),
    ]);
    let all = diags.all();
    for i in 1..all.len() {
        assert!(all[i - 1].line <= all[i].line, "diagnostics should be sorted by line");
    }
}

// ===========================================================================
// 7. Method Outline
// ===========================================================================

#[test]
fn outline_extracts_functions() {
    let outline = extract_outline(PLAYER_GD);
    let funcs: Vec<_> = outline.iter().filter(|e| e.kind == OutlineKind::Function).collect();
    let names: Vec<&str> = funcs.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"_ready"), "should find _ready");
    assert!(names.contains(&"_process"), "should find _process");
    assert!(names.contains(&"take_damage"), "should find take_damage");
    assert!(names.contains(&"_on_health_changed"), "should find _on_health_changed");
}

#[test]
fn outline_extracts_class() {
    let outline = extract_outline(PLAYER_GD);
    assert!(
        outline.iter().any(|e| e.kind == OutlineKind::Class && e.name == "InnerState"),
        "should find InnerState class"
    );
}

#[test]
fn outline_extracts_signal() {
    let outline = extract_outline(PLAYER_GD);
    assert!(
        outline.iter().any(|e| e.kind == OutlineKind::Signal && e.name == "health_changed"),
        "should find health_changed signal"
    );
}

#[test]
fn outline_extracts_enum() {
    let outline = extract_outline(PLAYER_GD);
    assert!(
        outline.iter().any(|e| e.kind == OutlineKind::Enum && e.name == "Direction"),
        "should find Direction enum"
    );
}

#[test]
fn outline_extracts_constant() {
    let outline = extract_outline(PLAYER_GD);
    assert!(
        outline.iter().any(|e| e.kind == OutlineKind::Constant && e.name == "MAX_SPEED"),
        "should find MAX_SPEED constant"
    );
}

#[test]
fn outline_extracts_exports() {
    let outline = extract_outline(PLAYER_GD);
    assert!(
        outline.iter().any(|e| e.kind == OutlineKind::Export && e.name == "health"),
        "should find @export health"
    );
}

#[test]
fn outline_line_numbers_valid() {
    let outline = extract_outline(PLAYER_GD);
    for entry in &outline {
        assert!(entry.line >= 1, "line numbers are 1-based");
    }
    // Entries should be in source order.
    for i in 1..outline.len() {
        assert!(
            outline[i].line >= outline[i - 1].line,
            "outline entries should be in source order"
        );
    }
}

// ===========================================================================
// 8. Script List Panel
// ===========================================================================

#[test]
fn script_list_builds_from_editor() {
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", PLAYER_GD);
    editor.open("res://enemy.gd", "extends Node2D\n");
    editor.open("res://ui/menu.gd", "extends Control\n");

    let list = build_script_list(&editor);
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].display_name, "player.gd");
    assert_eq!(list[0].path, "res://player.gd");
    assert_eq!(list[1].display_name, "enemy.gd");
    assert_eq!(list[2].display_name, "menu.gd");
}

#[test]
fn script_list_shows_modified_state() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", "var x = 1\n");
    editor.tab_mut(0).unwrap().set_source("var x = 2\n");

    let list = build_script_list(&editor);
    assert!(list[0].modified, "modified tab should show modified");

    editor.tab_mut(0).unwrap().mark_saved();
    let list2 = build_script_list(&editor);
    assert!(!list2[0].modified, "saved tab should not show modified");
}

#[test]
fn script_list_tab_indices_match() {
    let mut editor = ScriptEditor::new();
    editor.open("res://a.gd", "");
    editor.open("res://b.gd", "");
    editor.open("res://c.gd", "");

    let list = build_script_list(&editor);
    for (i, entry) in list.iter().enumerate() {
        assert_eq!(entry.tab_index, i, "tab index should match position");
    }
}

// ===========================================================================
// 9. Status Bar
// ===========================================================================

#[test]
fn status_bar_basic_display() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", PLAYER_GD);
    let tab = editor.tab(0).unwrap();
    let carets = MultiCaret::new();
    let diags = DiagnosticList::new();

    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    assert_eq!(bar.line, 1);
    assert_eq!(bar.col, 1);
    assert!(bar.total_lines > 0);
    assert_eq!(bar.language, "GDScript");
    assert_eq!(bar.error_count, 0);
    assert_eq!(bar.warning_count, 0);
}

#[test]
fn status_bar_shows_cursor_position() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", PLAYER_GD);
    editor.tab_mut(0).unwrap().set_cursor(5, 10);
    let tab = editor.tab(0).unwrap();
    let carets = MultiCaret::new();
    let diags = DiagnosticList::new();

    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    assert_eq!(bar.line, 5);
    assert_eq!(bar.col, 10);
}

#[test]
fn status_bar_shows_multi_caret_count() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", PLAYER_GD);
    let tab = editor.tab(0).unwrap();
    let mut carets = MultiCaret::new();
    carets.add_cursor(5, 1);
    carets.add_cursor(10, 1);
    let diags = DiagnosticList::new();

    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    assert_eq!(bar.selection_count, 3);
    let display = bar.display();
    assert!(display.contains("3 carets"), "display should mention caret count: {display}");
}

#[test]
fn status_bar_shows_diagnostics_count() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", PLAYER_GD);
    let tab = editor.tab(0).unwrap();
    let carets = MultiCaret::new();
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Error, 1, "e1"),
        make_diag(DiagnosticSeverity::Error, 2, "e2"),
        make_diag(DiagnosticSeverity::Warning, 3, "w1"),
    ]);

    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    assert_eq!(bar.error_count, 2);
    assert_eq!(bar.warning_count, 1);
    let display = bar.display();
    assert!(display.contains("2 errors"), "should show error count: {display}");
    assert!(display.contains("1 warnings"), "should show warning count: {display}");
}

#[test]
fn status_bar_display_format() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", "line1\nline2\nline3\n");
    let tab = editor.tab(0).unwrap();
    let carets = MultiCaret::new();
    let diags = DiagnosticList::new();

    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    let display = bar.display();
    assert!(display.contains("Ln "), "should contain 'Ln'");
    assert!(display.contains("Col "), "should contain 'Col'");
    assert!(display.contains("lines"), "should contain line count");
    assert!(display.contains("GDScript"), "should contain language");
}

// ===========================================================================
// 10. Find and Replace
// ===========================================================================

#[test]
fn find_basic_search() {
    let fr = FindReplace::new("health");
    let matches = fr.find_all(PLAYER_GD);
    assert!(!matches.is_empty(), "should find 'health' in source");
    for m in &matches {
        assert!(PLAYER_GD.lines().nth(m.line).unwrap().contains("health"));
    }
}

#[test]
fn find_case_sensitive() {
    let fr = FindReplace::new("MAX_SPEED")
        .with_options(FindOptions { case_sensitive: true, ..Default::default() });
    let matches = fr.find_all(PLAYER_GD);
    assert!(!matches.is_empty(), "should find MAX_SPEED case-sensitive");

    let fr2 = FindReplace::new("max_speed")
        .with_options(FindOptions { case_sensitive: true, ..Default::default() });
    let no_match = fr2.find_all(PLAYER_GD);
    assert!(no_match.is_empty(), "should not find max_speed case-sensitive");
}

#[test]
fn find_whole_word() {
    let fr = FindReplace::new("health")
        .with_options(FindOptions { whole_word: true, ..Default::default() });
    let matches = fr.find_all(PLAYER_GD);
    // Should find "health" standalone but not as part of "health_changed"
    for m in &matches {
        let line = PLAYER_GD.lines().nth(m.line).unwrap();
        let start = m.col;
        if start > 0 {
            let before = line.as_bytes()[start - 1];
            assert!(!before.is_ascii_alphanumeric() && before != b'_', "match should be whole word");
        }
    }
}

// ===========================================================================
// 11. Script Editor Tab Management
// ===========================================================================

#[test]
fn script_editor_open_close_tabs() {
    let mut editor = ScriptEditor::new();
    assert_eq!(editor.tab_count(), 0);

    editor.open("res://a.gd", "var a = 1");
    assert_eq!(editor.tab_count(), 1);
    assert_eq!(editor.active_tab_index(), Some(0));

    editor.open("res://b.gd", "var b = 2");
    assert_eq!(editor.tab_count(), 2);

    editor.close(0);
    assert_eq!(editor.tab_count(), 1);
    assert_eq!(editor.active().unwrap().path, "res://b.gd");
}

#[test]
fn script_editor_undo_redo() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", "original");
    let tab = editor.tab_mut(0).unwrap();

    tab.set_source("modified");
    assert_eq!(tab.source, "modified");
    assert!(tab.modified);

    assert!(tab.undo());
    assert_eq!(tab.source, "original");

    assert!(tab.redo());
    assert_eq!(tab.source, "modified");
}

#[test]
fn script_editor_highlight_active() {
    let mut editor = ScriptEditor::new();
    editor.open("res://test.gd", "var x = 42\nfunc f():\n    pass");
    let result = editor.highlight_active().unwrap().unwrap();
    assert!(!result.is_empty());
}

#[test]
fn script_editor_unsaved_tracking() {
    let mut editor = ScriptEditor::new();
    editor.open("res://a.gd", "original");
    assert!(!editor.has_unsaved());

    editor.tab_mut(0).unwrap().set_source("changed");
    assert!(editor.has_unsaved());

    editor.tab_mut(0).unwrap().mark_saved();
    assert!(!editor.has_unsaved());
}

#[test]
fn script_editor_open_paths() {
    let mut editor = ScriptEditor::new();
    editor.open("res://a.gd", "");
    editor.open("res://b.gd", "");
    let paths = editor.open_paths();
    assert_eq!(paths, vec!["res://a.gd", "res://b.gd"]);
}

// ===========================================================================
// 12. Gutter (breakpoints + bookmarks) with navigation
// ===========================================================================

#[test]
fn gutter_breakpoint_toggle_and_navigate() {
    use gdeditor::script_editor::Gutter;
    let mut gutter = Gutter::new();

    assert!(gutter.toggle_breakpoint(5));
    assert!(gutter.toggle_breakpoint(15));
    assert!(gutter.toggle_breakpoint(25));
    assert_eq!(gutter.breakpoint_count(), 3);
    assert_eq!(gutter.breakpoints(), vec![5, 15, 25]);

    // Remove middle.
    assert!(!gutter.toggle_breakpoint(15));
    assert_eq!(gutter.breakpoints(), vec![5, 25]);
}

#[test]
fn gutter_bookmark_navigation() {
    use gdeditor::script_editor::Gutter;
    let mut gutter = Gutter::new();
    gutter.set_bookmark(10);
    gutter.set_bookmark(30);
    gutter.set_bookmark(50);

    assert_eq!(gutter.next_bookmark(10), Some(30));
    assert_eq!(gutter.next_bookmark(30), Some(50));
    assert_eq!(gutter.next_bookmark(50), None);
    assert_eq!(gutter.prev_bookmark(50), Some(30));
    assert_eq!(gutter.prev_bookmark(10), None);
}

#[test]
fn gutter_markers_at_line() {
    use gdeditor::script_editor::Gutter;
    let mut gutter = Gutter::new();
    gutter.set_breakpoint(10);
    gutter.set_bookmark(10);

    let markers = gutter.markers_at(10);
    assert_eq!(markers.len(), 2);
    assert!(markers.contains(&GutterMarker::Breakpoint));
    assert!(markers.contains(&GutterMarker::Bookmark));

    let empty = gutter.markers_at(5);
    assert!(empty.is_empty());
}

#[test]
fn gutter_clear_all() {
    use gdeditor::script_editor::Gutter;
    let mut gutter = Gutter::new();
    gutter.set_breakpoint(1);
    gutter.set_breakpoint(2);
    gutter.set_bookmark(3);
    gutter.set_bookmark(4);

    gutter.clear_all();
    assert_eq!(gutter.breakpoint_count(), 0);
    assert_eq!(gutter.bookmark_count(), 0);
}

// ===========================================================================
// 13. Integration: full editing session simulation
// ===========================================================================

#[test]
fn full_editing_session_simulation() {
    // Simulate opening a script, editing, using completion, folding, diagnostics.
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", PLAYER_GD);

    // 1. Syntax highlighting works.
    let spans = editor.highlight_active().unwrap().unwrap();
    assert!(!spans.is_empty());

    // 2. Code completion.
    let provider = CompletionProvider::new();
    let completions = provider.complete("ta", PLAYER_GD);
    assert!(
        completions.iter().any(|c| c.label == "take_damage"),
        "should complete 'ta' to 'take_damage'"
    );

    // 3. Code folding.
    let regions = detect_fold_regions(PLAYER_GD);
    assert!(!regions.is_empty());
    let mut folding = CodeFolding::new();
    folding.fold(regions[0].start_line);
    assert!(folding.is_folded(regions[0].start_line));

    // 4. Multi-caret.
    let mut carets = MultiCaret::new();
    carets.set_cursor(13, 5); // _ready
    carets.add_cursor(17, 5); // _process
    assert_eq!(carets.caret_count(), 2);

    // 5. Minimap.
    let mut minimap = Minimap::new();
    let tab = editor.tab(0).unwrap();
    minimap.update(1, 30, tab.line_count());
    assert!(minimap.viewport_fraction() > 0.0);

    // 6. Diagnostics.
    let mut diags = DiagnosticList::new();
    diags.set(vec![
        make_diag(DiagnosticSeverity::Warning, 10, "unused variable"),
    ]);

    // 7. Outline.
    let outline = extract_outline(PLAYER_GD);
    assert!(outline.len() >= 6, "should have multiple outline entries");

    // 8. Status bar.
    let bar = ScriptStatusBar::from_editor(tab, &carets, &diags);
    assert_eq!(bar.selection_count, 2);
    assert_eq!(bar.warning_count, 1);
    let display = bar.display();
    assert!(display.contains("GDScript"));

    // 9. Script list.
    let list = build_script_list(&editor);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].display_name, "player.gd");
}
