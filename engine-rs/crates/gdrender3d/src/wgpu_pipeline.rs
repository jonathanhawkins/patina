//! GPU-accelerated 3D render pipeline using wgpu with WGSL shaders.
//!
//! Provides [`GpuRenderer3D`] — a wgpu-based renderer that implements the
//! [`RenderingServer3D`] trait with real vertex and fragment shader stages
//! running on the GPU. Feature-gated behind `"gpu"`.
//!
//! The pipeline supports three shading modes matching the software renderer:
//! - **Unlit**: albedo + emission, no lighting
//! - **Lambert**: diffuse lighting with ambient term
//! - **Phong**: diffuse + Blinn-Phong specular highlights

use std::collections::HashMap;

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::reflection_probe::ReflectionProbeId;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::ShaderMaterial3D;
use gdserver3d::viewport::Viewport3D;

// ── WGSL shader source ──────────────────────────────────────────────

/// Standard 3D vertex + fragment shader in WGSL.
///
/// Vertex stage: applies model-view-projection transforms, passes world
/// position and normal to fragment stage.
///
/// Fragment stage: selects shading via `shading_mode` uniform —
/// 0 = unlit, 1 = Lambert, 2 = Phong.
pub const SHADER_3D_WGSL: &str = r#"
// ── Bind group 0: per-frame camera uniforms ─────────────────────────

struct CameraUniforms {
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    _pad0: f32,
};
@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

// ── Bind group 1: per-object model + material uniforms ──────────────

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    albedo: vec4<f32>,
    emission: vec4<f32>,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,   // 0=unlit, 1=lambert, 2=phong
    num_lights: u32,
};
@group(1) @binding(0)
var<uniform> model: ModelUniforms;

// ── Bind group 2: light array ───────────────────────────────────────

struct LightData {
    // kind: 0=directional, 1=point, 2=spot
    kind: u32,
    shadow_enabled: u32,
    range: f32,
    attenuation: f32,
    direction: vec3<f32>,
    spot_angle: f32,
    color: vec3<f32>,
    spot_angle_attenuation: f32,
    position: vec3<f32>,
    _pad: f32,
};

struct LightArray {
    lights: array<LightData, 16>,
};
@group(2) @binding(0)
var<uniform> light_array: LightArray;

// ── Vertex stage ────────────────────────────────────────────────────

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    let world_pos = model.model_matrix * vec4<f32>(in.position, 1.0);
    out.world_position = world_pos.xyz;

    let view_pos = camera.view_matrix * world_pos;
    out.clip_position = camera.projection_matrix * view_pos;

    // Transform normal by upper-left 3x3 of model matrix.
    let normal_mat = mat3x3<f32>(
        model.model_matrix[0].xyz,
        model.model_matrix[1].xyz,
        model.model_matrix[2].xyz,
    );
    out.world_normal = normalize(normal_mat * in.normal);

    out.uv = in.uv;

    return out;
}

// ── Fragment stage ──────────────────────────────────────────────────

fn distance_attenuation(dist: f32, range: f32, atten: f32) -> f32 {
    if (range <= 0.0) {
        return 1.0;
    }
    let ratio = min(dist / range, 1.0);
    return max(1.0 - pow(ratio, atten), 0.0);
}

fn evaluate_light(light: LightData, frag_pos: vec3<f32>) -> vec2<f32> {
    // Returns (dot_product_factor, intensity) packed as vec2.
    // Caller uses the light direction separately.
    if (light.kind == 0u) {
        // Directional.
        return vec2<f32>(1.0, 1.0);
    }
    let to_light = light.position - frag_pos;
    let dist = length(to_light);
    if (dist < 1e-6) {
        return vec2<f32>(1.0, 1.0);
    }
    let atten = distance_attenuation(dist, light.range, light.attenuation);
    if (light.kind == 2u) {
        // Spot cone attenuation.
        let dir = normalize(to_light);
        let cos_angle = dot(-dir, light.direction);
        let cos_outer = cos(light.spot_angle);
        if (cos_angle <= cos_outer) {
            return vec2<f32>(0.0, 0.0);
        }
        let t = clamp((cos_angle - cos_outer) / (1.0 - cos_outer), 0.0, 1.0);
        return vec2<f32>(1.0, atten * pow(t, light.spot_angle_attenuation));
    }
    return vec2<f32>(1.0, atten);
}

