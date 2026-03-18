//! # gdrender2d
//!
//! 2D rendering implementation and render testing adapters
//! for the Patina Engine runtime.
//!
//! Provides a software renderer that implements the [`gdserver2d::RenderingServer2D`]
//! trait, along with drawing primitives, texture support, and test utilities.

pub mod draw;
pub mod renderer;
pub mod test_adapter;
pub mod texture;

pub use renderer::{FrameBuffer, SoftwareRenderer};
pub use texture::Texture2D;
