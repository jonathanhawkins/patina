//! Parsed scene-tree path type.
//!
//! `NodePath` mirrors Godot's `NodePath`: a pre-parsed path through the
//! scene tree with optional property subpath support.
//!
//! # Path syntax
//!
//! - Absolute: `"/root/Player/Sprite"`
//! - Relative: `"Player/Sprite"`, `"../Sibling"`
//! - Property subpath: `"Player:position"` (name followed by `:property`)
//! - Multiple subnames: `"Player:position:x"`

use std::fmt;

/// A parsed scene-tree path, analogous to Godot's `NodePath`.
///
/// A `NodePath` consists of:
/// - A sequence of *names* (the node path segments),
/// - An optional sequence of *subnames* (property path after `:`),
/// - A flag indicating whether the path is absolute (starts with `/`).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NodePath {
    /// The node name segments (e.g. `["root", "Player", "Sprite"]`).
    names: Vec<String>,
    /// Property subnames after the `:` separator (e.g. `["position", "x"]`).
    subnames: Vec<String>,
    /// Whether the path begins with `/`.
    absolute: bool,
}

impl NodePath {
    /// Creates an empty `NodePath`.
    pub fn empty() -> Self {
        Self {
            names: Vec::new(),
            subnames: Vec::new(),
            absolute: false,
        }
    }

    /// Parses a `NodePath` from a string.
    ///
    /// # Syntax
    ///
    /// - `"/root/Player"` — absolute path
    /// - `"Player/Sprite"` — relative path
    /// - `"../Sibling"` — relative path with parent traversal
    /// - `"Player:position"` — path with property subname
    /// - `"Player:position:x"` — path with multiple subnames
    pub fn new(path: &str) -> Self {
        if path.is_empty() {
            return Self::empty();
        }

        let absolute = path.starts_with('/');
        let path = if absolute { &path[1..] } else { path };

        // Split the first `:` occurrence to separate node path from subnames.
        // However, all segments after the first `:` in the *last* name are subnames.
        // Godot's format: "path/to/node:subname1:subname2"
        // We need to find the first `:` to separate names from subnames.
        let (name_part, subname_part) = match path.find(':') {
            Some(idx) => (&path[..idx], Some(&path[idx + 1..])),
            None => (path, None),
        };

        let names: Vec<String> = if name_part.is_empty() {
            Vec::new()
        } else {
            name_part.split('/').map(String::from).collect()
        };

        let subnames: Vec<String> = match subname_part {
            Some(s) if !s.is_empty() => s.split(':').map(String::from).collect(),
            _ => Vec::new(),
        };

        Self {
            names,
            subnames,
            absolute,
        }
    }

    /// Returns `true` if this is an absolute path (starts with `/`).
    pub fn is_absolute(&self) -> bool {
        self.absolute
    }

