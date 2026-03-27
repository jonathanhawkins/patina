//! # gdrender3d
//!
//! 3D rendering implementation and parity testing adapters
//! for the Patina Engine runtime.
//!
//! Provides a software renderer that implements the
//! [`gdserver3d::RenderingServer3D`] trait with two modes:
//!
//! - **Wireframe**: draws mesh edges as colored lines (for debugging).
//! - **Solid**: rasterizes filled triangles through a programmable
//!   vertex/fragment shader pipeline with per-pixel lighting.
//!
//! Also includes depth buffering, framebuffer comparison, and test utilities.

pub mod compare;
pub mod depth_buffer;
pub mod rasterizer;
pub mod renderer;
pub mod shader;
pub mod shadow_map;
pub mod test_adapter;
#[cfg(feature = "gpu")]
pub mod wgpu_pipeline;

pub use compare::{compare_framebuffers_3d, diff_image_3d, DiffResult3D};
pub use depth_buffer::DepthBuffer;
pub use rasterizer::{clip_to_screen, rasterize_triangle, ScreenVertex};
pub use renderer::{FrameBuffer3D, RenderMode, SoftwareRenderer3D};
pub use shadow_map::{generate_omni_shadow_cubemaps, generate_shadow_maps, ShadowMap};
pub use shader::{
    CustomFragmentShader, FragmentInput, FragmentShader, LambertFragmentShader, LightKind,
    LightUniform, PhongFragmentShader, ShaderUniforms, StandardVertexShader,
    UnlitFragmentShader, VertexInput, VertexOutput, VertexShader,
};
