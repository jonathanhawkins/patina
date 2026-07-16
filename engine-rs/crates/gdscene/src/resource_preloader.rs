//! `ResourcePreloader` — a node bundling named resources into a scene.
//!
//! Mirrors Godot's `ResourcePreloader`: a `Node` that holds a named dictionary
//! of resources shipped inside a `.tscn`, fetched by name at runtime
//! (`add_resource`/`get_resource`/`has_resource`/`remove_resource`/
//! `rename_resource`/`get_resource_list`).
//!
//! Resources are referenced here by path; resolving them to loaded `Resource`
//! objects and the `.tscn` round-trip are handled by the loader/saver. Names
//! are kept in insertion order so `get_resource_list` enumerates them stably.

/// A node holding a named, order-preserving bundle of resource references
/// (Godot `ResourcePreloader`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourcePreloader {
    /// `(name, resource_path)` pairs in insertion order.
    entries: Vec<(String, String)>,
}

impl ResourcePreloader {
    /// Creates an empty preloader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores the resource at `path` under `name`. If `name` already exists its
    /// path is replaced in place (its position is preserved).
    pub fn add_resource(&mut self, name: impl Into<String>, path: impl Into<String>) {
        let name = name.into();
        for entry in &mut self.entries {
            if entry.0 == name {
                entry.1 = path.into();
                return;
            }
        }
        self.entries.push((name, path.into()));
    }

    /// Whether a resource is stored under `name`.
    pub fn has_resource(&self, name: &str) -> bool {
        self.entries.iter().any(|(n, _)| n.as_str() == name)
    }

    /// The resource path stored under `name`, if any.
    pub fn get_resource(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, path)| path.as_str())
    }

    /// Removes the resource stored under `name`. Returns whether it existed.
    pub fn remove_resource(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(n, _)| n.as_str() != name);
        self.entries.len() != before
    }

    /// Renames the resource stored under `old` to `new`, keeping its position
    /// and path. Returns `false` if `old` is absent or `new` is already taken
    /// (renaming to the same name is a no-op that reports success).
    pub fn rename_resource(&mut self, old: &str, new: &str) -> bool {
        if old == new {
            return self.has_resource(old);
        }
        if self.has_resource(new) {
            return false;
        }
        for entry in &mut self.entries {
            if entry.0.as_str() == old {
                entry.0 = new.to_string();
                return true;
            }
        }
        false
    }

    /// The stored resource names, in insertion order (Godot `get_resource_list`).
    pub fn get_resource_list(&self) -> Vec<&str> {
        self.entries.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Number of stored resources.
    pub fn resource_count(&self) -> usize {
        self.entries.len()
    }

    /// Whether no resources are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_preloader_add_get_and_list() {
        let mut p = ResourcePreloader::new();
        assert!(p.is_empty());
        assert!(!p.has_resource("a"));

        p.add_resource("a", "res://a.png");
        p.add_resource("b", "res://b.tscn");
        assert!(p.has_resource("a"));
        assert_eq!(p.get_resource("a"), Some("res://a.png"));
        assert_eq!(p.get_resource("missing"), None);
        assert_eq!(p.get_resource_list(), vec!["a", "b"]); // insertion order
        assert_eq!(p.resource_count(), 2);

        // Re-adding replaces the path but keeps the position.
        p.add_resource("a", "res://a2.png");
        assert_eq!(p.get_resource("a"), Some("res://a2.png"));
        assert_eq!(p.get_resource_list(), vec!["a", "b"]);
    }

    #[test]
    fn resource_preloader_remove_and_rename() {
        let mut p = ResourcePreloader::new();
        p.add_resource("a", "res://a");
        p.add_resource("b", "res://b");
        p.add_resource("c", "res://c");

        // Rename keeps position and path.
        assert!(p.rename_resource("b", "beta"));
        assert_eq!(p.get_resource_list(), vec!["a", "beta", "c"]);
        assert_eq!(p.get_resource("beta"), Some("res://b"));

        // Renaming onto an existing name, or from a missing name, fails.
        assert!(!p.rename_resource("a", "c"));
        assert!(!p.rename_resource("nope", "x"));
        // Renaming to the same name is a successful no-op.
        assert!(p.rename_resource("a", "a"));

        // Remove reports whether the entry existed.
        assert!(p.remove_resource("a"));
        assert!(!p.remove_resource("a"));
        assert_eq!(p.get_resource_list(), vec!["beta", "c"]);
    }
}
