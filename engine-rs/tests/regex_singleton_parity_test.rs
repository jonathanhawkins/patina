//! pat-h45pa: RegEx singleton wrapping Rust regex — parity test.
//!
//! Exercises the gdcore::regex::RegEx API to verify Godot-compatible behavior:
//! construction, compile, search, search_all, sub, get_group_count, get_names.
//! Validates the `end` parameter and `subject` field parity with Godot.

use gdcore::regex::RegEx;

// -- Construction & compile ---------------------------------------------------

#[test]
fn new_creates_empty_invalid_regex() {
    let re = RegEx::new();
    assert!(!re.is_valid());
    assert_eq!(re.get_pattern(), "");
}

#[test]
fn create_from_string_compiles_valid_pattern() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert!(re.is_valid());
    assert_eq!(re.get_pattern(), r"\d+");
}

#[test]
fn create_from_string_returns_none_for_invalid() {
    assert!(RegEx::create_from_string("[bad").is_none());
}

#[test]
fn compile_updates_pattern() {
    let mut re = RegEx::new();
    assert!(re.compile(r"\w+").is_ok());
    assert!(re.is_valid());
    assert_eq!(re.get_pattern(), r"\w+");
}

#[test]
fn compile_invalid_leaves_uncompiled() {
    let mut re = RegEx::new();
    assert!(re.compile("(unclosed").is_err());
    assert!(!re.is_valid());
}

#[test]
fn clear_resets_regex() {
    let mut re = RegEx::create_from_string(r"\d+").unwrap();
    re.clear();
    assert!(!re.is_valid());
    assert_eq!(re.get_pattern(), "");
}

// -- search -------------------------------------------------------------------

#[test]
fn search_returns_first_match_with_offsets() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    let m = re.search("abc123def456", 0, 0).unwrap();
    // Godot parity: subject is the full input string
    assert_eq!(m.subject, "abc123def456");
    assert_eq!(m.strings[0], "123");
    assert_eq!(m.start, 3);
    assert_eq!(m.end, 6);
}

#[test]
fn search_with_offset() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    let m = re.search("abc123def456", 6, 0).unwrap();
    assert_eq!(m.subject, "abc123def456");
    assert_eq!(m.strings[0], "456");
    assert_eq!(m.start, 9);
}

#[test]
fn search_no_match_returns_none() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert!(re.search("no digits here", 0, 0).is_none());
}

#[test]
fn search_capture_groups_indexed() {
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
    let m = re.search("admin@example", 0, 0).unwrap();
    assert!(m.names.iter().any(|(k, v)| k == "name" && v == "admin"));
    assert!(m.names.iter().any(|(k, v)| k == "domain" && v == "example"));
}

#[test]
fn search_optional_group_empty_when_not_captured() {
    let re = RegEx::create_from_string(r"(\d+)(-(\w+))?").unwrap();
    let m = re.search("42", 0, 0).unwrap();
    assert_eq!(m.strings[0], "42");
    assert_eq!(m.strings[1], "42");
    assert_eq!(m.strings[2], ""); // optional group not captured
}

#[test]
fn search_with_end_limits_range() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // end=6 limits haystack to "abc123" — match is "123"
    let m = re.search("abc123def456", 0, 6).unwrap();
    assert_eq!(m.strings[0], "123");
    assert_eq!(m.start, 3);
    assert_eq!(m.end, 6);
}

#[test]
fn search_with_end_excludes_later_content() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // end=3 limits haystack to "abc" — no digits
    assert!(re.search("abc123def456", 0, 3).is_none());
}

#[test]
fn search_with_offset_and_end() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // offset=3, end=9 limits to "123def" — match is "123"
    let m = re.search("abc123def456", 3, 9).unwrap();
    assert_eq!(m.strings[0], "123");
    assert_eq!(m.start, 3);
    assert_eq!(m.end, 6);
}

// -- search_all ---------------------------------------------------------------

