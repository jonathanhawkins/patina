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
pub mod font;
pub mod frame_server;
pub mod light2d;
pub mod polygon_raster;
pub mod text_layout;
pub mod text_server;
pub mod ttf_font;
pub mod renderer;
pub mod test_adapter;
pub mod texture;
#[cfg(feature = "gpu")]
pub mod wgpu_backend;

pub use compare::{compare_framebuffers, diff_image, DiffResult};
pub use export::{
    encode_bmp, encode_bmp_bgra, encode_png, encode_png_fast, encode_ppm, save_bmp, save_png,
    save_ppm,
};
pub use font::{BitmapFont, FontFile, Glyph};
pub use text_server::TextServerLite;
pub use ttf_font::{
    draw_label_text, draw_ttf_text, AtlasGlyph, GlyphAtlas, RasterGlyph, TtfFont,
};
pub use light2d::{
    apply_lights_to_framebuffer, is_occluded, light_factor_at, CanvasAmbient, Light2D,
    Light2DKind, LightOccluder2D, LIGHT2D_CLASS_NAMES,
};
pub use renderer::{Bgra8Image, FrameBuffer, Raster, SoftwareRenderer};
pub use texture::{decode_png, load_png, resolve_res_path, Texture2D};
#[cfg(feature = "gpu")]
pub use wgpu_backend::{BackendType, DeviceInfo, DrawBatch, SurfaceConfig, WgpuRenderer};