    /// Returns `true` if this path has no names and no subnames.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.subnames.is_empty()
    }

    /// Returns the number of name segments in the node path.
    pub fn get_name_count(&self) -> usize {
        self.names.len()
    }

    /// Returns the name at the given index, or `None` if out of bounds.
    pub fn get_name(&self, idx: usize) -> Option<&str> {
        self.names.get(idx).map(|s| s.as_str())
    }

    /// Returns the number of property subnames.
    pub fn get_subname_count(&self) -> usize {
        self.subnames.len()
    }

    /// Returns the property subname at the given index, or `None` if out of bounds.
    pub fn get_subname(&self, idx: usize) -> Option<&str> {
        self.subnames.get(idx).map(|s| s.as_str())
    }

    /// Returns the concatenated property subpath (e.g. `"position:x"`).
    ///
    /// Returns an empty string if there are no subnames.
    pub fn get_concatenated_subnames(&self) -> String {
        self.subnames.join(":")
    }

    /// Returns the concatenated NAME path (e.g. `"Player/Sprite"`), without the
    /// subnames or the leading slash (pat-s3rrv.60).
    ///
    /// Godot's counterpart to [`get_concatenated_subnames`](Self::get_concatenated_subnames);
    /// the separator is `/` because these are node names, not properties.
    pub fn get_concatenated_names(&self) -> String {
        self.names.join("/")
    }

    /// Godot `NodePath.hash()` — the value Godot's own hash maps key on
    /// (pat-s3rrv.60).
    ///
    /// Transcribed from `core/string/node_path.cpp:35-41`:
    /// ```text
    /// StringName path    = get_concatenated_names();
    /// StringName subpath = get_concatenated_subnames();
    /// uint32_t hash = HashMapHasherDefault::hash(Pair<...>(path, subpath));
    /// hash_cache = is_absolute() ? hash : ~hash;
    /// ```
    ///
    /// ⚠️ NOTE THE BITWISE NOT for a RELATIVE path. It is one character in the
    /// C++ and invisible to any test that only uses absolute paths, so both
    /// cases are pinned below.
    ///
    /// ⚠️ Named `godot_hash`, not `hash`: this type derives [`Hash`], and an
    /// inherent `hash` sitting beside `Hash::hash` is an ambiguity trap for
    /// every later caller. The script-facing name is still `"hash"` — that is a
    /// dispatch string, not this identifier.
    pub fn godot_hash(&self) -> u32 {
        let h1 = u64::from(godot_string_hash(&self.get_concatenated_names()));
        let h2 = u64::from(godot_string_hash(&self.get_concatenated_subnames()));
        let hash = hash_one_uint64((h1 << 32) | h2);
        if self.absolute {
            hash
        } else {
            !hash
        }
    }

    /// Godot's `NodePath.slice(begin, end)` — a sub-path over the NAMES
    /// (pat-s3rrv.60).
    ///
    /// Negative indices count from the end, and both bounds are clamped, so
    /// `slice(1, -1)` drops the first and last name rather than panicking. The
    /// result keeps this path's `absolute` flag only when it starts at 0 —
    /// a slice that skips the root is by definition relative.
    ///
    /// Subnames ride along only on a slice that reaches the end, mirroring
    /// Godot: they belong to the LAST name, so a slice that drops it drops them.
    pub fn slice(&self, begin: i64, end: i64) -> NodePath {
        let len = self.names.len() as i64;
        let norm = |i: i64| -> usize {
            let i = if i < 0 { len + i } else { i };
            i.clamp(0, len) as usize
        };
        let start = norm(begin);
        let stop = norm(end).max(start);
        NodePath {
            names: self.names[start..stop].to_vec(),
            subnames: if stop == self.names.len() {
                self.subnames.clone()
            } else {
                Vec::new()
            },
            absolute: self.absolute && start == 0,
        }
    }

    /// Returns a new `NodePath` representing only the property subpath.
    ///
    /// For example, if this path is `"Player:position:x"`, this returns
    /// a `NodePath` with names `["position", "x"]` and no subnames.
    pub fn get_as_property_path(&self) -> NodePath {
        NodePath {
            names: self.subnames.clone(),
            subnames: Vec::new(),
            absolute: false,
        }
    }
}

impl fmt::Debug for NodePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodePath(\"{}\")", self)
    }
}

impl fmt::Display for NodePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.absolute {
            write!(f, "/")?;
        }
        write!(f, "{}", self.names.join("/"))?;
        for subname in &self.subnames {
            write!(f, ":{subname}")?;
        }
        Ok(())
    }
}

