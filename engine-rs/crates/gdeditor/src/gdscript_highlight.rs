//! GDScript **syntax highlighting**.
//!
//! Produces colored spans for a GDScript source buffer: keywords, built-in
//! types, string literals, number literals, and comments. The highlighter holds
//! the current source and recomputes its spans whenever the text changes, so the
//! script editor can repaint as the user types.
//!
//! This is a lightweight lexical highlighter (not a full parser): it scans the
//! buffer once and emits a span per recognized token. Plain identifiers and
//! operators produce no span.

/// The kind of token a highlight span covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A language keyword (`func`, `var`, `if`, …).
    Keyword,
    /// A built-in type name (`int`, `Vector2`, `String`, …).
    Type,
    /// A string literal, including its quotes.
    StringLiteral,
    /// A numeric literal.
    Number,
    /// A `#` line comment, including the `#`.
    Comment,
}

/// A highlighted range of the source, as byte offsets `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Byte offset where the span starts.
    pub start: usize,
    /// Byte offset just past the span.
    pub end: usize,
    /// What the span represents.
    pub kind: TokenKind,
}

/// GDScript keywords.
const KEYWORDS: &[&str] = &[
    "if", "elif", "else", "for", "while", "match", "break", "continue", "pass",
    "return", "class", "class_name", "extends", "is", "as", "self", "tool",
    "signal", "func", "static", "const", "enum", "var", "onready", "export",
    "breakpoint", "preload", "yield", "await", "assert", "void", "and", "or",
    "not", "in", "true", "false", "null",
];

/// Built-in GDScript types.
const TYPES: &[&str] = &[
    "bool", "int", "float", "String", "StringName", "NodePath", "Vector2",
    "Vector2i", "Vector3", "Vector3i", "Rect2", "Transform2D", "Transform3D",
    "Color", "Array", "Dictionary", "Callable", "Signal", "Object", "Node",
    "Node2D", "Node3D", "Resource", "PackedScene", "Variant", "RID", "Basis",
    "Quaternion",
];

fn classify_word(word: &str) -> Option<TokenKind> {
    if KEYWORDS.contains(&word) {
        Some(TokenKind::Keyword)
    } else if TYPES.contains(&word) {
        Some(TokenKind::Type)
    } else {
        None
    }
}

/// Scans `source` and returns highlight spans in source order.
pub fn highlight(source: &str) -> Vec<HighlightSpan> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut spans = Vec::new();
    let mut i = 0;

    while i < n {
        let c = bytes[i];
        if c == b'#' {
            // Line comment through end of line (excluding the newline).
            let start = i;
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            spans.push(HighlightSpan {
                start,
                end: i,
                kind: TokenKind::Comment,
            });
        } else if c == b'"' || c == b'\'' {
            // String literal, honoring backslash escapes.
            let quote = c;
            let start = i;
            i += 1;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push(HighlightSpan {
                start,
                end: i,
                kind: TokenKind::StringLiteral,
            });
        } else if c.is_ascii_digit() {
            // Numeric literal (digits, decimal point, digit separators).
            let start = i;
            while i < n && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            spans.push(HighlightSpan {
                start,
                end: i,
                kind: TokenKind::Number,
            });
        } else if c.is_ascii_alphabetic() || c == b'_' {
            // Word: keyword, type, or plain identifier.
            let start = i;
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if let Some(kind) = classify_word(&source[start..i]) {
                spans.push(HighlightSpan { start, end: i, kind });
            }
        } else {
            i += 1;
        }
    }

    spans
}

/// Holds a GDScript buffer and its current highlight spans, recomputing on edit.
#[derive(Debug, Clone, Default)]
pub struct GdScriptHighlighter {
    source: String,
    spans: Vec<HighlightSpan>,
}

impl GdScriptHighlighter {
    /// Creates an empty highlighter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the source text and recomputes the highlight spans.
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
        self.spans = highlight(&self.source);
    }

    /// The current source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The current highlight spans, in source order.
    pub fn spans(&self) -> &[HighlightSpan] {
        &self.spans
    }

    /// The source text covered by a span.
    pub fn span_text(&self, span: HighlightSpan) -> &str {
        &self.source[span.start..span.end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(hl: &GdScriptHighlighter, text: &str, kind: TokenKind) -> bool {
        hl.spans()
            .iter()
            .any(|s| s.kind == kind && hl.span_text(*s) == text)
    }

    /// Acceptance (pat-wsg4l): opening a GDScript file highlights keywords,
    /// string/number literals, comments, and known types, and the spans update
    /// as the text changes.
    #[test]
    fn script_core_syntax_highlight() {
        let mut hl = GdScriptHighlighter::new();
        hl.set_source("func _ready():\n    var hp: int = 42 # set up\n    var s = \"hi\"\n");

        // Keywords.
        assert!(has(&hl, "func", TokenKind::Keyword));
        assert!(has(&hl, "var", TokenKind::Keyword));
        // Built-in type.
        assert!(has(&hl, "int", TokenKind::Type));
        // Number literal.
        assert!(has(&hl, "42", TokenKind::Number));
        // Comment (through end of line).
        assert!(has(&hl, "# set up", TokenKind::Comment));
        // String literal (with quotes).
        assert!(has(&hl, "\"hi\"", TokenKind::StringLiteral));
        // The identifier `hp` is not a keyword/type, so it gets no span.
        assert!(!hl.spans().iter().any(|s| hl.span_text(*s) == "hp"));

        // Editing the text recomputes the spans.
        hl.set_source("# only a comment\n");
        assert_eq!(hl.spans().len(), 1);
        assert_eq!(hl.spans()[0].kind, TokenKind::Comment);
        assert_eq!(hl.span_text(hl.spans()[0]), "# only a comment");

        // Another edit: a typed Vector2 with a float literal and string.
        hl.set_source("var pos: Vector2 = Vector2(1.5, 2.0) # pos\n");
        assert!(has(&hl, "Vector2", TokenKind::Type));
        assert!(has(&hl, "1.5", TokenKind::Number));
        assert!(has(&hl, "2.0", TokenKind::Number));
        assert!(has(&hl, "# pos", TokenKind::Comment));
    }

    /// Spans are emitted in source order and don't overlap.
    #[test]
    fn spans_are_ordered_and_disjoint() {
        let spans = highlight("var x = 1 # c\n");
        for pair in spans.windows(2) {
            assert!(pair[0].end <= pair[1].start, "spans are ordered and disjoint");
        }
        // A `#` inside a string is part of the string, not a comment.
        let s = highlight("var s = \"# not a comment\"");
        assert!(s.iter().any(|sp| sp.kind == TokenKind::StringLiteral));
        assert!(!s.iter().any(|sp| sp.kind == TokenKind::Comment));
    }
}
