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
//! - [`resource`] — The core [`Resource`] type.
//! - [`uid`] — A bidirectional [`UidRegistry`] mapping UIDs to paths.
//! - [`loader`] — The [`ResourceLoader`] trait and a [`TresLoader`] for
//!   Godot's `.tres` format.
//! - [`saver`] — The [`ResourceSaver`] trait and a [`TresSaver`] for
//!   writing `.tres` files.
//! - [`cache`] — A [`ResourceCache`] that deduplicates loads by path.

#![warn(clippy::all)]

pub mod bit_map;
pub mod block_compression;
pub mod cache;
pub mod config_file;
pub mod ctex;
pub mod dpi_texture;
pub mod fuzz_res;
pub mod gltf_import;
pub mod image;
pub mod image_svg;
pub mod image_tga_bmp;
pub mod importers;
pub mod loader;
pub mod missing;
pub mod noise;
pub mod pck;
pub mod portable_compressed_texture;
pub mod project;
pub mod radiance_hdr;
pub mod res_loader;
pub mod resource;
pub mod saver;
pub mod shader_ast;
pub mod shader_preprocessor;
pub mod shader_tokenizer;
pub mod uid;
pub mod unified;
pub mod zip;

// Re-export the most-used types at the crate root.
pub use cache::ResourceCache;
// `crate::` qualifier disambiguates the local module from the external `image`
// crate dependency (which is referenced elsewhere via `::image::`).
pub use crate::image::{Image, ImageFormat};
pub use config_file::{is_config_file_handle, ConfigFile};
pub use importers::{
    import_font, import_image, import_wav, load_import_file, parse_import_file, resolve_res_path,
    ImportFile, ResourceFormatLoader,
};
pub use loader::{parse_uid_string, parse_variant_value, ResourceLoader, TresLoader};
pub use missing::{MissingNode, MissingResource, PreservedProperties};
pub use noise::{FastNoiseLite, FractalType, NoiseType};
pub use project::{
    AutoloadEntry, GodotProjectFile, InputMapEntry, ProjectConfig, ProjectLoader, ShaderGlobal,
};
pub use res_loader::{
    is_res_binary, load_res_binary, parse_res_header, ResBinaryHeader, RES_MAGIC,
};
pub use resource::{ExtResource, Resource};
pub use saver::{ResourceSaver, TresSaver};
pub use uid::{id_to_text, parse_uid_sidecar, text_to_id, UidRegistry, UID_INVALID_TEXT};
pub use unified::UnifiedLoader;
pub use zip::{is_zip_packer_handle, is_zip_reader_handle, ZipAppend, ZipPacker, ZipReader};

/// Import a glTF 2.0 (`.gltf` / `.glb`) asset into a mesh document resource.
///
/// See [`gltf_import`] for supported features and the unsupported-extension list.
pub use gltf_import::{import_gltf, import_gltf_bytes};

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
