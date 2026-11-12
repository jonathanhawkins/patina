//! Script editor find and replace with regex support.
//!
//! Provides a headless find-and-replace engine for the script editor.
//! Supports plain text and regex search, case sensitivity, whole-word
//! matching, and batch or incremental replacement.
//!
//! The [`FindReplace`] struct holds configuration and operates on a text
//! buffer (a `&str` or `&mut String`) without owning the document.

use regex::{Regex, RegexBuilder};

// ---------------------------------------------------------------------------
// SearchMatch
// ---------------------------------------------------------------------------

/// A single match found in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Byte offset of the match start.
    pub start: usize,
    /// Byte offset of the match end (exclusive).
    pub end: usize,
    /// The line number (0-based) where the match starts.
    pub line: usize,
    /// The column (0-based, in bytes) where the match starts within the line.
    pub column: usize,
    /// The matched text.
    pub text: String,
}

// ---------------------------------------------------------------------------
// ReplaceResult
// ---------------------------------------------------------------------------

/// Result of a replace operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceResult {
    /// The new text after replacement.
    pub text: String,
    /// Number of replacements made.
    pub count: usize,
}

// ---------------------------------------------------------------------------
// FindReplaceError
// ---------------------------------------------------------------------------

/// Errors from find/replace operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindReplaceError {
    /// The regex pattern is invalid.
    InvalidRegex(String),
    /// The search pattern is empty.
    EmptyPattern,
    /// Match index out of bounds.
    MatchIndexOutOfBounds(usize),
}

impl std::fmt::Display for FindReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRegex(msg) => write!(f, "invalid regex: {msg}"),
            Self::EmptyPattern => write!(f, "search pattern is empty"),
            Self::MatchIndexOutOfBounds(idx) => write!(f, "match index {idx} out of bounds"),
        }
    }
}

// ---------------------------------------------------------------------------
// SearchMode
// ---------------------------------------------------------------------------

/// How to interpret the search pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Plain text (literal) matching.
    #[default]
    PlainText,
    /// Regular expression matching.
    Regex,
}

// ---------------------------------------------------------------------------
// FindReplaceConfig
// ---------------------------------------------------------------------------

/// Configuration for a find/replace operation.
#[derive(Debug, Clone)]
pub struct FindReplaceConfig {
    /// The search pattern.
    pub pattern: String,
    /// The replacement string (used for replace operations).
    pub replacement: String,
    /// Search mode (plain text or regex).
    pub mode: SearchMode,
    /// Whether the search is case-sensitive.
    pub case_sensitive: bool,
    /// Whether to match whole words only (plain text mode).
    pub whole_word: bool,
    /// Whether to wrap around the document.
    pub wrap_around: bool,
}

impl Default for FindReplaceConfig {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            replacement: String::new(),
            mode: SearchMode::PlainText,
            case_sensitive: false,
            whole_word: false,
            wrap_around: true,
        }
    }
}

impl FindReplaceConfig {
    /// Creates a new config with the given search pattern.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    /// Sets the replacement string.
    pub fn with_replacement(mut self, replacement: impl Into<String>) -> Self {
        self.replacement = replacement.into();
        self
    }

    /// Sets regex mode.
    pub fn with_regex(mut self) -> Self {
        self.mode = SearchMode::Regex;
        self
    }

    /// Sets case sensitivity.
    pub fn case_sensitive(mut self, yes: bool) -> Self {
        self.case_sensitive = yes;
        self
    }

    /// Sets whole-word matching.
    pub fn whole_word(mut self, yes: bool) -> Self {
        self.whole_word = yes;
        self
    }
}

// ---------------------------------------------------------------------------
// FindReplace
// ---------------------------------------------------------------------------

/// The find-and-replace engine.
///
/// Stateless — each operation takes a config and text. The engine builds
/// the appropriate regex or literal matcher from the config.
#[derive(Debug, Default)]
pub struct FindReplace;

impl FindReplace {
    /// Creates a new engine.
    pub fn new() -> Self {
        Self
    }

    /// Finds all matches in the given text.
    pub fn find_all(
        &self,
        text: &str,
        config: &FindReplaceConfig,
    ) -> Result<Vec<SearchMatch>, FindReplaceError> {
        if config.pattern.is_empty() {
            return Err(FindReplaceError::EmptyPattern);
        }

        let re = self.build_regex(config)?;
        let line_starts = compute_line_starts(text);
        let mut matches = Vec::new();

        for m in re.find_iter(text) {
            let (line, col) = offset_to_line_col(&line_starts, m.start());
            matches.push(SearchMatch {
                start: m.start(),
                end: m.end(),
                line,
                column: col,
                text: m.as_str().to_string(),
            });
        }

        Ok(matches)
    }

