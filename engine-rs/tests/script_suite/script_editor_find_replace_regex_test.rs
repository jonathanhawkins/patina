//! pat-kafee: Script editor find and replace with regex support.
//!
//! Integration tests covering:
//! 1. Plain-text find — single match, multiple matches, no match
//! 2. Case-insensitive find
//! 3. Whole-word matching
//! 4. Regex find — patterns, groups, quantifiers
//! 5. Find next/prev with wrap-around
//! 6. Replace next (plain and regex)
//! 7. Replace all (plain and regex)
//! 8. Regex backreferences in replacement
//! 9. Empty query edge cases
//! 10. Integration with ScriptEditor tabs

use gdeditor::script_editor::{FindMatch, FindOptions, FindReplace};
use gdeditor::ScriptEditor;

const SAMPLE: &str = "extends Node2D

var speed: float = 100.0
var health: int = 10
@export var name: String = \"Hero\"

func _ready():
    print(speed)
    print(health)

func _process(delta):
    speed += delta * 10
";

// ===========================================================================
// 1. Plain-text find
// ===========================================================================

#[test]
fn find_single_match() {
    let fr = FindReplace::new("health");
    let matches = fr.find_all(SAMPLE);
    assert!(matches.len() >= 2); // "health" appears in var decl and print
    assert_eq!(matches[0].text, "health");
}

#[test]
fn find_no_match() {
    let fr = FindReplace::new("nonexistent_identifier");
    assert_eq!(fr.count(SAMPLE), 0);
}