#[test]
fn search_all_returns_all_non_overlapping_matches() {
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
    assert_eq!(matches[1].strings[0], "333");
}

#[test]
fn search_all_with_end_limits_range() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // end=5 limits to "a1b22" — matches "1" and "22"
    let matches = re.search_all("a1b22c333", 0, 5);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].strings[0], "1");
    assert_eq!(matches[1].strings[0], "22");
}

#[test]
fn search_all_no_match_empty_vec() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert!(re.search_all("abc", 0, 0).is_empty());
}

// -- sub ----------------------------------------------------------------------

#[test]
fn sub_replaces_first_only() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert_eq!(re.sub("abc123def456", "N", false, 0, 0), "abcNdef456");
}

#[test]
fn sub_replaces_all() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert_eq!(re.sub("abc123def456", "N", true, 0, 0), "abcNdefN");
}

#[test]
fn sub_with_offset_preserves_prefix() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert_eq!(re.sub("abc123def456", "N", false, 6, 0), "abc123defN");
}

#[test]
fn sub_with_end_preserves_suffix() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // end=6 → haystack "abc123", replace "123" with "N", then append "def456"
    assert_eq!(re.sub("abc123def456", "N", false, 0, 6), "abcNdef456");
}

#[test]
fn sub_with_end_all_only_in_range() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    // end=9 → haystack "abc123def", only "123" matches, suffix "456" preserved
    assert_eq!(re.sub("abc123def456", "N", true, 0, 9), "abcNdef456");
}

#[test]
fn sub_with_backreference() {
    let re = RegEx::create_from_string(r"(\w+)@(\w+)").unwrap();
    assert_eq!(re.sub("user@host", "$2/$1", false, 0, 0), "host/user");
}

#[test]
fn sub_no_match_returns_original() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert_eq!(re.sub("no digits", "X", false, 0, 0), "no digits");
}

// -- get_group_count / get_names (Godot parity) --------------------------------

#[test]
fn get_group_count_zero_when_no_groups() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert_eq!(re.get_group_count(), 0);
}

#[test]
fn get_group_count_matches_captures() {
    let re = RegEx::create_from_string(r"(\w+)@(\w+)\.(\w+)").unwrap();
    assert_eq!(re.get_group_count(), 3);
}

#[test]
fn get_group_count_uncompiled_is_zero() {
    let re = RegEx::new();
    assert_eq!(re.get_group_count(), 0);
}

#[test]
fn get_names_returns_named_group_names() {
    let re = RegEx::create_from_string(r"(?P<user>\w+)@(?P<domain>\w+)").unwrap();
    let names = re.get_names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"user".to_string()));
    assert!(names.contains(&"domain".to_string()));
}

#[test]
fn get_names_empty_when_no_named_groups() {
    let re = RegEx::create_from_string(r"(\d+)").unwrap();
    assert!(re.get_names().is_empty());
}

#[test]
fn get_names_empty_when_uncompiled() {
    let re = RegEx::new();
    assert!(re.get_names().is_empty());
}

// -- Edge cases ---------------------------------------------------------------

#[test]
fn unicode_aware_matching() {
    let re = RegEx::create_from_string(r"\p{L}+").unwrap();
    let m = re.search("123 café 456", 0, 0).unwrap();
    assert_eq!(m.subject, "123 café 456");
    assert_eq!(m.strings[0], "café");
}

#[test]
fn offset_beyond_length_returns_none() {
    let re = RegEx::create_from_string(r"\d+").unwrap();
    assert!(re.search("abc", 999, 0).is_none());
    assert!(re.search_all("abc", 999, 0).is_empty());
    assert_eq!(re.sub("abc", "X", false, 999, 0), "abc");
}

#[test]
fn uncompiled_operations_return_defaults() {
    let re = RegEx::new();
    assert!(re.search("anything", 0, 0).is_none());
    assert!(re.search_all("anything", 0, 0).is_empty());
    assert_eq!(re.sub("anything", "X", false, 0, 0), "anything");
}
