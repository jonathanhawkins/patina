//! pat-66lve: Script editor with GDScript syntax highlighting.
//!
//! Integration tests covering:
//! 1. SyntaxHighlighter — tokenize GDScript, classify spans by HighlightKind
//! 2. ScriptTab — open, edit, undo/redo, cursor, line access, mark saved
//! 3. ScriptEditor — multi-tab management, open/close/switch, highlight active
//! 4. FindReplace — plain/regex find, case sensitivity, whole word, replace
//! 5. Full lifecycle — open scripts, edit, highlight, find/replace, undo

use gdeditor::script_editor::{
    FindOptions, FindReplace, HighlightKind, HighlightSpan, ScriptEditor, ScriptTab,
    SyntaxHighlighter,
};

// ===========================================================================
// Test GDScript sources
// ===========================================================================

const SIMPLE_SCRIPT: &str = r#"extends Node2D

var speed = 100.0
var health := 10

func _ready():
    pass

func _process(delta):
    position.x += speed * delta
"#;

const HIGHLIGHT_SAMPLE: &str = r#"var x = 42
var name = "hello"
var flag = true
"#;

const MULTI_FUNC_SCRIPT: &str = r#"extends CharacterBody2D

@export var speed: float = 200.0
@onready var sprite = $Sprite2D

signal hit

func _ready():
    print("ready")

func move(direction):
    velocity = direction * speed

func take_damage(amount):
    health -= amount
    if health <= 0:
        emit_signal("hit")
"#;

// ===========================================================================
// 1. SyntaxHighlighter
// ===========================================================================

#[test]
fn highlighter_simple_var_declaration() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("var x = 42").unwrap();

    let kinds: Vec<HighlightKind> = spans.iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&HighlightKind::Keyword)); // var
    assert!(kinds.contains(&HighlightKind::Identifier)); // x
    assert!(kinds.contains(&HighlightKind::Operator)); // =
    assert!(kinds.contains(&HighlightKind::NumberLiteral)); // 42
}

#[test]
fn highlighter_string_literal() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight(r#"var s = "hello""#).unwrap();

    let string_spans: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::StringLiteral)
        .collect();
    assert_eq!(string_spans.len(), 1);
    assert!(string_spans[0].text.contains("hello"));
}

#[test]
fn highlighter_boolean_and_null() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("var a = true\nvar b = null").unwrap();

    let constants: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::ConstantLiteral)
        .collect();
    assert_eq!(constants.len(), 2);
}

#[test]
fn highlighter_func_keyword() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("func _ready():").unwrap();

    let keywords: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Keyword)
        .collect();
    assert!(keywords.iter().any(|s| s.text == "func"));
}

#[test]
fn highlighter_annotations() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("@export var speed = 1.0\n@onready var node = $Node").unwrap();

    let annotations: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Annotation)
        .collect();
    assert_eq!(annotations.len(), 2);
}

#[test]
fn highlighter_operators() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("x += y * 2 - 1").unwrap();

    let ops: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Operator)
        .collect();
    assert!(ops.len() >= 3); // +=, *, -
}

#[test]
fn highlighter_punctuation() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("func foo(a, b):").unwrap();

    let punct: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Punctuation)
        .collect();
    assert!(punct.len() >= 3); // (, ,, ), :
}

#[test]
fn highlighter_multiline_script() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight(SIMPLE_SCRIPT).unwrap();

    // Should have multiple keywords (extends, var, func, pass)
    let keywords: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Keyword)
        .collect();
    assert!(keywords.len() >= 4);

    // Should have number literals (100.0, 10)
    let numbers: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::NumberLiteral)
        .collect();
    assert!(numbers.len() >= 2);
}

#[test]
fn highlighter_highlight_line() {
    let hl = SyntaxHighlighter::new();
    // Line 1: "var x = 42"
    let line1 = hl.highlight_line(HIGHLIGHT_SAMPLE, 1).unwrap();
    assert!(line1.iter().any(|s| s.kind == HighlightKind::NumberLiteral));

    // Line 2: var name = "hello"
    let line2 = hl.highlight_line(HIGHLIGHT_SAMPLE, 2).unwrap();
    assert!(line2.iter().any(|s| s.kind == HighlightKind::StringLiteral));

    // Line 3: var flag = true
    let line3 = hl.highlight_line(HIGHLIGHT_SAMPLE, 3).unwrap();
    assert!(line3.iter().any(|s| s.kind == HighlightKind::ConstantLiteral));
}

#[test]
fn highlighter_used_kinds() {
    let hl = SyntaxHighlighter::new();
    let kinds = hl.used_kinds(SIMPLE_SCRIPT).unwrap();

    assert!(kinds.contains(&HighlightKind::Keyword));
    assert!(kinds.contains(&HighlightKind::Identifier));
    assert!(kinds.contains(&HighlightKind::NumberLiteral));
    assert!(kinds.contains(&HighlightKind::Operator));
}

