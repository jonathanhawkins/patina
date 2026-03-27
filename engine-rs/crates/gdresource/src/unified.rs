//! Unified resource loading — accepts both `res://` paths and `uid://` references.
//!
//! The [`UnifiedLoader`] wraps a [`ResourceCache`] and a [`UidRegistry`],
//! providing a single `load()` entry point that:
//! 1. Detects `uid://` references and resolves them to `res://` paths via the registry.
//! 2. Falls back to direct path-based loading for `res://` paths.
//! 3. Deduplicates through the cache — loading by UID and by path for the same
//!    resource returns the same `Arc<Resource>`.

use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};

use crate::cache::ResourceCache;
use crate::loader::{parse_uid_string, ResourceLoader};
use crate::resource::Resource;
use crate::uid::UidRegistry;

/// A unified resource loader that accepts both `res://` paths and `uid://` references.
///
/// Internally resolves UIDs to paths via [`UidRegistry`], then loads and caches
/// through [`ResourceCache`]. This ensures that loading the same resource by
/// path or by UID always returns the same `Arc<Resource>`.
#[derive(Debug)]
pub struct UnifiedLoader<L: ResourceLoader> {
    cache: ResourceCache<L>,
    uid_registry: UidRegistry,
}

impl<L: ResourceLoader> UnifiedLoader<L> {
    /// Creates a new unified loader with the given backing loader and an empty UID registry.
    pub fn new(loader: L) -> Self {
        Self {
            cache: ResourceCache::new(loader),
            uid_registry: UidRegistry::new(),
        }
    }

    /// Creates a new unified loader with a pre-populated UID registry.
    pub fn with_registry(loader: L, uid_registry: UidRegistry) -> Self {
        Self {
            cache: ResourceCache::new(loader),
            uid_registry,
        }
    }

    /// Loads a resource by `res://` path or `uid://` reference.
    ///
    /// - If `reference` starts with `uid://`, resolves to a path via the registry,
    ///   then loads through the cache.
    /// - Otherwise, loads directly through the cache using the string as a path.
    ///
    /// Returns `EngineError::NotFound` if a `uid://` reference has no registered path.
    pub fn load(&mut self, reference: &str) -> EngineResult<Arc<Resource>> {
        if reference.starts_with("uid://") {
            let uid = parse_uid_string(reference);
            if !uid.is_valid() {
                return Err(EngineError::NotFound(format!(
                    "invalid UID reference: {reference}"
                )));
            }
            let path = self
                .uid_registry
                .lookup_uid(uid)
                .ok_or_else(|| {
                    EngineError::NotFound(format!("no path registered for {reference}"))
                })?
                .to_string();
            self.cache.load(&path)
        } else {
            self.cache.load(reference)
        }
    }

    /// Registers a UID-to-path mapping using a `uid://` string.
    ///
    /// Parses the UID string to a [`ResourceUid`] and registers it in the registry.
    pub fn register_uid_str(&mut self, uid_str: &str, path: impl Into<String>) {
        let uid = parse_uid_string(uid_str);
        if uid.is_valid() {
            self.uid_registry.register(uid, path);
        }
    }

    /// Registers a UID-to-path mapping using a [`ResourceUid`] directly.
    pub fn register_uid(&mut self, uid: gdcore::ResourceUid, path: impl Into<String>) {
        self.uid_registry.register(uid, path);
    }

    /// Returns a reference to the UID registry.
    pub fn uid_registry(&self) -> &UidRegistry {
        &self.uid_registry
    }

    /// Returns a mutable reference to the UID registry.
    pub fn uid_registry_mut(&mut self) -> &mut UidRegistry {
        &mut self.uid_registry
    }

    /// Resolves a `uid://` or `res://` reference to a `res://` path string.
    ///
    /// - If `reference` starts with `uid://`, resolves via the UID registry.
    /// - If `reference` starts with `res://`, returns it unchanged.
    /// - Returns `EngineError::NotFound` if a `uid://` reference has no registered path.
    pub fn resolve_to_path(&self, reference: &str) -> EngineResult<String> {
        if reference.starts_with("uid://") {
            let uid = parse_uid_string(reference);
            if !uid.is_valid() {
                return Err(EngineError::NotFound(format!(
                    "invalid UID reference: {reference}"
                )));
            }
            self.uid_registry
                .lookup_uid(uid)
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    EngineError::NotFound(format!("no path registered for {reference}"))
                })
        } else {
            Ok(reference.to_string())
        }
    }

    /// Invalidates a cached resource by path.
    pub fn invalidate(&mut self, path: &str) {
        self.cache.invalidate(path);
    }

    /// Clears the entire cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Returns the number of cached resources.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the given path is currently cached.
    pub fn is_cached(&self, path: &str) -> bool {
        self.cache.contains(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdvariant::Variant;

    struct FakeLoader;

    impl ResourceLoader for FakeLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            let mut r = Resource::new("Fake");
            r.path = path.to_string();
            r.set_property("path", Variant::String(path.to_string()));
            Ok(Arc::new(r))
        }
    }

    #[test]
    fn load_by_res_path() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        let res = ul.load("res://player.tres").unwrap();
        assert_eq!(res.path, "res://player.tres");
    }

    #[test]
    fn load_by_uid_resolves_to_path() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        ul.register_uid_str("uid://sword_42", "res://sword.tres");

        let res = ul.load("uid://sword_42").unwrap();
        assert_eq!(res.path, "res://sword.tres");
    }

    #[test]
    fn load_path_and_uid_same_arc() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        ul.register_uid_str("uid://enemy_ref", "res://enemy.tres");

        let by_path = ul.load("res://enemy.tres").unwrap();
        let by_uid = ul.load("uid://enemy_ref").unwrap();
        assert!(
            Arc::ptr_eq(&by_path, &by_uid),
            "loading by path and by UID for same resource must return same Arc"
        );
    }

    #[test]
    fn unknown_uid_returns_error() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        let result = ul.load("uid://nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn path_cached_after_uid_load() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        ul.register_uid_str("uid://cached_item", "res://item.tres");

        ul.load("uid://cached_item").unwrap();
        assert!(ul.is_cached("res://item.tres"));
        assert_eq!(ul.cache_len(), 1);
    }

    #[test]
    fn multiple_uids_different_paths() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        ul.register_uid_str("uid://alpha", "res://a.tres");
        ul.register_uid_str("uid://beta", "res://b.tres");

        let a = ul.load("uid://alpha").unwrap();
        let b = ul.load("uid://beta").unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(ul.cache_len(), 2);
    }

    #[test]
    fn invalidate_forces_reload_via_uid() {
        let mut ul = UnifiedLoader::new(FakeLoader);
        ul.register_uid_str("uid://weapon", "res://weapon.tres");

        let first = ul.load("uid://weapon").unwrap();
        ul.invalidate("res://weapon.tres");
        let second = ul.load("uid://weapon").unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }
}
