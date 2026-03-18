//! # gdserver2d
//!
//! Abstract 2D server-facing runtime surface for the Patina Engine runtime.
//!
//! This crate defines the rendering server trait, canvas items, draw commands,
//! and viewport management. Concrete rendering backends live in `gdrender2d`.

pub mod canvas;
pub mod server;
pub mod viewport;

pub use canvas::{CanvasItem, CanvasItemId, DrawCommand};
pub use server::{FrameData, RenderingServer2D};
pub use viewport::Viewport;
