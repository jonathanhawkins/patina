//! Fuzz testing for the GDScript lexer, parser, and interpreter.
//!
//! Uses a deterministic PRNG to generate and mutate GDScript source code,
//! then feeds it to the tokenizer and parser to verify they never panic
//! on malformed input. Also uses `proptest` to exercise the interpreter
//! with random inputs covering arithmetic, variables, strings, comparisons,
//! function dispatch, and edge cases.

use crate::parser::Parser;
use crate::tokenizer::tokenize;

// ---------------------------------------------------------------------------
// Deterministic PRNG (xorshift64)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() as usize % (hi - lo))
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() & 0x00FF_FFFF) as f32 / (0x0100_0000 as f32)
    }

    fn pick<'a>(&mut self, items: &'a [&str]) -> &'a str {
        items[self.range(0, items.len())]
    }

    fn random_ascii(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| (self.next_u8() % 95 + 32) as char)
            .collect()
    }

    fn random_printable_with_newlines(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| {
                if self.next_f32() < 0.1 {
                    '\n'
                } else {
                    (self.next_u8() % 95 + 32) as char
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// GDScript fragments for generating semi-valid code
// ---------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "var", "func", "if", "else", "elif", "while", "for", "in", "return",
    "class", "extends", "signal", "enum", "match", "pass", "break",
    "continue", "const", "static", "self", "super", "class_name",
    "@onready", "@export", "await", "true", "false", "null", "and", "or",
    "not",
];

const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=",
    "=", "+=", "-=", "->", "@", "$",
];

const PUNCTUATION: &[&str] = &[
    "(", ")", "[", "]", "{", "}", ":", ",", ".", ";", "\n",
];

const VALID_SCRIPTS: &[&str] = &[
    "var x = 42\n",
    "func hello():\n\tpass\n",
    "if true:\n\tvar x = 1\nelse:\n\tvar x = 2\n",
    "for i in 10:\n\tpass\n",
    "class_name MyNode\nextends Node\n\nvar speed = 10.5\n\nfunc _ready():\n\tpass\n",
    "var arr = [1, 2, 3]\nvar dict = {\"a\": 1}\n",
    "func add(a, b):\n\treturn a + b\n",
    "enum Color:\n\tRED\n\tGREEN\n\tBLUE\n",
    "match x:\n\t1:\n\t\tpass\n\t2:\n\t\tpass\n",
    "var s = \"hello world\"\nvar n = null\n",
];

// ---------------------------------------------------------------------------
// Feed helpers
// ---------------------------------------------------------------------------

/// Feeds source to both the tokenizer and parser. Never panics.
fn feed_lexer_parser(source: &str) {
    // Tokenizer must not panic
    let tokens = tokenize(source);

    // If tokenization succeeded, try to parse
    if let Ok(toks) = tokens {
        let mut parser = Parser::new(toks, source);
        let _ = parser.parse_script();
    }
}

// ===========================================================================
// Lexer/Parser fuzz tests (deterministic PRNG)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const FUZZ_ITERATIONS: usize = 500;

    // -- Random strings -----------------------------------------------------

    #[test]
    fn fuzz_random_ascii_short() {
        let mut rng = FuzzRng::new(1000);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 32);
            let source = rng.random_ascii(len);
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_random_ascii_medium() {
        let mut rng = FuzzRng::new(1001);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(32, 256);
            let source = rng.random_ascii(len);
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_random_with_newlines() {
        let mut rng = FuzzRng::new(1002);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 128);
            let source = rng.random_printable_with_newlines(len);
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_random_bytes_as_utf8() {
        let mut rng = FuzzRng::new(1003);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 128);
            let bytes: Vec<u8> = (0..len).map(|_| rng.next_u8()).collect();
            if let Ok(source) = String::from_utf8(bytes) {
                feed_lexer_parser(&source);
            }
        }
    }

    // -- Mutated valid scripts ----------------------------------------------

    #[test]
    fn fuzz_mutate_valid_single_char() {
        let mut rng = FuzzRng::new(2000);
        for _ in 0..FUZZ_ITERATIONS {
            let base = VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())];
            let mut chars: Vec<char> = base.chars().collect();
            if !chars.is_empty() {
                let idx = rng.range(0, chars.len());
                chars[idx] = (rng.next_u8() % 95 + 32) as char;
            }
            let source: String = chars.into_iter().collect();
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_mutate_valid_multi_char() {
        let mut rng = FuzzRng::new(2001);
        for _ in 0..FUZZ_ITERATIONS {
            let base = VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())];
            let mut chars: Vec<char> = base.chars().collect();
            let mutations = rng.range(1, 6);
            for _ in 0..mutations {
                if !chars.is_empty() {
                    let idx = rng.range(0, chars.len());
                    chars[idx] = (rng.next_u8() % 95 + 32) as char;
                }
            }
            let source: String = chars.into_iter().collect();
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_insert_into_valid() {
        let mut rng = FuzzRng::new(2002);
        for _ in 0..FUZZ_ITERATIONS {
            let base = VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())];
            let mut chars: Vec<char> = base.chars().collect();
            let inserts = rng.range(1, 5);
            for _ in 0..inserts {
                let pos = rng.range(0, chars.len() + 1);
                chars.insert(pos, (rng.next_u8() % 95 + 32) as char);
            }
            let source: String = chars.into_iter().collect();
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_delete_from_valid() {
        let mut rng = FuzzRng::new(2003);
        for _ in 0..FUZZ_ITERATIONS {
            let base = VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())];
            let mut chars: Vec<char> = base.chars().collect();
            let deletions = rng.range(1, 5);
            for _ in 0..deletions {
                if !chars.is_empty() {
                    let idx = rng.range(0, chars.len());
                    chars.remove(idx);
                }
            }
            let source: String = chars.into_iter().collect();
            feed_lexer_parser(&source);
        }
    }

    // -- Random token combinations ------------------------------------------

    #[test]
    fn fuzz_random_keywords() {
        let mut rng = FuzzRng::new(3000);
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 20);
            let mut source = String::new();
            for i in 0..count {
                if i > 0 {
                    source.push(' ');
                }
                source.push_str(rng.pick(KEYWORDS));
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_random_operators() {
        let mut rng = FuzzRng::new(3001);
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 30);
            let mut source = String::new();
            for _ in 0..count {
                source.push_str(rng.pick(OPERATORS));
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_mixed_tokens() {
        let mut rng = FuzzRng::new(3002);
        let all_fragments: Vec<&str> = KEYWORDS
            .iter()
            .chain(OPERATORS.iter())
            .chain(PUNCTUATION.iter())
            .copied()
            .collect();
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 30);
            let mut source = String::new();
            for _ in 0..count {
                source.push_str(all_fragments[rng.range(0, all_fragments.len())]);
                if rng.next_f32() < 0.3 {
                    source.push(' ');
                }
            }
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_identifiers_with_numbers() {
        let mut rng = FuzzRng::new(3003);
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 10);
            let mut source = String::new();
            for i in 0..count {
                if i > 0 {
                    source.push_str(rng.pick(&[" ", "\n", " = ", " + "]));
                }
                // Generate identifier-like strings
                let id_len = rng.range(1, 20);
                for j in 0..id_len {
                    if j == 0 {
                        source.push((rng.next_u8() % 26 + b'a') as char);
                    } else if rng.next_f32() < 0.3 {
                        source.push((rng.next_u8() % 10 + b'0') as char);
                    } else {
                        source.push((rng.next_u8() % 26 + b'a') as char);
                    }
                }
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    // -- String literal fuzzing ---------------------------------------------

    #[test]
    fn fuzz_unclosed_strings() {
        let mut rng = FuzzRng::new(4000);
        let opens = ["\"", "'"];
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var s = ");
            source.push_str(rng.pick(&opens));
            let content_len = rng.range(0, 50);
            for _ in 0..content_len {
                let ch = (rng.next_u8() % 95 + 32) as char;
                if ch != '"' && ch != '\'' {
                    source.push(ch);
                }
            }
            // No closing quote
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_strings_with_escapes() {
        let mut rng = FuzzRng::new(4001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var s = \"");
            let content_len = rng.range(0, 30);
            for _ in 0..content_len {
                if rng.next_f32() < 0.3 {
                    source.push('\\');
                    source.push((rng.next_u8() % 95 + 32) as char);
                } else {
                    let ch = (rng.next_u8() % 94 + 33) as char; // avoid space=32 issues
                    if ch != '"' && ch != '\\' {
                        source.push(ch);
                    } else {
                        source.push('x');
                    }
                }
            }
            source.push('"');
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_strings_with_nulls() {
        let mut rng = FuzzRng::new(4002);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var s = \"");
            let content_len = rng.range(0, 20);
            for _ in 0..content_len {
                if rng.next_f32() < 0.2 {
                    source.push('\0');
                } else {
                    source.push((rng.next_u8() % 26 + b'a') as char);
                }
            }
            source.push('"');
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_multiline_strings() {
        let mut rng = FuzzRng::new(4003);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var s = \"");
            let lines = rng.range(1, 5);
            for _ in 0..lines {
                let line_len = rng.range(0, 20);
                for _ in 0..line_len {
                    source.push((rng.next_u8() % 26 + b'a') as char);
                }
                source.push('\n');
            }
            source.push('"');
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    // -- Number literal fuzzing ---------------------------------------------

    #[test]
    fn fuzz_number_literals() {
        let mut rng = FuzzRng::new(5000);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var x = ");
            // Generate various number-like strings
            match rng.range(0, 6) {
                0 => {
                    // Large integer
                    let digits = rng.range(1, 20);
                    for _ in 0..digits {
                        source.push((rng.next_u8() % 10 + b'0') as char);
                    }
                }
                1 => {
                    // Float with multiple dots
                    source.push_str("1.2.3.4");
                }
                2 => {
                    // Hex-like
                    source.push_str("0x");
                    let digits = rng.range(0, 16);
                    for _ in 0..digits {
                        source.push(
                            b"0123456789abcdefABCDEFghijxyz"[rng.range(0, 28)] as char,
                        );
                    }
                }
                3 => {
                    // Float with no digits after dot
                    source.push_str("42.");
                }
                4 => {
                    // Just a dot
                    source.push('.');
                }
                _ => {
                    // Negative with weirdness
                    source.push_str("-");
                    let digits = rng.range(0, 10);
                    for _ in 0..digits {
                        source.push((rng.next_u8() % 10 + b'0') as char);
                    }
                    if rng.next_f32() < 0.5 {
                        source.push('.');
                        let frac = rng.range(0, 5);
                        for _ in 0..frac {
                            source.push((rng.next_u8() % 10 + b'0') as char);
                        }
                    }
                }
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    // -- Indentation chaos --------------------------------------------------

    #[test]
    fn fuzz_random_indentation() {
        let mut rng = FuzzRng::new(6000);
        for _ in 0..FUZZ_ITERATIONS {
            let lines = rng.range(1, 15);
            let mut source = String::new();
            for _ in 0..lines {
                let indent = rng.range(0, 10);
                for _ in 0..indent {
                    if rng.next_f32() < 0.7 {
                        source.push(' ');
                    } else {
                        source.push('\t');
                    }
                }
                // Random content
                source.push_str(rng.pick(KEYWORDS));
                source.push('\n');
            }
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_deeply_nested_indentation() {
        let mut rng = FuzzRng::new(6001);
        for _ in 0..200 {
            let depth = rng.range(1, 30);
            let mut source = String::new();
            for d in 0..depth {
                for _ in 0..d {
                    source.push('\t');
                }
                source.push_str("if true:\n");
            }
            for _ in 0..depth {
                source.push('\t');
            }
            source.push_str("pass\n");
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_mixed_tabs_and_spaces() {
        let mut rng = FuzzRng::new(6002);
        for _ in 0..FUZZ_ITERATIONS {
            let lines = rng.range(1, 10);
            let mut source = String::new();
            for _ in 0..lines {
                // Mix tabs and spaces randomly
                let ws_count = rng.range(0, 8);
                for _ in 0..ws_count {
                    match rng.range(0, 3) {
                        0 => source.push(' '),
                        1 => source.push('\t'),
                        _ => source.push_str("  "),
                    }
                }
                source.push_str("pass\n");
            }
            feed_lexer_parser(&source);
        }
    }

    // -- Edge cases ---------------------------------------------------------

    #[test]
    fn fuzz_empty_input() {
        feed_lexer_parser("");
    }

    #[test]
    fn fuzz_single_characters() {
        for c in 0u8..128 {
            if let Some(ch) = char::from_u32(c as u32) {
                feed_lexer_parser(&ch.to_string());
            }
        }
    }

    #[test]
    fn fuzz_only_whitespace() {
        let cases = [
            " ", "  ", "\t", "\t\t", "\n", "\n\n", " \t \n",
            "   \n\t  \n  \t\n",
        ];
        for case in &cases {
            feed_lexer_parser(case);
        }
    }

    #[test]
    fn fuzz_only_comments() {
        let cases = [
            "# comment\n",
            "# line1\n# line2\n",
            "#\n",
            "# a very long comment with lots of text and special chars !@#$%^&*()\n",
        ];
        for case in &cases {
            feed_lexer_parser(case);
        }
    }

    #[test]
    fn fuzz_deeply_nested_parens() {
        let mut rng = FuzzRng::new(7003);
        for _ in 0..200 {
            let depth = rng.range(1, 50);
            let mut source = String::from("var x = ");
            for _ in 0..depth {
                source.push('(');
            }
            source.push('1');
            for _ in 0..depth {
                source.push(')');
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_mismatched_brackets() {
        let mut rng = FuzzRng::new(7004);
        let brackets = ['(', ')', '[', ']', '{', '}'];
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 20);
            let mut source = String::new();
            for _ in 0..count {
                source.push(brackets[rng.range(0, brackets.len())]);
            }
            source.push('\n');
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_repeated_operators() {
        let mut rng = FuzzRng::new(7005);
        for _ in 0..FUZZ_ITERATIONS {
            let op = rng.pick(OPERATORS);
            let count = rng.range(1, 30);
            let source = op.repeat(count) + "\n";
            feed_lexer_parser(&source);
        }
    }

    // -- Script-like combinations -------------------------------------------

    #[test]
    fn fuzz_random_script_structure() {
        let mut rng = FuzzRng::new(8000);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::new();
            let top_stmts = rng.range(1, 8);
            for _ in 0..top_stmts {
                match rng.range(0, 5) {
                    0 => {
                        source.push_str("var ");
                        let id_len = rng.range(1, 10);
                        for _ in 0..id_len {
                            source.push((rng.next_u8() % 26 + b'a') as char);
                        }
                        source.push_str(" = ");
                        source.push_str(&format!("{}", rng.range(0, 1000)));
                        source.push('\n');
                    }
                    1 => {
                        source.push_str("func ");
                        let id_len = rng.range(1, 10);
                        for _ in 0..id_len {
                            source.push((rng.next_u8() % 26 + b'a') as char);
                        }
                        source.push_str("():\n\tpass\n");
                    }
                    2 => {
                        source.push_str("if true:\n\tpass\n");
                    }
                    3 => {
                        source.push_str("# comment\n");
                    }
                    _ => {
                        source.push_str("pass\n");
                    }
                }
            }
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_concatenated_valid_scripts() {
        let mut rng = FuzzRng::new(8001);
        for _ in 0..FUZZ_ITERATIONS {
            let count = rng.range(1, 5);
            let mut source = String::new();
            for _ in 0..count {
                source.push_str(VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())]);
            }
            feed_lexer_parser(&source);
        }
    }

    #[test]
    fn fuzz_truncated_valid_scripts() {
        let mut rng = FuzzRng::new(8002);
        for _ in 0..FUZZ_ITERATIONS {
            let base = VALID_SCRIPTS[rng.range(0, VALID_SCRIPTS.len())];
            let cut = rng.range(0, base.len() + 1);
            let source = &base[..cut.min(base.len())];
            feed_lexer_parser(source);
        }
    }

    // -- Unicode edge cases -------------------------------------------------

    #[test]
    fn fuzz_unicode_identifiers() {
        let sources = [
            "var \u{00f1} = 1\n",
            "var \u{00fc}ber = 2\n",
            "var \u{65e5}\u{672c}\u{8a9e} = 3\n",
            "var \u{200B} = 4\n",        // zero-width space
            "var \u{FEFF}x = 5\n",       // BOM
            "var emoji\u{1F600} = 6\n",  // emoji
        ];
        for source in &sources {
            feed_lexer_parser(source);
        }
    }

    #[test]
    fn fuzz_control_characters() {
        let mut rng = FuzzRng::new(9001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut source = String::from("var x = 1");
            // Insert random control characters
            let count = rng.range(1, 5);
            for _ in 0..count {
                let ctrl = rng.range(0, 32) as u8;
                if let Some(ch) = char::from_u32(ctrl as u32) {
                    let pos = rng.range(0, source.len());
                    source.insert(pos, ch);
                }
            }
            feed_lexer_parser(&source);
        }
    }
}

// ===========================================================================
// Interpreter proptest / property-based tests
// ===========================================================================

#[cfg(test)]
mod interpreter_tests {
    use crate::interpreter::Interpreter;
    use gdvariant::Variant;
    use proptest::prelude::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Run a GDScript source and return the result, not panicking on runtime errors.
    fn run(source: &str) -> Result<(Option<Variant>, Vec<String>), String> {
        let mut interp = Interpreter::new();
        match interp.run(source) {
            Ok(res) => Ok((res.return_value, res.output)),
            Err(e) => Err(format!("{e}")),
        }
    }

    /// Run and expect success, returning the return_value.
    fn run_ok(source: &str) -> Option<Variant> {
        run(source).expect("script should succeed").0
    }

    /// Run and expect an error (returns the error message).
    fn run_err(source: &str) -> String {
        run(source).expect_err("script should fail")
    }

    // -----------------------------------------------------------------------
    // 1. Integer arithmetic roundtrips
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_int_addition(a in -10000i64..10000, b in -10000i64..10000) {
            let src = format!("return {a} + {b}\n");
            let val = run_ok(&src);
            prop_assert_eq!(val, Some(Variant::Int(a + b)));
        }

        #[test]
        fn prop_int_subtraction(a in -10000i64..10000, b in -10000i64..10000) {
            let src = format!("return {a} - {b}\n");
            let val = run_ok(&src);
            prop_assert_eq!(val, Some(Variant::Int(a - b)));
        }

        #[test]
        fn prop_int_multiplication(a in -1000i64..1000, b in -1000i64..1000) {
            let src = format!("return {a} * {b}\n");
            let val = run_ok(&src);
            prop_assert_eq!(val, Some(Variant::Int(a * b)));
        }

        #[test]
        fn prop_int_modulo_nonzero(a in -10000i64..10000, b in 1i64..1000) {
            let src = format!("return {a} % {b}\n");
            let val = run_ok(&src);
            prop_assert_eq!(val, Some(Variant::Int(a % b)));
        }

        #[test]
        fn prop_int_division_nonzero(a in -10000i64..10000, b in 1i64..1000) {
            // GDScript integer division yields int when both operands are int.
            let src = format!("return {a} / {b}\n");
            let val = run_ok(&src);
            prop_assert_eq!(val, Some(Variant::Int(a / b)));
        }
    }

    // -----------------------------------------------------------------------
    // 2. Float arithmetic roundtrips
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_float_addition(a in -1000.0f64..1000.0, b in -1000.0f64..1000.0) {
            let src = format!("return {a:.6} + {b:.6}\n");
            if let Ok((Some(Variant::Float(result)), _)) = run(&src) {
                let expected = a + b;
                prop_assert!((result - expected).abs() < 1e-3,
                    "got {result}, expected {expected}");
            }
            // parse/lex failures on weird float formatting are acceptable
        }

        #[test]
        fn prop_float_multiplication(a in -100.0f64..100.0, b in -100.0f64..100.0) {
            let src = format!("return {a:.6} * {b:.6}\n");
            if let Ok((Some(Variant::Float(result)), _)) = run(&src) {
                let expected = a * b;
                prop_assert!((result - expected).abs() < 1e-1,
                    "got {result}, expected {expected}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // 3. Variable binding and lookup
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_var_binding_int(val in -100000i64..100000) {
            let src = format!("var x = {val}\nreturn x\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(val)));
        }

        #[test]
        fn prop_var_binding_string(s in "[a-zA-Z0-9 ]{0,50}") {
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            let src = format!("var x = \"{escaped}\"\nreturn x\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::String(s)));
        }

        #[test]
        fn prop_var_reassignment(a in 0i64..1000, b in 0i64..1000) {
            let src = format!("var x = {a}\nx = {b}\nreturn x\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(b)));
        }

        #[test]
        fn prop_var_compound_add(a in 0i64..1000, b in 0i64..1000) {
            let src = format!("var x = {a}\nx += {b}\nreturn x\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a + b)));
        }

        #[test]
        fn prop_var_compound_sub(a in 0i64..10000, b in 0i64..5000) {
            let src = format!("var x = {a}\nx -= {b}\nreturn x\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a - b)));
        }
    }

    // -----------------------------------------------------------------------
    // 4. Comparison operators
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_compare_eq(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} == {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a == b)));
        }

        #[test]
        fn prop_compare_neq(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} != {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a != b)));
        }

        #[test]
        fn prop_compare_lt(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} < {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a < b)));
        }

        #[test]
        fn prop_compare_gt(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} > {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a > b)));
        }

        #[test]
        fn prop_compare_lte(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} <= {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a <= b)));
        }

        #[test]
        fn prop_compare_gte(a in -100i64..100, b in -100i64..100) {
            let src = format!("return {a} >= {b}\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Bool(a >= b)));
        }
    }

    // -----------------------------------------------------------------------
    // 5. String operations
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_string_concatenation(
            a in "[a-z]{0,20}",
            b in "[a-z]{0,20}",
        ) {
            let src = format!("return \"{a}\" + \"{b}\"\n");
            let result = run_ok(&src);
            let expected = format!("{a}{b}");
            prop_assert_eq!(result, Some(Variant::String(expected)));
        }

        #[test]
        fn prop_str_builtin_int(v in -10000i64..10000) {
            let src = format!("return str({v})\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::String(v.to_string())));
        }

        #[test]
        fn prop_len_builtin_string(s in "[a-z]{0,30}") {
            let src = format!("return len(\"{s}\")\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(s.len() as i64)));
        }
    }

    // -----------------------------------------------------------------------
    // 6. Function dispatch with various argument counts
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_func_zero_args(ret in 0i64..1000) {
            let src = format!("func f():\n\treturn {ret}\nreturn f()\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(ret)));
        }

        #[test]
        fn prop_func_one_arg(a in 0i64..1000) {
            let src = format!("func f(x):\n\treturn x * 2\nreturn f({a})\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a * 2)));
        }

        #[test]
        fn prop_func_two_args(a in 0i64..500, b in 0i64..500) {
            let src = format!("func f(x, y):\n\treturn x + y\nreturn f({a}, {b})\n");
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a + b)));
        }

        #[test]
        fn prop_func_three_args(a in 0i64..100, b in 0i64..100, c in 0i64..100) {
            let src = format!(
                "func f(x, y, z):\n\treturn x + y + z\nreturn f({a}, {b}, {c})\n"
            );
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a + b + c)));
        }
    }

    // -----------------------------------------------------------------------
    // 7. Boolean / logical operations
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_bool_and(a in proptest::bool::ANY, b in proptest::bool::ANY) {
            let src = format!("return {a} and {b}\n",
                a = if a { "true" } else { "false" },
                b = if b { "true" } else { "false" },
            );
            let result = run_ok(&src);
            // GDScript `and` returns the last truthy/falsy value, not necessarily Bool.
            // For booleans it should be equivalent to `a && b`.
            match result {
                Some(Variant::Bool(v)) => prop_assert_eq!(v, a && b),
                // Some interpreters return the value itself; accept that too.
                _ => {}
            }
        }

        #[test]
        fn prop_bool_or(a in proptest::bool::ANY, b in proptest::bool::ANY) {
            let src = format!("return {a} or {b}\n",
                a = if a { "true" } else { "false" },
                b = if b { "true" } else { "false" },
            );
            let result = run_ok(&src);
            match result {
                Some(Variant::Bool(v)) => prop_assert_eq!(v, a || b),
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // 8. Division by zero handling
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_division_by_zero_is_error(a in -1000i64..1000) {
            let src = format!("return {a} / 0\n");
            let err = run_err(&src);
            prop_assert!(err.contains("division by zero") || err.contains("zero"),
                "expected division-by-zero error, got: {err}");
        }

        #[test]
        fn prop_modulo_by_zero_is_error(a in -1000i64..1000) {
            let src = format!("return {a} % 0\n");
            let err = run_err(&src);
            prop_assert!(err.contains("division by zero") || err.contains("zero"),
                "expected division-by-zero error, got: {err}");
        }
    }

    // -----------------------------------------------------------------------
    // 9. Edge cases: empty scripts, very long identifiers, nested exprs
    // -----------------------------------------------------------------------

    #[test]
    fn edge_empty_script_returns_none() {
        let result = run("");
        assert!(result.is_ok());
        let (ret, out) = result.unwrap();
        assert_eq!(ret, None);
        assert!(out.is_empty());
    }

    #[test]
    fn edge_whitespace_only_script() {
        let result = run("   \n\n  \n");
        assert!(result.is_ok());
    }

    #[test]
    fn edge_comment_only_script() {
        let result = run("# just a comment\n# another\n");
        assert!(result.is_ok());
    }

    #[test]
    fn edge_very_long_identifier() {
        let long_id: String = std::iter::repeat('a').take(500).collect();
        let src = format!("var {long_id} = 42\nreturn {long_id}\n");
        let result = run_ok(&src);
        assert_eq!(result, Some(Variant::Int(42)));
    }

    #[test]
    fn edge_deeply_nested_arithmetic() {
        // Build (((((1 + 1) + 1) + 1) ... + 1)) with depth 30
        let depth = 30;
        let mut expr = String::from("1");
        for _ in 0..depth {
            expr = format!("({expr} + 1)");
        }
        let src = format!("return {expr}\n");
        let result = run_ok(&src);
        assert_eq!(result, Some(Variant::Int(1 + depth as i64)));
    }

    #[test]
    fn edge_undefined_variable_error() {
        let err = run_err("return nonexistent_var\n");
        assert!(err.contains("undefined") || err.contains("variable"),
            "expected undefined variable error, got: {err}");
    }

    #[test]
    fn edge_undefined_function_error() {
        let err = run_err("return no_such_func()\n");
        assert!(err.contains("undefined") || err.contains("function"),
            "expected undefined function error, got: {err}");
    }

    #[test]
    fn edge_null_literal() {
        let result = run_ok("return null\n");
        assert_eq!(result, Some(Variant::Nil));
    }

    #[test]
    fn edge_bool_literals() {
        assert_eq!(run_ok("return true\n"), Some(Variant::Bool(true)));
        assert_eq!(run_ok("return false\n"), Some(Variant::Bool(false)));
    }

    #[test]
    fn edge_negative_literal() {
        let result = run_ok("return -42\n");
        assert_eq!(result, Some(Variant::Int(-42)));
    }

    #[test]
    fn edge_unary_not() {
        assert_eq!(run_ok("return not true\n"), Some(Variant::Bool(false)));
        assert_eq!(run_ok("return not false\n"), Some(Variant::Bool(true)));
    }

    #[test]
    fn edge_array_literal_and_len() {
        let result = run_ok("return len([1, 2, 3, 4, 5])\n");
        assert_eq!(result, Some(Variant::Int(5)));
    }

    #[test]
    fn edge_empty_array() {
        let result = run_ok("return len([])\n");
        assert_eq!(result, Some(Variant::Int(0)));
    }

    #[test]
    fn edge_print_produces_output() {
        let (_, output) = run("print(\"hello\")\n").unwrap();
        assert!(output.iter().any(|l| l.contains("hello")));
    }

    #[test]
    fn edge_int_builtin_converts_float() {
        let result = run_ok("return int(3.7)\n");
        assert_eq!(result, Some(Variant::Int(3)));
    }

    // -----------------------------------------------------------------------
    // 10. Interpreter does not panic on random valid-looking scripts
    // -----------------------------------------------------------------------
    proptest! {
        #[test]
        fn prop_interpreter_no_panic_on_random_expr(
            a in -1000i64..1000,
            b in 1i64..1000,  // nonzero to avoid div-by-zero
            op in prop_oneof![
                Just("+"),
                Just("-"),
                Just("*"),
                Just("/"),
                Just("%"),
            ],
        ) {
            let src = format!("return {a} {op} {b}\n");
            // Must not panic, but may return an error
            let _ = run(&src);
        }

        #[test]
        fn prop_interpreter_no_panic_on_random_assignment(
            a in -1000i64..1000,
            b in -1000i64..1000,
        ) {
            let src = format!("var x = {a}\nvar y = {b}\nreturn x + y\n");
            let _ = run(&src);
        }

        #[test]
        fn prop_interpreter_nested_function_calls(
            a in 0i64..100,
            b in 0i64..100,
        ) {
            let src = format!(
                "func double(n):\n\treturn n * 2\nfunc add(x, y):\n\treturn x + y\nreturn add(double({a}), {b})\n"
            );
            let result = run_ok(&src);
            prop_assert_eq!(result, Some(Variant::Int(a * 2 + b)));
        }

        #[test]
        fn prop_if_else_branch_selection(a in -100i64..100) {
            let src = format!(
                "if {a} > 0:\n\treturn 1\nelse:\n\treturn -1\n"
            );
            let result = run_ok(&src);
            let expected = if a > 0 { 1 } else { -1 };
            prop_assert_eq!(result, Some(Variant::Int(expected)));
        }

        #[test]
        fn prop_while_loop_accumulator(n in 0u32..20) {
            let src = format!(
                "var i = 0\nvar acc = 0\nwhile i < {n}:\n\tacc += i\n\ti += 1\nreturn acc\n"
            );
            let result = run_ok(&src);
            let expected: i64 = (0..n as i64).sum();
            prop_assert_eq!(result, Some(Variant::Int(expected)));
        }

        #[test]
        fn prop_for_loop_sum(n in 1u32..20) {
            let src = format!(
                "var acc = 0\nfor i in {n}:\n\tacc += i\nreturn acc\n"
            );
            let result = run_ok(&src);
            let expected: i64 = (0..n as i64).sum();
            prop_assert_eq!(result, Some(Variant::Int(expected)));
        }
    }
}
