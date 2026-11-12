//! # gdrender2d
//!
//! 2D rendering implementation and render testing adapters
//! for the Patina Engine runtime.
//!
//! Provides a software renderer that implements the [`gdserver2d::RenderingServer2D`]
//! trait, along with drawing primitives, texture support, and test utilities.

pub mod canvas_material;
pub mod compare;
pub mod draw;
pub mod export;
pub mod external_texture;
pub mod font;
pub mod frame_server;
pub mod label_settings;
pub mod light2d;
pub mod occluder_polygon;
pub mod polygon_raster;
pub mod renderer;
pub mod sdf2d;
pub mod test_adapter;
pub mod text_layout;
pub mod text_server;
pub mod texture;
pub mod ttf_font;
#[cfg(feature = "gpu")]
pub mod wgpu_backend;

pub use compare::{compare_framebuffers, diff_image, DiffResult};
pub use export::{
    encode_bmp, encode_bmp_bgra, encode_png, encode_png_fast, encode_ppm, save_bmp, save_png,
    save_ppm,
};
pub use font::{BitmapFont, FontFile, Glyph};
pub use label_settings::{FontHinting, FontSpacing, FontVariation, LabelSettings, SystemFont};
pub use light2d::{
    apply_lights_to_framebuffer, apply_lights_with_item_masks, composite_light, is_occluded,
    light_contribution_at, light_factor_at, shade_pixel, shadow_visibility, CanvasAmbient, Light2D,
    Light2DBlendMode, Light2DKind, LightOccluder2D, ShadowFilter, LIGHT2D_CLASS_NAMES,
};
pub use occluder_polygon::{OccluderCullMode, OccluderPolygon2D};
pub use renderer::{Bgra8Image, FrameBuffer, Raster, SoftwareRenderer};
pub use text_server::TextServerLite;
pub use texture::{decode_png, load_png, resolve_res_path, Texture2D};
pub use ttf_font::{draw_label_text, draw_ttf_text, AtlasGlyph, GlyphAtlas, RasterGlyph, TtfFont};
#[cfg(feature = "gpu")]
pub use wgpu_backend::{BackendType, DeviceInfo, DrawBatch, SurfaceConfig, WgpuRenderer};
