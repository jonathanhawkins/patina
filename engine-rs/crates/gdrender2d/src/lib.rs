//! # gdrender2d
//!
//! 2D rendering implementation and render testing adapters
//! for the Patina Engine runtime.
//!
//! Provides a software renderer that implements the [`gdserver2d::RenderingServer2D`]
//! trait, along with drawing primitives, texture support, and test utilities.

pub mod compare;
pub mod draw;
pub mod export;
pub mod font;
pub mod frame_server;
pub mod renderer;
pub mod test_adapter;
pub mod texture;
#[cfg(feature = "gpu")]
pub mod wgpu_backend;

pub use compare::{compare_framebuffers, diff_image, DiffResult};
pub use export::{encode_bmp, encode_png, encode_ppm, save_bmp, save_png, save_ppm};
pub use font::{BitmapFont, FontFile, Glyph};
pub use renderer::{FrameBuffer, SoftwareRenderer};
pub use texture::{decode_png, load_png, resolve_res_path, Texture2D};
#[cfg(feature = "gpu")]
pub use wgpu_backend::{BackendType, DeviceInfo, DrawBatch, SurfaceConfig, WgpuRenderer};
