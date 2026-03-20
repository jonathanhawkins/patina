//! # gdserver2d
//!
//! Abstract 2D/3D server-facing runtime surface for the Patina Engine runtime.
//!
//! This crate defines the rendering server traits, canvas items, draw commands,
//! viewport management, and 3D mesh/material/server types.
//! Concrete rendering backends live in `gdrender2d`.

pub mod animated_sprite;
pub mod canvas;
pub mod canvas_layer;
pub mod material;
pub mod mesh;
pub mod parallax;
pub mod server;
pub mod server3d;
pub mod shader;
pub mod viewport;

pub use animated_sprite::{AnimatedSprite, SpriteFrame, SpriteFrames};
pub use canvas::{CanvasItem, CanvasItemId, DrawCommand};
pub use canvas_layer::CanvasLayer;
pub use material::{
    BlendMode, CanvasItemMaterial, CullMode, LightMode, Material3D, StandardMaterial3D,
    TransparencyMode, VisualShader, VisualShaderConnection, VisualShaderNode, VisualShaderNodeType,
};
pub use mesh::{Mesh3D, PrimitiveType};
pub use parallax::ParallaxLayer;
pub use server::{FrameData, RenderingServer2D};
pub use server3d::{
    perspective_projection_matrix, FrameData3D, Instance3D, Instance3DId, RenderingServer3D,
    Viewport3D,
};
pub use shader::{
    eval_color_expr, eval_float_expr, execute_fragment, parse_fragment_body, parse_uniforms,
    tokenize_shader, ColorExpr, CompiledShader, FloatExpr, FragmentContext, FragmentInstruction,
    Shader, ShaderCompiler, ShaderKeyword, ShaderMaterial, ShaderProcessor, ShaderToken,
    ShaderType, ShaderUniform, UniformType, UvAxis,
};
pub use viewport::Viewport;
