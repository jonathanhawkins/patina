//! Texture caching for the editor viewport.
//!
//! Provides [`TextureCache`] which lazily loads PNG textures from disk
//! and caches them for fast repeated access during rendering.

use std::collections::HashMap;

use gdrender2d::texture::{load_png, resolve_res_path, Texture2D};

/// Cache of loaded textures keyed by their `res://` path.
///
/// Textures are loaded on first access and reused on subsequent lookups.
/// A failed load is cached as `None` to avoid repeated disk access.
#[derive(Debug, Clone)]
pub struct TextureCache {
    /// Loaded textures keyed by resource path (e.g. `"res://icon.png"`).
    cache: HashMap<String, Option<Texture2D>>,
    /// Project root directory for resolving `res://` paths.
    project_root: String,
}

impl TextureCache {
    /// Creates a new empty texture cache.
    pub fn new(project_root: impl Into<String>) -> Self {
        Self {
            cache: HashMap::new(),
            project_root: project_root.into(),
        }
    }

    /// Returns the cached texture for the given resource path, loading it
    /// from disk on first access.
    ///
    /// Returns `None` if the texture could not be loaded (file not found,
    /// unsupported format, etc.). Failed loads are cached to avoid re-trying.
    pub fn get(&mut self, res_path: &str) -> Option<&Texture2D> {
        if !self.cache.contains_key(res_path) {
            let fs_path = resolve_res_path(res_path, &self.project_root);
            let texture = load_png(&fs_path);
            if texture.is_some() {
                tracing::debug!(
                    "TextureCache: loaded {} ({} bytes)",
                    res_path,
                    fs_path.len()
                );
            } else {
                tracing::debug!("TextureCache: failed to load {}", fs_path);
            }
            self.cache.insert(res_path.to_string(), texture);
        }
        self.cache.get(res_path).and_then(|opt| opt.as_ref())
    }

    /// Returns the number of cached entries (including failed loads).
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clears the entire cache, forcing textures to be reloaded on next access.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Removes a specific entry from the cache.
    pub fn invalidate(&mut self, res_path: &str) {
        self.cache.remove(res_path);
    }

    /// Returns the project root path.
    pub fn project_root(&self) -> &str {
        &self.project_root
    }

    /// Inserts a texture directly into the cache (useful for testing).
    pub fn insert(&mut self, res_path: &str, texture: Texture2D) {
        self.cache.insert(res_path.to_string(), Some(texture));
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    #[test]
    fn new_cache_is_empty() {
        let cache = TextureCache::new("/project");
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let mut cache = TextureCache::new("/project");
        let tex = Texture2D::solid(4, 4, Color::rgb(1.0, 0.0, 0.0));
        cache.insert("res://test.png", tex);
        assert_eq!(cache.len(), 1);
        let loaded = cache.get("res://test.png");
        assert!(loaded.is_some());
        let t = loaded.unwrap();
        assert_eq!(t.width, 4);
        assert_eq!(t.height, 4);
    }

    #[test]
    fn get_nonexistent_returns_none_and_caches() {
        let mut cache = TextureCache::new("/nonexistent/path");
        let result = cache.get("res://missing.png");
        assert!(result.is_none());
        // Should be cached as None so we don't re-attempt.
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn clear_empties_cache() {
        let mut cache = TextureCache::new("/project");
        cache.insert("res://a.png", Texture2D::solid(1, 1, Color::WHITE));
        cache.insert("res://b.png", Texture2D::solid(1, 1, Color::BLACK));
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn invalidate_removes_single_entry() {
        let mut cache = TextureCache::new("/project");
        cache.insert("res://a.png", Texture2D::solid(1, 1, Color::WHITE));
        cache.insert("res://b.png", Texture2D::solid(1, 1, Color::BLACK));
        cache.invalidate("res://a.png");
        assert_eq!(cache.len(), 1);
        assert!(cache.get("res://b.png").is_some());
    }

    #[test]
    fn project_root_accessor() {
        let cache = TextureCache::new("/my/project");
        assert_eq!(cache.project_root(), "/my/project");
    }

    #[test]
    fn default_has_empty_root() {
        let cache = TextureCache::default();
        assert_eq!(cache.project_root(), "");
        assert!(cache.is_empty());
    }

    #[test]
    fn get_caches_result_on_second_call() {
        let mut cache = TextureCache::new("/project");
        cache.insert(
            "res://cached.png",
            Texture2D::solid(2, 2, Color::rgb(0.0, 1.0, 0.0)),
        );
        // First access.
        let t1 = cache.get("res://cached.png").unwrap();
        assert_eq!(t1.width, 2);
        // Second access (should come from cache).
        let t2 = cache.get("res://cached.png").unwrap();
        assert_eq!(t2.width, 2);
    }
}
