//! Texture caching for the editor viewport.
//!
//! Provides [`TextureCache`] which lazily loads PNG textures from disk
//! and caches them for fast repeated access during rendering.

use std::collections::HashMap;

use gdcore::math::Color;
use gdrender2d::texture::{load_png, resolve_res_path, Texture2D};
use gdvariant::Variant;

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
            // PNG keeps its direct decode; everything else — webp, svg, jpg, ctex,
            // and a png whose source was replaced by an imported artifact — goes
            // through the engine's format loader, which nothing used to call
            // (pat-0q2kx.45). That is why only PNG ever rendered here.
            let texture = load_png(&fs_path).or_else(|| load_via_format_loader(&fs_path));
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

    /// Updates the project root used to resolve `res://` paths.
    ///
    /// Clears the cache so any textures loaded against the previous root are
    /// reloaded relative to the new project (pat-v89tw: opening a real game
    /// project re-roots the editor).
    pub fn set_project_root(&mut self, project_root: impl Into<String>) {
        self.project_root = project_root.into();
        self.cache.clear();
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

/// Loads a texture through the engine's registered resource format loaders
/// (pat-0q2kx.45).
///
/// This is what makes `.webp`, `.svg`, `.jpg` and `.ctex` render in the editor
/// viewport, and what resolves a `.png` whose source was replaced by an imported
/// artifact: each image importer checks for a `<source>.import` sidecar and loads
/// the compiled `.godot/imported/*.ctex` instead. `None` when no loader claims the
/// extension or the file cannot be decoded — the caller keeps its "failed load"
/// cache entry rather than retrying every frame.
fn load_via_format_loader(fs_path: &str) -> Option<Texture2D> {
    let resource = gdresource::importers::ResourceFormatLoader::with_defaults()
        .load_resource(std::path::Path::new(fs_path))
        .ok()?;
    let int_prop = |key: &str| match resource.get_property(key) {
        Some(Variant::Int(i)) if *i > 0 => Some(*i as u32),
        _ => None,
    };
    let (width, height) = (int_prop("width")?, int_prop("height")?);
    let pixels: Vec<Color> = match resource.get_property("pixels") {
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        _ => return None,
    };
    // A truncated pixel array would index out of bounds during rasterization, so
    // require exactly the declared extent rather than padding it.
    if pixels.len() != (width as usize) * (height as usize) {
        return None;
    }
    Some(Texture2D {
        width,
        height,
        pixels,
    })
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
    fn set_project_root_updates_and_clears_cache() {
        let mut cache = TextureCache::new("/old/project");
        cache.insert(
            "res://icon.png",
            Texture2D::solid(1, 1, Color::rgb(1.0, 0.0, 0.0)),
        );
        assert_eq!(cache.len(), 1);
        // Re-rooting at a new project (pat-v89tw) updates the root and clears
        // textures resolved against the old root.
        cache.set_project_root("/new/project");
        assert_eq!(cache.project_root(), "/new/project");
        assert!(cache.is_empty());
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
