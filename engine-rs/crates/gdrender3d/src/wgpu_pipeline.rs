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
    // -1 = no shadow map bound; 0 = sample `shadow_depth` (primary directional).
    shadow_map_index: i32,
    // Light-space clip matrix: proj * view (column-major), matching software
    // `ShadowMap::{view_matrix, proj_matrix}` composition.
    shadow_view_proj: mat4x4<f32>,
};

struct LightArray {
    lights: array<LightData, 16>,
};
@group(2) @binding(0)
var<uniform> light_array: LightArray;

// pat-32edz / pat-mst61: CPU shadow maps as R32Float 2D-array layers
// (layer index = LightData.shadow_map_index).
@group(2) @binding(1)
var shadow_depth: texture_2d_array<f32>;

// pat-g4i60: omni (point-light) shadow cubemap faces as R32Float layers —
// 6 consecutive layers per shadow-casting point light, +X,−X,+Y,−Y,+Z,−Z
// (LightData.shadow_map_index = the light's base layer, i.e. its +X face).
@group(2) @binding(2)
var omni_shadow_depth: texture_2d_array<f32>;

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

// pat-32edz: directional shadow factor. Mirrors software `ShadowMap::sample` +
// `compute_shadow_factor` (returns 1.0 fully lit, 0.0 fully shadowed).
// Bias matches `shadow_map::SHADOW_BIAS`.
const SHADOW_BIAS: f32 = 0.005;
// Sentinel for unwritten shadow texels (CPU uses f32::MAX).
const SHADOW_EMPTY: f32 = 1.0e30;

// pat-g4i60: omni shadow bias — matches the software `OMNI_SHADOW_BIAS`
// (shader.rs); cubemap depths are linear distance from the light.
const OMNI_SHADOW_BIAS: f32 = 0.05;

// Mirrors `ShadowCubemap::sample` + `direction_to_face_uv` (gdserver3d
// light.rs): dominant-axis face select, (sc/ma, tc/ma) → [0,1] UV, truncating
// texel conversion, then the linear-distance compare with OMNI_SHADOW_BIAS.
// Returns 1.0 fully lit, 0.0 fully shadowed.
fn omni_shadow_factor(light: LightData, world_pos: vec3<f32>) -> f32 {
    if (light.shadow_enabled == 0u || light.kind != 1u || light.shadow_map_index < 0) {
        return 1.0;
    }
    let to_frag = world_pos - light.position;
    let dist = length(to_frag);

    let ax = abs(to_frag.x);
    let ay = abs(to_frag.y);
    let az = abs(to_frag.z);
    var face: i32;
    var sc: f32;
    var tc: f32;
    var ma: f32;
    if (ax >= ay && ax >= az) {
        if (to_frag.x > 0.0) {
            face = 0; sc = -to_frag.z; tc = -to_frag.y; ma = ax;
        } else {
            face = 1; sc = to_frag.z; tc = -to_frag.y; ma = ax;
        }
    } else if (ay >= ax && ay >= az) {
        if (to_frag.y > 0.0) {
            face = 2; sc = to_frag.x; tc = to_frag.z; ma = ay;
        } else {
            face = 3; sc = to_frag.x; tc = -to_frag.z; ma = ay;
        }
    } else {
        if (to_frag.z > 0.0) {
            face = 4; sc = to_frag.x; tc = -to_frag.y; ma = az;
        } else {
            face = 5; sc = -to_frag.x; tc = -to_frag.y; ma = az;
        }
    }
    var u = 0.5;
    var v = 0.5;
    if (ma >= 1.1920929e-7) { // f32::EPSILON, like the software guard
        u = clamp(sc / ma * 0.5 + 0.5, 0.0, 1.0);
        v = clamp(tc / ma * 0.5 + 0.5, 0.0, 1.0);
    }

    let dims = textureDimensions(omni_shadow_depth);
    let tx = min(u32(u * f32(dims.x)), dims.x - 1u);
    let ty = min(u32(v * f32(dims.y)), dims.y - 1u);
    let layer = light.shadow_map_index + face;
    let stored = textureLoad(omni_shadow_depth, vec2<i32>(i32(tx), i32(ty)), layer, 0).r;
    if (stored >= SHADOW_EMPTY) {
        return 1.0; // No occluder recorded in this texel.
    }
    if (dist > stored + OMNI_SHADOW_BIAS) {
        return 0.0;
    }
    return 1.0;
}

