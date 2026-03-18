//! # gdserver2d
//!
//! Abstract 2D/3D server-facing runtime surface for the Patina Engine runtime.
//!
//! This crate defines the rendering server traits, canvas items, draw commands,
//! viewport management, and 3D mesh/material/server types.
//! Concrete rendering backends live in `gdrender2d`.

pub mod canvas;
pub mod material;
pub mod mesh;
pub mod server;
pub mod server3d;
pub mod viewport;

pub use canvas::{CanvasItem, CanvasItemId, DrawCommand};
pub use material::Material3D;
pub use mesh::{Mesh3D, PrimitiveType};
pub use server::{FrameData, RenderingServer2D};
pub use server3d::{
    perspective_projection_matrix, FrameData3D, Instance3D, Instance3DId, RenderingServer3D,
    Viewport3D,
};
pub use viewport::Viewport;