#[test]
fn highlighter_empty_source() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("").unwrap();
    assert!(spans.is_empty());
}

#[test]
fn highlighter_all_keywords_classified() {
    let hl = SyntaxHighlighter::new();
    let keywords_source = "var func if else elif while for in return class extends signal enum match pass break continue const static self super class_name await";
    let spans = hl.highlight(keywords_source).unwrap();

    let keyword_spans: Vec<&HighlightSpan> = spans
        .iter()
        .filter(|s| s.kind == HighlightKind::Keyword)
        .collect();
    // Most of these should be keywords (some like "self" may tokenize differently)
    assert!(keyword_spans.len() >= 15);
}

#[test]
fn highlighter_complex_script() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight(MULTI_FUNC_SCRIPT).unwrap();

    // Should have annotations
    assert!(spans.iter().any(|s| s.kind == HighlightKind::Annotation));
    // Should have string literal ("ready")
    assert!(spans.iter().any(|s| s.kind == HighlightKind::StringLiteral));
    // Should have many keywords
    let kw_count = spans.iter().filter(|s| s.kind == HighlightKind::Keyword).count();
    assert!(kw_count >= 6);
}

#[test]
fn highlight_span_has_line_and_col() {
    let hl = SyntaxHighlighter::new();
    let spans = hl.highlight("var x = 1").unwrap();
    for span in &spans {
        assert!(span.line >= 1);
        assert!(span.col >= 1);
        assert!(!span.text.is_empty() || span.kind == HighlightKind::Whitespace);
    }
}

// ===========================================================================
// 2. ScriptTab
// ===========================================================================

#[test]
fn script_tab_new() {
    let tab = ScriptTab::new("res://player.gd", SIMPLE_SCRIPT);
    assert_eq!(tab.path, "res://player.gd");
    assert_eq!(tab.source, SIMPLE_SCRIPT);
    assert!(!tab.modified);
    assert_eq!(tab.cursor_line, 1);
    assert_eq!(tab.cursor_col, 1);
}

#[test]
fn script_tab_set_source_marks_modified() {
    let mut tab = ScriptTab::new("test.gd", "var x = 1");
    assert!(!tab.modified);

    tab.set_source("var x = 2");
    assert!(tab.modified);
    assert_eq!(tab.source, "var x = 2");
}

#[test]
fn script_tab_undo_redo() {
    let mut tab = ScriptTab::new("test.gd", "original");
    tab.set_source("edit1");
    tab.set_source("edit2");

    assert_eq!(tab.source, "edit2");

    assert!(tab.undo());
    assert_eq!(tab.source, "edit1");

    assert!(tab.undo());
    assert_eq!(tab.source, "original");

    assert!(!tab.undo()); // nothing more to undo

    assert!(tab.redo());
    assert_eq!(tab.source, "edit1");

    assert!(tab.redo());
    assert_eq!(tab.source, "edit2");

    assert!(!tab.redo()); // nothing more to redo
}

#[test]
fn script_tab_edit_clears_redo() {
    let mut tab = ScriptTab::new("test.gd", "original");
    tab.set_source("edit1");
    tab.undo();
    assert_eq!(tab.source, "original");

    // New edit should clear redo stack
    tab.set_source("edit_new");
    assert!(!tab.redo());
}

#[test]
fn script_tab_mark_saved() {
    let mut tab = ScriptTab::new("test.gd", "var x = 1");
    tab.set_source("var x = 2");
    assert!(tab.modified);

    tab.mark_saved();
    assert!(!tab.modified);
}

#[test]
fn script_tab_cursor() {
    let mut tab = ScriptTab::new("test.gd", "line1\nline2\nline3");
    tab.set_cursor(2, 5);
    assert_eq!(tab.cursor_line, 2);
    assert_eq!(tab.cursor_col, 5);
}

#[test]
fn script_tab_line_count() {
    let tab = ScriptTab::new("test.gd", "a\nb\nc");
    assert_eq!(tab.line_count(), 3);
}

#[test]
fn script_tab_get_line() {
    let tab = ScriptTab::new("test.gd", "first\nsecond\nthird");
    assert_eq!(tab.get_line(1), Some("first"));
    assert_eq!(tab.get_line(2), Some("second"));
    assert_eq!(tab.get_line(3), Some("third"));
    assert_eq!(tab.get_line(4), None);
}

// ===========================================================================
// 3. ScriptEditor — multi-tab management
// ===========================================================================

