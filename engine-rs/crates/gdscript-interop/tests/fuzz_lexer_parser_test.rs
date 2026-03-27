//! Fuzz testing for GDScript lexer and parser using proptest.
//!
//! Ensures that arbitrary input strings never cause panics in the tokenizer
//! or parser. Invalid GDScript should produce `Err` results, not crashes.
//! Also tests structured random GDScript fragments for deeper parser coverage.

use proptest::prelude::*;

use gdscript_interop::parser::Parser;
use gdscript_interop::tokenizer::tokenize;

// ---------------------------------------------------------------------------
// Strategy: completely random strings (byte-level fuzzing)
// ---------------------------------------------------------------------------

/// Arbitrary strings including non-ASCII, control chars, and unicode.
fn random_string() -> impl Strategy<Value = String> {
    prop::string::string_regex(".{0,200}")
        .unwrap()
}

/// ASCII-only random strings (more likely to hit keyword/operator paths).
fn random_ascii() -> impl Strategy<Value = String> {
    prop::collection::vec(0x20u8..0x7e, 0..300)
        .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Strategy: structured GDScript-like fragments
// ---------------------------------------------------------------------------

/// GDScript keywords and built-in names.
const KEYWORDS: &[&str] = &[
    "var", "func", "if", "else", "elif", "while", "for", "in", "return",
    "class", "extends", "signal", "enum", "match", "pass", "break",
    "continue", "const", "static", "export", "onready", "tool", "true",
    "false", "null", "self", "not", "and", "or", "is", "as", "yield",
    "await", "class_name", "preload", "super", "void",
];

const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "=", "==", "!=", "<", ">", "<=", ">=",
    "+=", "-=", "*=", "/=", "->", ".", ",", ":", ";", "(", ")", "[", "]",
    "{", "}", "@", "$", "~", "&", "|", "^", "<<", ">>", "!",
];

/// Generates structured GDScript-like fragments with proper indentation.
fn gdscript_fragment() -> impl Strategy<Value = String> {
    prop::collection::vec(gdscript_line(), 1..20)
        .prop_map(|lines| lines.join("\n"))
}

fn gdscript_line() -> impl Strategy<Value = String> {
    prop_oneof![
        // Variable declaration
        (gdscript_identifier(), gdscript_expr()).prop_map(|(name, expr)| {
            format!("var {} = {}", name, expr)
        }),
        // Function declaration
        (gdscript_identifier(), prop::collection::vec(gdscript_identifier(), 0..4))
            .prop_map(|(name, params)| {
                format!("func {}({}):", name, params.join(", "))
            }),
        // If statement
        gdscript_expr().prop_map(|expr| format!("if {}:", expr)),
        // Return statement
        gdscript_expr().prop_map(|expr| format!("\treturn {}", expr)),
        // Pass
        Just("\tpass".to_string()),
        // Signal declaration
        gdscript_identifier().prop_map(|name| format!("signal {}", name)),
        // Comment
        ".*".prop_map(|s: String| format!("# {}", s.chars().take(50).collect::<String>())),
        // Class extends
        gdscript_identifier().prop_map(|name| format!("extends {}", name)),
        // Blank line
        Just("".to_string()),
        // Indented expression
        gdscript_expr().prop_map(|expr| format!("\t{}", expr)),
    ]
}

fn gdscript_identifier() -> impl Strategy<Value = String> {
    prop_oneof![
        // Random valid identifiers
        "[a-z_][a-z0-9_]{0,15}",
        // Keywords (test keyword-in-identifier-position)
        prop::sample::select(KEYWORDS).prop_map(|s| s.to_string()),
    ]
}

fn gdscript_expr() -> impl Strategy<Value = String> {
    prop_oneof![
        // Integer literals
        (-10000i64..10000).prop_map(|n| n.to_string()),
        // Float literals
        (-1000.0f64..1000.0).prop_map(|f| format!("{:.2}", f)),
        // String literals
        "[a-zA-Z0-9 _]{0,20}".prop_map(|s| format!("\"{}\"", s)),
        // Identifier
        gdscript_identifier(),
        // Binary expression
        (gdscript_identifier(), prop::sample::select(OPERATORS), gdscript_identifier())
            .prop_map(|(a, op, b)| format!("{} {} {}", a, op, b)),
        // Function call
        (gdscript_identifier(), gdscript_identifier())
            .prop_map(|(func, arg)| format!("{}({})", func, arg)),
        // Boolean/null
        prop_oneof![Just("true"), Just("false"), Just("null")]
            .prop_map(|s| s.to_string()),
        // Array literal
        prop::collection::vec((-100i64..100).prop_map(|n| n.to_string()), 0..5)
            .prop_map(|items| format!("[{}]", items.join(", "))),
    ]
}

