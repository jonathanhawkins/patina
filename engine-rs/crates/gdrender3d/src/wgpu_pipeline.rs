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

use gdcore::math::{srgb_to_linear_channel, Color, Vector3};
use gdcore::math3d::Transform3D;

use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::reflection_probe::{sample_probes_at, ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::ShaderMaterial3D;
use gdserver3d::sky::SkyMaterial;
use gdserver3d::viewport::Viewport3D;

use crate::renderer::invert_projection;

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
    ambient: vec4<f32>,
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
    var color = vec3<f32>(
        camera.ambient.r * albedo.r,
        camera.ambient.g * albedo.g,
        camera.ambient.b * albedo.b,
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

/// Fullscreen procedural-sky background shader in WGSL.
///
/// Draws one fullscreen triangle and evaluates the exact math of the software
/// rasterizer's background fill per pixel: `pixel_view_direction` (unproject
/// the pixel's NDC through the inverse projection, rotate by the camera
/// basis) followed by `sample_procedural_sky` — the Godot 4.6
/// `ProceduralSkyMaterial` shader port: `mix(top, horizon, pow(1 ∓ y,
/// inv_curve))` gradients per hemisphere plus up to 4 directional-light sun
/// discs/halos on the sky half. Keeping the math identical is what makes the
/// GPU editor viewport background match the software reference 1:1.
pub const SKY_WGSL: &str = r#"
struct SkyUniforms {
    inv_projection: mat4x4<f32>,
    // Camera world basis (rotation only) in the upper-left 3x3.
    camera_basis: mat4x4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground_bottom: vec4<f32>,
    ground_horizon: vec4<f32>,
    // x = inv_sky_curve (0.6 / sky_curve), y = inv_ground_curve,
    // z = sky material energy, w = ground material energy
    params: vec4<f32>,
    // x = viewport width, y = viewport height
    size: vec4<f32>,
    // Per directional light: xyz = world direction toward the sun, w = energy.
    sun_dir_energy: array<vec4<f32>, 4>,
    // Per directional light: rgb = color, w = angular size in radians.
    sun_color_size: array<vec4<f32>, 4>,
    // x = cos(sun_angle_max), y = inv_sun_curve (1.6 / pow(sun_curve, 1.4)),
    // z = enabled sun count, w = environment background energy.
    sun_params: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> sky: SkyUniforms;

struct SkyVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyVertexOutput {
    // Fullscreen triangle, no vertex buffer.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: SkyVertexOutput;
    out.clip_position = vec4<f32>(pos[vi], 1.0, 1.0);
    return out;
}

@fragment
fn fs_sky(in: SkyVertexOutput) -> @location(0) vec4<f32> {
    // Framebuffer coords are pixel centers (x+0.5, y+0.5) — the same sample
    // points the software fill uses when it computes NDC per pixel.
    let ndc_x = 2.0 * in.clip_position.x / sky.size.x - 1.0;
    let ndc_y = 1.0 - 2.0 * in.clip_position.y / sky.size.y;

    // Unproject NDC to a view-space direction (software pixel_view_direction).
    let v = sky.inv_projection * vec4<f32>(ndc_x, ndc_y, -1.0, 1.0);
    var view_dir = v.xyz;
    if (abs(v.w) > 1e-6) {
        view_dir = v.xyz / v.w;
    }
    let world = normalize((sky.camera_basis * vec4<f32>(view_dir, 0.0)).xyz);

    // Godot ProceduralSkyMaterial (software sample_procedural_sky).
    let y = clamp(world.y, -1.0, 1.0);
    var rgb: vec3<f32>;
    if (y >= 0.0) {
        // Sky hemisphere: horizon→top gradient, then sun discs/halos.
        let t = clamp(pow(1.0 - y, sky.params.x), 0.0, 1.0);
        rgb = mix(sky.sky_top.rgb, sky.sky_horizon.rgb, t) * sky.params.z;
        let count = u32(sky.sun_params.z);
        for (var i = 0u; i < count; i = i + 1u) {
            let de = sky.sun_dir_energy[i];
            let cs = sky.sun_color_size[i];
            let sun_angle = dot(de.xyz, world);
            let sun_size = cos(cs.w);
            let sun_rgb = cs.rgb * de.w;
            if (sun_angle > sun_size) {
                rgb = sun_rgb;
            } else if (sun_angle > sky.sun_params.x && sun_size > sky.sun_params.x) {
                let c2 = (sun_size - sun_angle) / (sun_size - sky.sun_params.x);
                let f = clamp(pow(1.0 - c2, sky.sun_params.y), 0.0, 1.0);
                rgb = mix(rgb, sun_rgb, f);
            }
        }
    } else {
        // Ground hemisphere: bottom→horizon gradient (no sun below horizon).
        let t = clamp(pow(1.0 + y, sky.params.y), 0.0, 1.0);
        rgb = mix(sky.ground_bottom.rgb, sky.ground_horizon.rgb, t) * sky.params.w;
    }
    // Output linear scene-referred HDR (background energy applied, no clamp):
    // the sun can exceed 1.0 so the camera post-process can bloom/tonemap it.
    return vec4<f32>(rgb * sky.sun_params.w, 1.0);
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
    /// Environment ambient light color (linear); `rgb` used, `a` unused. Applied
    /// as `ambient * albedo` in the fragment shader, matching the software path.
    pub ambient: [f32; 4],
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

/// Sky-pass uniform (group 0 of the sky pipeline). Mirrors `SkyUniforms` in
/// [`SKY_WGSL`]: the pixel-ray reconstruction matrices plus the procedural
/// gradient parameters, all taken verbatim from the environment so the GPU
/// background matches the software fill exactly.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyUniformData {
    /// Inverse of the viewport's perspective projection (column-major).
    pub inv_projection: [[f32; 4]; 4],
    /// Camera world basis in the upper-left 3x3 (column-major).
    pub camera_basis: [[f32; 4]; 4],
    /// `ProceduralSkyMaterial::sky_top_color`.
    pub sky_top: [f32; 4],
    /// `ProceduralSkyMaterial::sky_horizon_color`.
    pub sky_horizon: [f32; 4],
    /// `ProceduralSkyMaterial::ground_bottom_color`.
    pub ground_bottom: [f32; 4],
    /// `ProceduralSkyMaterial::ground_horizon_color`.
    pub ground_horizon: [f32; 4],
    /// x = `0.6 / sky_curve`, y = `0.6 / ground_curve` (Godot's
    /// `inv_sky_curve`/`inv_ground_curve` uniform mapping), z/w = sky/ground
    /// material energy multipliers.
    pub params: [f32; 4],
    /// x = viewport width, y = viewport height (z, w unused).
    pub size: [f32; 4],
    /// Up to 4 sky suns: xyz = world direction toward the light, w = energy.
    pub sun_dir_energy: [[f32; 4]; 4],
    /// Up to 4 sky suns: rgb = color, w = angular size in radians.
    pub sun_color_size: [[f32; 4]; 4],
    /// x = `cos(sun_angle_max)`, y = `1.6 / pow(sun_curve, 1.4)` (Godot's
    /// `inv_sun_curve`), z = enabled sun count, w = environment background
    /// energy multiplier.
    pub sun_params: [f32; 4],
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

/// Fullscreen procedural-sky background pipeline (see [`SKY_WGSL`]).
///
/// Drawn as the first 3 vertices of a render pass, before the meshes: the
/// depth test is disabled and the depth write off, so scene geometry always
/// rasterizes over the sky exactly like meshes drawn over the software
/// background fill.
pub struct SkyPipeline3D {
    /// The wgpu render pipeline.
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group layout for [`SkyUniformData`] (group 0).
    pub layout: wgpu::BindGroupLayout,
}

impl SkyPipeline3D {
    /// Creates the sky background pipeline targeting `target_format`.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-sky-shader"),
            source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky-bind-group-layout"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-sky-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-sky-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self { pipeline, layout }
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

    // A directional light's stored direction is the direction it *travels*,
    // but the shader's `dot(N, L)` wants the direction *toward* the source —
    // negate it, exactly like the software rasterizer's light uniforms. Spot
    // lights keep the forward direction for cone math; point lights derive
    // their direction per-fragment from the position.
    let direction = match light.light_type {
        LightType::Directional => {
            Vector3::new(-light.direction.x, -light.direction.y, -light.direction.z).normalized()
        }
        LightType::Spot => light.direction.normalized(),
        LightType::Point => Vector3::ZERO,
    };

    GpuLightData {
        kind,
        shadow_enabled: if light.shadow_enabled { 1 } else { 0 },
        range: light.range,
        attenuation: light.attenuation,
        direction: [direction.x, direction.y, direction.z],
        spot_angle: light.spot_angle,
        // Light color is sRGB authoring → linear, times energy — the scene is
        // shaded in linear space (matches the software `LightUniform`).
        color: [
            srgb_to_linear_channel(light.color.r) * light.energy,
            srgb_to_linear_channel(light.color.g) * light.energy,
            srgb_to_linear_channel(light.color.b) * light.energy,
        ],
        spot_angle_attenuation: light.spot_angle_attenuation,
        position: [light.position.x, light.position.y, light.position.z],
        _pad: 0.0,
    }
}

/// Bytes per pixel for the editor color/readback formats used here.
fn bytes_per_pixel(format: wgpu::TextureFormat) -> u32 {
    match format {
        wgpu::TextureFormat::Rgba16Float => 8,
        wgpu::TextureFormat::Rgba32Float => 16,
        // Bgra8Unorm / Rgba8Unorm and friends.
        _ => 4,
    }
}

/// Decodes an IEEE-754 binary16 (half) bit pattern to `f32`. Used to unpack the
/// `Rgba16Float` HDR readback buffer.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 0x1;
    let exp = (bits >> 10) & 0x1f;
    let frac = bits & 0x3ff;
    let sign_f = if sign == 1 { -1.0f32 } else { 1.0f32 };
    match exp {
        0 => {
            // Subnormal or zero: value = frac / 2^10 * 2^(-14).
            sign_f * (frac as f32) * 2.0f32.powi(-24)
        }
        0x1f => {
            // Inf/NaN — clamp to a finite large value so downstream math is safe.
            if frac == 0 {
                sign_f * f32::MAX
            } else {
                0.0
            }
        }
        _ => {
            // Normalized: value = (1 + frac/2^10) * 2^(exp-15).
            sign_f * (1.0 + (frac as f32) / 1024.0) * 2.0f32.powi(exp as i32 - 15)
        }
    }
}

/// Builds the sky-pass uniform for `env` if (and only if) its background is a
/// procedural sky: `BackgroundMode::Sky` with a `SkyMaterial::Procedural`
/// material. Everything the WGSL sky needs is taken from the same inputs
/// the software background fill reads — the viewport's projection (inverted),
/// the camera basis, the material's colors/curves/energies with Godot's
/// uniform mappings applied, and the sky-visible directional lights — so the
/// two paths compute identical pixels. Returns `None` for any other
/// background (the caller clears to a flat color instead, like the software
/// fill's non-sky arms).
fn sky_uniform_for_env(
    env: &Environment3D,
    viewport: &Viewport3D,
    w: u32,
    h: u32,
    suns: &[crate::renderer::SkySun],
) -> Option<SkyUniformData> {
    if env.background_mode != BackgroundMode::Sky {
        return None;
    }
    let sky = env.sky.as_ref()?;
    let SkyMaterial::Procedural(mat) = &sky.material else {
        return None;
    };

    let proj = perspective_projection_matrix(
        viewport.fov,
        viewport.aspect(),
        viewport.near,
        viewport.far,
    );
    let inv_projection = invert_projection(&proj);
    let b = &viewport.camera_transform.basis;
    let energy = env.background_energy_multiplier;

    let mut sun_dir_energy = [[0.0f32; 4]; 4];
    let mut sun_color_size = [[0.0f32; 4]; 4];
    let sun_count = suns.len().min(4);
    for (i, sun) in suns.iter().take(4).enumerate() {
        sun_dir_energy[i] = [sun.direction.x, sun.direction.y, sun.direction.z, sun.energy];
        // LIGHTn_COLOR is the light color in linear space (sky.cpp), * energy
        // applied in the shader via `de.w`.
        sun_color_size[i] = [
            srgb_to_linear_channel(sun.color.r),
            srgb_to_linear_channel(sun.color.g),
            srgb_to_linear_channel(sun.color.b),
            sun.size,
        ];
    }

    // The gradient endpoints are `source_color` uniforms: Godot premultiplies
    // each by its hemisphere energy in sRGB space, then binds it as linear —
    // `srgb_to_linear(channel * energy)`. Folding the energy into the color
    // here (and leaving the shader's per-hemisphere energy at 1.0) makes the
    // GPU sky mix identical to the software `sample_procedural_sky`.
    let lin4 = |c: &Color, e: f32| {
        [
            srgb_to_linear_channel(c.r * e),
            srgb_to_linear_channel(c.g * e),
            srgb_to_linear_channel(c.b * e),
            1.0,
        ]
    };
    Some(SkyUniformData {
        inv_projection,
        camera_basis: [
            [b.x.x, b.x.y, b.x.z, 0.0],
            [b.y.x, b.y.y, b.y.z, 0.0],
            [b.z.x, b.z.y, b.z.z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        sky_top: lin4(&mat.sky_top_color, mat.sky_energy_multiplier),
        sky_horizon: lin4(&mat.sky_horizon_color, mat.sky_energy_multiplier),
        ground_bottom: lin4(&mat.ground_bottom_color, mat.ground_energy_multiplier),
        ground_horizon: lin4(&mat.ground_horizon_color, mat.ground_energy_multiplier),
        params: [
            0.6 / mat.sky_curve.max(1e-4),
            0.6 / mat.ground_curve.max(1e-4),
            // Hemisphere energy is already folded into the colors above.
            1.0,
            1.0,
        ],
        size: [w as f32, h as f32, 0.0, 0.0],
        sun_dir_energy,
        sun_color_size,
        sun_params: [
            mat.sun_angle_max.to_radians().cos(),
            1.6 / mat.sun_curve.max(1e-4).powf(1.4),
            sun_count as f32,
            energy,
        ],
    })
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
    reflection_probes: Vec<ReflectionProbe>,
    next_id: u64,
    /// The texture format used for color attachments.
    pub target_format: wgpu::TextureFormat,
    /// Lazily-built pipeline for the editor color-only path. Uses a **non-sRGB**
    /// (`Bgra8Unorm`) target so shader output is written to bytes linearly,
    /// matching the software rasterizer's `color * 255` — which keeps GPU meshes
    /// visually consistent with the (linear) software overlays and background
    /// drawn around them. Separate from `pipeline` so the sRGB `render_frame`
    /// trait path and its tests are unaffected.
    editor_pipeline: Option<RenderPipeline3D>,
    /// Lazily-built fullscreen sky pipeline for the editor path (same non-sRGB
    /// target as `editor_pipeline`). Draws the environment's procedural sky
    /// gradient behind the meshes, matching the software background fill.
    editor_sky_pipeline: Option<SkyPipeline3D>,
    /// Cached editor render targets, reused across frames and recreated only on
    /// resize — the per-frame texture/readback-buffer churn is a real cost at
    /// 60fps.
    editor_targets: Option<EditorTargets>,
}

/// Cached GPU render targets for the editor color path. Held across frames and
/// rebuilt only when the viewport size changes.
struct EditorTargets {
    /// Cached width the targets were built for.
    width: u32,
    /// Cached height the targets were built for.
    height: u32,
    /// Color render target (also a copy source for readback).
    color_tex: wgpu::Texture,
    /// View of `color_tex` used as the render-pass color attachment.
    color_view: wgpu::TextureView,
    /// Depth attachment view (used for z-testing; never read back).
    depth_view: wgpu::TextureView,
    /// Persistent mappable buffer the color texture is copied into each frame.
    readback_buf: wgpu::Buffer,
    /// 256-aligned bytes-per-row of `readback_buf`.
    padded_row: u32,
}

impl EditorTargets {
    /// Builds fresh color + depth targets and a readback buffer sized `w`×`h`.
    fn new(device: &wgpu::Device, w: u32, h: u32, color_format: wgpu::TextureFormat) -> Self {
        let color_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("editor-color-target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("editor-depth-target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (w * bytes_per_pixel(color_format) + align - 1) / align * align;
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("editor-color-readback"),
            size: (padded_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self { width: w, height: h, color_view, color_tex, depth_view, readback_buf, padded_row }
    }
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
            reflection_probes: Vec::new(),
            next_id: 1,
            target_format: format,
            editor_pipeline: None,
            editor_sky_pipeline: None,
            editor_targets: None,
        })
    }

    /// Clears all instances, lights, and probes so the persistent renderer can be
    /// repopulated for a fresh frame, while keeping the expensive-to-create GPU
    /// state (device, pipelines, cached targets) alive. The editor rebuilds its
    /// render list every frame, so its per-frame render calls this first.
    pub fn reset_scene(&mut self) {
        self.instances.clear();
        self.lights.clear();
        self.reflection_probes.clear();
        self.next_id = 1;
    }

    /// Editor-optimized render: rasterizes the scene's meshes into a **cached**,
    /// non-sRGB color target and reads back only the color buffer (clearing to
    /// `clear`), returning tight **top-down BGRA bytes** `(width, height, data)`.
    /// Tuned for per-frame use at 60fps, it differs from the trait
    /// [`render_frame`](RenderingServer3D::render_frame) in four ways:
    ///
    /// - **Cached targets**: the color texture, depth texture, and readback buffer
    ///   are reused across frames and rebuilt only on resize, instead of being
    ///   allocated every frame.
    /// - **No depth readback**: depth is still used for z-testing but is never
    ///   copied back, eliminating a second texture copy and a second blocking GPU
    ///   sync per frame (the editor draws its own overlays and never reads GPU
    ///   depth).
    /// - **Linear target** (`Bgra8Unorm`): shader output is written to bytes
    ///   without sRGB encoding, so the bytes match the software rasterizer's
    ///   `color * 255` exactly and GPU meshes stay consistent with the software
    ///   overlays/background composited around them.
    /// - **Byte-native output**: returns raw BGRA (de-padded per row, no per-pixel
    ///   float conversion), so the caller can composite overlays and encode BMP
    ///   without the `bytes → Color → bytes` round-trip that dominates a full-res
    ///   frame.
    pub fn render_editor_color_bgra(
        &mut self,
        viewport: &Viewport3D,
        clear: Color,
    ) -> (u32, u32, Vec<u8>) {
        let w = viewport.width.max(1);
        let h = viewport.height.max(1);
        // The editor scene is rendered to a linear **HDR** target so the sky's
        // bright sun and any emissive materials keep values above 1.0 for the
        // camera post-process (glow + tonemap) to work on. The post-process
        // (shared with the software path) collapses it to display sRGB on
        // readback, so the two backends produce identical images.
        let format = wgpu::TextureFormat::Rgba16Float;

        // Lazily build the editor HDR pipelines and (re)build cached targets on
        // first use or resize. Done before borrowing device/queue immutably.
        if self.editor_pipeline.is_none() {
            self.editor_pipeline = Some(RenderPipeline3D::new(&self.ctx.device, format));
        }
        if self.editor_sky_pipeline.is_none() {
            self.editor_sky_pipeline = Some(SkyPipeline3D::new(&self.ctx.device, format));
        }
        let need_new = match &self.editor_targets {
            Some(t) => t.width != w || t.height != h,
            None => true,
        };
        if need_new {
            self.editor_targets = Some(EditorTargets::new(&self.ctx.device, w, h, format));
        }

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let pipeline = self.editor_pipeline.as_ref().unwrap();
        let sky_pipeline = self.editor_sky_pipeline.as_ref().unwrap();
        let targets = self.editor_targets.as_ref().unwrap();

        // Resolve the background from the viewport environment the way the
        // software rasterizer's `fill_background` does: a procedural sky draws
        // a fullscreen gradient pass behind the meshes, a custom color clears
        // to it, and anything else clears to the caller-provided backdrop.
        let sky_suns = crate::renderer::sky_suns_from_lights(&self.lights);
        let sky_bind = viewport
            .environment
            .as_ref()
            .and_then(|env| sky_uniform_for_env(env, viewport, w, h, &sky_suns))
            .map(|data| {
                let buf = create_buffer_init(
                    device,
                    queue,
                    "editor-sky-uniform",
                    bytemuck::bytes_of(&data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("editor-sky-bind-group"),
                    layout: &sky_pipeline.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    }],
                });
                (buf, bg)
            });
        // `clear` stays the display-sRGB backdrop (used for the device-lost
        // fallback); `clear_linear` is what actually clears the linear HDR
        // target, so after the post-process's linear→sRGB encode the flat
        // backdrop comes back out at its authored sRGB value.
        let clear = match viewport.environment.as_ref() {
            Some(env) if env.background_mode == BackgroundMode::CustomColor => {
                env.background_color
            }
            _ => clear,
        };
        let clear_linear = clear.srgb_to_linear();

        // Camera uniforms.
        let view_matrix = transform_to_view_matrix(&viewport.camera_transform);
        let projection_matrix = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );
        let cam_pos = viewport.camera_transform.origin;
        // Environment ambient (linear), so the GPU shades surfaces with the
        // sky's tint like the software path.
        let ambient = viewport
            .environment
            .as_ref()
            .map(crate::renderer::environment_ambient_color)
            .unwrap_or(Color::new(0.1, 0.1, 0.1, 1.0));
        let camera_data = CameraUniformData {
            view_matrix,
            projection_matrix,
            camera_position: [cam_pos.x, cam_pos.y, cam_pos.z],
            _pad0: 0.0,
            ambient: [ambient.r, ambient.g, ambient.b, 1.0],
        };
        let camera_buf = create_buffer_init(
            device,
            queue,
            "editor-camera-uniform",
            bytemuck::bytes_of(&camera_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("editor-camera-bind-group"),
            layout: &pipeline.camera_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() }],
        });

        // Light uniforms.
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
            "editor-light-uniform",
            bytemuck::bytes_of(&light_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let light_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("editor-light-bind-group"),
            layout: &pipeline.light_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: light_buf.as_entire_binding() }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor-3d-encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor-3d-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_linear.r as f64,
                            g: clear_linear.g as f64,
                            b: clear_linear.b as f64,
                            a: clear_linear.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            // Sky background first (depth-test off, depth-write off), so the
            // meshes drawn next rasterize over it — the GPU equivalent of the
            // software path's background fill before rasterization.
            if let Some((_sky_buf, sky_bg)) = &sky_bind {
                pass.set_pipeline(&sky_pipeline.pipeline);
                pass.set_bind_group(0, sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }

            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &camera_bg, &[]);
            pass.set_bind_group(2, &light_bg, &[]);

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
                    "editor-vertex",
                    bytemuck::cast_slice(&vertices),
                    wgpu::BufferUsages::VERTEX,
                );
                let index_buf = create_buffer_init(
                    device,
                    queue,
                    "editor-index",
                    bytemuck::cast_slice(&mesh.indices),
                    wgpu::BufferUsages::INDEX,
                );
                let material = inst.material.as_ref().cloned().unwrap_or_default();
                let shading_mode = match material.shading_mode {
                    ShadingMode::Unlit => 0u32,
                    ShadingMode::Lambert => 1u32,
                    ShadingMode::Phong => 2u32,
                };
                // Material colors are sRGB authoring values; upload them in
                // linear so the WGSL shades in linear (the HDR target + camera
                // post-process encode back to sRGB), matching the software path.
                let alb = material.albedo.srgb_to_linear();
                let emis = material.emission.srgb_to_linear();
                let model_data = ModelUniformData {
                    model_matrix: transform_to_model_matrix(&inst.transform),
                    albedo: [alb.r, alb.g, alb.b, material.albedo.a],
                    emission: [emis.r, emis.g, emis.b, material.emission.a],
                    roughness: material.roughness,
                    metallic: material.metallic,
                    shading_mode,
                    num_lights: self.lights.len().min(16) as u32,
                };
                let model_buf = create_buffer_init(
                    device,
                    queue,
                    "editor-model-uniform",
                    bytemuck::bytes_of(&model_data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let model_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("editor-model-bind-group"),
                    layout: &pipeline.model_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: model_buf.as_entire_binding() }],
                });
                pass.set_bind_group(1, &model_bg, &[]);
                pass.set_vertex_buffer(0, vertex_buf.slice(..));
                pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
            }
        }

        // Copy color → cached readback buffer (no depth copy).
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.color_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map, sync once, de-pad each row into a tight top-down BGRA buffer, then
        // unmap so the cached buffer can be mapped again next frame. This is a
        // per-row copy (no per-pixel float work): the caller composites overlays
        // and encodes BMP directly on these bytes.
        let slice = targets.readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .ok();
        // Device-lost hardening (pat-ism62 #4): if the map failed (e.g. the GPU
        // device was lost on sleep/wake), degrade gracefully — return a flat
        // cleared frame instead of panicking the (indefinitely-running) editor
        // render loop inside `get_mapped_range`. A failed map leaves the buffer
        // unmapped, so the next frame can retry.
        if !matches!(rx.try_recv(), Ok(true)) {
            let px = [
                (clear.b.clamp(0.0, 1.0) * 255.0) as u8,
                (clear.g.clamp(0.0, 1.0) * 255.0) as u8,
                (clear.r.clamp(0.0, 1.0) * 255.0) as u8,
                255,
            ];
            let data: Vec<u8> = px.iter().copied().cycle().take((w * h * 4) as usize).collect();
            return (w, h, data);
        }
        // Decode the Rgba16Float HDR readback (8 bytes/pixel, top-down) into a
        // linear `Color` buffer, run the shared camera post-process (glow →
        // tonemap → linear→sRGB — identical code to the software path, so the
        // two backends match), then pack the display image to BGRA bytes.
        let mapped = slice.get_mapped_range();
        let mut pixels: Vec<Color> = Vec::with_capacity((w * h) as usize);
        for row in 0..h as usize {
            let start = row * targets.padded_row as usize;
            let row_bytes = &mapped[start..start + (w * 8) as usize];
            for px in row_bytes.chunks_exact(8) {
                let r = f16_to_f32(u16::from_le_bytes([px[0], px[1]]));
                let g = f16_to_f32(u16::from_le_bytes([px[2], px[3]]));
                let b = f16_to_f32(u16::from_le_bytes([px[4], px[5]]));
                let a = f16_to_f32(u16::from_le_bytes([px[6], px[7]]));
                pixels.push(Color::new(r, g, b, a));
            }
        }
        drop(mapped);
        targets.readback_buf.unmap();

        let default_env = Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);
        crate::postprocess::apply_camera_post(&mut pixels, w, h, env);

        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for c in &pixels {
            // Stored order is B, G, R, A (the format the caller's BMP encode
            // expects). Post-process already clamped to [0, 1].
            data.push((c.b * 255.0) as u8);
            data.push((c.g * 255.0) as u8);
            data.push((c.r * 255.0) as u8);
            data.push((c.a * 255.0) as u8);
        }

        (w, h, data)
    }

    /// Color-buffer twin of [`render_editor_color_bgra`], returning a
    /// [`FrameData3D`] (RGBA `Color` pixels, empty depth). Kept for tests and any
    /// consumer that wants `Color`; the editor hot path uses the byte-native
    /// `_bgra` method to avoid this per-pixel unpack.
    pub fn render_editor_color(&mut self, viewport: &Viewport3D, clear: Color) -> FrameData3D {
        let (w, h, data) = self.render_editor_color_bgra(viewport, clear);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for px in data.chunks_exact(4) {
            // Stored order is B, G, R, A.
            pixels.push(Color::new(
                px[2] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[0] as f32 / 255.0,
                px[3] as f32 / 255.0,
            ));
        }
        FrameData3D { width: w, height: h, pixels, depth: Vec::new() }
    }

    /// Returns the reflection probes currently registered with the renderer.
    pub fn reflection_probes(&self) -> &[ReflectionProbe] {
        &self.reflection_probes
    }

    /// Samples the combined ambient contribution of all reflection probes
    /// whose influence box contains `world_pos`. See
    /// [`gdserver3d::reflection_probe::sample_probes_at`] for the parity rule.
    pub fn sample_reflection_probes(&self, world_pos: Vector3) -> Color {
        sample_probes_at(&self.reflection_probes, world_pos)
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
        let ambient = viewport
            .environment
            .as_ref()
            .map(crate::renderer::environment_ambient_color)
            .unwrap_or(Color::new(0.1, 0.1, 0.1, 1.0));
        let camera_data = CameraUniformData {
            view_matrix,
            projection_matrix,
            camera_position: [cam_pos.x, cam_pos.y, cam_pos.z],
            _pad0: 0.0,
            ambient: [ambient.r, ambient.g, ambient.b, 1.0],
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
        self.instances.insert(id_val, Instance3D::new(id));
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

    fn add_reflection_probe(&mut self, id: ReflectionProbeId) {
        if !self.reflection_probes.iter().any(|p| p.id == id) {
            self.reflection_probes.push(ReflectionProbe::new(id));
        }
    }

    fn update_reflection_probe(&mut self, probe: &ReflectionProbe) {
        if let Some(existing) = self
            .reflection_probes
            .iter_mut()
            .find(|p| p.id == probe.id)
        {
            *existing = probe.clone();
        } else {
            self.reflection_probes.push(probe.clone());
        }
    }

    fn remove_reflection_probe(&mut self, id: ReflectionProbeId) {
        self.reflection_probes.retain(|p| p.id != id);
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
    fn sky_shader_source_compiles() {
        let result = wgpu::naga::front::wgsl::parse_str(SKY_WGSL);
        assert!(result.is_ok(), "sky WGSL parse error: {:?}", result.err());
    }

    #[test]
    fn sky_uniform_size_and_alignment() {
        // 2 mat4x4 (128) + 4 vec4 colors (64) + params (16) + size (16)
        // + 2×4 sun vec4s (128) + sun_params (16) = 352.
        assert_eq!(std::mem::size_of::<SkyUniformData>(), 352);
    }

    /// The sky pass runs only for a procedural-sky environment; every other
    /// background resolves to a flat clear, mirroring the software
    /// `fill_background` arms.
    #[test]
    fn sky_uniform_only_for_procedural_sky() {
        use gdserver3d::sky::Sky;

        let vp = Viewport3D::new(64, 64);

        // Default environment: ClearColor → no sky pass.
        let env = Environment3D::default();
        assert!(sky_uniform_for_env(&env, &vp, 64, 64, &[]).is_none());

        // Sky mode without a sky resource → no sky pass.
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        assert!(sky_uniform_for_env(&env, &vp, 64, 64, &[]).is_none());

        // Sky mode with the default (procedural) sky → sky pass with Godot's
        // uniform mappings and the background energy multiplier.
        env.sky = Some(Sky::default());
        env.background_energy_multiplier = 2.0;
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &[]).expect("procedural sky pass");
        assert_eq!(data.size[0], 64.0);
        assert_eq!(data.size[1], 64.0);
        // Defaults: sky_curve 0.15 → inv 4.0, ground_curve 0.02 → inv 30.0
        // (Godot's `0.6 / curve` mapping); material energies stay 1.0 and the
        // environment background multiplier rides in sun_params.w.
        assert!((data.params[0] - 4.0).abs() < 1e-4);
        assert!((data.params[1] - 30.0).abs() < 1e-4);
        // Hemisphere energy is folded into the colors, so params.z/w stay 1.0.
        assert!((data.params[2] - 1.0).abs() < 1e-6);
        assert!((data.params[3] - 1.0).abs() < 1e-6);
        // Default ProceduralSkyMaterial top color, linearized (source_color).
        assert!((data.sky_top[0] - srgb_to_linear_channel(0.385)).abs() < 1e-5);
        assert!((data.sky_top[1] - srgb_to_linear_channel(0.454)).abs() < 1e-5);
        assert!((data.sky_top[2] - srgb_to_linear_channel(0.55)).abs() < 1e-5);
        // Sun uniforms: no suns → count 0; Godot's sun_angle_max default 30°
        // → cos ≈ 0.866, sun_curve 0.15 → inv_sun_curve = 1.6/0.15^1.4 ≈ 22.78.
        assert_eq!(data.sun_params[2], 0.0);
        assert!((data.sun_params[0] - 30f32.to_radians().cos()).abs() < 1e-6);
        assert!((data.sun_params[1] - 22.78).abs() < 0.05);
        assert!((data.sun_params[3] - 2.0).abs() < 1e-6);
    }

    /// Sky suns flow into the uniform the way Godot feeds `LIGHTn_*` to the
    /// sky shader: direction toward the light, energy in w, color + angular
    /// size packed per light, count in sun_params.z.
    #[test]
    fn sky_uniform_packs_directional_suns() {
        use crate::renderer::SkySun;
        use gdcore::math::Vector3;
        use gdserver3d::sky::Sky;

        let vp = Viewport3D::new(64, 64);
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.sky = Some(Sky::default());

        let suns = [SkySun {
            direction: Vector3::new(0.0, 1.0, 0.0),
            color: Color::new(1.0, 0.9, 0.8, 1.0),
            energy: 2.5,
            size: 0.5f32.to_radians(),
        }];
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &suns).expect("sky pass");
        assert_eq!(data.sun_params[2], 1.0);
        assert_eq!(data.sun_dir_energy[0][1], 1.0);
        assert!((data.sun_dir_energy[0][3] - 2.5).abs() < 1e-6);
        // Sun color is linearized (LIGHTn_COLOR); energy rides in dir_energy.w.
        assert!((data.sun_color_size[0][1] - srgb_to_linear_channel(0.9)).abs() < 1e-5);
        assert!((data.sun_color_size[0][3] - 0.5f32.to_radians()).abs() < 1e-6);
        // Unused slots stay zeroed.
        assert_eq!(data.sun_dir_energy[1], [0.0; 4]);
    }

    #[test]
    fn gpu_vertex_layout_size() {
        // position (3*4) + normal (3*4) + uv (2*4) = 32 bytes.
        assert_eq!(std::mem::size_of::<GpuVertex>(), 32);
    }

    #[test]
    fn camera_uniform_size() {
        // 2 mat4x4 (2*64) + vec3 (12) + pad (4) + ambient vec4 (16) = 160 bytes.
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 160);
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
        // A sun shining straight down (travel −Y) must reach the shader as the
        // toward-the-source direction (+Y), matching the software rasterizer —
        // the unnegated direction made every sun-facing surface ambient-black.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, -1.0, 0.0);
        let gpu = light_to_gpu(&light);
        assert_eq!(gpu.kind, 0);
        assert_eq!(gpu.direction, [0.0, 1.0, 0.0]);
        assert_eq!(gpu.color, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn light_to_gpu_directional_normalizes() {
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(-3.0, -4.0, 0.0);
        let gpu = light_to_gpu(&light);
        assert!((gpu.direction[0] - 0.6).abs() < 1e-6);
        assert!((gpu.direction[1] - 0.8).abs() < 1e-6);
        assert!((gpu.direction[2] - 0.0).abs() < 1e-6);
    }
}