fn get_light_dir(light: LightData, frag_pos: vec3<f32>) -> vec3<f32> {
    if (light.kind == 0u) {
        return light.direction;
    }
    let to_light = light.position - frag_pos;
    let dist = length(to_light);
    if (dist < 1e-6) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return to_light / dist;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo = model.albedo;
    let emission = model.emission;

    // Unlit mode.
    if (model.shading_mode == 0u) {
        return vec4<f32>(
            min(albedo.r + emission.r, 1.0),
            min(albedo.g + emission.g, 1.0),
            min(albedo.b + emission.b, 1.0),
            albedo.a,
        );
    }

    let normal = normalize(in.world_normal);
    let ambient = 0.1;
    var color = vec3<f32>(
        ambient * albedo.r,
        ambient * albedo.g,
        ambient * albedo.b,
    );

    let num_lights = min(model.num_lights, 16u);
    for (var i = 0u; i < num_lights; i = i + 1u) {
        let light = light_array.lights[i];
        let light_dir = get_light_dir(light, in.world_position);
        let eval = evaluate_light(light, in.world_position);
        let intensity = eval.y;

        let n_dot_l = max(dot(normal, light_dir), 0.0);
        let contrib = n_dot_l * intensity;

        // Diffuse.
        color += vec3<f32>(
            albedo.r * light.color.r * contrib,
            albedo.g * light.color.g * contrib,
            albedo.b * light.color.b * contrib,
        );

        // Phong specular (mode == 2).
        if (model.shading_mode == 2u && n_dot_l > 0.0 && intensity > 0.0) {
            let view_dir = normalize(camera.camera_position - in.world_position);
            let half_dir = normalize(light_dir + view_dir);
            let shininess = max((1.0 - model.roughness) * 128.0, 1.0);
            let spec = pow(max(dot(normal, half_dir), 0.0), shininess);
            let spec_strength = model.metallic * 0.5 + 0.5;
            color += light.color * spec * spec_strength * intensity;
        }
    }

    return vec4<f32>(
        min(color.r + emission.r, 1.0),
        min(color.g + emission.g, 1.0),
        min(color.b + emission.b, 1.0),
        albedo.a,
    );
}
"#;

// ── GPU-side data types ─────────────────────────────────────────────

/// GPU vertex with position, normal, and UV.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuVertex {
    /// Position (x, y, z).
    pub position: [f32; 3],
    /// Normal (x, y, z).
    pub normal: [f32; 3],
    /// Texture coordinates (u, v).
    pub uv: [f32; 2],
}

impl GpuVertex {
    /// Returns the wgpu vertex buffer layout for this type.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // normal
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // uv
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// Camera uniform data (bind group 0).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniformData {
    /// View matrix (column-major 4x4).
    pub view_matrix: [[f32; 4]; 4],
    /// Projection matrix (column-major 4x4).
    pub projection_matrix: [[f32; 4]; 4],
    /// Camera world position.
    pub camera_position: [f32; 3],
    /// Padding.
    pub _pad0: f32,
}

/// Per-object model + material uniform data (bind group 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniformData {
    /// Model matrix (column-major 4x4).
    pub model_matrix: [[f32; 4]; 4],
    /// Albedo RGBA.
    pub albedo: [f32; 4],
    /// Emission RGBA.
    pub emission: [f32; 4],
    /// Roughness.
    pub roughness: f32,
    /// Metallic.
    pub metallic: f32,
    /// Shading mode: 0=unlit, 1=lambert, 2=phong.
    pub shading_mode: u32,
    /// Number of active lights.
    pub num_lights: u32,
}

/// Single light data for the GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLightData {
    /// 0=directional, 1=point, 2=spot.
    pub kind: u32,
    /// Whether shadows are enabled.
    pub shadow_enabled: u32,
    /// Light range.
    pub range: f32,
    /// Distance attenuation exponent.
    pub attenuation: f32,
    /// Light direction (normalized).
    pub direction: [f32; 3],
    /// Spot angle in radians.
    pub spot_angle: f32,
    /// Light color (RGB).
    pub color: [f32; 3],
    /// Spot angle attenuation.
    pub spot_angle_attenuation: f32,
    /// Light position.
    pub position: [f32; 3],
    /// Padding.
    pub _pad: f32,
}

