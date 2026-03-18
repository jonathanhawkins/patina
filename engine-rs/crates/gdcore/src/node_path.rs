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

#[cfg(test)]
mod tests {
    use super::*;

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
