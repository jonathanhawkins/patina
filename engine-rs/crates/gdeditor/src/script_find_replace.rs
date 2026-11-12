//! Script-editor in-file **find & replace** navigation (pat-dn4is).
//!
//! Find locates every match of a query in the buffer, honoring three toggles —
//! case-sensitivity, whole-word, and regex — and lets the user cycle forward and
//! backward through the matches (wrapping around). Replace substitutes the
//! current match; replace-all substitutes every match and reports how many were
//! replaced.
//!
//! All four search modes are expressed through the `regex` crate: a literal
//! (non-regex) query is escaped first, whole-word wraps the pattern in `\b`
//! anchors, and case-sensitivity maps to the case-insensitive build flag.

use regex::{Regex, RegexBuilder};

/// The find toggles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Match case exactly when true.
    pub case_sensitive: bool,
    /// Only match whole words (bounded by word boundaries) when true.
    pub whole_word: bool,
    /// Treat the query as a regular expression when true; otherwise literal.
    pub regex: bool,
}

impl FindOptions {
    /// A literal, case-insensitive, non-whole-word search.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets case sensitivity.
    pub fn case_sensitive(mut self, on: bool) -> Self {
        self.case_sensitive = on;
        self
    }

    /// Sets whole-word matching.
    pub fn whole_word(mut self, on: bool) -> Self {
        self.whole_word = on;
        self
    }

    /// Sets regex interpretation.
    pub fn regex(mut self, on: bool) -> Self {
        self.regex = on;
        self
    }
}

/// Builds a compiled matcher for `query` under `opts`, or `None` if the query is
/// empty or an invalid regex.
fn build(query: &str, opts: FindOptions) -> Option<Regex> {
    if query.is_empty() {
        return None;
    }
    let mut pat = if opts.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if opts.whole_word {
        pat = format!(r"\b(?:{})\b", pat);
    }
    RegexBuilder::new(&pat)
        .case_insensitive(!opts.case_sensitive)
        .build()
        .ok()
}

/// A find/replace session over a text buffer.
#[derive(Debug, Clone)]
pub struct ScriptFindReplace {
    text: String,
    options: FindOptions,
    query: String,
    matches: Vec<(usize, usize)>,
    current: usize,
}

impl ScriptFindReplace {
    /// Creates a session over `text` with no active query.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            options: FindOptions::new(),
            query: String::new(),
            matches: Vec::new(),
            current: 0,
        }
    }

    /// The current buffer text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the buffer text and re-runs the active query.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.recompute();
    }

    /// Runs `query` under `opts`, replacing the match set and selecting the
    /// first match. Returns the number of matches found.
    pub fn search(&mut self, query: &str, opts: FindOptions) -> usize {
        self.query = query.to_string();
        self.options = opts;
        self.current = 0;
        self.recompute();
        self.matches.len()
    }

    fn recompute(&mut self) {
        self.matches = match build(&self.query, self.options) {
            Some(re) => re
                .find_iter(&self.text)
                .map(|m| (m.start(), m.end()))
                .collect(),
            None => Vec::new(),
        };
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    /// The number of matches.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// All match ranges, in source order.
    pub fn matches(&self) -> &[(usize, usize)] {
        &self.matches
    }

    /// The currently-selected match range, if any.
    pub fn current_match(&self) -> Option<(usize, usize)> {
        self.matches.get(self.current).copied()
    }

    /// The buffer text covered by a match range.
    pub fn matched_text(&self, range: (usize, usize)) -> &str {
        &self.text[range.0..range.1]
    }

    /// Advances to the next match (wrapping) and returns it.
    pub fn next_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.current_match()
    }

    /// Steps to the previous match (wrapping) and returns it.
    pub fn prev_match(&mut self) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        self.current_match()
    }

    /// Replaces the currently-selected match with `replacement` (literally — no
    /// regex substitution syntax) and re-runs the query. Returns whether a match
    /// was replaced.
    pub fn replace_current(&mut self, replacement: &str) -> bool {
        match self.current_match() {
            Some((s, e)) => {
                self.text.replace_range(s..e, replacement);
                self.recompute();
                true
            }
            None => false,
        }
    }

    /// Replaces every match with `replacement` (literally) and returns the count
    /// that were replaced.
    pub fn replace_all(&mut self, replacement: &str) -> usize {
        let n = self.matches.len();
        if n == 0 {
            return 0;
        }
        // Splice right-to-left so earlier offsets stay valid.
        for &(s, e) in self.matches.clone().iter().rev() {
            self.text.replace_range(s..e, replacement);
        }
        self.current = 0;
        self.recompute();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-dn4is): find highlights and cycles matches honoring
    /// case/whole-word/regex toggles, and replace / replace-all substitute
    /// matches and report the replacement count.
    #[test]
    fn script_nav_find_replace() {
        let text = "Foo foo FOO food";

        // Case-insensitive, literal, not whole-word: every "foo" run matches,
        // including the one inside "food".
        let mut fr = ScriptFindReplace::new(text);
        assert_eq!(fr.search("foo", FindOptions::new()), 4);
        assert_eq!(fr.current_match(), Some((0, 3)));
        assert_eq!(fr.matched_text((0, 3)), "Foo");

        // Cycling wraps around the match set.
        assert_eq!(fr.next_match(), Some((4, 7)));
        assert_eq!(fr.next_match(), Some((8, 11)));
        assert_eq!(fr.next_match(), Some((12, 15)));
        assert_eq!(fr.next_match(), Some((0, 3))); // wrapped
        assert_eq!(fr.prev_match(), Some((12, 15))); // wraps backward

        // Case-sensitive: only the lowercase "foo" runs (in "foo" and "food").
        assert_eq!(fr.search("foo", FindOptions::new().case_sensitive(true)), 2);
        assert_eq!(fr.matched_text(fr.current_match().unwrap()), "foo");

        // Whole-word (case-insensitive): "food" is excluded, leaving 3.
        assert_eq!(fr.search("foo", FindOptions::new().whole_word(true)), 3);

        // Regex: words beginning with f — all four tokens.
        assert_eq!(fr.search(r"f\w+", FindOptions::new().regex(true)), 4);

        // Replace-all the case-insensitive "foo" runs with "bar"; reports 4.
        let mut fr = ScriptFindReplace::new(text);
        fr.search("foo", FindOptions::new());
        assert_eq!(fr.replace_all("bar"), 4);
        assert_eq!(fr.text(), "bar bar bar bard");

        // Replace just the current match.
        let mut fr = ScriptFindReplace::new("aa aa aa");
        assert_eq!(fr.search("aa", FindOptions::new()), 3);
        assert!(fr.replace_current("bb"));
        assert_eq!(fr.text(), "bb aa aa");
        // After replacing, the query still has the remaining two matches.
        assert_eq!(fr.match_count(), 2);

        // Replace-all with no matches reports zero and leaves the text alone.
        let mut fr = ScriptFindReplace::new("hello");
        fr.search("zzz", FindOptions::new());
        assert_eq!(fr.replace_all("x"), 0);
        assert_eq!(fr.text(), "hello");
    }

    /// An invalid regex yields no matches rather than panicking.
    #[test]
    fn script_find_replace_invalid_regex_is_empty() {
        let mut fr = ScriptFindReplace::new("abc");
        assert_eq!(fr.search("(unclosed", FindOptions::new().regex(true)), 0);
        assert!(fr.current_match().is_none());
    }
}