#[test]
fn editor_open_and_switch_tabs() {
    let mut editor = ScriptEditor::new();
    assert_eq!(editor.tab_count(), 0);
    assert!(editor.active().is_none());

    let idx0 = editor.open("player.gd", "extends Node2D");
    assert_eq!(idx0, 0);
    assert_eq!(editor.tab_count(), 1);
    assert_eq!(editor.active_tab_index(), Some(0));

    let idx1 = editor.open("enemy.gd", "extends CharacterBody2D");
    assert_eq!(idx1, 1);
    assert_eq!(editor.active_tab_index(), Some(1));

    // Switch back
    assert!(editor.set_active_tab(0));
    assert_eq!(editor.active().unwrap().path, "player.gd");
}

#[test]
fn editor_open_same_path_switches_tab() {
    let mut editor = ScriptEditor::new();
    editor.open("player.gd", "extends Node2D");
    editor.open("enemy.gd", "extends Node");

    // Re-opening player.gd should switch to it, not create new tab
    let idx = editor.open("player.gd", "different source");
    assert_eq!(idx, 0);
    assert_eq!(editor.tab_count(), 2);
    assert_eq!(editor.active_tab_index(), Some(0));
}

#[test]
fn editor_close_tab() {
    let mut editor = ScriptEditor::new();
    editor.open("a.gd", "");
    editor.open("b.gd", "");
    editor.open("c.gd", "");
    assert_eq!(editor.tab_count(), 3);

    assert!(editor.close(1)); // close b.gd
    assert_eq!(editor.tab_count(), 2);
    assert_eq!(editor.open_paths(), vec!["a.gd", "c.gd"]);
}

#[test]
fn editor_close_last_tab_clears_active() {
    let mut editor = ScriptEditor::new();
    editor.open("only.gd", "");
    editor.close(0);
    assert_eq!(editor.tab_count(), 0);
    assert!(editor.active_tab_index().is_none());
}

#[test]
fn editor_close_nonexistent_returns_false() {
    let mut editor = ScriptEditor::new();
    assert!(!editor.close(0));
}

#[test]
fn editor_open_paths() {
    let mut editor = ScriptEditor::new();
    editor.open("a.gd", "");
    editor.open("b.gd", "");
    assert_eq!(editor.open_paths(), vec!["a.gd", "b.gd"]);
}

#[test]
fn editor_has_unsaved() {
    let mut editor = ScriptEditor::new();
    editor.open("test.gd", "var x = 1");
    assert!(!editor.has_unsaved());

    editor.active_mut().unwrap().set_source("var x = 2");
    assert!(editor.has_unsaved());

    editor.active_mut().unwrap().mark_saved();
    assert!(!editor.has_unsaved());
}

#[test]
fn editor_set_active_tab_out_of_range() {
    let mut editor = ScriptEditor::new();
    editor.open("a.gd", "");
    assert!(!editor.set_active_tab(5));
    assert_eq!(editor.active_tab_index(), Some(0));
}

#[test]
fn editor_tab_access() {
    let mut editor = ScriptEditor::new();
    editor.open("a.gd", "source_a");
    editor.open("b.gd", "source_b");

    assert_eq!(editor.tab(0).unwrap().source, "source_a");
    assert_eq!(editor.tab(1).unwrap().source, "source_b");
    assert!(editor.tab(2).is_none());
}

// ===========================================================================
// 4. ScriptEditor — highlighting integration
// ===========================================================================

#[test]
fn editor_highlight_active_tab() {
    let mut editor = ScriptEditor::new();
    editor.open("player.gd", SIMPLE_SCRIPT);

    let result = editor.highlight_active().unwrap();
    let spans = result.unwrap();
    assert!(!spans.is_empty());
    assert!(spans.iter().any(|s| s.kind == HighlightKind::Keyword));
}

#[test]
fn editor_highlight_active_line() {
    let mut editor = ScriptEditor::new();
    editor.open("player.gd", HIGHLIGHT_SAMPLE);

    let result = editor.highlight_active_line(1).unwrap();
    let spans = result.unwrap();
    assert!(spans.iter().any(|s| s.kind == HighlightKind::NumberLiteral));
}

#[test]
fn editor_highlight_no_active_returns_none() {
    let editor = ScriptEditor::new();
    assert!(editor.highlight_active().is_none());
    assert!(editor.highlight_active_line(1).is_none());
}

#[test]
fn editor_highlighter_access() {
    let editor = ScriptEditor::new();
    let hl = editor.highlighter();
    let spans = hl.highlight("var x = 1").unwrap();
    assert!(!spans.is_empty());
}

// ===========================================================================
// 5. FindReplace
// ===========================================================================

#[test]
fn find_all_plain() {
    let fr = FindReplace::new("var");
    let matches = fr.find_all(SIMPLE_SCRIPT);
    assert!(matches.len() >= 2); // at least "var speed" and "var health"
}

