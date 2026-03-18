//! Unique resource identifiers and path resolution.
//!
//! The [`UidRegistry`] maintains a bidirectional mapping between
//! [`ResourceUid`] values and `res://` paths, allowing resources to be
//! looked up by either identifier.

use std::collections::HashMap;

use gdcore::ResourceUid;

/// A bidirectional registry mapping resource UIDs to paths and vice versa.
#[derive(Debug, Default)]
pub struct UidRegistry {
    uid_to_path: HashMap<i64, String>,
    path_to_uid: HashMap<String, ResourceUid>,
}

impl UidRegistry {
    /// Creates a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a UID-to-path mapping. Overwrites any previous mapping for
    /// either the UID or the path.
    pub fn register(&mut self, uid: ResourceUid, path: impl Into<String>) {
        let path = path.into();
        // Remove stale reverse mapping if this UID was previously bound.
        if let Some(old_path) = self.uid_to_path.get(&uid.raw()) {
            self.path_to_uid.remove(old_path);
        }
        // Remove stale forward mapping if this path was previously bound.
        if let Some(old_uid) = self.path_to_uid.get(&path) {
            self.uid_to_path.remove(&old_uid.raw());
        }
        self.uid_to_path.insert(uid.raw(), path.clone());
        self.path_to_uid.insert(path, uid);
    }

    /// Removes the mapping for a given UID.
    pub fn unregister_uid(&mut self, uid: ResourceUid) {
        if let Some(path) = self.uid_to_path.remove(&uid.raw()) {
            self.path_to_uid.remove(&path);
        }
    }

    /// Removes the mapping for a given path.
    pub fn unregister_path(&mut self, path: &str) {
        if let Some(uid) = self.path_to_uid.remove(path) {
            self.uid_to_path.remove(&uid.raw());
        }
    }

    /// Looks up the path for a given UID.
    pub fn lookup_uid(&self, uid: ResourceUid) -> Option<&str> {
        self.uid_to_path.get(&uid.raw()).map(String::as_str)
    }

    /// Looks up the UID for a given path.
    pub fn lookup_path(&self, path: &str) -> Option<ResourceUid> {
        self.path_to_uid.get(path).copied()
    }

    /// Returns the number of registered mappings.
    pub fn len(&self) -> usize {
        self.uid_to_path.len()
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.uid_to_path.is_empty()
    }

    /// Removes all mappings.
    pub fn clear(&mut self) {
        self.uid_to_path.clear();
        self.path_to_uid.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(100);
        reg.register(uid, "res://my_resource.tres");

        assert_eq!(reg.lookup_uid(uid), Some("res://my_resource.tres"));
        assert_eq!(reg.lookup_path("res://my_resource.tres"), Some(uid));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let reg = UidRegistry::new();
        assert_eq!(reg.lookup_uid(ResourceUid::new(999)), None);
        assert_eq!(reg.lookup_path("res://nothing"), None);
    }

    #[test]
    fn unregister_uid() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(1);
        reg.register(uid, "res://a.tres");
        reg.unregister_uid(uid);

        assert_eq!(reg.lookup_uid(uid), None);
        assert_eq!(reg.lookup_path("res://a.tres"), None);
        assert!(reg.is_empty());
    }

    #[test]
    fn unregister_path() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(2);
        reg.register(uid, "res://b.tres");
        reg.unregister_path("res://b.tres");

        assert_eq!(reg.lookup_uid(uid), None);
        assert_eq!(reg.lookup_path("res://b.tres"), None);
    }

    #[test]
    fn overwrite_mapping() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(10);
        reg.register(uid, "res://old.tres");
        reg.register(uid, "res://new.tres");

        assert_eq!(reg.lookup_uid(uid), Some("res://new.tres"));
        assert_eq!(reg.lookup_path("res://old.tres"), None);
        assert_eq!(reg.lookup_path("res://new.tres"), Some(uid));
        assert_eq!(reg.len(), 1);
    }
}