/// Light array uniform (bind group 2). Fixed-size array of 16 lights.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightArrayData {
    /// Up to 16 lights.
    pub lights: [GpuLightData; 16],
}

// ── GPU context ─────────────────────────────────────────────────────

/// Core wgpu handles for 3D rendering.
pub struct GpuContext3D {
    /// The wgpu instance.
    pub instance: wgpu::Instance,
    /// The selected adapter.
    pub adapter: wgpu::Adapter,
    /// The logical device.
    pub device: wgpu::Device,
    /// The command queue.
    pub queue: wgpu::Queue,
}

impl std::fmt::Debug for GpuContext3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let info = self.adapter.get_info();
        f.debug_struct("GpuContext3D")
            .field("adapter", &info.name)
            .field("backend", &info.backend)
            .finish()
    }
}

impl GpuContext3D {
    /// Creates a new headless GPU context.
    ///
    /// Returns `None` if no adapter is available (headless CI without GPU).
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("patina-3d-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .ok()?;

        Some(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Returns the adapter name string.
    pub fn adapter_name(&self) -> String {
        self.adapter.get_info().name
    }
}

// ── Render pipeline ─────────────────────────────────────────────────

/// A compiled 3D render pipeline with bind group layouts.
pub struct RenderPipeline3D {
    /// The wgpu render pipeline.
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group layout for camera uniforms (group 0).
    pub camera_layout: wgpu::BindGroupLayout,
    /// Bind group layout for model uniforms (group 1).
    pub model_layout: wgpu::BindGroupLayout,
    /// Bind group layout for lights (group 2).
    pub light_layout: wgpu::BindGroupLayout,
}

impl RenderPipeline3D {
    /// Creates the standard 3D render pipeline from the built-in WGSL shader.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-3d-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_3D_WGSL.into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-3d-pipeline-layout"),
            bind_group_layouts: &[
                Some(&camera_layout),
                Some(&model_layout),
                Some(&light_layout),
            ],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-3d-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[GpuVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            camera_layout,
            model_layout,
            light_layout,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Converts a [`Transform3D`] into the inverse view matrix as column-major 4x4.
fn transform_to_view_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    let inv = t.inverse();
    [
        [inv.basis.x.x, inv.basis.x.y, inv.basis.x.z, 0.0],
        [inv.basis.y.x, inv.basis.y.y, inv.basis.y.z, 0.0],
        [inv.basis.z.x, inv.basis.z.y, inv.basis.z.z, 0.0],
        [inv.origin.x, inv.origin.y, inv.origin.z, 1.0],
    ]
}

/// Converts a [`Transform3D`] into a column-major model matrix.
fn transform_to_model_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    [
        [t.basis.x.x, t.basis.x.y, t.basis.x.z, 0.0],
        [t.basis.y.x, t.basis.y.y, t.basis.y.z, 0.0],
        [t.basis.z.x, t.basis.z.y, t.basis.z.z, 0.0],
        [t.origin.x, t.origin.y, t.origin.z, 1.0],
    ]
}

/// Converts a scene [`Light3D`] into a [`GpuLightData`].
fn light_to_gpu(light: &Light3D) -> GpuLightData {
    let kind = match light.light_type {
        LightType::Directional => 0u32,
        LightType::Point => 1u32,
        LightType::Spot => 2u32,
    };

    GpuLightData {
        kind,
        shadow_enabled: if light.shadow_enabled { 1 } else { 0 },
        range: light.range,
        attenuation: light.attenuation,
        direction: [light.direction.x, light.direction.y, light.direction.z],
        spot_angle: light.spot_angle,
        color: [
            light.color.r * light.energy,
            light.color.g * light.energy,
            light.color.b * light.energy,
        ],
        spot_angle_attenuation: light.spot_angle_attenuation,
        position: [light.position.x, light.position.y, light.position.z],
        _pad: 0.0,
    }
}

/// Creates a buffer and writes initial data to it.
fn create_buffer_init(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    data: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: data.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, data);
    buf
}

/// Packs mesh vertex data into a GPU vertex buffer.
fn mesh_to_gpu_vertices(mesh: &Mesh3D) -> Vec<GpuVertex> {
    let vert_count = mesh.vertices.len();
    let mut out = Vec::with_capacity(vert_count);
    for i in 0..vert_count {
        let pos = mesh.vertices[i];
        let normal = if i < mesh.normals.len() {
            mesh.normals[i]
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        let uv = if i < mesh.uvs.len() {
            mesh.uvs[i]
        } else {
            [0.0, 0.0]
        };
        out.push(GpuVertex {
            position: [pos.x, pos.y, pos.z],
            normal: [normal.x, normal.y, normal.z],
            uv,
        });
    }
    out
}

// ── GpuRenderer3D ───────────────────────────────────────────────────

/// A wgpu-based 3D renderer implementing [`RenderingServer3D`].
///
/// Renders 3D scenes using a real GPU render pipeline with WGSL vertex
/// and fragment shaders. Supports unlit, Lambert, and Phong shading.
pub struct GpuRenderer3D {
    ctx: GpuContext3D,
    pipeline: RenderPipeline3D,
    instances: HashMap<u64, Instance3D>,
    lights: Vec<Light3D>,
    next_id: u64,
    /// The texture format used for color attachments.
    pub target_format: wgpu::TextureFormat,
}

impl std::fmt::Debug for GpuRenderer3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer3D")
            .field("adapter", &self.ctx.adapter_name())
            .field("instances", &self.instances.len())
            .field("lights", &self.lights.len())
            .finish()
    }
}

