//! RegEx singleton wrapping the Rust `regex` crate.
//!
//! Provides a Godot-compatible `RegEx` API backed by the `regex` crate.
//! Godot's `RegEx` class compiles a pattern once and then offers `search`,
//! `search_all`, `sub`, and `is_valid` operations on arbitrary subjects.

/// Result of a single regex match, mirroring Godot's `RegExMatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegExMatch {
    /// The full original subject string (Godot parity: `RegExMatch.subject`
    /// returns the entire input, not just the matched portion).
    pub subject: String,
    /// Byte offset of the match start in the original string.
    pub start: usize,
    /// Byte offset one past the match end.
    pub end: usize,
    /// Captured groups (index 0 = full match, 1.. = capture groups).
    /// Groups that did not participate contain an empty string.
    pub strings: Vec<String>,
    /// Named captures (name -> matched text).
    pub names: Vec<(String, String)>,
}

/// A compiled regular expression, mirroring Godot's `RegEx` class.
#[derive(Debug, Clone)]
pub struct RegEx {
    pattern: String,
    compiled: Option<regex::Regex>,
}

impl RegEx {
    /// Creates a new empty `RegEx` (no pattern compiled yet).
    pub fn new() -> Self {
        Self {
            pattern: String::new(),
            compiled: None,
        }
    }

    /// Creates a `RegEx` from a pattern string, returning `None` if the
    /// pattern is invalid. Mirrors `RegEx.create_from_string()`.
    pub fn create_from_string(pattern: &str) -> Option<Self> {
        let compiled = regex::Regex::new(pattern).ok()?;
        Some(Self {
            pattern: pattern.to_string(),
            compiled: Some(compiled),
        })
    }