impl From<&str> for NodePath {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for NodePath {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

/// Godot's `String::hash` — djb2 (`core/string/ustring.cpp:2734`).
///
/// ⚠️ It walks **code points** (`char32_t`), not UTF-8 bytes. Hashing
/// `s.bytes()` agrees with Godot for ASCII and silently diverges the moment a
/// path contains a non-ASCII character, which is exactly the kind of bug that
/// only shows up when a value crosses an engine boundary.
fn godot_string_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for ch in s.chars() {
        // `hashv = ((hashv << 5) + hashv) + c` — all uint32, wrapping.
        h = h.wrapping_shl(5).wrapping_add(h).wrapping_add(ch as u32);
    }
    h
}

/// Godot's `hash_one_uint64` (`core/templates/hashfuncs.h:87-96`), the 64→32
/// finaliser the Pair hasher ends with.
fn hash_one_uint64(p_int: u64) -> u32 {
    let mut v = p_int;
    v = (!v).wrapping_add(v << 18);
    v ^= v >> 31;
    v = v.wrapping_mul(21);
    v ^= v >> 11;
    v = v.wrapping_add(v << 6);
    v ^= v >> 22;
    v as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔑 `NodePath.hash()` against values from an INDEPENDENT transcription.
    ///
    /// ⚠️ These constants were produced by re-transcribing Godot's chain
    /// (`String::hash` djb2 → the `Pair` hasher → `hash_one_uint64`) into Python
    /// and RUNNING it — not by printing what this implementation returns. A
    /// wrong-but-stable hash satisfies every round-trip and self-consistency
    /// test and only breaks when a value crosses an engine boundary, so
    /// self-comparison would proves nothing here.
    #[test]
    fn godot_hash_matches_the_upstream_chain() {
        assert_eq!(
            NodePath::new("/root/Main").godot_hash(),
            1_105_670_561,
            "absolute path"
        );
        assert_eq!(
            NodePath::new("Node:position:x").godot_hash(),
            1_901_452_884,
            "subnames participate as a separate StringName joined by ':'"
        );
    }

    /// ⚠️ THE BITWISE NOT IS THE WHOLE TEST.
    ///
    /// Godot returns `hash` for an absolute path and `~hash` for a relative one
    /// (`node_path.cpp:40`). It is one character upstream, and a port that drops
    /// it passes every absolute-path assertion. Pinning both directions with the
    /// SAME name content is what catches it.
    #[test]
    fn a_relative_path_hashes_to_the_complement_of_the_absolute_one() {
        let rel = NodePath::new("Main/Sprite");
        assert_eq!(rel.godot_hash(), 3_441_622_773, "relative path");

        let abs = NodePath::new("/Main/Sprite");
        assert_eq!(
            rel.godot_hash(),
            !abs.godot_hash(),
            "the only difference between the two is the leading slash, so their \
             hashes must be exact bitwise complements"
        );
        assert_ne!(
            rel.godot_hash(),
            abs.godot_hash(),
            "a port that ignored is_absolute would make these equal"
        );
    }

    /// The primitives, pinned separately so a failure says WHICH stage drifted.
    #[test]
    fn the_hash_primitives_match_godot() {
        // `String::hash("")` is what Godot's `StringName::get_empty_hash`
        // returns, so an empty path is not a special case.
        assert_eq!(godot_string_hash(""), 5381, "djb2 seed");
        assert_eq!(
            godot_string_hash("a"),
            5381 * 33 + 97,
            "djb2 step is *33 + c"
        );

        // ⚠️ Code points, not bytes. 'Ü' is two UTF-8 bytes but ONE char32_t.
        assert_eq!(
            godot_string_hash("Ü"),
            5381u32.wrapping_mul(33).wrapping_add(0xDC),
            "the hash must consume the code point U+00DC, not its two UTF-8 bytes"
        );

        assert_eq!(NodePath::new("Ünï").godot_hash(), 1_261_690_316);
    }

    /// An empty path still hashes, and to the documented value.
    #[test]
    fn an_empty_path_hashes_without_special_casing() {
        assert_eq!(NodePath::empty().godot_hash(), 2_757_153_842);
    }

    #[test]
    fn parse_absolute_path() {
        let p = NodePath::new("/root/Player/Sprite");
        assert!(p.is_absolute());
        assert_eq!(p.get_name_count(), 3);
        assert_eq!(p.get_name(0), Some("root"));
        assert_eq!(p.get_name(1), Some("Player"));
        assert_eq!(p.get_name(2), Some("Sprite"));
    }

    #[test]
    fn parse_relative_path() {
        let p = NodePath::new("Player/Sprite");
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 2);
        assert_eq!(p.get_name(0), Some("Player"));
        assert_eq!(p.get_name(1), Some("Sprite"));
    }

