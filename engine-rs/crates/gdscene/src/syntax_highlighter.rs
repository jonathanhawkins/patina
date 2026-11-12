//! Runtime syntax highlighting for `TextEdit`/`CodeEdit` (pat-nzgdu.198).
//!
//! Godot's `SyntaxHighlighter` returns, for a given line, a map from a start
//! column to the colour that begins there; the `TextEdit` applies those spans
//! when it draws the line. `CodeHighlighter` is the concrete one whose colours
//! come from a keyword/member/number/symbol config
//! (`scene/resources/syntax_highlighter.cpp`).
//!
//! This is the runtime span computation the control renderer needs. It is not
//! the editor's highlighter (that is `gdeditor`) and it is not the ClassDB
//! schema (that is [`gdobject`]); it is the actual "text + config → coloured
//! runs" function, kept here so the renderer stays a pure function of the tree.
//!
//! Scope: **keyword colouring** — the path the bead's acceptance exercises
//! (`func` → blue). The schema registers `number_color` / `symbol_color` /
//! `function_color` / `member_keyword_colors` / `color_regions` too; colouring
//! those at draw time is a follow-up. Keyword matching is whole-word, so the
//! keyword `func` does not colour the identifier `function`.

use std::collections::BTreeMap;

use gdcore::math::Color;
use gdvariant::Variant;

/// A coloured run of a single line, in original text order.
#[derive(Debug, Clone, PartialEq)]
pub struct HlSpan {
    /// The substring this run covers, verbatim (so column positions line up).
    pub text: String,
    /// The colour to draw it in.
    pub color: Color,
}

/// The runtime side of Godot's `CodeHighlighter`: the keyword→colour config and
/// the default text colour, plus the per-line span computation a `TextEdit`
/// draws with.
///
/// No `Default`: the only sensible field to default is `default_color`, and
/// `gdcore`'s `Color` has no `Default` impl — construct with [`Self::new`],
/// which takes the text colour explicitly.
#[derive(Debug, Clone)]
pub struct CodeHighlighter {
    /// Whole-word keyword → colour. `BTreeMap` for deterministic iteration.
    keyword_colors: BTreeMap<String, Color>,
    /// Colour for everything not otherwise coloured.
    default_color: Color,
}

impl CodeHighlighter {
    /// A highlighter that colours only with `default_color` until keywords are
    /// added.
    pub fn new(default_color: Color) -> Self {
        Self {
            keyword_colors: BTreeMap::new(),
            default_color,
        }
    }

    /// Adds one keyword→colour mapping (Godot's `add_keyword_color`).
    pub fn add_keyword_color(&mut self, keyword: impl Into<String>, color: Color) {
        self.keyword_colors.insert(keyword.into(), color);
    }

    /// The colour bound to `keyword`, if any (Godot's `get_keyword_color`).
    pub fn keyword_color(&self, keyword: &str) -> Option<Color> {
        self.keyword_colors.get(keyword).copied()
    }

    /// `true` when no keyword is configured — such a highlighter draws the whole
    /// line in `default_color`, matching Godot's "an empty highlighter colours
    /// nothing" behaviour.
    pub fn is_empty(&self) -> bool {
        self.keyword_colors.is_empty()
    }

    /// Builds a highlighter from a `keyword_colors` [`Variant::Dictionary`] — the
    /// shape Godot's `CodeHighlighter.keyword_colors` has (`{ String: Color }`).
    /// Non-`String` keys and non-`Color` values are skipped rather than
    /// erroring, so a partially-typed dictionary degrades gracefully.
    pub fn from_keyword_dict(dict: &Variant, default_color: Color) -> Self {
        let mut hl = Self::new(default_color);
        if let Variant::Dictionary(d) = dict {
            for (k, v) in d.to_dict().iter() {
                if let (Variant::String(word), Variant::Color(color)) = (k, v) {
                    hl.add_keyword_color(word.clone(), *color);
                }
            }
        }
        hl
    }

    /// Splits one line into coloured runs. A maximal run of identifier
    /// characters (`[A-Za-z0-9_]`) is a *word*; a word that is a configured
    /// keyword takes its colour, everything else takes `default_color`.
    /// Adjacent runs of the same colour are merged, so a line with no keyword
    /// yields a single default-coloured span and a keyword yields exactly three
    /// (before / keyword / after, minus any empty edges).
    ///
    /// The concatenation of the returned `text`s always reproduces `line`
    /// exactly — nothing is dropped or reordered — so a caller can lay the runs
    /// out left to right and land on the original columns.
    pub fn line_spans(&self, line: &str) -> Vec<HlSpan> {
        let mut spans: Vec<HlSpan> = Vec::new();
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if is_word_byte(bytes[i]) {
                let start = i;
                while i < bytes.len() && is_word_byte(bytes[i]) {
                    i += 1;
                }
                let word = &line[start..i];
                let color = self.keyword_color(word).unwrap_or(self.default_color);
                push_run(&mut spans, word, color);
            } else {
                // A maximal run of non-word bytes, always the default colour.
                let start = i;
                while i < bytes.len() && !is_word_byte(bytes[i]) {
                    i += 1;
                }
                push_run(&mut spans, &line[start..i], self.default_color);
            }
        }
        spans
    }
}

