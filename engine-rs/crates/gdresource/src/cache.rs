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

    // ── Regression tests for cache deduplication (pat-2iu) ──

    #[test]
    fn load_same_tres_twice_returns_same_arc_with_strong_count() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let a = cache.load("res://item.tres").unwrap();
        assert_eq!(Arc::strong_count(&a), 2); // one in cache, one local

        let b = cache.load("res://item.tres").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "second load must return the same Arc");
        assert_eq!(Arc::strong_count(&a), 3); // cache + a + b
        assert_eq!(cache.loader.call_count.get(), 1);
    }

    #[test]
    fn invalidate_reload_produces_different_arc_old_survives() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let old = cache.load("res://scene.tres").unwrap();
        assert_eq!(Arc::strong_count(&old), 2);

        cache.invalidate("res://scene.tres");
        // Old Arc is still alive — only the cache dropped its reference.
        assert_eq!(Arc::strong_count(&old), 1);

        let new = cache.load("res://scene.tres").unwrap();
        assert!(
            !Arc::ptr_eq(&old, &new),
            "reload after invalidation must allocate a new Arc"
        );
        assert_eq!(Arc::strong_count(&new), 2);
        assert_eq!(cache.loader.call_count.get(), 2);
    }

    #[test]
    fn sequential_loads_all_return_cached() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let mut arcs = Vec::new();
        for _ in 0..10 {
            arcs.push(cache.load("res://repeated.tres").unwrap());
        }

        // Every Arc must be pointer-equal to the first.
        for arc in &arcs[1..] {
            assert!(Arc::ptr_eq(&arcs[0], arc));
        }
        // Loader called exactly once despite 10 loads.
        assert_eq!(cache.loader.call_count.get(), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn multiple_paths_independent_after_invalidation() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let a1 = cache.load("res://a.tres").unwrap();
        let b1 = cache.load("res://b.tres").unwrap();
        assert_eq!(cache.loader.call_count.get(), 2);

        // Invalidate only A.
        cache.invalidate("res://a.tres");
        assert!(!cache.contains("res://a.tres"));
        assert!(cache.contains("res://b.tres"));

        // B is still the same cached Arc.
        let b2 = cache.load("res://b.tres").unwrap();
        assert!(
            Arc::ptr_eq(&b1, &b2),
            "B must remain cached after A is invalidated"
        );
        assert_eq!(cache.loader.call_count.get(), 2); // no new load for B

        // Reloading A gives a new Arc.
        let a2 = cache.load("res://a.tres").unwrap();
        assert!(
            !Arc::ptr_eq(&a1, &a2),
            "A must be a fresh allocation after invalidation"
        );
        assert_eq!(cache.loader.call_count.get(), 3);
    }

    #[test]
    fn clear_then_reload_produces_new_allocations() {
        let loader = CountingLoader::new();
        let mut cache = ResourceCache::new(loader);

        let paths = ["res://x.tres", "res://y.tres", "res://z.tres"];
        let old: Vec<_> = paths.iter().map(|p| cache.load(p).unwrap()).collect();
        assert_eq!(cache.loader.call_count.get(), 3);

        cache.clear();
        assert!(cache.is_empty());

        // Old Arcs survive (strong_count == 1, held only locally).
        for arc in &old {
            assert_eq!(Arc::strong_count(arc), 1);
        }

        // Reload all — must produce new, distinct Arcs.
        let new: Vec<_> = paths.iter().map(|p| cache.load(p).unwrap()).collect();
        assert_eq!(cache.loader.call_count.get(), 6); // 3 original + 3 reloads

        for (o, n) in old.iter().zip(new.iter()) {
            assert!(
                !Arc::ptr_eq(o, n),
                "post-clear reload must not reuse old Arcs"
            );
        }
    }
}