fn directional_shadow_factor(light: LightData, world_pos: vec3<f32>) -> f32 {
    // kind 0 = directional, 2 = spot (both use the 2D depth map).
    if (light.shadow_enabled == 0u || (light.kind != 0u && light.kind != 2u) || light.shadow_map_index < 0) {
        return 1.0;
    }
    let clip = light.shadow_view_proj * vec4<f32>(world_pos, 1.0);
    let w = clip.w;
    if (w <= 0.0) {
        return 1.0;
    }
    let ndc_x = clip.x / w;
    let ndc_y = clip.y / w;
    let depth = clip.z / w;

    // NDC [-1,1] → UV [0,1], flip Y like software ShadowMap::sample.
    let u = (ndc_x + 1.0) * 0.5;
    let v = (1.0 - ndc_y) * 0.5;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return 1.0; // Outside shadow map = lit.
    }

    let dims = textureDimensions(shadow_depth);
    let tx = i32(u * f32(dims.x));
    let ty = i32(v * f32(dims.y));
    if (tx < 0 || ty < 0 || tx >= i32(dims.x) || ty >= i32(dims.y)) {
        return 1.0;
    }
    let layer = light.shadow_map_index;
    let stored_depth = textureLoad(shadow_depth, vec2<i32>(tx, ty), layer, 0).r;
    if (stored_depth >= SHADOW_EMPTY) {
        return 1.0; // No occluder at this texel.
    }
    // Farther than stored depth + bias → in shadow.
    if (depth > stored_depth + SHADOW_BIAS) {
        return 0.0;
    }
    return 1.0;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo = model.albedo;
    let emission = model.emission;

    // Unlit mode. Scene-referred linear HDR: no clamp, so emissive values > 1
    // survive to feed glow (the target is Rgba16Float; the camera post pass
    // tonemaps and clamps at the end) — same as the software shaders.
    if (model.shading_mode == 0u) {
        return vec4<f32>(
            albedo.r + emission.r,
            albedo.g + emission.g,
            albedo.b + emission.b,
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
        // Each factor is 1.0 for light kinds it doesn't handle, so the
        // product applies whichever map (2D or cubemap) this light carries.
        let shadow = directional_shadow_factor(light, in.world_position)
            * omni_shadow_factor(light, in.world_position);
        let contrib = n_dot_l * intensity * shadow;

        // Lambert diffuse (mode == 1).
        if (model.shading_mode == 1u) {
            color += vec3<f32>(
                albedo.r * light.color.r * contrib,
                albedo.g * light.color.g * contrib,
                albedo.b * light.color.b * contrib,
            );
        }

        // Burley + Schlick-GGX (mode == 2, Godot default PBR / pat-sf07y).
        if (model.shading_mode == 2u && n_dot_l > 0.0 && intensity > 0.0) {
            let view_dir = normalize(camera.camera_position - in.world_position);
            let n_dot_v = max(dot(normal, view_dir), 1e-4);
            let h = normalize(light_dir + view_dir);
            let n_dot_h = max(dot(normal, h), 1e-4);
            let l_dot_h = max(dot(light_dir, h), 1e-4);
            let roughness = clamp(model.roughness, 0.0, 1.0);
            let metallic = clamp(model.metallic, 0.0, 1.0);
            let alpha = max(roughness * roughness, 1e-4);
            let f0_d = 0.04;
            let f0 = mix(vec3<f32>(f0_d), albedo.rgb, metallic);
            let albedo_lin = albedo.rgb * (1.0 - metallic);

            // Burley diffuse.
            let fd90 = 0.5 + 2.0 * l_dot_h * l_dot_h * roughness;
            let fl = pow(1.0 - n_dot_l, 5.0);
            let fv = pow(1.0 - n_dot_v, 5.0);
            let burley = (1.0 / 3.14159265) * (1.0 + (fd90 - 1.0) * fl) * (1.0 + (fd90 - 1.0) * fv);
            color += albedo_lin * light.color * (n_dot_l * intensity * shadow * burley);

            // GGX D * V * F.
            let a = n_dot_h * alpha;
            let k = alpha / max(1.0 - n_dot_h * n_dot_h + a * a, 1e-6);
            let d = k * k * (1.0 / 3.14159265);
            let vis_a = 2.0 * n_dot_l * n_dot_v;
            let vis_b = n_dot_l + n_dot_v;
            // Cap like the software v_ggx (min 1e4): uncapped grazing-angle
            // visibility fireflies only on the GPU.
            let vis = min(0.5 / max(mix(vis_a, vis_b, alpha), 1e-6), 1.0e4);
            let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - l_dot_h, 5.0);
            color += light.color * f * (d * vis * n_dot_l * intensity * shadow);
        }
    }

    // Scene-referred linear HDR: no clamp (tonemapping clamps at the end).
    return vec4<f32>(
        color.r + emission.r,
        color.g + emission.g,
        color.b + emission.b,
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

    // pat-fw141: physical (Preetham) sky mode (sun_params.z >= 2000) — a
    // verbatim port of Godot's generated PhysicalSkyMaterial shader and of the
    // software `sample_physical_sky`. LIGHT0 = sun_dir_energy[0] /
    // sun_color_size[0]; with no sun Godot shows only the (default black)
    // night-sky texture.
    if (sky.sun_params.z >= 2000.0) {
        if (sky.sun_params.z < 2000.5) {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0); // no sun → night sky
        }
        let d = normalize(world);
        let sun_dir = normalize(sky.sun_dir_energy[0].xyz);
        let zenith_angle = clamp(sun_dir.y, -1.0, 1.0);
        let sun_energy_p = max(0.0, 0.757 * zenith_angle) * sky.sun_dir_energy[0].w;
        let sun_fade = 1.0 - clamp(1.0 - exp(sun_dir.y), 0.0, 1.0);

        let rayleigh_coeff = sky.sky_top.a - 1.0 * (1.0 - sun_fade);
        let rayleigh_beta = rayleigh_coeff * sky.sky_top.rgb * 0.0001;
        // mie_beta = turbidity * mie * mie_color * 0.000434 (Preetham).
        let mie_beta = sky.ground_bottom.a * sky.sky_horizon.a * sky.sky_horizon.rgb * 0.000434;

        let zenith = max(0.0, d.y);
        let optical_mass = 1.0 / (zenith + 0.15 * pow(3.885 + 54.5 * zenith, -1.253));
        let extinction = exp(-(rayleigh_beta * (8.4e3 * optical_mass) + mie_beta * (1.25e3 * optical_mass)));

        let cos_theta = dot(d, sun_dir);
        let rayleigh_phase = (3.0 / (16.0 * 3.14159265)) * (1.0 + pow(cos_theta * 0.5 + 0.5, 2.0));
        let g = sky.params.x;
        let mie_phase = 0.0795774715459 * (1.0 - g * g)
            / pow(max(1.0 + g * g - 2.0 * g * cos_theta, 1e-9), 1.5);

        let beta_theta = rayleigh_beta * rayleigh_phase + mie_beta * mie_phase;
        let beta_sum = max(rayleigh_beta + mie_beta, vec3<f32>(1e-12));
        let base = sun_energy_p * (beta_theta / beta_sum);
        var lin = pow(max(base * (vec3<f32>(1.0) - extinction), vec3<f32>(0.0)), vec3<f32>(1.5));
        let l_low = pow(max(base * extinction, vec3<f32>(0.0)), vec3<f32>(0.5));
        let low_mix = clamp(pow(1.0 - zenith_angle, 5.0), 0.0, 1.0);
        lin = lin * (vec3<f32>(1.0) + (l_low - vec3<f32>(1.0)) * low_mix);
        let ground_blend = smoothstep(-0.1, 0.1, d.y);
        lin = lin * (sky.ground_bottom.rgb + (vec3<f32>(1.0) - sky.ground_bottom.rgb) * ground_blend);

        // Solar disk (guarded like the software smoothstep for size 0).
        let cos_max = cos(sky.sun_color_size[0].w * sky.params.y);
        let cos_max2 = cos(sky.sun_color_size[0].w * sky.params.y * 0.5);
        var sundisk = 0.0;
        if (cos_max2 - cos_max > 1e-9) {
            sundisk = smoothstep(cos_max, cos_max2, cos_theta);
        } else if (cos_theta >= cos_max) {
            sundisk = 1.0;
        }
        let l0 = (sun_energy_p * sundisk) * extinction * sky.sun_color_size[0].rgb;

        let final_pow = 1.0 / (1.2 + 1.2 * sun_fade);
        let col = pow(max(lin + l0, vec3<f32>(0.0)), vec3<f32>(final_pow)) * sky.sun_params.w;
        return vec4<f32>(col, 1.0);
    }

    // pat-coz7r: panoramic / equirect mode (sun_params.z >= 1000).
    // UV matches software equirect_uv AND Godot sky.glsl panorama_coords:
    // u = atan2(x,-z)/(2π) wrapped into [0,1) with NO half-turn shift
    // (a +0.5 here yaw-rotates every panorama by 180°), v = acos(y)/π.
    // Full HDR texture bind is a follow-up; until then, sample a vertical
    // gradient as a stand-in that uses the same UV so paths stay aligned.
    if (sky.sun_params.z >= 1000.0) {
        let d = normalize(world);
        let u = atan2(d.x, -d.z) / (2.0 * 3.14159265);
        let v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
        let _u = fract(u + 1.0); // wrap like rem_euclid; keep live for the texture sample
        let rgb = mix(sky.sky_top.rgb, sky.sky_horizon.rgb, clamp(v, 0.0, 1.0));
        return vec4<f32>(rgb * sky.sun_params.w, 1.0);
    }

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
    /// Shadow map slot: `-1` = none, `0` = primary directional map texture.
    pub shadow_map_index: i32,
    /// Light-space clip matrix (`proj * view`), column-major.
    pub shadow_view_proj: [[f32; 4]; 4],
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // pat-32edz / pat-mst61: shadow depth array (R32Float layers).
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // pat-g4i60: omni (point-light) shadow cubemap faces — 6
                // layers per shadow-casting point light, in its own array
                // (128² faces vs the 256² directional/spot maps).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
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

/// Identity 4×4 (column-major).
fn identity_mat4() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Column-major `a * b` (apply `b` then `a`).
fn mul_mat4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            r[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    r
}