    /// Counts the number of matches without collecting them.
    pub fn count_matches(
        &self,
        text: &str,
        config: &FindReplaceConfig,
    ) -> Result<usize, FindReplaceError> {
        if config.pattern.is_empty() {
            return Err(FindReplaceError::EmptyPattern);
        }
        let re = self.build_regex(config)?;
        Ok(re.find_iter(text).count())
    }

    /// Finds the next match at or after the given byte offset.
    /// If `wrap_around` is true and no match is found after the offset,
    /// wraps to the beginning.
    pub fn find_next(
        &self,
        text: &str,
        config: &FindReplaceConfig,
        from_offset: usize,
    ) -> Result<Option<SearchMatch>, FindReplaceError> {
        if config.pattern.is_empty() {
            return Err(FindReplaceError::EmptyPattern);
        }

        let re = self.build_regex(config)?;
        let line_starts = compute_line_starts(text);

        // Search from offset.
        let search_text = if from_offset < text.len() {
            &text[from_offset..]
        } else {
            ""
        };

        if let Some(m) = re.find(search_text) {
            let abs_start = from_offset + m.start();
            let abs_end = from_offset + m.end();
            let (line, col) = offset_to_line_col(&line_starts, abs_start);
            return Ok(Some(SearchMatch {
                start: abs_start,
                end: abs_end,
                line,
                column: col,
                text: m.as_str().to_string(),
            }));
        }

        // Wrap around.
        if config.wrap_around && from_offset > 0 {
            if let Some(m) = re.find(text) {
                if m.start() < from_offset {
                    let (line, col) = offset_to_line_col(&line_starts, m.start());
                    return Ok(Some(SearchMatch {
                        start: m.start(),
                        end: m.end(),
                        line,
                        column: col,
                        text: m.as_str().to_string(),
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Replaces all matches and returns the new text with count.
    pub fn replace_all(
        &self,
        text: &str,
        config: &FindReplaceConfig,
    ) -> Result<ReplaceResult, FindReplaceError> {
        if config.pattern.is_empty() {
            return Err(FindReplaceError::EmptyPattern);
        }

        let re = self.build_regex(config)?;
        let mut count = 0usize;
        let result = re.replace_all(text, |_caps: &regex::Captures| {
            count += 1;
            config.replacement.as_str()
        });

        Ok(ReplaceResult {
            text: result.into_owned(),
            count,
        })
    }

    /// Replaces a single match at the given index (0-based among all matches).
    pub fn replace_at(
        &self,
        text: &str,
        config: &FindReplaceConfig,
        match_index: usize,
    ) -> Result<ReplaceResult, FindReplaceError> {
        let matches = self.find_all(text, config)?;
        if match_index >= matches.len() {
            return Err(FindReplaceError::MatchIndexOutOfBounds(match_index));
        }

        let m = &matches[match_index];
        let mut result = String::with_capacity(text.len());
        result.push_str(&text[..m.start]);
        result.push_str(&config.replacement);
        result.push_str(&text[m.end..]);

        Ok(ReplaceResult {
            text: result,
            count: 1,
        })
    }

    /// Builds a regex from the config.
    fn build_regex(&self, config: &FindReplaceConfig) -> Result<Regex, FindReplaceError> {
        let pattern = match config.mode {
            SearchMode::PlainText => {
                let escaped = regex::escape(&config.pattern);
                if config.whole_word {
                    format!(r"\b{escaped}\b")
                } else {
                    escaped
                }
            }
            SearchMode::Regex => config.pattern.clone(),
        };

        RegexBuilder::new(&pattern)
            .case_insensitive(!config.case_sensitive)
            .build()
            .map_err(|e| FindReplaceError::InvalidRegex(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Computes the byte offset of each line start.
fn compute_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Converts a byte offset to (line, column) using precomputed line starts.
fn offset_to_line_col(line_starts: &[usize], offset: usize) -> (usize, usize) {
    let line = match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    let col = offset - line_starts[line];
    (line, col)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> FindReplace {
        FindReplace::new()
    }

    const SAMPLE: &str =
        "func _ready():\n    var speed = 10\n    var speed_max = 100\n    print(speed)\n";

    // -- SearchMatch --

    #[test]
    fn search_match_fields() {
        let m = SearchMatch {
            start: 0,
            end: 4,
            line: 0,
            column: 0,
            text: "func".into(),
        };
        assert_eq!(m.text, "func");
        assert_eq!(m.line, 0);
    }

    // -- FindReplaceConfig --

    #[test]
    fn config_defaults() {
        let cfg = FindReplaceConfig::default();
        assert!(cfg.pattern.is_empty());
        assert!(!cfg.case_sensitive);
        assert!(!cfg.whole_word);
        assert!(cfg.wrap_around);
        assert_eq!(cfg.mode, SearchMode::PlainText);
    }

    #[test]
    fn config_builder() {
        let cfg = FindReplaceConfig::new("test")
            .with_replacement("new")
            .with_regex()
            .case_sensitive(true)
            .whole_word(true);
        assert_eq!(cfg.pattern, "test");
        assert_eq!(cfg.replacement, "new");
        assert_eq!(cfg.mode, SearchMode::Regex);
        assert!(cfg.case_sensitive);
        assert!(cfg.whole_word);
    }

    // -- Empty pattern --

    #[test]
    fn empty_pattern_error() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("");
        assert_eq!(
            fr.find_all("text", &cfg).unwrap_err(),
            FindReplaceError::EmptyPattern
        );
    }

    // -- Plain text find --

    #[test]
    fn find_all_plain() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed");
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 3); // speed, speed_max, speed
    }

    #[test]
    fn find_all_plain_case_insensitive() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("SPEED");
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 3); // case insensitive by default
    }

    #[test]
    fn find_all_plain_case_sensitive() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("SPEED").case_sensitive(true);
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn find_all_whole_word() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed").whole_word(true);
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        // "speed" as whole word: line 2 ("var speed = 10") and line 4 ("print(speed)")
        // "speed_max" should NOT match
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn find_all_line_and_column() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("func");
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[0].column, 0);
    }

    #[test]
    fn find_all_second_line() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("var").case_sensitive(true);
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 1);
        assert_eq!(matches[1].line, 2);
    }

    // -- Regex find --

    #[test]
    fn find_all_regex() {
        let fr = engine();
        let cfg = FindReplaceConfig::new(r"speed\b").with_regex();
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        // matches "speed" but not "speed_max"
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn find_all_regex_groups() {
        let fr = engine();
        let cfg = FindReplaceConfig::new(r"\d+").with_regex();
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 2); // 10 and 100
        assert_eq!(matches[0].text, "10");
        assert_eq!(matches[1].text, "100");
    }

    #[test]
    fn invalid_regex_error() {
        let fr = engine();
        let cfg = FindReplaceConfig::new(r"[invalid").with_regex();
        let result = fr.find_all("text", &cfg);
        assert!(matches!(result, Err(FindReplaceError::InvalidRegex(_))));
    }

    // -- Count --

    #[test]
    fn count_matches_plain() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed");
        assert_eq!(fr.count_matches(SAMPLE, &cfg).unwrap(), 3);
    }

    #[test]
    fn count_matches_regex() {
        let fr = engine();
        let cfg = FindReplaceConfig::new(r"\d+").with_regex();
        assert_eq!(fr.count_matches(SAMPLE, &cfg).unwrap(), 2);
    }

    // -- Find next --

    #[test]
    fn find_next_from_start() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("var");
        let m = fr.find_next(SAMPLE, &cfg, 0).unwrap().unwrap();
        assert_eq!(m.line, 1);
    }

