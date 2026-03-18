//! Resource caching and deduplication.
//!
//! The [`ResourceCache`] stores loaded resources by path and returns
//! existing [`Arc`] references on subsequent loads, avoiding redundant
//! parsing and allocation.

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::error::EngineResult;

use crate::loader::ResourceLoader;
use crate::resource::Resource;

/// A cache of loaded resources keyed by path.
///
/// When a resource is requested via [`load`](ResourceCache::load), the
/// cache first checks whether it has already been loaded. If so, it
/// returns the existing `Arc<Resource>` (pointer equality guaranteed).
#[derive(Debug)]
pub struct ResourceCache<L: ResourceLoader> {
    loader: L,
    cache: HashMap<String, Arc<Resource>>,
}

impl<L: ResourceLoader> ResourceCache<L> {
    /// Creates a new cache backed by the given loader.
    pub fn new(loader: L) -> Self {
        Self {
            loader,
            cache: HashMap::new(),
        }
    }

    /// Loads a resource, returning a cached copy if available.
    pub fn load(&mut self, path: &str) -> EngineResult<Arc<Resource>> {
        if let Some(existing) = self.cache.get(path) {
            return Ok(Arc::clone(existing));
        }

        let resource = self.loader.load(path)?;
        self.cache.insert(path.to_string(), Arc::clone(&resource));
        Ok(resource)
    }

    /// Removes a specific path from the cache.
    pub fn invalidate(&mut self, path: &str) {
        self.cache.remove(path);
    }

    /// Clears the entire cache.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Returns the number of cached resources.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns `true` if the given path is currently cached.
    pub fn contains(&self, path: &str) -> bool {
        self.cache.contains_key(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdvariant::Variant;
    use std::cell::Cell;

    /// A test loader that counts how many times `load` is called and
    /// returns a simple resource.
    struct CountingLoader {
        call_count: Cell<u32>,
    }

    impl CountingLoader {
        fn new() -> Self {
            Self {
                call_count: Cell::new(0),
            }
        }
    }

    impl ResourceLoader for CountingLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            self.call_count.set(self.call_count.get() + 1);
            let mut r = Resource::new("TestResource");
            r.path = path.to_string();
            r.set_property("loaded", Variant::Bool(true));
            Ok(Arc::new(r))
        }
    }

    #[test]
    fn cache_returns_same_arc() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let a = cache.load("res://test.tres").unwrap();
        let b = cache.load("res://test.tres").unwrap();

        // Must be the same allocation — pointer equality.
        assert!(Arc::ptr_eq(&a, &b));
        // Loader should have been called only once.
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_different_paths_are_separate() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let a = cache.load("res://a.tres").unwrap();
        let b = cache.load("res://b.tres").unwrap();

        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_invalidate() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        cache.load("res://test.tres").unwrap();
        assert!(cache.contains("res://test.tres"));

        cache.invalidate("res://test.tres");
        assert!(!cache.contains("res://test.tres"));
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_clear() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        cache.load("res://a.tres").unwrap();
        cache.load("res://b.tres").unwrap();
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        // Use TresLoader which reads from disk — nonexistent file should error
        let mut cache = ResourceCache::new(crate::TresLoader::new());
        let result = cache.load("/nonexistent/path/that/does/not/exist.tres");
        assert!(result.is_err());
    }

    #[test]
    fn invalidate_nonexistent_path_does_not_panic() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);
        // Should not panic
        cache.invalidate("res://never_loaded.tres");
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_reload_after_invalidate() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let a = cache.load("res://test.tres").unwrap();
        cache.invalidate("res://test.tres");
        let b = cache.load("res://test.tres").unwrap();

        // After invalidation, a new resource is loaded (different Arc)
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn contains_returns_false_for_unknown() {
        let loader = CountingLoader::new();
        let cache = ResourceCache::new(loader);
        assert!(!cache.contains("res://unknown.tres"));
    }
}