/// Default empty [`GpuLightData`] slot.
fn empty_gpu_light() -> GpuLightData {
    GpuLightData {
        kind: 0,
        shadow_enabled: 0,
        range: 0.0,
        attenuation: 1.0,
        direction: [0.0, -1.0, 0.0],
        spot_angle: 0.0,
        color: [0.0, 0.0, 0.0],
        spot_angle_attenuation: 1.0,
        position: [0.0, 0.0, 0.0],
        shadow_map_index: -1,
        shadow_view_proj: identity_mat4(),
    }
}

/// Converts a scene [`Light3D`] into a [`GpuLightData`] (no shadow matrix yet).
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
        shadow_map_index: -1,
        shadow_view_proj: identity_mat4(),
    }
}

/// Builds light-array uniforms from lights + already-generated shadow maps:
/// each directional/spot light with a map gets that map's atlas layer as its
/// `shadow_map_index` (layer order = `maps` order, pat-mst61 multi-light atlas).
fn pack_lights_from_maps(
    lights: &[Light3D],
    maps: &[crate::shadow_map::ShadowMap],
    omni: &[(u64, gdserver3d::light::ShadowCubemap)],
) -> LightArrayData {
    let mut light_data = LightArrayData {
        lights: [empty_gpu_light(); 16],
    };
    for (i, light) in lights.iter().enumerate().take(16) {
        let mut gpu = light_to_gpu(light);
        if light.shadow_enabled
            && matches!(
                light.light_type,
                LightType::Directional | LightType::Spot
            )
        {
            if let Some((layer, sm)) =
                maps.iter().enumerate().find(|(_, m)| m.light_id == light.id)
            {
                gpu.shadow_view_proj = mul_mat4(&sm.proj_matrix, &sm.view_matrix);
                gpu.shadow_map_index = layer as i32;
            }
        }
        // Point lights index into the omni face array instead: base layer =
        // 6 × the cubemap's packed position (pat-g4i60).
        if light.shadow_enabled && light.light_type == LightType::Point {
            if let Some(k) = omni.iter().position(|(id, _)| *id == light.id.0) {
                gpu.shadow_map_index = (k * 6) as i32;
            }
        }
        light_data.lights[i] = gpu;
    }
    light_data
}

/// Builds light-array uniforms and ordered shadow maps for GPU upload.
///
/// Uncached variant kept for tests; the render paths go through
/// [`GpuRenderer3D::ensure_shadow_atlas`], which skips generation entirely
/// when the shadow-relevant scene state hasn't changed (pat-2be0q).
fn pack_lights_with_shadows<'a>(
    lights: &[Light3D],
    instances: impl Iterator<Item = &'a Instance3D>,
) -> (LightArrayData, Vec<crate::shadow_map::ShadowMap>) {
    let instance_list: Vec<Instance3D> = instances.cloned().collect();
    let maps = crate::shadow_map::generate_shadow_maps(lights, &instance_list);
    let omni = pack_omni_cubemaps(lights, &instance_list);
    let light_data = pack_lights_from_maps(lights, &maps, &omni);
    (light_data, maps)
}

/// Generates omni cubemaps for the shadow-casting point lights and packs the
/// `Some` results as `(light id, cubemap)` in light order — the layout
/// [`pack_lights_from_maps`] and the omni face array share (pat-g4i60).
fn pack_omni_cubemaps(
    lights: &[Light3D],
    instances: &[Instance3D],
) -> Vec<(u64, gdserver3d::light::ShadowCubemap)> {
    crate::shadow_map::generate_omni_shadow_cubemaps(lights, instances)
        .into_iter()
        .zip(lights)
        .filter_map(|(cube, light)| cube.map(|c| (light.id.0, c)))
        .collect()
}

/// Content hash of everything that can change a shadow map: each light's
/// shadow-relevant parameters, and — only when some light actually casts —
/// every occluder's visibility, transform, and mesh geometry. Multimesh and
/// material state are deliberately excluded: the CPU shadow rasterizer draws
/// `inst.mesh` only (extend this key if that ever changes).
fn shadow_state_key<'a>(
    lights: &[Light3D],
    instances: impl Iterator<Item = &'a Instance3D>,
) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut any_shadow = false;
    for l in lights {
        let casts = l.shadow_enabled
            && (matches!(l.light_type, LightType::Directional | LightType::Spot)
                || (l.light_type == LightType::Point
                    && l.omni_shadow_mode == gdserver3d::light::OmniShadowMode::Cube));
        any_shadow |= casts;
        h.write_u64(l.id.0);
        h.write_u8(casts as u8);
        h.write_u8(match l.light_type {
            LightType::Directional => 0,
            LightType::Point => 1,
            LightType::Spot => 2,
        });
        for f in [
            l.direction.x,
            l.direction.y,
            l.direction.z,
            l.position.x,
            l.position.y,
            l.position.z,
            l.spot_angle,
            l.range,
        ] {
            h.write_u32(f.to_bits());
        }
    }
    if !any_shadow {
        // No caster → occluders can't matter; every no-shadow scene shares
        // one cached (empty) atlas.
        return h.finish();
    }
    for inst in instances {
        h.write_u8(inst.visible as u8);
        let t = &inst.transform;
        for f in [
            t.basis.x.x, t.basis.x.y, t.basis.x.z,
            t.basis.y.x, t.basis.y.y, t.basis.y.z,
            t.basis.z.x, t.basis.z.y, t.basis.z.z,
            t.origin.x, t.origin.y, t.origin.z,
        ] {
            h.write_u32(f.to_bits());
        }
        match &inst.mesh {
            Some(m) => h.write_u64(mesh_content_hash(m)),
            None => h.write_u64(0),
        }
    }
    h.finish()
}

/// Uploads one or more CPU shadow maps as an `R32Float` 2D array texture.
/// Empty atlas is a 1×1×1 sentinel layer of `f32::MAX` (fully lit).
fn upload_shadow_depth_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    maps: &[crate::shadow_map::ShadowMap],
) -> (wgpu::Texture, wgpu::TextureView) {
    let (w, h, layers, bytes): (u32, u32, u32, Vec<u8>) = if maps.is_empty() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&f32::MAX.to_le_bytes());
        (1, 1, 1, bytes)
    } else {
        let (dw, dh) = maps[0].depth.dimensions();
        let mut bytes = Vec::with_capacity(maps.len() * (dw * dh) as usize * 4);
        for sm in maps {
            // Every layer must match the array texture's dimensions — a
            // mismatched map would silently shear all following layers.
            debug_assert_eq!(
                sm.depth.dimensions(),
                (dw, dh),
                "shadow maps in one atlas must share dimensions"
            );
            for &d in sm.depth.as_slice() {
                bytes.extend_from_slice(&d.to_le_bytes());
            }
        }
        (dw, dh, maps.len() as u32, bytes)
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow-depth-array"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w.max(1) * 4),
            rows_per_image: Some(h.max(1)),
        },
        wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: layers.max(1),
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("shadow-depth-array-view"),
        format: Some(wgpu::TextureFormat::R32Float),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(layers.max(1)),
        usage: None,
    });
    (texture, view)
}

