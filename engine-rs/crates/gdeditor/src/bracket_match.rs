//! Bracket/quote auto-close and matching-pair highlighting for the script editor.
//!
//! Two behaviors:
//! 1. **Auto-close** — typing an opening bracket (`(`, `[`, `{`) or a quote
//!    (`"`, `'`) inserts the closing counterpart and leaves the caret between
//!    the pair. Typing a closing bracket (or quote) when that same character is
//!    already immediately to the right "types over" it instead of inserting a
//!    duplicate.
//! 2. **Matching-pair highlight** — given the caret position, locate the bracket
//!    adjacent to the caret and its matching partner so the editor can highlight
//!    both. Matching is nesting-aware and ignores brackets that live inside
//!    string literals.
//!
//! Offsets are byte offsets into a single line. The functions operate on one
//! line at a time, which is sufficient for auto-close and for the common case of
//! same-line bracket matching.

/// The closing counterpart for an opening bracket or quote, if `ch` opens a pair.
pub fn closing_for(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

/// Whether `ch` is a closing bracket (`)`, `]`, `}`).
pub fn is_closing(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

/// Whether `ch` is a quote character (`"` or `'`).
pub fn is_quote(ch: char) -> bool {
    ch == '"' || ch == '\''
}

/// The opening bracket that matches a closing bracket, for `()[]{}` only.
fn bracket_open(close: char) -> Option<char> {
    match close {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// The closing bracket that matches an opening bracket, for `()[]{}` only.
fn bracket_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

/// The result of typing a character with auto-close behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCloseEdit {
    /// The line text after the edit.
    pub line: String,
    /// The caret byte offset after the edit.
    pub caret: usize,
    /// Whether a closing counterpart was auto-inserted.
    pub auto_closed: bool,
}

/// Inserts `ch` at byte offset `caret` in `line`, applying auto-close rules.
///
/// - If `ch` opens a pair, the matching closer is inserted too and the caret is
///   placed between them (`auto_closed = true`).
/// - If `ch` is a closing bracket or quote and the character immediately to the
///   right of the caret is exactly `ch`, the caret simply moves past it (no
///   insertion) — natural "type-over".
/// - Otherwise `ch` is inserted normally.
pub fn insert_char(line: &str, caret: usize, ch: char) -> AutoCloseEdit {
    let caret = caret.min(line.len());
    let next = line[caret..].chars().next();

    // Type-over: skip past an existing matching closer/quote.
    if (is_closing(ch) || is_quote(ch)) && next == Some(ch) {
        return AutoCloseEdit {
            line: line.to_string(),
            caret: caret + ch.len_utf8(),
            auto_closed: false,
        };
    }

    let mut out = String::with_capacity(line.len() + 2);
    out.push_str(&line[..caret]);
    if let Some(close) = closing_for(ch) {
        out.push(ch);
        out.push(close);
        out.push_str(&line[caret..]);
        AutoCloseEdit {
            line: out,
            caret: caret + ch.len_utf8(),
            auto_closed: true,
        }
    } else {
        out.push(ch);
        out.push_str(&line[caret..]);
        AutoCloseEdit {
            line: out,
            caret: caret + ch.len_utf8(),
            auto_closed: false,
        }
    }
}

/// Finds the matching bracket pair adjacent to `caret` (a byte offset).
///
/// Returns the byte offsets `(open, close)` of the two bracket characters, with
/// `open < close`. The bracket considered is, in priority order: the one
/// starting at the caret if it opens, the one ending at the caret if it closes,
/// the one at the caret if it closes, then the one ending at the caret if it
/// opens. Brackets inside string literals are skipped. Returns `None` when no
/// bracket is adjacent to the caret or the match is unbalanced.
pub fn matching_pair(line: &str, caret: usize) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    // Vec index of the char starting exactly at `caret` (None if caret == len).
    let at = chars.iter().position(|(b, _)| *b == caret);
    // Vec index of the char immediately preceding `caret`.
    let before = if caret == 0 {
        None
    } else {
        chars.iter().rposition(|(b, _)| *b < caret)
    };

    if let Some(i) = at {
        if bracket_close(chars[i].1).is_some() {
            return match_forward(&chars, i);
        }
    }
    if let Some(i) = before {
        if bracket_open(chars[i].1).is_some() {
            return match_backward(&chars, i);
        }
    }
    if let Some(i) = at {
        if bracket_open(chars[i].1).is_some() {
            return match_backward(&chars, i);
        }
    }
    if let Some(i) = before {
        if bracket_close(chars[i].1).is_some() {
            return match_forward(&chars, i);
        }
    }
    None
}

/// Scans forward from the opening bracket at vec index `i` to its match.
fn match_forward(chars: &[(usize, char)], i: usize) -> Option<(usize, usize)> {
    let open = chars[i].1;
    let close = bracket_close(open)?;
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    for &(b, ch) in &chars[i + 1..] {
        if let Some(q) = in_string {
            if ch == q {
                in_string = None;
            }
            continue;
        }
        if is_quote(ch) {
            in_string = Some(ch);
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return Some((chars[i].0, b));
            }
            depth -= 1;
        }
    }
    None
}

/// Scans backward from the closing bracket at vec index `i` to its match.
fn match_backward(chars: &[(usize, char)], i: usize) -> Option<(usize, usize)> {
    let close = chars[i].1;
    let open = bracket_open(close)?;
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    for &(b, ch) in chars[..i].iter().rev() {
        if let Some(q) = in_string {
            if ch == q {
                in_string = None;
            }
            continue;
        }
        if is_quote(ch) {
            in_string = Some(ch);
        } else if ch == close {
            depth += 1;
        } else if ch == open {
            if depth == 0 {
                return Some((b, chars[i].0));
            }
            depth -= 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-8txu8): typing an opening bracket or quote inserts the
    /// closing counterpart with the caret between them, and the editor can
    /// highlight the matching pair around the caret.
    #[test]
    fn script_core_bracket_autoclose_match() {
        // Typing an opener inserts the closer with the caret between the pair.
        let e = insert_char("foo", 3, '(');
        assert_eq!(e.line, "foo()");
        assert_eq!(e.caret, 4);
        assert!(e.auto_closed);

        // Works mid-line: caret stays between the inserted pair.
        let e = insert_char("ab", 1, '[');
        assert_eq!(e.line, "a[]b");
        assert_eq!(e.caret, 2);
        assert!(e.auto_closed);

        // Quotes auto-close too.
        let e = insert_char("x = ", 4, '"');
        assert_eq!(e.line, "x = \"\"");
        assert_eq!(e.caret, 5);
        assert!(e.auto_closed);

        // A non-pair character inserts normally, no auto-close.
        let e = insert_char("a", 1, ';');
        assert_eq!(e.line, "a;");
        assert_eq!(e.caret, 2);
        assert!(!e.auto_closed);

        // Typing the closer when it already sits to the right types over it
        // instead of inserting a duplicate.
        let e = insert_char("()", 1, ')');
        assert_eq!(e.line, "()");
        assert_eq!(e.caret, 2);
        assert!(!e.auto_closed);

        // The same applies to quotes (e.g. closing an auto-inserted pair).
        let e = insert_char("\"\"", 1, '"');
        assert_eq!(e.line, "\"\"");
        assert_eq!(e.caret, 2);
        assert!(!e.auto_closed);

        // Matching-pair highlight: caret just after the opener highlights both.
        assert_eq!(matching_pair("foo()", 4), Some((3, 4)));
        // Caret just after the closer also highlights the pair.
        assert_eq!(matching_pair("foo()", 5), Some((3, 4)));
        // Nested brackets resolve to the correct partner.
        //   f(0) ((1) a(2) [(3) 0(4) ](5) )(6)
        assert_eq!(matching_pair("f(a[0])", 4), Some((3, 5))); // after '[' -> ']'
        assert_eq!(matching_pair("f(a[0])", 2), Some((1, 6))); // after '(' -> outer ')'
        // No bracket adjacent to the caret -> None.
        assert_eq!(matching_pair("foo()", 1), None);
        // Brackets inside a string literal are ignored when matching.
        //   p(0) ((1) "(2) )(3) "(4) )(5)  -> '(' matches the ')' at 5, not 3.
        assert_eq!(matching_pair("p(\")\")", 2), Some((1, 5)));
    }

    /// Pair-classification helpers behave as documented.
    #[test]
    fn pair_classification() {
        assert_eq!(closing_for('('), Some(')'));
        assert_eq!(closing_for('['), Some(']'));
        assert_eq!(closing_for('{'), Some('}'));
        assert_eq!(closing_for('"'), Some('"'));
        assert_eq!(closing_for('\''), Some('\''));
        assert_eq!(closing_for('a'), None);

        assert!(is_closing(')'));
        assert!(is_closing(']'));
        assert!(is_closing('}'));
        assert!(!is_closing('('));

        assert!(is_quote('"'));
        assert!(is_quote('\''));
        assert!(!is_quote('`'));
    }

    /// Matching is symmetric across all three bracket kinds and reports `None`
    /// for unbalanced input.
    #[test]
    fn matching_pair_kinds_and_unbalanced() {
        assert_eq!(matching_pair("[a]", 1), Some((0, 2))); // after '['
        assert_eq!(matching_pair("{a}", 3), Some((0, 2))); // after '}'
        assert_eq!(matching_pair("(", 1), None); // unbalanced opener
        assert_eq!(matching_pair(")", 1), None); // unbalanced closer
        assert_eq!(matching_pair("", 0), None); // empty line
    }
}
