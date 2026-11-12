//! Texture caching for the editor viewport (pat-mgixk.28).
//!
//! The cache itself moved to [`gdscene::texture_cache`] with the 2D canvas
//! builder that needs it: `gdscene::canvas_render` resolves a `Sprite2D`'s
//! texture through it, and `gdscene` cannot depend on `gdeditor`. Nothing about
//! it was editor-specific — it is a `res://` path to [`Texture2D`] map with a
//! project root — so it moved whole rather than being duplicated.
//!
//! Re-exported here so `EditorState::texture_cache` and every other call site
//! in this crate keep naming `crate::texture_cache::TextureCache`.

pub use gdscene::texture_cache::*;
