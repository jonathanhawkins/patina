//! # gdresource
//!
//! Resources, loaders, savers, cache, and UID/path semantics
//! for the Patina Engine runtime.
//!
//! A **resource** is a loadable, cacheable data object identified by a file
//! path or a unique ID ([`ResourceUid`](gdcore::ResourceUid)). Resources
//! hold typed properties as [`Variant`](gdvariant::Variant) values and may
//! reference sub-resources and external resources.
//!
//! This crate provides:
//!
//! - [`resource`] — The core [`Resource`](resource::Resource) type.
//! - [`uid`] — A bidirectional [`UidRegistry`](uid::UidRegistry) mapping
//!   UIDs to paths.
//! - [`loader`] — The [`ResourceLoader`](loader::ResourceLoader) trait and
//!   a [`TresLoader`](loader::TresLoader) for Godot's `.tres` format.
//! - [`saver`] — The [`ResourceSaver`](saver::ResourceSaver) trait and
//!   a [`TresSaver`](saver::TresSaver) for writing `.tres` files.
//! - [`cache`] — A [`ResourceCache`](cache::ResourceCache) that deduplicates
//!   loads by path.

#![warn(clippy::all)]

pub mod cache;
pub mod importers;
pub mod loader;
pub mod resource;
pub mod saver;
pub mod uid;

// Re-export the most-used types at the crate root.
pub use cache::ResourceCache;
pub use importers::{
    import_font, import_image, import_wav, load_import_file, parse_import_file, resolve_res_path,
    ImportFile, ResourceFormatLoader,
};
pub use loader::{parse_variant_value, ResourceLoader, TresLoader};
pub use resource::{ExtResource, Resource};
pub use saver::{ResourceSaver, TresSaver};
pub use uid::UidRegistry;

#[cfg(test)]
mod integration_tests {
    use super::*;
    use gdvariant::Variant;
    use std::sync::Arc;

    /// Roundtrip test: parse a .tres string, save it back, re-parse,
    /// and verify the resources are equivalent.
    #[test]
    fn roundtrip_tres() {
        let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "RoundTrip"
value = 42
position = Vector2(10, 20)
flag = true
"#;

        let loader = TresLoader::new();
        let res1 = loader.parse_str(source, "res://rt.tres").unwrap();

        // Save to string.
        let saver = TresSaver::new();
        let saved = saver.save_to_string(&res1).unwrap();

        // Re-parse.
        let res2 = loader.parse_str(&saved, "res://rt.tres").unwrap();

        // Verify properties match.
        assert_eq!(res1.class_name, res2.class_name);
        assert_eq!(res1.get_property("name"), res2.get_property("name"));
        assert_eq!(res1.get_property("value"), res2.get_property("value"));
        assert_eq!(res1.get_property("position"), res2.get_property("position"));
        assert_eq!(res1.get_property("flag"), res2.get_property("flag"));
    }

    /// Roundtrip with sub-resources.
    #[test]
    fn roundtrip_with_subresources() {
        let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_001"]
bg_color = Color(0.2, 0.3, 0.4, 1)
border_width = 2

[resource]
name = "MyTheme"
"#;

        let loader = TresLoader::new();
        let res1 = loader.parse_str(source, "res://theme.tres").unwrap();

        let saver = TresSaver::new();
        let saved = saver.save_to_string(&res1).unwrap();

        let res2 = loader.parse_str(&saved, "res://theme.tres").unwrap();

        assert_eq!(res1.class_name, res2.class_name);
        assert_eq!(res1.subresources.len(), res2.subresources.len());

        let sub1 = &res1.subresources["StyleBoxFlat_001"];
        let sub2 = &res2.subresources["StyleBoxFlat_001"];
        assert_eq!(sub1.class_name, sub2.class_name);
        assert_eq!(sub1.get_property("bg_color"), sub2.get_property("bg_color"));
        assert_eq!(
            sub1.get_property("border_width"),
            sub2.get_property("border_width")
        );
    }

    /// Cache returns pointer-equal Arcs using a TresLoader-based cache
    /// (via a small adapter).
    #[test]
    fn cache_with_inline_loader() {
        use gdcore::error::EngineResult;

        /// A loader that always returns a fixed resource.
        struct InlineLoader;

        impl ResourceLoader for InlineLoader {
            fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
                let mut r = Resource::new("Cached");
                r.path = path.to_string();
                r.set_property("cached", Variant::Bool(true));
                Ok(Arc::new(r))
            }
        }

        let mut cache = ResourceCache::new(InlineLoader);
        let a = cache.load("res://cached.tres").unwrap();
        let b = cache.load("res://cached.tres").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