/// Appends `text` to `spans`, merging into the previous run when the colour
/// matches so a line with no keyword collapses to one span.
fn push_run(spans: &mut Vec<HlSpan>, text: &str, color: Color) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.color == color {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(HlSpan {
        text: text.to_owned(),
        color,
    });
}

/// Identifier byte: ASCII alphanumeric or `_`. Non-ASCII (multi-byte UTF-8)
/// bytes are treated as non-word, which keeps the `&line[start..i]` slices on
/// char boundaries (ASCII word chars are single bytes) — a keyword is ASCII, so
/// this never splits a keyword.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blue() -> Color {
        Color::new(0.0, 0.0, 1.0, 1.0)
    }
    fn white() -> Color {
        Color::new(1.0, 1.0, 1.0, 1.0)
    }

    #[test]
    fn keyword_gets_its_own_colored_span() {
        let mut hl = CodeHighlighter::new(white());
        hl.add_keyword_color("func", blue());

        let spans = hl.line_spans("func foo()");
        // "func" (blue) then " foo()" (default) — the keyword is its own run.
        assert_eq!(
            spans,
            vec![
                HlSpan {
                    text: "func".into(),
                    color: blue()
                },
                HlSpan {
                    text: " foo()".into(),
                    color: white()
                },
            ]
        );
    }

    #[test]
    fn spans_reproduce_the_line_exactly() {
        let mut hl = CodeHighlighter::new(white());
        hl.add_keyword_color("return", blue());
        let line = "  return x + 1  ";
        let joined: String = hl
            .line_spans(line)
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(joined, line, "no text may be dropped or reordered");
    }

    #[test]
    fn keyword_matches_whole_words_only() {
        let mut hl = CodeHighlighter::new(white());
        hl.add_keyword_color("func", blue());

        // `function` contains `func` but is a different word — it must NOT color.
        let spans = hl.line_spans("function");
        assert_eq!(
            spans,
            vec![HlSpan {
                text: "function".into(),
                color: white()
            }]
        );

        // `func` as a standalone word DOES color.
        let spans = hl.line_spans("a func b");
        assert!(spans.iter().any(|s| s.text == "func" && s.color == blue()));
    }

    #[test]
    fn empty_highlighter_colors_nothing_but_default() {
        let hl = CodeHighlighter::new(white());
        assert!(hl.is_empty());
        let spans = hl.line_spans("func foo");
        // One default-colored run for the whole line.
        assert_eq!(
            spans,
            vec![HlSpan {
                text: "func foo".into(),
                color: white()
            }]
        );
    }

    #[test]
    fn multiple_keywords_each_colored() {
        let mut hl = CodeHighlighter::new(white());
        hl.add_keyword_color("func", blue());
        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        hl.add_keyword_color("return", red);

        let spans = hl.line_spans("func return");
        let colored: Vec<_> = spans
            .iter()
            .filter(|s| s.color != white())
            .map(|s| (s.text.clone(), s.color))
            .collect();
        assert_eq!(
            colored,
            vec![("func".into(), blue()), ("return".into(), red)]
        );
    }

    #[test]
    fn builds_from_keyword_dictionary() {
        let mut dict = gdvariant::Dictionary::new();
        dict.insert(Variant::String("func".into()), Variant::Color(blue()));
        let hl = CodeHighlighter::from_keyword_dict(&Variant::Dictionary(dict.into()), white());

        assert_eq!(hl.keyword_color("func"), Some(blue()));
        assert!(!hl.is_empty());
    }

    #[test]
    fn from_dict_skips_wrongly_typed_entries() {
        let mut dict = gdvariant::Dictionary::new();
        dict.insert(Variant::String("ok".into()), Variant::Color(blue()));
        dict.insert(Variant::Int(7), Variant::Color(blue())); // non-string key
        dict.insert(Variant::String("bad".into()), Variant::Int(3)); // non-color value
        let hl = CodeHighlighter::from_keyword_dict(&Variant::Dictionary(dict.into()), white());

        assert_eq!(hl.keyword_color("ok"), Some(blue()));
        assert_eq!(hl.keyword_color("bad"), None);
    }
}