    /// Compiles a pattern. Returns `Ok(())` on success or an error message.
    /// Mirrors Godot's `RegEx.compile()`.
    pub fn compile(&mut self, pattern: &str) -> Result<(), String> {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                self.pattern = pattern.to_string();
                self.compiled = Some(re);
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Returns the source pattern string.
    pub fn get_pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns whether a valid pattern is currently compiled.
    pub fn is_valid(&self) -> bool {
        self.compiled.is_some()
    }

    /// Clears the compiled pattern, returning the object to the empty state.
    pub fn clear(&mut self) {
        self.pattern.clear();
        self.compiled = None;
    }

    /// Returns the number of capturing groups in the compiled pattern
    /// (excluding group 0, the full match). Mirrors Godot's
    /// `RegEx.get_group_count()`.
    pub fn get_group_count(&self) -> usize {
        self.compiled
            .as_ref()
            .map(|re| re.captures_len().saturating_sub(1))
            .unwrap_or(0)
    }

    /// Returns the list of named capture group names in the compiled pattern.
    /// Mirrors Godot's `RegEx.get_names()`.
    pub fn get_names(&self) -> Vec<String> {
        self.compiled
            .as_ref()
            .map(|re| {
                re.capture_names()
                    .flatten()
                    .map(|n| n.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Searches `subject` starting at byte `offset` up to `end` and returns
    /// the first match, or `None`. Mirrors Godot's `RegEx.search()`.
    ///
    /// If `end` is 0, searches to the end of the subject (Godot convention).
    pub fn search(&self, subject: &str, offset: usize, end: usize) -> Option<RegExMatch> {
        let re = self.compiled.as_ref()?;
        let actual_end = if end == 0 {
            subject.len()
        } else {
            end.min(subject.len())
        };
        let haystack = subject.get(offset..actual_end)?;
        let caps = re.captures(haystack)?;
        let full = caps.get(0)?;

        let strings: Vec<String> = (0..caps.len())
            .map(|i| {
                caps.get(i)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            })
            .collect();

        let names: Vec<(String, String)> = re
            .capture_names()
            .flatten()
            .filter_map(|name| {
                caps.name(name)
                    .map(|m| (name.to_string(), m.as_str().to_string()))
            })
            .collect();

        Some(RegExMatch {
            subject: subject.to_string(),
            start: offset + full.start(),
            end: offset + full.end(),
            strings,
            names,
        })
    }

    /// Returns all non-overlapping matches in `subject` starting at `offset`
    /// up to `end`. Mirrors Godot's `RegEx.search_all()`.
    ///
    /// If `end` is 0, searches to the end of the subject (Godot convention).
    pub fn search_all(&self, subject: &str, offset: usize, end: usize) -> Vec<RegExMatch> {
        let re = match self.compiled.as_ref() {
            Some(r) => r,
            None => return Vec::new(),
        };
        let actual_end = if end == 0 {
            subject.len()
        } else {
            end.min(subject.len())
        };
        let haystack = match subject.get(offset..actual_end) {
            Some(h) => h,
            None => return Vec::new(),
        };

        re.captures_iter(haystack)
            .map(|caps| {
                let full = caps.get(0).unwrap();
                let strings: Vec<String> = (0..caps.len())
                    .map(|i| {
                        caps.get(i)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                let names: Vec<(String, String)> = re
                    .capture_names()
                    .flatten()
                    .filter_map(|name| {
                        caps.name(name)
                            .map(|m| (name.to_string(), m.as_str().to_string()))
                    })
                    .collect();
                RegExMatch {
                    subject: subject.to_string(),
                    start: offset + full.start(),
                    end: offset + full.end(),
                    strings,
                    names,
                }
            })
            .collect()
    }

    /// Replaces the first occurrence of the pattern in `subject` (from
    /// `offset` up to `end`) with `replacement`. Mirrors Godot's `RegEx.sub()`.
    ///
    /// If `all` is true, replaces every non-overlapping match.
    /// If `end` is 0, searches to the end of the subject (Godot convention).
    pub fn sub(
        &self,
        subject: &str,
        replacement: &str,
        all: bool,
        offset: usize,
        end: usize,
    ) -> String {
        let re = match self.compiled.as_ref() {
            Some(r) => r,
            None => return subject.to_string(),
        };
        let actual_end = if end == 0 {
            subject.len()
        } else {
            end.min(subject.len())
        };
        let (prefix, haystack, suffix) = match subject
            .get(..offset)
            .and_then(|p| subject.get(offset..actual_end).map(|h| (p, h)))
            .and_then(|(p, h)| subject.get(actual_end..).map(|s| (p, h, s)))
        {
            Some(triple) => triple,
            None => return subject.to_string(),
        };

        let result = if all {
            re.replace_all(haystack, replacement)
        } else {
            re.replace(haystack, replacement)
        };

        let mut out = prefix.to_string();
        out.push_str(&result);
        out.push_str(suffix);
        out
    }
}

impl Default for RegEx {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- construction ---------------------------------------------------------

    #[test]
    fn new_is_not_valid() {
        let re = RegEx::new();
        assert!(!re.is_valid());
        assert_eq!(re.get_pattern(), "");
    }

    #[test]
    fn create_from_string_valid_pattern() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert!(re.is_valid());
        assert_eq!(re.get_pattern(), r"\d+");
    }

    #[test]
    fn create_from_string_invalid_returns_none() {
        assert!(RegEx::create_from_string("[invalid").is_none());
    }

    #[test]
    fn compile_valid_pattern() {
        let mut re = RegEx::new();
        assert!(re.compile(r"\w+").is_ok());
        assert!(re.is_valid());
    }

    #[test]
    fn compile_invalid_pattern_returns_error() {
        let mut re = RegEx::new();
        assert!(re.compile("(unclosed").is_err());
        assert!(!re.is_valid());
    }

    #[test]
    fn compile_replaces_previous_pattern() {
        let mut re = RegEx::create_from_string(r"\d+").unwrap();
        re.compile(r"[a-z]+").unwrap();
        assert_eq!(re.get_pattern(), r"[a-z]+");
    }

    #[test]
    fn clear_resets_to_empty() {
        let mut re = RegEx::create_from_string(r"\d+").unwrap();
        re.clear();
        assert!(!re.is_valid());
        assert_eq!(re.get_pattern(), "");
    }

    // -- search ---------------------------------------------------------------

    #[test]
    fn search_finds_first_match() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        let m = re.search("abc123def456", 0, 0).unwrap();
        // subject stores the full input (Godot parity)
        assert_eq!(m.subject, "abc123def456");
        assert_eq!(m.strings[0], "123");
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn search_with_offset_skips_prefix() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        let m = re.search("abc123def456", 6, 0).unwrap();
        assert_eq!(m.subject, "abc123def456");
        assert_eq!(m.strings[0], "456");
        assert_eq!(m.start, 9);
        assert_eq!(m.end, 12);
    }

    #[test]
    fn search_no_match_returns_none() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert!(re.search("abcdef", 0, 0).is_none());
    }

    #[test]
    fn search_uncompiled_returns_none() {
        let re = RegEx::new();
        assert!(re.search("anything", 0, 0).is_none());
    }

    #[test]
    fn search_capture_groups() {
        let re = RegEx::create_from_string(r"(\w+)@(\w+)").unwrap();
        let m = re.search("user@host", 0, 0).unwrap();
        assert_eq!(m.strings.len(), 3);
        assert_eq!(m.strings[0], "user@host");
        assert_eq!(m.strings[1], "user");
        assert_eq!(m.strings[2], "host");
    }

    #[test]
    fn search_named_captures() {
        let re = RegEx::create_from_string(r"(?P<name>\w+)@(?P<domain>\w+)").unwrap();
        let m = re.search("user@host", 0, 0).unwrap();
        assert!(m.names.iter().any(|(k, v)| k == "name" && v == "user"));
        assert!(m.names.iter().any(|(k, v)| k == "domain" && v == "host"));
    }

    #[test]
    fn search_optional_group_missing() {
        let re = RegEx::create_from_string(r"(\d+)(-(\w+))?").unwrap();
        let m = re.search("42", 0, 0).unwrap();
        assert_eq!(m.strings[0], "42");
        assert_eq!(m.strings[1], "42");
        // Groups 2 and 3 did not participate — should be empty.
        assert_eq!(m.strings[2], "");
        assert_eq!(m.strings[3], "");
    }

    #[test]
    fn search_with_end_limits_range() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        // "abc123def456" — end=6 limits to "abc123", so match is "123"
        let m = re.search("abc123def456", 0, 6).unwrap();
        assert_eq!(m.strings[0], "123");
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn search_with_end_excludes_later_match() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        // "abc123def456" — end=3 limits to "abc", no digits
        assert!(re.search("abc123def456", 0, 3).is_none());
    }

    // -- search_all -----------------------------------------------------------

    #[test]
    fn search_all_finds_all_matches() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        let matches = re.search_all("a1b22c333", 0, 0);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].strings[0], "1");
        assert_eq!(matches[1].strings[0], "22");
        assert_eq!(matches[2].strings[0], "333");
        // All matches reference the full subject
        assert_eq!(matches[0].subject, "a1b22c333");
    }