/// Uploads omni shadow cubemaps as an `R32Float` 2D array: 6 face layers per
/// cubemap in [`CubeFace`] order (+X, −X, +Y, −Y, +Z, −Z), so cubemap `k`'s
/// faces occupy layers `6k..6k+6` (pat-g4i60). Empty input uploads a 1×1×1
/// `f32::MAX` sentinel layer (fully lit).
fn upload_omni_shadow_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cubes: &[(u64, gdserver3d::light::ShadowCubemap)],
) -> (wgpu::Texture, wgpu::TextureView) {
    let (res, layers, bytes): (u32, u32, Vec<u8>) = if cubes.is_empty() {
        (1, 1, f32::MAX.to_le_bytes().to_vec())
    } else {
        let res = cubes[0].1.resolution.max(1);
        let mut bytes =
            Vec::with_capacity(cubes.len() * 6 * (res * res) as usize * 4);
        for (_, cube) in cubes {
            debug_assert_eq!(
                cube.resolution, res,
                "omni cubemaps in one array must share a resolution"
            );
            for face in &cube.faces {
                for &d in face {
                    bytes.extend_from_slice(&d.to_le_bytes());
                }
            }
        }
        (res, cubes.len() as u32 * 6, bytes)
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("omni-shadow-array"),
        size: wgpu::Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(res * 4),
            rows_per_image: Some(res),
        },
        wgpu::Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: layers,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("omni-shadow-array-view"),
        format: Some(wgpu::TextureFormat::R32Float),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(layers),
        usage: None,
    });
    (texture, view)
}