#[test]
fn find_all_case_insensitive() {
    let fr = FindReplace::new("VAR").with_options(FindOptions {
        case_sensitive: false,
        ..Default::default()
    });
    let matches = fr.find_all("var x = 1\nVar y = 2");
    assert_eq!(matches.len(), 2);
}

#[test]
fn find_all_whole_word() {
    let fr = FindReplace::new("var").with_options(FindOptions {
        whole_word: true,
        ..Default::default()
    });
    let source = "var x = 1\nvariation = 2";
    let matches = fr.find_all(source);
    assert_eq!(matches.len(), 1); // only "var", not "variation"
}

#[test]
fn find_count() {
    let fr = FindReplace::new("func");
    let count = fr.count(MULTI_FUNC_SCRIPT);
    assert!(count >= 3); // _ready, move, take_damage
}

#[test]
fn find_next_wraps_around() {
    let fr = FindReplace::new("var");
    let source = "var x = 1\nvar y = 2";
    // Start from past the end
    let m = fr.find_next(source, 99, 0);
    assert!(m.is_some()); // wraps to first match
}

#[test]
fn find_next_no_wrap() {
    let fr = FindReplace::new("var").with_options(FindOptions {
        wrap_around: false,
        ..Default::default()
    });
    let source = "var x = 1";
    let m = fr.find_next(source, 99, 0);
    assert!(m.is_none());
}

#[test]
fn find_prev() {
    let fr = FindReplace::new("var");
    let source = "var x = 1\nvar y = 2\nvar z = 3";
    let m = fr.find_prev(source, 2, 0);
    assert!(m.is_some());
    assert!(m.unwrap().line < 2); // a "var" before line 2
}

#[test]
fn find_empty_query() {
    let fr = FindReplace::new("");
    assert!(fr.find_all("anything").is_empty());
}

#[test]
fn replace_next_plain() {
    let fr = FindReplace::new("var").with_replacement("let");
    let result = fr.replace_next("var x = 1\nvar y = 2").unwrap();
    assert!(result.starts_with("let x"));
    // Second var should still be var
    assert!(result.contains("var y"));
}

#[test]
fn replace_all_plain() {
    let fr = FindReplace::new("var").with_replacement("let");
    let result = fr.replace_all("var x = 1\nvar y = 2");
    assert!(!result.contains("var"));
    assert_eq!(result.matches("let").count(), 2);
}

#[test]
fn find_match_fields() {
    let fr = FindReplace::new("speed");
    let matches = fr.find_all("var speed = 100");
    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert_eq!(m.line, 0);
    assert_eq!(m.length, 5);
    assert_eq!(m.text, "speed");
}

// ===========================================================================
// 6. Full lifecycle
// ===========================================================================

#[test]
fn full_script_editor_lifecycle() {
    let mut editor = ScriptEditor::new();

    // 1. Open a script
    editor.open("player.gd", SIMPLE_SCRIPT);
    assert_eq!(editor.tab_count(), 1);

    // 2. Highlight it
    let spans = editor.highlight_active().unwrap().unwrap();
    assert!(spans.iter().any(|s| s.kind == HighlightKind::Keyword));

    // 3. Edit it
    let tab = editor.active_mut().unwrap();
    let original = tab.source.clone();
    tab.set_source(format!("{}\nfunc jump():\n    pass", original));
    assert!(tab.modified);

    // 4. Highlight again — should include the new func
    let spans2 = editor.highlight_active().unwrap().unwrap();
    let func_count = spans2.iter().filter(|s| s.text == "func").count();
    assert!(func_count >= 3);

    // 5. Find/replace
    let fr = FindReplace::new("speed").with_replacement("velocity");
    let tab = editor.active_mut().unwrap();
    let new_source = fr.replace_all(&tab.source);
    tab.set_source(new_source);
    assert!(!tab.source.contains("speed"));
    assert!(tab.source.contains("velocity"));

    // 6. Undo the replace
    tab.undo();
    assert!(tab.source.contains("speed"));

    // 7. Open another script
    editor.open("enemy.gd", MULTI_FUNC_SCRIPT);
    assert_eq!(editor.tab_count(), 2);
    assert_eq!(editor.active().unwrap().path, "enemy.gd");

    // 8. Check unsaved state
    assert!(editor.has_unsaved()); // player.gd was modified

    // 9. Save and verify
    editor.set_active_tab(0);
    editor.active_mut().unwrap().mark_saved();

    // 10. Close enemy.gd
    editor.close(1);
    assert_eq!(editor.tab_count(), 1);
    assert_eq!(editor.active().unwrap().path, "player.gd");
}