    #[test]
    fn search_all_with_offset() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        let matches = re.search_all("a1b22c333", 3, 0);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].strings[0], "22");
        assert_eq!(matches[0].start, 3);
        assert_eq!(matches[1].strings[0], "333");
        assert_eq!(matches[1].start, 6);
    }

    #[test]
    fn search_all_with_end_limits_range() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        // "a1b22c333" — end=5 limits to "a1b22", so only "1" and "22"
        let matches = re.search_all("a1b22c333", 0, 5);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].strings[0], "1");
        assert_eq!(matches[1].strings[0], "22");
    }

    #[test]
    fn search_all_no_matches_returns_empty() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert!(re.search_all("abc", 0, 0).is_empty());
    }

    #[test]
    fn search_all_uncompiled_returns_empty() {
        let re = RegEx::new();
        assert!(re.search_all("123", 0, 0).is_empty());
    }

    // -- sub ------------------------------------------------------------------

    #[test]
    fn sub_replaces_first_occurrence() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert_eq!(re.sub("abc123def456", "NUM", false, 0, 0), "abcNUMdef456");
    }

    #[test]
    fn sub_replaces_all_occurrences() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert_eq!(re.sub("abc123def456", "NUM", true, 0, 0), "abcNUMdefNUM");
    }

    #[test]
    fn sub_with_offset_preserves_prefix() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert_eq!(re.sub("abc123def456", "NUM", false, 6, 0), "abc123defNUM");
    }

    #[test]
    fn sub_with_end_preserves_suffix() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        // end=6 limits to "abc123", replaces "123" with "NUM", then appends "def456"
        assert_eq!(re.sub("abc123def456", "NUM", false, 0, 6), "abcNUMdef456");
    }

    #[test]
    fn sub_with_end_all_only_in_range() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        // end=9 limits to "abc123def", only "123" is a digit match
        assert_eq!(re.sub("abc123def456", "NUM", true, 0, 9), "abcNUMdef456");
    }

    #[test]
    fn sub_no_match_returns_original() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert_eq!(re.sub("abcdef", "NUM", false, 0, 0), "abcdef");
    }

    #[test]
    fn sub_uncompiled_returns_original() {
        let re = RegEx::new();
        assert_eq!(re.sub("abc123", "NUM", false, 0, 0), "abc123");
    }

    #[test]
    fn sub_with_backreference() {
        let re = RegEx::create_from_string(r"(\w+)@(\w+)").unwrap();
        assert_eq!(re.sub("user@host", "$2/$1", false, 0, 0), "host/user");
    }

    // -- edge cases -----------------------------------------------------------

    #[test]
    fn search_empty_pattern_matches_start() {
        let re = RegEx::create_from_string("").unwrap();
        let m = re.search("hello", 0, 0).unwrap();
        assert_eq!(m.subject, "hello");
        assert_eq!(m.strings[0], "");
        assert_eq!(m.start, 0);
    }

    #[test]
    fn search_empty_subject() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert!(re.search("", 0, 0).is_none());
    }

    #[test]
    fn offset_beyond_subject_len() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert!(re.search("abc", 999, 0).is_none());
        assert!(re.search_all("abc", 999, 0).is_empty());
        assert_eq!(re.sub("abc", "X", false, 999, 0), "abc");
    }

    // -- get_group_count / get_names ------------------------------------

    #[test]
    fn get_group_count_no_groups() {
        let re = RegEx::create_from_string(r"\d+").unwrap();
        assert_eq!(re.get_group_count(), 0);
    }

    #[test]
    fn get_group_count_with_groups() {
        let re = RegEx::create_from_string(r"(\w+)@(\w+)\.(\w+)").unwrap();
        assert_eq!(re.get_group_count(), 3);
    }

    #[test]
    fn get_group_count_uncompiled() {
        let re = RegEx::new();
        assert_eq!(re.get_group_count(), 0);
    }

    #[test]
    fn get_names_no_named_groups() {
        let re = RegEx::create_from_string(r"(\d+)").unwrap();
        assert!(re.get_names().is_empty());
    }

    #[test]
    fn get_names_with_named_groups() {
        let re = RegEx::create_from_string(r"(?P<user>\w+)@(?P<domain>\w+)").unwrap();
        let names = re.get_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"user".to_string()));
        assert!(names.contains(&"domain".to_string()));
    }

    #[test]
    fn get_names_uncompiled() {
        let re = RegEx::new();
        assert!(re.get_names().is_empty());
    }

    #[test]
    fn unicode_subject() {
        let re = RegEx::create_from_string(r"\p{L}+").unwrap();
        let m = re.search("123 café 456", 0, 0).unwrap();
        assert_eq!(m.subject, "123 café 456");
        assert_eq!(m.strings[0], "café");
    }
}