// ---------------------------------------------------------------------------
// Tests: tokenizer never panics
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn tokenizer_never_panics_on_random_bytes(input in random_string()) {
        // Must not panic — Ok or Err are both fine
        let _ = tokenize(&input);
    }

    #[test]
    fn tokenizer_never_panics_on_random_ascii(input in random_ascii()) {
        let _ = tokenize(&input);
    }

    #[test]
    fn tokenizer_never_panics_on_gdscript_fragments(input in gdscript_fragment()) {
        let _ = tokenize(&input);
    }

    #[test]
    fn tokenizer_handles_extreme_indentation(
        depth in 0usize..100,
        content in "[a-z]{1,10}"
    ) {
        let tabs = "\t".repeat(depth);
        let input = format!("{}{}", tabs, content);
        let _ = tokenize(&input);
    }

    #[test]
    fn tokenizer_handles_long_lines(
        content in "[a-zA-Z0-9+\\-*/= ]{0,1000}"
    ) {
        let _ = tokenize(&content);
    }

    #[test]
    fn tokenizer_handles_many_newlines(count in 0usize..200) {
        let input = "\n".repeat(count);
        let _ = tokenize(&input);
    }

    #[test]
    fn tokenizer_handles_unclosed_strings(
        prefix in "[a-z ]{0,20}",
        quote in prop_oneof![Just("\""), Just("'")],
        content in "[a-zA-Z0-9 ]{0,30}"
    ) {
        let input = format!("{}{}{}", prefix, quote, content);
        let _ = tokenize(&input);
    }
}

// ---------------------------------------------------------------------------
// Tests: parser never panics on tokenizer output
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn parser_never_panics_on_random_input(input in random_ascii()) {
        if let Ok(tokens) = tokenize(&input) {
            let mut parser = Parser::new(tokens, &input);
            let _ = parser.parse_script();
        }
    }

    #[test]
    fn parser_never_panics_on_gdscript_fragments(input in gdscript_fragment()) {
        if let Ok(tokens) = tokenize(&input) {
            let mut parser = Parser::new(tokens, &input);
            let _ = parser.parse_script();
        }
    }

    #[test]
    fn parser_never_panics_on_valid_looking_scripts(
        class_name in "[A-Z][a-z]{2,10}",
        base_class in prop_oneof![
            Just("Node"), Just("Node2D"), Just("Control"),
            Just("CharacterBody2D"), Just("Resource")
        ],
        var_name in "[a-z_][a-z0-9_]{1,8}",
        func_name in "[a-z_][a-z0-9_]{1,8}",
    ) {
        let script = format!(
            "class_name {class_name}\nextends {base_class}\n\nvar {var_name} = 0\n\nfunc {func_name}():\n\tpass\n"
        );
        if let Ok(tokens) = tokenize(&script) {
            let mut parser = Parser::new(tokens, &script);
            let _ = parser.parse_script();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: tokenizer determinism (same input → same output)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn tokenizer_is_deterministic(input in random_ascii()) {
        let result1 = tokenize(&input);
        let result2 = tokenize(&input);
        match (result1, result2) {
            (Ok(t1), Ok(t2)) => prop_assert_eq!(t1.len(), t2.len(),
                "tokenizer produced different token counts for same input"),
            (Err(_), Err(_)) => {} // both errored, fine
            _ => prop_assert!(false, "tokenizer was non-deterministic"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: specific edge cases that often crash parsers
// ---------------------------------------------------------------------------

#[test]
fn tokenizer_handles_null_bytes() {
    let _ = tokenize("\0");
    let _ = tokenize("var x\0= 5");
    let _ = tokenize("\0\0\0");
}

#[test]
fn tokenizer_handles_unicode_edge_cases() {
    let _ = tokenize("var \u{200B} = 5"); // zero-width space
    let _ = tokenize("var \u{FEFF} = 5"); // BOM
    let _ = tokenize("# \u{1F600}"); // emoji in comment
    let _ = tokenize("var x = \"\u{0}\""); // null in string
    let _ = tokenize("var \u{FFFF} = 1"); // non-character
}

#[test]
fn tokenizer_handles_deeply_nested_expressions() {
    // Deeply nested parentheses
    let open = "(".repeat(100);
    let close = ")".repeat(100);
    let input = format!("var x = {}1{}", open, close);
    let _ = tokenize(&input);
}

#[test]
fn parser_handles_deeply_nested_expressions() {
    let open = "(".repeat(50);
    let close = ")".repeat(50);
    let input = format!("var x = {}1{}", open, close);
    if let Ok(tokens) = tokenize(&input) {
        let mut parser = Parser::new(tokens, &input);
        let _ = parser.parse_script();
    }
}

#[test]
fn tokenizer_handles_very_long_identifier() {
    let long_id = "a".repeat(10000);
    let input = format!("var {} = 5", long_id);
    let _ = tokenize(&input);
}

#[test]
fn tokenizer_handles_very_long_number() {
    let long_num = "9".repeat(10000);
    let input = format!("var x = {}", long_num);
    let _ = tokenize(&input);
}

#[test]
fn tokenizer_handles_mixed_line_endings() {
    let _ = tokenize("var x = 1\r\nvar y = 2\rvar z = 3\n");
}

#[test]
fn parser_handles_empty_input() {
    if let Ok(tokens) = tokenize("") {
        let mut parser = Parser::new(tokens, "");
        let _ = parser.parse_script();
    }
}

#[test]
fn parser_handles_only_comments() {
    let input = "# comment 1\n# comment 2\n# comment 3\n";
    if let Ok(tokens) = tokenize(input) {
        let mut parser = Parser::new(tokens, input);
        let _ = parser.parse_script();
    }
}