    #[test]
    fn parse_parent_traversal() {
        let p = NodePath::new("../Sibling");
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 2);
        assert_eq!(p.get_name(0), Some(".."));
        assert_eq!(p.get_name(1), Some("Sibling"));
    }

    #[test]
    fn parse_property_subpath() {
        let p = NodePath::new("Player:position");
        assert_eq!(p.get_name_count(), 1);
        assert_eq!(p.get_name(0), Some("Player"));
        assert_eq!(p.get_subname_count(), 1);
        assert_eq!(p.get_subname(0), Some("position"));
    }

    #[test]
    fn parse_multiple_subnames() {
        let p = NodePath::new("Player:position:x");
        assert_eq!(p.get_name_count(), 1);
        assert_eq!(p.get_name(0), Some("Player"));
        assert_eq!(p.get_subname_count(), 2);
        assert_eq!(p.get_subname(0), Some("position"));
        assert_eq!(p.get_subname(1), Some("x"));
        assert_eq!(p.get_concatenated_subnames(), "position:x");
    }

    #[test]
    fn property_path_extraction() {
        let p = NodePath::new("Player:position:x");
        let prop = p.get_as_property_path();
        assert!(!prop.is_absolute());
        assert_eq!(prop.get_name_count(), 2);
        assert_eq!(prop.get_name(0), Some("position"));
        assert_eq!(prop.get_name(1), Some("x"));
        assert_eq!(prop.get_subname_count(), 0);
    }

    #[test]
    fn empty_path() {
        let p = NodePath::new("");
        assert!(p.is_empty());
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 0);
    }

    #[test]
    fn display_absolute() {
        let p = NodePath::new("/root/Player");
        assert_eq!(format!("{p}"), "/root/Player");
    }

    #[test]
    fn display_relative_with_subnames() {
        let p = NodePath::new("Player:position:x");
        assert_eq!(format!("{p}"), "Player:position:x");
    }

    #[test]
    fn from_str() {
        let p: NodePath = "/root/Node".into();
        assert!(p.is_absolute());
        assert_eq!(p.get_name_count(), 2);
    }

    #[test]
    fn from_string() {
        let p: NodePath = String::from("Relative/Path").into();
        assert!(!p.is_absolute());
    }

    #[test]
    fn debug_format() {
        let p = NodePath::new("/root");
        assert_eq!(format!("{p:?}"), "NodePath(\"/root\")");
    }

    #[test]
    fn get_name_out_of_bounds() {
        let p = NodePath::new("A/B");
        assert_eq!(p.get_name(5), None);
    }

    #[test]
    fn equality() {
        let a = NodePath::new("/root/Player");
        let b = NodePath::new("/root/Player");
        assert_eq!(a, b);

        let c = NodePath::new("root/Player");
        assert_ne!(a, c); // absolute vs relative
    }

    #[test]
    fn root_only_path() {
        let p = NodePath::new("/");
        assert!(p.is_absolute());
        assert_eq!(p.get_name_count(), 0);
        assert!(p.subnames.is_empty());
    }

    #[test]
    fn subnames_only_path() {
        let p = NodePath::new(":prop:sub");
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 0);
        assert_eq!(p.get_subname_count(), 2);
        assert_eq!(p.get_subname(0), Some("prop"));
        assert_eq!(p.get_subname(1), Some("sub"));
    }

    #[test]
    fn double_dot_navigation() {
        let p = NodePath::new("../../Sibling/Child");
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 4);
        assert_eq!(p.get_name(0), Some(".."));
        assert_eq!(p.get_name(1), Some(".."));
        assert_eq!(p.get_name(2), Some("Sibling"));
        assert_eq!(p.get_name(3), Some("Child"));
    }

    #[test]
    fn single_dot_path() {
        let p = NodePath::new(".");
        assert!(!p.is_absolute());
        assert_eq!(p.get_name_count(), 1);
        assert_eq!(p.get_name(0), Some("."));
    }

    #[test]
    fn get_subname_out_of_bounds() {
        let p = NodePath::new("A:b");
        assert_eq!(p.get_subname(5), None);
    }

    #[test]
    fn display_empty_path() {
        let p = NodePath::empty();
        assert_eq!(format!("{p}"), "");
    }

    #[test]
    fn display_root_only() {
        let p = NodePath::new("/");
        assert_eq!(format!("{p}"), "/");
    }

    #[test]
    fn display_subnames_only() {
        let p = NodePath::new(":prop:sub");
        assert_eq!(format!("{p}"), ":prop:sub");
    }

    #[test]
    fn hash_consistent_with_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NodePath::new("/root/A"));
        assert!(set.contains(&NodePath::new("/root/A")));
        assert!(!set.contains(&NodePath::new("/root/B")));
    }

    #[test]
    fn concatenated_subnames_empty_when_none() {
        let p = NodePath::new("A/B");
        assert_eq!(p.get_concatenated_subnames(), "");
    }

    #[test]
    fn property_path_from_no_subnames() {
        let p = NodePath::new("A/B");
        let prop = p.get_as_property_path();
        assert!(prop.is_empty());
    }
}

