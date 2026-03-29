//! # gdserver3d
//!
//! Abstract 3D rendering server surface for the Patina Engine runtime.
//!
//! This crate defines the 3D rendering server trait, instances, viewports,
//! meshes, materials, lights, projection math, sky resources, and
//! environment settings. Concrete rendering backends live in `gdrender3d`.

pub mod csg;
pub mod environment;
pub mod fog_volume;
pub mod gi;
pub mod instance;
pub mod light;
pub mod material;
pub mod mesh;
pub mod multimesh;
pub mod navigation;
pub mod occluder;
pub mod particles3d;
pub mod primitive_mesh;
pub mod projection;
pub mod reflection_probe;
pub mod server;
pub mod shader;
pub mod sky;
pub mod viewport;

pub use csg::{CSGBox3D, CSGCombiner3D, CSGCylinder3D, CSGMesh3D, CSGOperation, CSGSphere3D};
pub use environment::{AmbientSource, BackgroundMode, Environment3D, ToneMapper};
pub use fog_volume::{FogMaterial, FogVolume, FogVolumeShape};
pub use gi::{
    LightmapBakeQuality, LightmapGI, LightmapGIId, LightmapProbeGeneration, VoxelGI, VoxelGIId,
    VoxelGISubdiv,
};
pub use instance::{Instance3D, Instance3DId};
pub use light::{CubeFace, Light3D, Light3DId, LightType, OmniShadowMode, ShadowCubemap};
pub use material::{Material3D, ShadingMode, StandardMaterial3D, TextureSlot};
pub use mesh::Surface3D;
pub use mesh::{Mesh3D, PrimitiveType};
pub use multimesh::{MultiMesh3D, TransformFormat};
pub use navigation::{
    bake_navigation_mesh, BakeSourceGeometry3D, NavPolygon3D, NavigationMesh3D, NavigationRegion3D,
};
pub use occluder::{Occluder3D, OccluderInstance3D};
pub use particles3d::{
    DrawMode3D, EmissionShape3D, GPUParticles3D, Particle3D, ParticleProcessMaterial3D,
};
pub use primitive_mesh::{
    ArrayMesh, BoxMesh, CapsuleMesh, CylinderMesh, PlaneMesh, PrimitiveMeshType, SphereMesh,
};
pub use projection::perspective_projection_matrix;
pub use reflection_probe::{
    ReflectionProbe, ReflectionProbeAmbientMode, ReflectionProbeId, ReflectionProbeUpdateMode,
};
pub use server::{FrameData3D, RenderingServer3D};
pub use shader::{
    uniform_type_to_wgsl, uniform_type_wgsl_size, CompiledShader3D, DiagnosticSeverity,
    FragmentContext3D, RenderModeFlags, Shader3D, ShaderCompiler3D, ShaderDiagnostic,
    ShaderMaterial3D, ShaderProcessor3D, ShaderType3D, ShaderUniform3D, UniformType,
};
pub use sky::{
    PanoramicSkyMaterial, PhysicalSkyMaterial, ProceduralSkyMaterial, Sky, SkyMaterial,
    SkyProcessMode,
};
pub use viewport::Viewport3D;