impl GpuRenderer3D {
    /// Creates a new GPU renderer.
    ///
    /// Returns `None` if no GPU adapter is available.
    pub fn new() -> Option<Self> {
        let ctx = GpuContext3D::new()?;
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let pipeline = RenderPipeline3D::new(&ctx.device, format);
        Some(Self {
            ctx,
            pipeline,
            instances: HashMap::new(),
            lights: Vec::new(),
            next_id: 1,
            target_format: format,
        })
    }

    /// Returns a reference to the underlying GPU context.
    pub fn context(&self) -> &GpuContext3D {
        &self.ctx
    }

    /// Renders a frame and reads back the pixel data from the GPU.
    fn render_and_readback(&mut self, viewport: &Viewport3D) -> FrameData3D {
        let w = viewport.width.max(1);
        let h = viewport.height.max(1);
        let device = &self.ctx.device;
        let queue = &self.ctx.queue;

        // Create color and depth textures.
        let color_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d-color-target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d-depth-target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Prepare camera uniforms.
        let view_matrix = transform_to_view_matrix(&viewport.camera_transform);
        let projection_matrix = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );
        let cam_pos = viewport.camera_transform.origin;
        let camera_data = CameraUniformData {
            view_matrix,
            projection_matrix,
            camera_position: [cam_pos.x, cam_pos.y, cam_pos.z],
            _pad0: 0.0,
        };
        let camera_buf = create_buffer_init(
            device,
            queue,
            "camera-uniform-buf",
            bytemuck::bytes_of(&camera_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind-group"),
            layout: &self.pipeline.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        // Prepare light uniform.
        let mut light_data = LightArrayData {
            lights: [GpuLightData {
                kind: 0,
                shadow_enabled: 0,
                range: 0.0,
                attenuation: 1.0,
                direction: [0.0, -1.0, 0.0],
                spot_angle: 0.0,
                color: [0.0, 0.0, 0.0],
                spot_angle_attenuation: 1.0,
                position: [0.0, 0.0, 0.0],
                _pad: 0.0,
            }; 16],
        };
        for (i, light) in self.lights.iter().enumerate().take(16) {
            light_data.lights[i] = light_to_gpu(light);
        }
        let light_buf = create_buffer_init(
            device,
            queue,
            "light-uniform-buf",
            bytemuck::bytes_of(&light_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let light_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light-bind-group"),
            layout: &self.pipeline.light_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("3d-render-encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &camera_bg, &[]);
            pass.set_bind_group(2, &light_bg, &[]);

            // Draw each visible instance.
            for inst in self.instances.values() {
                if !inst.visible {
                    continue;
                }
                let mesh = match &inst.mesh {
                    Some(m) => m,
                    None => continue,
                };

                let vertices = mesh_to_gpu_vertices(mesh);
                if vertices.is_empty() {
                    continue;
                }

                let vertex_buf = create_buffer_init(
                    device,
                    queue,
                    "vertex-buf",
                    bytemuck::cast_slice(&vertices),
                    wgpu::BufferUsages::VERTEX,
                );

                let index_buf = create_buffer_init(
                    device,
                    queue,
                    "index-buf",
                    bytemuck::cast_slice(&mesh.indices),
                    wgpu::BufferUsages::INDEX,
                );

                let material = inst.material.as_ref().cloned().unwrap_or_default();
                let shading_mode = match material.shading_mode {
                    ShadingMode::Unlit => 0u32,
                    ShadingMode::Lambert => 1u32,
                    ShadingMode::Phong => 2u32,
                };

                let model_data = ModelUniformData {
                    model_matrix: transform_to_model_matrix(&inst.transform),
                    albedo: [
                        material.albedo.r,
                        material.albedo.g,
                        material.albedo.b,
                        material.albedo.a,
                    ],
                    emission: [
                        material.emission.r,
                        material.emission.g,
                        material.emission.b,
                        material.emission.a,
                    ],
                    roughness: material.roughness,
                    metallic: material.metallic,
                    shading_mode,
                    num_lights: self.lights.len().min(16) as u32,
                };

                let model_buf = create_buffer_init(
                    device,
                    queue,
                    "model-uniform-buf",
                    bytemuck::bytes_of(&model_data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let model_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("model-bind-group"),
                    layout: &self.pipeline.model_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: model_buf.as_entire_binding(),
                    }],
                });

                pass.set_bind_group(1, &model_bg, &[]);
                pass.set_vertex_buffer(0, vertex_buf.slice(..));
                pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
            }
        }

        // Copy color texture to readback buffer.
        let bytes_per_pixel = 4u32;
        let unpadded_row = w * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (unpadded_row + align - 1) / align * align;

        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color-readback"),
            size: (padded_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &color_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        // Copy depth texture to readback buffer.
        let depth_bytes_per_pixel = 4u32; // Depth32Float = 4 bytes
        let depth_unpadded_row = w * depth_bytes_per_pixel;
        let depth_padded_row = (depth_unpadded_row + align - 1) / align * align;
        let depth_readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth-readback"),
            size: (depth_padded_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &depth_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &depth_readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(depth_padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map and read color data.
        let color_slice = readback_buf.slice(..);
        color_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        let color_mapped = color_slice.get_mapped_range();

        let mut pixels = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            let start = (row * padded_row) as usize;
            for col in 0..w {
                let offset = start + (col * bytes_per_pixel) as usize;
                // BGRA → RGBA
                let b = color_mapped[offset] as f32 / 255.0;
                let g = color_mapped[offset + 1] as f32 / 255.0;
                let r = color_mapped[offset + 2] as f32 / 255.0;
                let a = color_mapped[offset + 3] as f32 / 255.0;
                pixels.push(Color::new(r, g, b, a));
            }
        }
        drop(color_mapped);

        // Map and read depth data.
        let depth_slice = depth_readback_buf.slice(..);
        depth_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        let depth_mapped = depth_slice.get_mapped_range();

        let mut depth = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            let start = (row * depth_padded_row) as usize;
            for col in 0..w {
                let offset = start + (col * depth_bytes_per_pixel) as usize;
                let d = f32::from_le_bytes([
                    depth_mapped[offset],
                    depth_mapped[offset + 1],
                    depth_mapped[offset + 2],
                    depth_mapped[offset + 3],
                ]);
                depth.push(d);
            }
        }
        drop(depth_mapped);

        FrameData3D {
            width: w,
            height: h,
            pixels,
            depth,
        }
    }
}

impl RenderingServer3D for GpuRenderer3D {
    fn create_instance(&mut self) -> Instance3DId {
        let id_val = self.next_id;
        self.next_id += 1;
        let id = Instance3DId(id_val);
        self.instances.insert(
            id_val,
            Instance3D {
                id,
                mesh: None,
                material: None,
                shader_material: None,
                transform: Transform3D::IDENTITY,
                visible: true,
            },
        );
        id
    }

    fn free_instance(&mut self, id: Instance3DId) {
        self.instances.remove(&id.0);
    }

    fn set_mesh(&mut self, id: Instance3DId, mesh: Mesh3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.mesh = Some(mesh);
        }
    }

    fn set_material(&mut self, id: Instance3DId, material: Material3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.material = Some(material);
        }
    }