/// Builds the light bind group (uniform + shadow depth array).
fn create_light_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    light_data: &LightArrayData,
    shadow_view: &wgpu::TextureView,
    omni_view: &wgpu::TextureView,
    label: &str,
) -> (wgpu::BindGroup, wgpu::Buffer) {
    let light_buf = create_buffer_init(
        device,
        queue,
        &format!("{label}-light-uniform"),
        bytemuck::bytes_of(light_data),
        wgpu::BufferUsages::UNIFORM,
    );
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(omni_view),
            },
        ],
    });
    (bg, light_buf)
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

    // pat-coz7r: panoramic skies also run through the sky pass (equirect UV in
    // WGSL). Without a bound HDR texture yet, use a neutral gradient tinted by
    // energy so the path is active; software still owns pixel-accurate sampling.
    let (sky_top, sky_horizon, ground_bottom, ground_horizon, params, sky_mode) =
        match &sky.material {
            SkyMaterial::Procedural(mat) => (
                lin4(&mat.sky_top_color, mat.sky_energy_multiplier),
                lin4(&mat.sky_horizon_color, mat.sky_energy_multiplier),
                lin4(&mat.ground_bottom_color, mat.ground_energy_multiplier),
                lin4(&mat.ground_horizon_color, mat.ground_energy_multiplier),
                [
                    0.6 / mat.sky_curve.max(1e-4),
                    0.6 / mat.ground_curve.max(1e-4),
                    1.0,
                    1.0,
                ],
                0.0_f32,
            ),
            SkyMaterial::Panoramic(mat) => {
                let e = mat.energy_multiplier;
                let top = Color::new(0.35, 0.4, 0.55, 1.0);
                let hor = Color::new(0.45, 0.48, 0.52, 1.0);
                let bot = Color::new(0.2, 0.18, 0.15, 1.0);
                (
                    lin4(&top, e),
                    lin4(&hor, e),
                    lin4(&bot, e),
                    lin4(&hor, e),
                    [4.0, 30.0, 1.0, 1.0],
                    1.0_f32, // equirect mode flag (sun_params.z)
                )
            }
            // pat-fw141: physical (Preetham) sky — pack the material through
            // the existing uniform slots (colors linear via `source_color`,
            // coefficients in the alpha channels; see the WGSL physical
            // branch): sky_top = rayleigh color + coefficient, sky_horizon =
            // mie color + coefficient, ground_bottom = ground color +
            // turbidity, params = (mie eccentricity, sun disk scale, -, -).
            SkyMaterial::Physical(mat) => (
                [
                    srgb_to_linear_channel(mat.rayleigh_color.r),
                    srgb_to_linear_channel(mat.rayleigh_color.g),
                    srgb_to_linear_channel(mat.rayleigh_color.b),
                    mat.rayleigh_coefficient,
                ],
                [
                    srgb_to_linear_channel(mat.mie_color.r),
                    srgb_to_linear_channel(mat.mie_color.g),
                    srgb_to_linear_channel(mat.mie_color.b),
                    mat.mie_coefficient,
                ],
                [
                    srgb_to_linear_channel(mat.ground_color.r),
                    srgb_to_linear_channel(mat.ground_color.g),
                    srgb_to_linear_channel(mat.ground_color.b),
                    mat.turbidity,
                ],
                [0.0, 0.0, 0.0, 0.0],
                [mat.mie_eccentricity, mat.sun_disk_scale, 0.0, 0.0],
                2.0_f32, // physical mode flag (sun_params.z)
            ),
        };

    Some(SkyUniformData {
        inv_projection,
        camera_basis: [
            [b.x.x, b.x.y, b.x.z, 0.0],
            [b.y.x, b.y.y, b.y.z, 0.0],
            [b.z.x, b.z.y, b.z.z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        sky_top,
        sky_horizon,
        ground_bottom,
        ground_horizon,
        params,
        size: [w as f32, h as f32, 0.0, 0.0],
        sun_dir_energy,
        sun_color_size,
        sun_params: {
            let (cos_sun, inv_curve) = match &sky.material {
                SkyMaterial::Procedural(mat) => (
                    mat.sun_angle_max.to_radians().cos(),
                    1.6 / mat.sun_curve.max(1e-4).powf(1.4),
                ),
                _ => (0.0_f32, 1.0_f32),
            };
            // The physical sky's `exposure` uniform is the material energy
            // multiplied with the environment's background energy.
            let w_energy = match &sky.material {
                SkyMaterial::Physical(mat) => mat.energy_multiplier * energy,
                _ => energy,
            };
            [
                cos_sun,
                inv_curve,
                // z encodes mode + sun count: procedural = count (0..4),
                // panoramic = 1000 + count, physical = 2000 + count.
                if sky_mode > 1.5 {
                    2000.0 + sun_count as f32
                } else if sky_mode > 0.5 {
                    1000.0 + sun_count as f32
                } else {
                    sun_count as f32
                },
                w_energy,
            ]
        },
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

// ── Draw-entry expansion (multimesh + shader material) ──────────────

/// A single resolved draw: one mesh + world transform + material parameters,
/// ready to upload to the GPU. Produced by [`instance_draw_entries`].
struct GpuDrawEntry<'a> {
    mesh: &'a Mesh3D,
    transform: Transform3D,
    albedo: Color,
    emission: Color,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,
}

/// Resolves an albedo colour for a shader material, matching the software path's
/// common case: a `source_color` albedo uniform (e.g. `albedo_color`) on an
/// otherwise `unshaded` shader. Full custom-shader execution is out of scope for
/// the GPU path (it would need a shader→WGSL transpiler); this covers the constant
/// albedo override the parity tests + typical editor shader materials use.
fn resolve_shader_albedo(mat: &ShaderMaterial3D) -> Option<Color> {
    for name in ["albedo_color", "albedo", "color", "modulate", "source_color"] {
        if let Some(gdvariant::Variant::Color(c)) = mat.get_shader_parameter(name) {
            return Some(*c);
        }
    }
    // Fall back to any Color-typed shader parameter.
    mat.parameters().find_map(|(_, v)| match v {
        gdvariant::Variant::Color(c) => Some(*c),
        _ => None,
    })
}

/// Expands one instance into the concrete draws the GPU must record, mirroring
/// [`SoftwareRenderer3D`](crate::renderer::SoftwareRenderer3D)'s logic so the two
/// backends agree:
///
/// - **Multimesh**: one entry per multimesh instance, composing the instance's
///   world transform with each per-instance transform and applying per-instance
///   colours (when present).
/// - **Single mesh**: one entry at the instance transform.
/// - **Shader material**: overrides the albedo with the resolved shader colour and
///   shades unlit (matching the common `render_mode unshaded` override).
///
/// Returns no entries for an instance with neither a mesh nor a multimesh-with-
/// mesh — the previous GPU loop silently skipped those; this makes multimesh and
/// shader-material instances render on the GPU like they already do in software.
fn instance_draw_entries(inst: &Instance3D) -> Vec<GpuDrawEntry<'_>> {
    let base = inst.material.clone().unwrap_or_default();
    let base_mode = match base.shading_mode {
        ShadingMode::Unlit => 0u32,
        ShadingMode::Lambert => 1u32,
        ShadingMode::Phong => 2u32,
    };
    // A shader material overrides the albedo and shades unlit (the common
    // `unshaded; uniform vec4 albedo_color : source_color;` case).
    let shader_albedo = inst.shader_material.as_ref().and_then(resolve_shader_albedo);
    let mode = if shader_albedo.is_some() { 0 } else { base_mode };

    let mut entries = Vec::new();
    if let Some(mm) = &inst.multimesh {
        if let Some(shared) = &mm.mesh {
            let has_colors = !mm.instance_colors.is_empty();
            for i in 0..mm.instance_count() {
                let transform = inst.transform * mm.get_instance_transform(i);
                let albedo = shader_albedo
                    .or_else(|| has_colors.then(|| mm.get_instance_color(i)))
                    .unwrap_or(base.albedo);
                entries.push(GpuDrawEntry {
                    mesh: shared,
                    transform,
                    albedo,
                    emission: base.emission,
                    roughness: base.roughness,
                    metallic: base.metallic,
                    shading_mode: mode,
                });
            }
        }
    } else if let Some(mesh) = &inst.mesh {
        entries.push(GpuDrawEntry {
            mesh,
            transform: inst.transform,
            albedo: shader_albedo.unwrap_or(base.albedo),
            emission: base.emission,
            roughness: base.roughness,
            metallic: base.metallic,
            shading_mode: mode,
        });
    }
    entries
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
    /// Persistent cache of uploaded mesh vertex/index buffers, keyed by a content
    /// hash, so static geometry isn't re-tessellated/re-uploaded every frame
    /// (pat-ism62 #2b — matters once a scene holds hundreds+ of meshes). Survives
    /// `reset_scene` (it's GPU resource state, not scene state); bounded by
    /// [`MESH_CACHE_CAP`] so per-frame-changing geometry can't leak unboundedly.
    mesh_gpu_cache: HashMap<u64, CachedMeshBuffers>,
    /// Cached CPU-rasterized shadow maps + their uploaded depth-array texture,
    /// keyed by a content hash of the shadow-relevant scene state (pat-2be0q).
    /// A static scene re-renders without deep-cloning every instance,
    /// re-rasterizing 256² maps, or re-uploading the atlas each frame. Survives
    /// `reset_scene` like the other GPU resource caches — the key is
    /// content-based, so the editor's rebuild-everything-per-frame flow still
    /// hits.
    shadow_atlas_cache: Option<ShadowAtlasCache>,
    /// How many times the shadow atlas was (re)built. Instrumentation for the
    /// cache-reuse tests; also a useful perf probe.
    shadow_atlas_rebuilds: u64,
}

/// Cached shadow state for [`GpuRenderer3D::shadow_atlas_cache`].
struct ShadowAtlasCache {
    /// Hash of the lights' shadow parameters + occluder geometry/transforms.
    key: u64,
    /// CPU shadow maps in atlas-layer order (drives `shadow_map_index`).
    maps: Vec<crate::shadow_map::ShadowMap>,
    /// The uploaded `R32Float` depth array (kept alive for `view`).
    _texture: wgpu::Texture,
    /// View bound as the shadow depth array in the light bind group.
    view: wgpu::TextureView,
    /// Omni cubemaps in packed order `(light id, cubemap)` — the k-th entry's
    /// faces occupy layers `6k..6k+6` of the omni array (pat-g4i60).
    omni: Vec<(u64, gdserver3d::light::ShadowCubemap)>,
    /// The uploaded omni face array (kept alive for `omni_view`).
    _omni_texture: wgpu::Texture,
    /// View bound as the omni shadow array (binding 2).
    omni_view: wgpu::TextureView,
}

/// Cached GPU buffers for one mesh's geometry (see `GpuRenderer3D::mesh_gpu_cache`).
struct CachedMeshBuffers {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
}

/// A resolved draw for the buffer-cached render path: references cached geometry
/// by `mesh_key` (into [`GpuRenderer3D::mesh_gpu_cache`]) and carries the per-draw
/// transform + material. Built by `build_draw_calls` (which uploads any uncached
/// geometry first), then consumed inside the render pass.
struct GpuDrawCall {
    mesh_key: u64,
    transform: Transform3D,
    albedo: Color,
    emission: Color,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,
}

/// Max distinct meshes cached before the whole cache is dropped, bounding memory
/// for scenes whose geometry changes every frame.
const MESH_CACHE_CAP: usize = 2048;

/// A cheap content fingerprint of a mesh's geometry, keying
/// [`GpuRenderer3D::mesh_gpu_cache`]. Hashes vertex positions/normals/uvs + index
/// bytes, so meshes that pack to identical GPU vertex buffers share a cache entry.
/// A 64-bit collision between *distinct* geometry would draw the wrong mesh —
/// astronomically unlikely with lengths + full attribute bytes hashed, but not
/// harmless, so keep the full-content hash if this is ever "optimized".
fn mesh_content_hash(mesh: &Mesh3D) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(mesh.vertices.len());
    h.write_usize(mesh.normals.len());
    h.write_usize(mesh.uvs.len());
    h.write_usize(mesh.indices.len());
    for v in &mesh.vertices {
        h.write_u32(v.x.to_bits());
        h.write_u32(v.y.to_bits());
        h.write_u32(v.z.to_bits());
    }
    for n in &mesh.normals {
        h.write_u32(n.x.to_bits());
        h.write_u32(n.y.to_bits());
        h.write_u32(n.z.to_bits());
    }
    for uv in &mesh.uvs {
        h.write_u32(uv[0].to_bits());
        h.write_u32(uv[1].to_bits());
    }
    h.write(bytemuck::cast_slice::<u32, u8>(&mesh.indices));
    h.finish()
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
            size: padded_row as u64 * h as u64,
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
            mesh_gpu_cache: HashMap::new(),
            shadow_atlas_cache: None,
            shadow_atlas_rebuilds: 0,
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

    /// How many times the shadow atlas has been (re)rasterized + uploaded.
    /// A static scene should hold this constant across frames; it increments
    /// only when a light or occluder actually changes (pat-2be0q).
    pub fn shadow_atlas_rebuilds(&self) -> u64 {
        self.shadow_atlas_rebuilds
    }

    /// Ensures `shadow_atlas_cache` matches the current shadow-relevant scene
    /// state and returns this frame's light uniforms (pat-2be0q). On a cache
    /// hit — the common case for a static scene — no instance is cloned, no
    /// shadow map is rasterized, and no texture is uploaded; only the cheap
    /// uniform packing runs. Call with `&mut self` before the render pass
    /// takes its immutable device/target borrows; read the atlas view from
    /// `shadow_atlas_cache` afterwards.
    fn ensure_shadow_atlas(&mut self) -> LightArrayData {
        let key = shadow_state_key(&self.lights, self.instances.values());
        if self.shadow_atlas_cache.as_ref().map(|c| c.key) != Some(key) {
            // Miss: the deep clone + rasterization only happen when the scene
            // actually changed (they're dwarfed by the re-raster itself).
            let instance_list: Vec<Instance3D> = self.instances.values().cloned().collect();
            let maps = crate::shadow_map::generate_shadow_maps(&self.lights, &instance_list);
            let omni = pack_omni_cubemaps(&self.lights, &instance_list);
            let (texture, view) =
                upload_shadow_depth_array(&self.ctx.device, &self.ctx.queue, &maps);
            let (omni_texture, omni_view) =
                upload_omni_shadow_array(&self.ctx.device, &self.ctx.queue, &omni);
            self.shadow_atlas_cache = Some(ShadowAtlasCache {
                key,
                maps,
                _texture: texture,
                view,
                omni,
                _omni_texture: omni_texture,
                omni_view,
            });
            self.shadow_atlas_rebuilds += 1;
        }
        let cache = self.shadow_atlas_cache.as_ref().expect("just ensured");
        pack_lights_from_maps(&self.lights, &cache.maps, &cache.omni)
    }

    /// Phase 1 of a cached render (pat-ism62 #2b): expands every visible instance
    /// into concrete [`GpuDrawCall`]s (multimesh + shader material via
    /// [`instance_draw_entries`]) and uploads any geometry not already in
    /// `mesh_gpu_cache`, so the render pass reuses cached vertex/index buffers
    /// instead of re-uploading static geometry every frame. Runs with `&mut self`
    /// before the immutable device/target borrows the render pass needs.
    fn build_draw_calls(&mut self) -> Vec<GpuDrawCall> {
        // Bound memory for scenes whose geometry changes every frame.
        if self.mesh_gpu_cache.len() > MESH_CACHE_CAP {
            self.mesh_gpu_cache.clear();
        }
        let mut calls: Vec<GpuDrawCall> = Vec::new();
        // Collect misses (key → mesh clone) without holding the `instances` borrow
        // across the `&mut mesh_gpu_cache` insert below.
        let mut to_upload: Vec<(u64, Mesh3D)> = Vec::new();
        for inst in self.instances.values() {
            if !inst.visible {
                continue;
            }
            for entry in instance_draw_entries(inst) {
                if entry.mesh.indices.is_empty() {
                    continue;
                }
                let key = mesh_content_hash(entry.mesh);
                if !self.mesh_gpu_cache.contains_key(&key)
                    && !to_upload.iter().any(|(k, _)| *k == key)
                {
                    to_upload.push((key, entry.mesh.clone()));
                }
                calls.push(GpuDrawCall {
                    mesh_key: key,
                    transform: entry.transform,
                    albedo: entry.albedo,
                    emission: entry.emission,
                    roughness: entry.roughness,
                    metallic: entry.metallic,
                    shading_mode: entry.shading_mode,
                });
            }
        }
        for (key, mesh) in to_upload {
            let vertices = mesh_to_gpu_vertices(&mesh);
            if vertices.is_empty() {
                continue;
            }
            let vertex_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "cached-vertex",
                bytemuck::cast_slice(&vertices),
                wgpu::BufferUsages::VERTEX,
            );
            let index_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "cached-index",
                bytemuck::cast_slice(&mesh.indices),
                wgpu::BufferUsages::INDEX,
            );
            self.mesh_gpu_cache.insert(
                key,
                CachedMeshBuffers { vertex_buf, index_buf, index_count: mesh.indices.len() as u32 },
            );
        }
        calls
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

        // Shadow atlas + light uniforms (pat-32edz / pat-2be0q) — needs
        // `&mut self`, so it runs before the immutable borrows below; a
        // static scene reuses the cached maps + texture untouched.
        let light_data = self.ensure_shadow_atlas();

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

        // Light bind group over the cached shadow atlases (pat-2be0q).
        let atlas = self
            .shadow_atlas_cache
            .as_ref()
            .expect("ensure_shadow_atlas ran above");
        let (light_bg, _light_buf) = create_light_bind_group(
            device,
            queue,
            &pipeline.light_layout,
            &light_data,
            &atlas.view,
            &atlas.omni_view,
            "editor-light-bind-group",
        );

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
                // Expand multimesh + shader material into concrete draws (matching
                // the software renderer), instead of drawing only a single mesh.
                for entry in instance_draw_entries(inst) {
                    let vertices = mesh_to_gpu_vertices(entry.mesh);
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
                        bytemuck::cast_slice(&entry.mesh.indices),
                        wgpu::BufferUsages::INDEX,
                    );
                    // Material colors are sRGB authoring values; upload them in
                    // linear so the WGSL shades in linear (the HDR target + camera
                    // post-process encode back to sRGB), matching the software path.
                    let alb = entry.albedo.srgb_to_linear();
                    let emis = entry.emission.srgb_to_linear();
                    let model_data = ModelUniformData {
                        model_matrix: transform_to_model_matrix(&entry.transform),
                        albedo: [alb.r, alb.g, alb.b, entry.albedo.a],
                        emission: [emis.r, emis.g, emis.b, entry.emission.a],
                        roughness: entry.roughness,
                        metallic: entry.metallic,
                        shading_mode: entry.shading_mode,
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
                    pass.draw_indexed(0..entry.mesh.indices.len() as u32, 0, 0..1);
                }
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
        let mut pixels: Vec<Color> = Vec::with_capacity(w as usize * h as usize);
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

        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
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
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
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
        // Render to a linear HDR target + procedural sky + linear-shaded meshes,
        // then apply the shared camera post-process (glow → tonemap → sRGB) on
        // readback, so `render_frame` produces the SAME display-sRGB image as
        // `SoftwareRenderer3D::render_frame` and the editor GPU path (not the old
        // sRGB-target / no-sky / no-tonemap output). Depth is still read back.
        let color_format = wgpu::TextureFormat::Rgba16Float;

        // Lazily build the HDR mesh + sky pipelines (shared with the editor path),
        // and upload any uncached mesh geometry — all with `&mut self`, before the
        // immutable device/target borrows the render pass needs.
        if self.editor_pipeline.is_none() {
            self.editor_pipeline = Some(RenderPipeline3D::new(&self.ctx.device, color_format));
        }
        if self.editor_sky_pipeline.is_none() {
            self.editor_sky_pipeline = Some(SkyPipeline3D::new(&self.ctx.device, color_format));
        }
        let draw_calls = self.build_draw_calls();
        // Shadow atlas + light uniforms (pat-32edz / pat-2be0q): cached across
        // frames, regenerated only when the shadow-relevant state changes.
        let light_data = self.ensure_shadow_atlas();

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let pipeline = self.editor_pipeline.as_ref().unwrap();
        let sky_pipeline = self.editor_sky_pipeline.as_ref().unwrap();

        // Create color (linear HDR) and depth textures.
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
            format: color_format,
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

        // Procedural sky background (same as the editor path): a fullscreen pass
        // drawn before the meshes when the environment uses a Sky background.
        let sky_suns = crate::renderer::sky_suns_from_lights(&self.lights);
        let sky_bind = viewport
            .environment
            .as_ref()
            .and_then(|env| sky_uniform_for_env(env, viewport, w, h, &sky_suns))
            .map(|data| {
                let buf = create_buffer_init(
                    device,
                    queue,
                    "sky-uniform",
                    bytemuck::bytes_of(&data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sky-bind-group"),
                    layout: &sky_pipeline.layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
                });
                (buf, bg)
            });
        // Clear the linear HDR target to the environment's custom backdrop (linear)
        // or black; after the post-process's linear→sRGB encode the flat backdrop
        // returns at its authored value.
        let clear_linear = match viewport.environment.as_ref() {
            Some(env) if env.background_mode == BackgroundMode::CustomColor => env.background_color,
            _ => Color::new(0.0, 0.0, 0.0, 1.0),
        }
        .srgb_to_linear();

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
            layout: &pipeline.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        // Light bind group over the cached shadow atlases (pat-2be0q).
        let atlas = self
            .shadow_atlas_cache
            .as_ref()
            .expect("ensure_shadow_atlas ran above");
        let (light_bg, _light_buf) = create_light_bind_group(
            device,
            queue,
            &pipeline.light_layout,
            &light_data,
            &atlas.view,
            &atlas.omni_view,
            "light-bind-group",
        );
        let num_lights = self.lights.len().min(16) as u32;

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
                        // The environment's backdrop (linear), not plain black —
                        // a `CustomColor` background must survive the GPU trait
                        // path exactly like the software `fill_background`.
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
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            // Sky background first (depth-test off), so meshes rasterize over it.
            if let Some((_sky_buf, sky_bg)) = &sky_bind {
                pass.set_pipeline(&sky_pipeline.pipeline);
                pass.set_bind_group(0, sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }

            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &camera_bg, &[]);
            pass.set_bind_group(2, &light_bg, &[]);

            // Draw each expanded instance from the cached geometry (pat-ism62 #2b).
            for call in &draw_calls {
                let cached = match self.mesh_gpu_cache.get(&call.mesh_key) {
                    Some(c) => c,
                    None => continue,
                };
                // Shade in linear (HDR target + post-process encode back to sRGB).
                let alb = call.albedo.srgb_to_linear();
                let emis = call.emission.srgb_to_linear();
                let model_data = ModelUniformData {
                    model_matrix: transform_to_model_matrix(&call.transform),
                    albedo: [alb.r, alb.g, alb.b, call.albedo.a],
                    emission: [emis.r, emis.g, emis.b, call.emission.a],
                    roughness: call.roughness,
                    metallic: call.metallic,
                    shading_mode: call.shading_mode,
                    num_lights,
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
                    layout: &pipeline.model_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: model_buf.as_entire_binding(),
                    }],
                });

                pass.set_bind_group(1, &model_bg, &[]);
                pass.set_vertex_buffer(0, cached.vertex_buf.slice(..));
                pass.set_index_buffer(cached.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..cached.index_count, 0, 0..1);
            }
        }

        // Copy color texture to readback buffer (Rgba16Float = 8 bytes/pixel).
        let bytes_per_pixel = 8u32;
        let unpadded_row = w * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (unpadded_row + align - 1) / align * align;

        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color-readback"),
            size: padded_row as u64 * h as u64,
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
            size: depth_padded_row as u64 * h as u64,
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

        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h {
            let start = (row * padded_row) as usize;
            for col in 0..w {
                let o = start + (col * bytes_per_pixel) as usize;
                // Rgba16Float, RGBA order: decode each half-float channel.
                let r = f16_to_f32(u16::from_le_bytes([color_mapped[o], color_mapped[o + 1]]));
                let g = f16_to_f32(u16::from_le_bytes([color_mapped[o + 2], color_mapped[o + 3]]));
                let b = f16_to_f32(u16::from_le_bytes([color_mapped[o + 4], color_mapped[o + 5]]));
                let a = f16_to_f32(u16::from_le_bytes([color_mapped[o + 6], color_mapped[o + 7]]));
                pixels.push(Color::new(r, g, b, a));
            }
        }
        drop(color_mapped);

        // Map and read depth data (needed by the fog pass below, before the
        // camera post-process — the software path fogs in linear scene space).
        let depth_slice = depth_readback_buf.slice(..);
        depth_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        let depth_mapped = depth_slice.get_mapped_range();

        let mut depth = Vec::with_capacity(w as usize * h as usize);
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

        // Fog (linear scene space) then the shared camera post-process
        // (glow → tonemap → linear→sRGB) — the exact order and functions the
        // software `render_frame` uses, so `render_frame`'s pixels come out
        // 1:1 with `SoftwareRenderer3D::render_frame` including fog
        // (pat-kr04d: fog runs on the read-back HDR buffer with the read-back
        // depth; both backends store raw z/w with background = 1.0).
        let default_env = gdserver3d::environment::Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);
        let fog_inv_proj = invert_projection(&projection_matrix);
        crate::renderer::SoftwareRenderer3D::apply_fog_pass(
            &mut pixels,
            &depth,
            w,
            h,
            env,
            &fog_inv_proj,
            &viewport.camera_transform,
            &crate::renderer::sky_suns_from_lights(&self.lights),
        );
        crate::postprocess::apply_camera_post(&mut pixels, w, h, env);

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
    fn shader_has_directional_shadow_hook_and_pbr() {
        // pat-32edz / pat-sf07y: WGSL must wire shadow factor + Burley/GGX.
        assert!(SHADER_3D_WGSL.contains("directional_shadow_factor"));
        assert!(SHADER_3D_WGSL.contains("burley") || SHADER_3D_WGSL.contains("Burley") || SHADER_3D_WGSL.contains("fd90"));
        assert!(SHADER_3D_WGSL.contains("metallic"));
        // Real sampling — not the old always-1.0 placeholder.
        assert!(
            SHADER_3D_WGSL.contains("textureLoad(shadow_depth"),
            "WGSL must sample the bound shadow depth texture"
        );
        assert!(
            SHADER_3D_WGSL.contains("texture_2d_array"),
            "WGSL must use a shadow depth array (multi-light atlas)"
        );
        assert!(
            SHADER_3D_WGSL.contains("SHADOW_BIAS"),
            "WGSL must apply shadow bias matching software path"
        );
        assert!(
            !SHADER_3D_WGSL.contains("Placeholder: full light"),
            "placeholder shadow factor must be removed"
        );
        assert!(
            SHADER_3D_WGSL.contains("shadow_view_proj"),
            "light data must carry light-space view-projection"
        );
    }

    #[test]
    fn gpu_light_data_layout_includes_shadow_matrix() {
        // 64 (base) + 64 (mat4) = 128 bytes per light.
        assert_eq!(std::mem::size_of::<GpuLightData>(), 128);
        assert_eq!(std::mem::size_of::<LightArrayData>(), 128 * 16);
    }

    #[test]
    fn pack_lights_assigns_primary_shadow_slot() {
        use gdcore::math3d::Transform3D;
        use gdserver3d::instance::Instance3DId;
        use gdserver3d::light::Light3DId;
        use gdserver3d::mesh::Mesh3D;

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Occluder so the map has written depth.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(4.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let (data, maps) = pack_lights_with_shadows(&[light], std::iter::once(&inst));
        assert_eq!(maps.len(), 1, "one directional shadow map");
        assert_eq!(
            data.lights[0].shadow_map_index, 0,
            "primary directional light must bind slot 0"
        );
        assert_eq!(data.lights[0].shadow_enabled, 1);
        // Matrix must not be identity when a map was generated.
        let m = data.lights[0].shadow_view_proj;
        let is_identity = m == identity_mat4();
        assert!(!is_identity, "shadow_view_proj should be proj*view, not identity");
    }

    /// pat-mst61: two shadow-enabled directionals each get a unique layer index.
    #[test]
    fn pack_lights_assigns_multi_shadow_layers() {
        use gdcore::math3d::Transform3D;
        use gdserver3d::instance::Instance3DId;
        use gdserver3d::light::Light3DId;
        use gdserver3d::mesh::Mesh3D;

        let mut l0 = Light3D::directional(Light3DId(1));
        l0.shadow_enabled = true;
        l0.direction = Vector3::new(0.0, -1.0, 0.0);
        let mut l1 = Light3D::directional(Light3DId(2));
        l1.shadow_enabled = true;
        l1.direction = Vector3::new(-1.0, -1.0, 0.0).normalized();

        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let (data, maps) = pack_lights_with_shadows(&[l0, l1], std::iter::once(&inst));
        assert_eq!(maps.len(), 2, "two shadow maps for two lights");
        assert_eq!(data.lights[0].shadow_map_index, 0);
        assert_eq!(data.lights[1].shadow_map_index, 1);
    }

    #[test]
    fn mul_mat4_identity_is_noop() {
        let a = identity_mat4();
        let b = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [1.0, 2.0, 3.0, 1.0],
        ];
        assert_eq!(mul_mat4(&a, &b), b);
        assert_eq!(mul_mat4(&b, &a), b);
    }

    #[test]
    fn sky_shader_source_compiles() {
        let result = wgpu::naga::front::wgsl::parse_str(SKY_WGSL);
        assert!(result.is_ok(), "sky WGSL parse error: {:?}", result.err());
    }

    #[test]
    fn sky_uniform_size_and_alignment() {
        // 2 mat4x4 (128) + 4 vec4 colors (64) + params (16) + size (16)
        // + sun_dir_energy 4×vec4 (64) + sun_color_size 4×vec4 (64)
        // + sun_params (16) = 368.
        assert_eq!(std::mem::size_of::<SkyUniformData>(), 368);
    }

    /// The sky pass runs for procedural and panoramic sky materials.
    #[test]
    fn sky_uniform_only_for_procedural_sky() {
        use gdserver3d::sky::{PanoramicSkyMaterial, Sky, SkyMaterial};

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
        assert!(data.sun_params[2] < 1000.0, "procedural mode flag");
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

        // pat-coz7r: panoramic material also yields a sky pass with equirect mode.
        env.sky = Some(Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                energy_multiplier: 1.5,
                ..Default::default()
            }),
            ..Default::default()
        });
        let pano = sky_uniform_for_env(&env, &vp, 32, 32, &[]).expect("panoramic sky pass");
        assert!(
            pano.sun_params[2] >= 1000.0,
            "panoramic mode flag encoded in sun_params.z"
        );
        assert!(SKY_WGSL.contains("atan2(d.x, -d.z)"), "WGSL equirect u");
        assert!(SKY_WGSL.contains("acos(clamp(d.y"), "WGSL equirect v");
        // Godot's convention has NO half-turn shift — a `+ 0.5` on the
        // equirect u yaw-rotates every panorama by 180° (this regressed once).
        assert!(
            !SKY_WGSL.contains("3.14159265) + 0.5"),
            "WGSL equirect u must not carry a +0.5 shift"
        );
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

    /// A physical sky packs its material through the shared uniform slots
    /// (pat-fw141): mode flag 2000+count in sun_params.z, coefficients in the
    /// color alpha channels, exposure = material energy × background energy.
    #[test]
    fn sky_uniform_packs_physical_material() {
        use crate::renderer::SkySun;
        use gdcore::math::Vector3;
        use gdserver3d::sky::{PhysicalSkyMaterial, Sky, SkyMaterial};

        let vp = Viewport3D::new(64, 64);
        let mut mat = PhysicalSkyMaterial::default();
        mat.energy_multiplier = 1.5;
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.background_energy_multiplier = 2.0;
        env.sky = Some(Sky {
            material: SkyMaterial::Physical(mat.clone()),
            ..Sky::default()
        });

        let suns = [SkySun {
            direction: Vector3::new(0.0, 1.0, 0.0),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            size: 0.5f32.to_radians(),
        }];
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &suns).expect("physical sky pass");
        assert_eq!(data.sun_params[2], 2001.0, "mode flag 2000 + sun count");
        assert!((data.sky_top[3] - mat.rayleigh_coefficient).abs() < 1e-6);
        assert!((data.sky_horizon[3] - mat.mie_coefficient).abs() < 1e-6);
        assert!((data.ground_bottom[3] - mat.turbidity).abs() < 1e-6);
        assert!((data.params[0] - mat.mie_eccentricity).abs() < 1e-6);
        assert!((data.params[1] - mat.sun_disk_scale).abs() < 1e-6);
        assert!(
            (data.sun_params[3] - 3.0).abs() < 1e-6,
            "exposure = material 1.5 × background 2.0"
        );
        // No sun → the mode flag alone (shader shows the black night sky).
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &[]).expect("physical, no sun");
        assert_eq!(data.sun_params[2], 2000.0);
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
        // Base 64 + shadow_view_proj mat4 (64) = 128 (pat-32edz).
        assert_eq!(std::mem::size_of::<GpuLightData>(), 128);
    }

    #[test]
    fn light_array_size() {
        // 16 * 128 = 2048 bytes.
        assert_eq!(std::mem::size_of::<LightArrayData>(), 2048);
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
            ..Default::default()
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

    /// The shadow cache key is stable for identical state and sensitive to
    /// exactly the inputs that change a shadow map — and with no shadow
    /// caster it ignores occluder churn entirely (pat-2be0q).
    #[test]
    fn shadow_state_key_tracks_relevant_changes() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));

        let k = shadow_state_key(&[light.clone()], std::iter::once(&inst));
        assert_eq!(
            k,
            shadow_state_key(&[light.clone()], std::iter::once(&inst)),
            "identical state must hash identically"
        );

        let mut moved = inst.clone();
        moved.transform.origin.x += 1.0;
        assert_ne!(
            k,
            shadow_state_key(&[light.clone()], std::iter::once(&moved)),
            "moving an occluder must change the key"
        );

        let mut turned = light.clone();
        turned.direction = Vector3::new(0.3, -1.0, 0.0);
        assert_ne!(
            k,
            shadow_state_key(&[turned], std::iter::once(&inst)),
            "turning the light must change the key"
        );

        let mut no_shadow = light.clone();
        no_shadow.shadow_enabled = false;
        assert_eq!(
            shadow_state_key(&[no_shadow.clone()], std::iter::once(&inst)),
            shadow_state_key(&[no_shadow], std::iter::once(&moved)),
            "with no shadow caster, occluder moves must not churn the key"
        );
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