    #[test]
    fn find_next_from_middle() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("var");
        // Find first, then find next from after it.
        let first = fr.find_next(SAMPLE, &cfg, 0).unwrap().unwrap();
        let second = fr.find_next(SAMPLE, &cfg, first.end).unwrap().unwrap();
        assert_eq!(second.line, 2);
    }

    #[test]
    fn find_next_wrap_around() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("func");
        // Start past "func", should wrap around.
        let m = fr.find_next(SAMPLE, &cfg, 10).unwrap().unwrap();
        assert_eq!(m.start, 0);
    }

    #[test]
    fn find_next_no_wrap() {
        let fr = engine();
        let mut cfg = FindReplaceConfig::new("func");
        cfg.wrap_around = false;
        let m = fr.find_next(SAMPLE, &cfg, 10).unwrap();
        assert!(m.is_none());
    }

    #[test]
    fn find_next_past_end() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("func");
        let m = fr.find_next(SAMPLE, &cfg, SAMPLE.len() + 100).unwrap();
        // Should wrap and find it.
        assert!(m.is_some());
    }

    // -- Replace all --

    #[test]
    fn replace_all_plain() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed")
            .with_replacement("velocity")
            .whole_word(true);
        let result = fr.replace_all(SAMPLE, &cfg).unwrap();
        assert_eq!(result.count, 2);
        assert!(result.text.contains("velocity"));
        assert!(result.text.contains("speed_max")); // whole word, not replaced
    }

    #[test]
    fn replace_all_regex() {
        let fr = engine();
        let cfg = FindReplaceConfig::new(r"\d+")
            .with_regex()
            .with_replacement("0");
        let result = fr.replace_all(SAMPLE, &cfg).unwrap();
        assert_eq!(result.count, 2);
        assert!(result.text.contains("var speed = 0"));
        assert!(result.text.contains("var speed_max = 0"));
    }

    #[test]
    fn replace_all_empty_replacement() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("var ")
            .with_replacement("")
            .case_sensitive(true);
        let result = fr.replace_all(SAMPLE, &cfg).unwrap();
        assert_eq!(result.count, 2);
        assert!(!result.text.contains("var "));
    }

    #[test]
    fn replace_all_no_matches() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("zzzzz").with_replacement("x");
        let result = fr.replace_all(SAMPLE, &cfg).unwrap();
        assert_eq!(result.count, 0);
        assert_eq!(result.text, SAMPLE);
    }

    // -- Replace at index --

    #[test]
    fn replace_at_first() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed").with_replacement("vel");
        let result = fr.replace_at(SAMPLE, &cfg, 0).unwrap();
        assert_eq!(result.count, 1);
        // Only first "speed" replaced.
        let remaining = FindReplaceConfig::new("speed");
        let after = fr.count_matches(&result.text, &remaining).unwrap();
        assert_eq!(after, 2); // was 3, now 2
    }

    #[test]
    fn replace_at_last() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed").with_replacement("vel");
        let result = fr.replace_at(SAMPLE, &cfg, 2).unwrap();
        assert_eq!(result.count, 1);
    }

    #[test]
    fn replace_at_out_of_bounds() {
        let fr = engine();
        let cfg = FindReplaceConfig::new("speed").with_replacement("x");
        let result = fr.replace_at(SAMPLE, &cfg, 99);
        assert!(matches!(
            result,
            Err(FindReplaceError::MatchIndexOutOfBounds(99))
        ));
    }

    // -- Error display --

    #[test]
    fn error_display() {
        assert_eq!(
            FindReplaceError::EmptyPattern.to_string(),
            "search pattern is empty"
        );
        assert_eq!(
            FindReplaceError::MatchIndexOutOfBounds(5).to_string(),
            "match index 5 out of bounds"
        );
        let e = FindReplaceError::InvalidRegex("bad".into());
        assert!(e.to_string().contains("invalid regex"));
    }

    // -- Line/column helpers --

    #[test]
    fn line_starts_single_line() {
        let starts = compute_line_starts("hello");
        assert_eq!(starts, vec![0]);
    }

    #[test]
    fn line_starts_multi_line() {
        let starts = compute_line_starts("a\nb\nc");
        assert_eq!(starts, vec![0, 2, 4]);
    }

    #[test]
    fn offset_to_line_col_first_line() {
        let starts = compute_line_starts("hello\nworld");
        assert_eq!(offset_to_line_col(&starts, 0), (0, 0));
        assert_eq!(offset_to_line_col(&starts, 3), (0, 3));
    }

    #[test]
    fn offset_to_line_col_second_line() {
        let starts = compute_line_starts("hello\nworld");
        assert_eq!(offset_to_line_col(&starts, 6), (1, 0));
        assert_eq!(offset_to_line_col(&starts, 8), (1, 2));
    }

    // -- Multiline regex --

    #[test]
    fn regex_multiline_not_dotall() {
        let fr = engine();
        // By default, `.` doesn't match newlines.
        let cfg = FindReplaceConfig::new(r"func.*var").with_regex();
        let matches = fr.find_all(SAMPLE, &cfg).unwrap();
        assert_eq!(matches.len(), 0); // spans lines, won't match
    }
}