    fn set_shader_material(&mut self, id: Instance3DId, material: ShaderMaterial3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.shader_material = Some(material);
        }
    }

    fn set_multimesh(&mut self, id: Instance3DId, multimesh: gdserver3d::multimesh::MultiMesh3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.multimesh = Some(multimesh);
        }
    }

    fn clear_multimesh(&mut self, id: Instance3DId) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.multimesh = None;
        }
    }

    fn set_transform(&mut self, id: Instance3DId, transform: Transform3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.transform = transform;
        }
    }

    fn set_visible(&mut self, id: Instance3DId, visible: bool) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.visible = visible;
        }
    }

    fn add_light(&mut self, _id: Light3DId) {
        // Light is added via update_light.
    }

    fn remove_light(&mut self, id: Light3DId) {
        self.lights.retain(|l| l.id != id);
    }

    fn update_light(&mut self, light: &Light3D) {
        if let Some(existing) = self.lights.iter_mut().find(|l| l.id == light.id) {
            *existing = light.clone();
        } else {
            self.lights.push(light.clone());
        }
    }

    fn add_reflection_probe(&mut self, _id: ReflectionProbeId) {
        // Reflection probes not yet supported in GPU pipeline.
    }

    fn remove_reflection_probe(&mut self, _id: ReflectionProbeId) {
        // Reflection probes not yet supported in GPU pipeline.
    }

    fn render_frame(&mut self, viewport: &Viewport3D) -> FrameData3D {
        self.render_and_readback(viewport)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_source_compiles() {
        // Validate that the WGSL source parses without errors.
        // This doesn't need a GPU — naga can validate WGSL statically.
        let result = wgpu::naga::front::wgsl::parse_str(SHADER_3D_WGSL);
        assert!(result.is_ok(), "WGSL parse error: {:?}", result.err());
    }

    #[test]
    fn gpu_vertex_layout_size() {
        // position (3*4) + normal (3*4) + uv (2*4) = 32 bytes.
        assert_eq!(std::mem::size_of::<GpuVertex>(), 32);
    }

    #[test]
    fn camera_uniform_size() {
        // 2 mat4x4 (2*64) + vec3 (12) + pad (4) = 144 bytes.
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 144);
    }

    #[test]
    fn model_uniform_size() {
        // mat4x4 (64) + 2*vec4 (32) + 2*f32 (8) + 2*u32 (8) = 112 bytes.
        assert_eq!(std::mem::size_of::<ModelUniformData>(), 112);
    }

    #[test]
    fn light_data_size() {
        // Needs to be 64 bytes for GPU alignment.
        assert_eq!(std::mem::size_of::<GpuLightData>(), 64);
    }

    #[test]
    fn light_array_size() {
        // 16 * 64 = 1024 bytes.
        assert_eq!(std::mem::size_of::<LightArrayData>(), 1024);
    }

    #[test]
    fn mesh_to_gpu_vertices_basic() {
        let mesh = Mesh3D {
            vertices: vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            primitive_type: gdserver3d::mesh::PrimitiveType::Triangles,
            surfaces: vec![],
        };
        let verts = mesh_to_gpu_vertices(&mesh);
        assert_eq!(verts.len(), 3);
        assert_eq!(verts[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(verts[1].position, [1.0, 0.0, 0.0]);
        assert_eq!(verts[2].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn transform_to_model_identity() {
        let m = transform_to_model_matrix(&Transform3D::IDENTITY);
        // Column-major identity.
        assert_eq!(m[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(m[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(m[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(m[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn light_to_gpu_directional() {
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, -1.0, 0.0);
        let gpu = light_to_gpu(&light);
        assert_eq!(gpu.kind, 0);
        assert_eq!(gpu.direction, [0.0, -1.0, 0.0]);
        assert_eq!(gpu.color, [1.0, 1.0, 1.0]);
    }
}