#[test]
fn find_multiple_on_different_lines() {
    let fr = FindReplace::new("var");
    let matches = fr.find_all(SAMPLE);
    assert!(matches.len() >= 3); // speed, health, name
                                 // All on different lines
    let lines: Vec<usize> = matches.iter().map(|m| m.line).collect();
    assert!(lines.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn find_match_positions() {
    let fr = FindReplace::new("speed");
    let matches = fr.find_all(SAMPLE);
    assert!(!matches.is_empty());
    // First match should be on the "var speed" line
    let first = &matches[0];
    assert_eq!(first.text, "speed");
    assert!(first.col > 0); // after "var "
}

// ===========================================================================
// 2. Case-insensitive find
// ===========================================================================

#[test]
fn case_insensitive_find() {
    let source = "Hello World\nhello world\nHELLO WORLD";
    let fr = FindReplace::new("hello").with_options(FindOptions {
        case_sensitive: false,
        ..Default::default()
    });
    assert_eq!(fr.count(source), 3);
}

#[test]
fn case_sensitive_find() {
    let source = "Hello World\nhello world\nHELLO WORLD";
    let fr = FindReplace::new("hello");
    assert_eq!(fr.count(source), 1); // only lowercase match
}

// ===========================================================================
// 3. Whole-word matching
// ===========================================================================

#[test]
fn whole_word_excludes_substrings() {
    let source = "var speed_fast = 10\nvar speed = 5\nvar speedy = 3";
    let fr = FindReplace::new("speed").with_options(FindOptions {
        whole_word: true,
        ..Default::default()
    });
    let matches = fr.find_all(source);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line, 1); // only "speed" on line 1
}

#[test]
fn whole_word_at_line_boundaries() {
    let source = "speed\nfast speed\nspeed fast";
    let fr = FindReplace::new("speed").with_options(FindOptions {
        whole_word: true,
        ..Default::default()
    });
    assert_eq!(fr.count(source), 3);
}

// ===========================================================================
// 4. Regex find
// ===========================================================================

#[test]
fn regex_find_pattern() {
    let fr = FindReplace::new(r"\d+\.?\d*").with_options(FindOptions {
        regex: true,
        ..Default::default()
    });
    let matches = fr.find_all(SAMPLE);
    assert!(matches.len() >= 3); // 100.0, 10, 10
}

#[test]
fn regex_find_func_names() {
    let fr = FindReplace::new(r"func _\w+").with_options(FindOptions {
        regex: true,
        ..Default::default()
    });
    let matches = fr.find_all(SAMPLE);
    assert_eq!(matches.len(), 2);
    assert!(matches[0].text.contains("_ready"));
    assert!(matches[1].text.contains("_process"));
}

#[test]
fn regex_case_insensitive() {
    let source = "Node2D\nnode2d\nNODE2D";
    let fr = FindReplace::new("node2d").with_options(FindOptions {
        regex: true,
        case_sensitive: false,
        ..Default::default()
    });
    assert_eq!(fr.count(source), 3);
}

#[test]
fn regex_invalid_pattern_returns_empty() {
    let fr = FindReplace::new(r"[invalid").with_options(FindOptions {
        regex: true,
        ..Default::default()
    });
    assert_eq!(fr.count(SAMPLE), 0);
}

// ===========================================================================
// 5. Find next/prev with wrap-around
// ===========================================================================

#[test]
fn find_next_from_beginning() {
    let fr = FindReplace::new("var");
    let m = fr.find_next(SAMPLE, 0, 0).unwrap();
    assert_eq!(m.text, "var");
}

#[test]
fn find_next_from_middle() {
    let fr = FindReplace::new("print");
    let matches = fr.find_all(SAMPLE);
    assert!(matches.len() >= 2);
    let first = &matches[0];
    // Find next after first match
    let second = fr.find_next(SAMPLE, first.line, first.col + 1).unwrap();
    assert!(second.line > first.line || second.col > first.col);
}

#[test]
fn find_next_wraps_around() {
    let fr = FindReplace::new("extends");
    // Start past the only "extends" on line 0
    let m = fr.find_next(SAMPLE, 100, 0).unwrap();
    assert_eq!(m.line, 0); // wrapped back to beginning
}

#[test]
fn find_next_no_wrap() {
    let fr = FindReplace::new("extends").with_options(FindOptions {
        wrap_around: false,
        ..Default::default()
    });
    let m = fr.find_next(SAMPLE, 100, 0);
    assert!(m.is_none());
}

#[test]
fn find_prev_from_end() {
    let fr = FindReplace::new("print");
    let m = fr.find_prev(SAMPLE, 100, 0).unwrap();
    assert_eq!(m.text, "print");
}

#[test]
fn find_prev_wraps_around() {
    let fr = FindReplace::new("delta");
    let m = fr.find_prev(SAMPLE, 0, 0).unwrap();
    // Should wrap to last occurrence
    assert!(m.line > 0);
}

// ===========================================================================
// 6. Replace next
// ===========================================================================

#[test]
fn replace_next_plain() {
    let fr = FindReplace::new("speed").with_replacement("velocity");
    let result = fr.replace_next(SAMPLE).unwrap();
    // First "speed" replaced, rest unchanged
    assert!(result.contains("velocity"));
    // Should still have other "speed" instances
    assert!(result.contains("speed"));
}

#[test]
fn replace_next_no_match() {
    let fr = FindReplace::new("zzzzz").with_replacement("xxx");
    assert!(fr.replace_next(SAMPLE).is_none());
}

#[test]
fn replace_next_regex() {
    let fr = FindReplace::new(r"\d+\.\d+")
        .with_replacement("0.0")
        .with_options(FindOptions {
            regex: true,
            ..Default::default()
        });
    let result = fr.replace_next(SAMPLE).unwrap();
    assert!(result.contains("0.0"));
}

// ===========================================================================
// 7. Replace all
// ===========================================================================

#[test]
fn replace_all_plain() {
    let fr = FindReplace::new("var").with_replacement("let");
    let result = fr.replace_all(SAMPLE);
    assert!(!result.contains("var"));
    assert!(result.contains("let speed"));
    assert!(result.contains("let health"));
}

#[test]
fn replace_all_no_match_returns_original() {
    let fr = FindReplace::new("zzzzz").with_replacement("xxx");
    let result = fr.replace_all(SAMPLE);
    assert_eq!(result, SAMPLE);
}

#[test]
fn replace_all_case_insensitive() {
    let source = "Hello hello HELLO";
    let fr = FindReplace::new("hello")
        .with_replacement("hi")
        .with_options(FindOptions {
            case_sensitive: false,
            ..Default::default()
        });
    let result = fr.replace_all(source);
    assert_eq!(result, "hi hi hi");
}

#[test]
fn replace_all_regex() {
    let source = "foo123 bar456 baz789";
    let fr = FindReplace::new(r"\d+")
        .with_replacement("NUM")
        .with_options(FindOptions {
            regex: true,
            ..Default::default()
        });
    let result = fr.replace_all(source);
    assert_eq!(result, "fooNUM barNUM bazNUM");
}

// ===========================================================================
// 8. Regex backreferences
// ===========================================================================

#[test]
fn replace_regex_with_capture_groups() {
    let source = "func _ready():\nfunc _process(delta):";
    let fr = FindReplace::new(r"func (\w+)")
        .with_replacement("method $1")
        .with_options(FindOptions {
            regex: true,
            ..Default::default()
        });
    let result = fr.replace_all(source);
    assert!(result.contains("method _ready"));
    assert!(result.contains("method _process"));
}

// ===========================================================================
// 9. Empty query edge cases
// ===========================================================================

#[test]
fn empty_query_finds_nothing() {
    let fr = FindReplace::new("");
    assert_eq!(fr.count(SAMPLE), 0);
    assert!(fr.find_all(SAMPLE).is_empty());
}

#[test]
fn empty_query_replace_returns_none() {
    let fr = FindReplace::new("").with_replacement("xxx");
    assert!(fr.replace_next(SAMPLE).is_none());
}

#[test]
fn find_in_empty_source() {
    let fr = FindReplace::new("test");
    assert_eq!(fr.count(""), 0);
}

// ===========================================================================
// 10. Integration with ScriptEditor tabs
// ===========================================================================

#[test]
fn find_in_active_tab() {
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", SAMPLE);
    let tab = editor.active().unwrap();

    let fr = FindReplace::new("speed");
    let matches = fr.find_all(&tab.source);
    assert!(matches.len() >= 2);
}

#[test]
fn replace_in_tab_source() {
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", SAMPLE);

    let fr = FindReplace::new("speed").with_replacement("velocity");
    let new_source = fr.replace_all(&editor.active().unwrap().source);
    editor.active_mut().unwrap().set_source(new_source);

    assert!(editor.active().unwrap().source.contains("velocity"));
    assert!(!editor.active().unwrap().source.contains("speed"));
    assert!(editor.active().unwrap().modified);
}

// ===========================================================================
// 11. Builder pattern and options
// ===========================================================================

#[test]
fn builder_pattern() {
    let fr = FindReplace::new("test")
        .with_replacement("replaced")
        .with_options(FindOptions {
            case_sensitive: false,
            regex: true,
            whole_word: true,
            wrap_around: false,
        });
    assert_eq!(fr.query(), "test");
    assert_eq!(fr.replacement(), "replaced");
    assert!(!fr.options().case_sensitive);
    assert!(fr.options().regex);
    assert!(fr.options().whole_word);
    assert!(!fr.options().wrap_around);
}

#[test]
fn set_query_and_replacement() {
    let mut fr = FindReplace::new("old");
    fr.set_query("new_query");
    fr.set_replacement("new_replacement");
    assert_eq!(fr.query(), "new_query");
    assert_eq!(fr.replacement(), "new_replacement");
}

#[test]
fn default_options() {
    let opts = FindOptions::default();
    assert!(opts.case_sensitive);
    assert!(!opts.regex);
    assert!(!opts.whole_word);
    assert!(opts.wrap_around);
}