/// Property-based tests for NodePath parsing roundtrips.
///
/// Uses `proptest` to verify that `NodePath::new(s).to_string()` → `NodePath::new()`
/// produces an equivalent path for all valid inputs.
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy for valid node name segments (no `/` or `:`).
    fn node_name() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-zA-Z_][a-zA-Z0-9_]{0,15}",
            Just("..".to_string()),
            Just(".".to_string()),
        ]
    }

    /// Strategy for a non-empty list of node name segments.
    fn node_names() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(node_name(), 1..6)
    }

    /// Strategy for property subname segments.
    fn subname() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,10}"
    }

    /// Strategy for optional subnames.
    fn subnames() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(subname(), 0..4)
    }

    proptest! {
        /// Roundtrip: parse → display → parse produces an equivalent NodePath.
        #[test]
        fn roundtrip_parse_display(
            abs in any::<bool>(),
            names in node_names(),
            subs in subnames(),
        ) {
            let mut path_str = String::new();
            if abs {
                path_str.push('/');
            }
            path_str.push_str(&names.join("/"));
            for s in &subs {
                path_str.push(':');
                path_str.push_str(s);
            }

            let p1 = NodePath::new(&path_str);
            let displayed = format!("{p1}");
            let p2 = NodePath::new(&displayed);

            prop_assert_eq!(p1.is_absolute(), p2.is_absolute(),
                "Absolute flag mismatch after roundtrip: {:?} vs {:?}", path_str, displayed);
            prop_assert_eq!(p1.get_name_count(), p2.get_name_count(),
                "Name count mismatch after roundtrip");
            for i in 0..p1.get_name_count() {
                prop_assert_eq!(p1.get_name(i), p2.get_name(i),
                    "Name mismatch at index {}", i);
            }
            prop_assert_eq!(p1.get_subname_count(), p2.get_subname_count(),
                "Subname count mismatch after roundtrip");
            for i in 0..p1.get_subname_count() {
                prop_assert_eq!(p1.get_subname(i), p2.get_subname(i),
                    "Subname mismatch at index {}", i);
            }
        }

        /// Display output is idempotent: displaying twice gives the same string.
        #[test]
        fn display_idempotent(
            abs in any::<bool>(),
            names in node_names(),
            subs in subnames(),
        ) {
            let mut path_str = String::new();
            if abs {
                path_str.push('/');
            }
            path_str.push_str(&names.join("/"));
            for s in &subs {
                path_str.push(':');
                path_str.push_str(s);
            }

            let p = NodePath::new(&path_str);
            let s1 = format!("{p}");
            let s2 = format!("{}", NodePath::new(&s1));
            prop_assert_eq!(&s1, &s2,
                "Display not idempotent: first={:?}, second={:?}", s1, s2);
        }

        /// Empty path always produces is_empty() = true.
        #[test]
        fn empty_is_empty(_dummy in 0u8..1) {
            let p = NodePath::new("");
            prop_assert!(p.is_empty());
            prop_assert!(!p.is_absolute());
        }

        /// Absolute paths always start with `/` in display.
        #[test]
        fn absolute_display_starts_with_slash(
            names in node_names(),
        ) {
            let path_str = format!("/{}", names.join("/"));
            let p = NodePath::new(&path_str);
            let displayed = format!("{p}");
            prop_assert!(displayed.starts_with('/'),
                "Absolute path display should start with /: {:?}", displayed);
        }

        /// Relative paths never start with `/` in display.
        #[test]
        fn relative_display_no_leading_slash(
            names in node_names(),
        ) {
            let path_str = names.join("/");
            let p = NodePath::new(&path_str);
            let displayed = format!("{p}");
            prop_assert!(!displayed.starts_with('/'),
                "Relative path display should not start with /: {:?}", displayed);
        }

        /// Subname count matches the number of `:` separated segments after the path.
        #[test]
        fn subname_count_matches_input(
            names in node_names(),
            subs in prop::collection::vec(subname(), 1..5),
        ) {
            let path_str = format!("{}:{}", names.join("/"), subs.join(":"));
            let p = NodePath::new(&path_str);
            prop_assert_eq!(p.get_subname_count(), subs.len(),
                "Expected {} subnames, got {}", subs.len(), p.get_subname_count());
        }

        /// Fuzz: arbitrary byte strings never cause a panic in NodePath::new.
        #[test]
        fn no_panic_on_arbitrary_input(s in "\\PC{0,100}") {
            let _p = NodePath::new(&s);
            // Just verifying no panic occurs
        }
    }
}
