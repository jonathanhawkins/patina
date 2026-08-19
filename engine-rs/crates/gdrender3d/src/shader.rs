//! Vertex and fragment shader abstractions for the 3D render pipeline.
//!
//! Provides trait-based shader stages that transform vertices and compute
//! per-fragment colors. Built-in implementations cover unlit, Lambert diffuse,
//! and Phong specular shading models matching [`gdserver3d::ShadingMode`].

use std::sync::Arc;

use gdcore::math::{Color, Vector3};

use gdserver3d::light::ShadowCubemap;
use gdserver3d::material::TextureImage;

use crate::shadow_map::ShadowMap;

/// Data passed into the vertex shader for each vertex.
#[derive(Debug, Clone, Copy)]
pub struct VertexInput {
    /// Object-space position.
    pub position: Vector3,
    /// Object-space normal.
    pub normal: Vector3,
    /// Texture coordinates.
    pub uv: [f32; 2],
}

/// Data output from the vertex shader, consumed by rasterization and
/// interpolated per-fragment.
#[derive(Debug, Clone, Copy)]
pub struct VertexOutput {
    /// Clip-space position (after projection).
    pub clip_position: [f32; 4],
    /// World-space position (for lighting calculations).
    pub world_position: Vector3,
    /// World-space normal (for lighting calculations).
    pub world_normal: Vector3,
    /// Interpolated texture coordinates.
    pub uv: [f32; 2],
}

impl VertexOutput {
    /// Linearly interpolates between two vertex outputs by factor `t`.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            clip_position: [
                a.clip_position[0] + (b.clip_position[0] - a.clip_position[0]) * t,
                a.clip_position[1] + (b.clip_position[1] - a.clip_position[1]) * t,
                a.clip_position[2] + (b.clip_position[2] - a.clip_position[2]) * t,
                a.clip_position[3] + (b.clip_position[3] - a.clip_position[3]) * t,
            ],
            world_position: Vector3::new(
                a.world_position.x + (b.world_position.x - a.world_position.x) * t,
                a.world_position.y + (b.world_position.y - a.world_position.y) * t,
                a.world_position.z + (b.world_position.z - a.world_position.z) * t,
            ),
            world_normal: Vector3::new(
                a.world_normal.x + (b.world_normal.x - a.world_normal.x) * t,
                a.world_normal.y + (b.world_normal.y - a.world_normal.y) * t,
                a.world_normal.z + (b.world_normal.z - a.world_normal.z) * t,
            ),
            uv: [
                a.uv[0] + (b.uv[0] - a.uv[0]) * t,
                a.uv[1] + (b.uv[1] - a.uv[1]) * t,
            ],
        }
    }

    /// Barycentric interpolation of three vertex outputs.
    pub fn barycentric(a: &Self, b: &Self, c: &Self, u: f32, v: f32, w: f32) -> Self {
        Self {
            clip_position: [
                a.clip_position[0] * u + b.clip_position[0] * v + c.clip_position[0] * w,
                a.clip_position[1] * u + b.clip_position[1] * v + c.clip_position[1] * w,
                a.clip_position[2] * u + b.clip_position[2] * v + c.clip_position[2] * w,
                a.clip_position[3] * u + b.clip_position[3] * v + c.clip_position[3] * w,
            ],
            world_position: Vector3::new(
                a.world_position.x * u + b.world_position.x * v + c.world_position.x * w,
                a.world_position.y * u + b.world_position.y * v + c.world_position.y * w,
                a.world_position.z * u + b.world_position.z * v + c.world_position.z * w,
            ),
            world_normal: Vector3::new(
                a.world_normal.x * u + b.world_normal.x * v + c.world_normal.x * w,
                a.world_normal.y * u + b.world_normal.y * v + c.world_normal.y * w,
                a.world_normal.z * u + b.world_normal.z * v + c.world_normal.z * w,
            ),
            uv: [
                a.uv[0] * u + b.uv[0] * v + c.uv[0] * w,
                a.uv[1] * u + b.uv[1] * v + c.uv[1] * w,
            ],
        }
    }
}

/// Per-fragment input produced by rasterization and attribute interpolation.
#[derive(Debug, Clone, Copy)]
pub struct FragmentInput {
    /// World-space position of this fragment.
    pub world_position: Vector3,
    /// Interpolated world-space normal (not necessarily unit-length).
    pub world_normal: Vector3,
    /// Interpolated texture coordinates.
    pub uv: [f32; 2],
    /// NDC depth of this fragment (for informational use; depth test is external).
    pub depth: f32,
}

/// Uniforms available to both vertex and fragment shaders.
#[derive(Debug, Clone)]
pub struct ShaderUniforms {
    /// Model-to-world transform columns (column-major 4x4).
    pub model_matrix: [[f32; 4]; 4],
    /// View matrix (inverse camera transform, column-major 4x4).
    pub view_matrix: [[f32; 4]; 4],
    /// Projection matrix (column-major 4x4).
    pub projection_matrix: [[f32; 4]; 4],
    /// Material albedo color.
    pub albedo: Color,
    /// Material emission color.
    pub emission: Color,
    /// Ambient light color (linear) applied as `ambient * albedo`. Derived from
    /// the environment (sky irradiance, flat color, or background); replaces the
    /// old hardcoded grey ambient so surfaces pick up the sky's tint like Godot.
    pub ambient: Color,
    /// Material roughness.
    pub roughness: f32,
    /// Material metallic factor.
    pub metallic: f32,
    /// Camera world-space position.
    pub camera_position: Vector3,
    /// Scene lights for shading.
    pub lights: Vec<LightUniform>,
    /// Directional light shadow maps, indexed parallel to `lights`.
    /// Only populated for directional lights with `shadow_enabled = true`;
    /// other entries are `None`.
    pub directional_shadow_maps: Vec<Option<Arc<ShadowMap>>>,
    /// Omni (point) light shadow cubemaps, indexed parallel to `lights`.
    /// Only populated for point lights with `shadow_enabled = true` and
    /// `omni_shadow_mode == Cube`; other entries are `None`.
    pub omni_shadow_cubemaps: Vec<Option<Arc<ShadowCubemap>>>,
    /// Optional per-fragment material texture maps sampled at the interpolated
    /// UV. When present they modulate the flat `albedo`/`emission`/`roughness`/
    /// `metallic` scalars and perturb the shading normal. `None` fields fall
    /// back to the flat values, so an untextured material is unchanged.
    pub textures: MaterialTextures,
}

/// The set of texture maps bound for a draw, sampled per-fragment at the
/// interpolated UV. Mirrors Godot's `StandardMaterial3D` texture channels.
///
/// Color maps (albedo, emission) hold sRGB texels and are decoded to linear
/// before modulating the (already-linear) flat colors. Data maps (normal,
/// roughness, metallic) hold raw linear values.
#[derive(Debug, Clone, Default)]
pub struct MaterialTextures {
    /// Albedo map; decoded texel multiplies the flat albedo.
    pub albedo: Option<Arc<TextureImage>>,
    /// Tangent-space normal map; perturbs the interpolated normal.
    pub normal: Option<Arc<TextureImage>>,
    /// Normal-map strength (1.0 = full).
    pub normal_scale: f32,
    /// Roughness map; green channel multiplies the flat roughness.
    pub roughness: Option<Arc<TextureImage>>,
    /// Metallic map; blue channel multiplies the flat metallic.
    pub metallic: Option<Arc<TextureImage>>,
    /// Emission map; decoded texel multiplies the flat emission.
    pub emission: Option<Arc<TextureImage>>,
}

/// Per-fragment material values after sampling any bound textures at a UV.
///
/// All colors are **linear** and ready to feed lighting; `normal` is the
/// world-space shading normal (normal-mapped if a normal map is bound).
#[derive(Debug, Clone, Copy)]
pub struct SampledMaterial {
    /// Linear albedo (flat albedo × decoded albedo texel).
    pub albedo: Color,
    /// Linear emission (flat emission × decoded emission texel).
    pub emission: Color,
    /// Effective roughness in `[0, 1]`.
    pub roughness: f32,
    /// Effective metallic in `[0, 1]`.
    pub metallic: f32,
    /// World-space shading normal (not necessarily unit-length).
    pub normal: Vector3,
}

impl ShaderUniforms {
    /// Resolves the effective per-fragment material by sampling any bound
    /// texture maps at `uv`, starting from the flat scalar material and the
    /// interpolated geometric `world_normal`.
    ///
    /// Bilinear filtering with repeat wrap (Godot's default). Color maps are
    /// sRGB-decoded; the normal map is decoded from tangent space and rotated
    /// into world space via a tangent frame derived from `world_normal`
    /// (meshes carry no tangents here — see [`tangent_frame`]).
    pub fn sample_material(&self, uv: [f32; 2], world_normal: Vector3) -> SampledMaterial {
        let (u, v) = (uv[0], uv[1]);
        let t = &self.textures;

        let mut albedo = self.albedo;
        if let Some(map) = &t.albedo {
            let texel = map.sample_bilinear(u, v).srgb_to_linear();
            albedo = Color::new(
                albedo.r * texel.r,
                albedo.g * texel.g,
                albedo.b * texel.b,
                albedo.a * texel.a,
            );
        }

        let mut emission = self.emission;
        if let Some(map) = &t.emission {
            let texel = map.sample_bilinear(u, v).srgb_to_linear();
            emission = Color::new(
                emission.r * texel.r,
                emission.g * texel.g,
                emission.b * texel.b,
                emission.a * texel.a,
            );
        }

        let mut roughness = self.roughness;
        if let Some(map) = &t.roughness {
            // Godot samples roughness from the green channel by default.
            roughness *= map.sample_bilinear(u, v).g;
        }

        let mut metallic = self.metallic;
        if let Some(map) = &t.metallic {
            // Godot samples metallic from the blue channel by default.
            metallic *= map.sample_bilinear(u, v).b;
        }

        let normal = match &t.normal {
            Some(map) => {
                let texel = map.sample_bilinear(u, v);
                apply_normal_map(world_normal, texel, t.normal_scale)
            }
            None => world_normal,
        };

        SampledMaterial {
            albedo,
            emission,
            roughness: roughness.clamp(0.0, 1.0),
            metallic: metallic.clamp(0.0, 1.0),
            normal,
        }
    }
}

/// Builds an orthonormal tangent frame `(tangent, bitangent)` for a unit
/// `normal` using Duff et al.'s branchless method. Meshes here carry no
/// tangents, so the frame is derived purely from the normal — deterministic,
/// but not aligned to UV gradients. Sufficient to perturb lighting from a
/// normal map (the perturbation direction is stable per normal).
pub fn tangent_frame(normal: Vector3) -> (Vector3, Vector3) {
    let n = normal.normalized();
    let s = if n.z >= 0.0 { 1.0 } else { -1.0 };
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let tangent = Vector3::new(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bitangent = Vector3::new(b, s + n.y * n.y * a, -n.y);
    (tangent, bitangent)
}

/// Decodes a tangent-space normal-map texel (`rgb` in `[0,1]` encoding a
/// direction in `[-1,1]`) and rotates it into world space via the tangent
/// frame of `world_normal`, scaling the XY perturbation by `scale`.
pub fn apply_normal_map(world_normal: Vector3, texel: Color, scale: f32) -> Vector3 {
    let n = world_normal.normalized();
    // Decode from [0,1] to [-1,1]; a flat map (0.5,0.5,1.0) → (0,0,1) = no tilt.
    let tx = (texel.r * 2.0 - 1.0) * scale;
    let ty = (texel.g * 2.0 - 1.0) * scale;
    let tz = (texel.b * 2.0 - 1.0).max(1e-4);
    let (tangent, bitangent) = tangent_frame(n);
    let perturbed = Vector3::new(
        tangent.x * tx + bitangent.x * ty + n.x * tz,
        tangent.y * tx + bitangent.y * ty + n.y * tz,
        tangent.z * tx + bitangent.z * ty + n.z * tz,
    );
    perturbed.normalized()
}

/// Type tag for light uniforms passed to fragment shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    /// Infinite directional light (no position, no attenuation).
    Directional,
    /// Point light emitting in all directions from a position.
    Point,
    /// Spot light emitting in a cone from a position.
    Spot,
}

/// A light source passed to the fragment shader.
#[derive(Debug, Clone, Copy)]
pub struct LightUniform {
    /// Type of this light source.
    pub kind: LightKind,
    /// Light direction (normalized, pointing toward the light source for
    /// directional lights, or the spot direction for spot lights).
    pub direction: Vector3,
    /// Light color multiplied by energy.
    pub color: Color,
    /// World-space position (ignored for directional lights).
    pub position: Vector3,
    /// Maximum range (0 = infinite).
    pub range: f32,
    /// Distance attenuation exponent (1.0 = linear, 2.0 = quadratic).
    pub attenuation: f32,
    /// Spot cone half-angle in radians.
    pub spot_angle: f32,
    /// Spot cone angle attenuation exponent.
    pub spot_angle_attenuation: f32,
    /// Whether this light casts shadows.
    pub shadow_enabled: bool,
}

impl LightUniform {
    /// Creates a directional light uniform (backward-compatible helper).
    pub fn directional(direction: Vector3, color: Color) -> Self {
        Self {
            kind: LightKind::Directional,
            direction,
            color,
            position: Vector3::ZERO,
            range: 0.0,
            attenuation: 1.0,
            spot_angle: 0.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
        }
    }

    /// Computes the effective contribution factor for this light at a given
    /// world position. Returns `(light_dir, intensity)` where `light_dir`
    /// points from the fragment toward the light source and `intensity` is
    /// in [0, 1] incorporating distance and cone attenuation.
    pub fn evaluate(&self, frag_pos: Vector3) -> (Vector3, f32) {
        match self.kind {
            LightKind::Directional => (self.direction, 1.0),
            LightKind::Point => {
                let to_light = self.position - frag_pos;
                let dist = to_light.length();
                if dist < 1e-6 {
                    return (Vector3::UP, 1.0);
                }
                let dir = to_light * (1.0 / dist);
                let atten = Self::distance_attenuation(dist, self.range, self.attenuation);
                (dir, atten)
            }
            LightKind::Spot => {
                let to_light = self.position - frag_pos;
                let dist = to_light.length();
                if dist < 1e-6 {
                    return (Vector3::UP, 1.0);
                }
                let dir = to_light * (1.0 / dist);
                let dist_atten = Self::distance_attenuation(dist, self.range, self.attenuation);
                let cone_atten = self.cone_attenuation(dir);
                (dir, dist_atten * cone_atten)
            }
        }
    }

    /// Godot-style distance attenuation: `max(0, 1 - (d/range)^atten)`.
    fn distance_attenuation(dist: f32, range: f32, atten: f32) -> f32 {
        if range <= 0.0 {
            return 1.0; // Infinite range.
        }
        let ratio = (dist / range).min(1.0);
        (1.0 - ratio.powf(atten)).max(0.0)
    }

    /// Computes spot cone attenuation for a fragment direction.
    /// `frag_dir` points from the fragment toward the light.
    fn cone_attenuation(&self, frag_dir: Vector3) -> f32 {
        // `self.direction` for a spot light is the forward direction of the
        // light (where it points). The angle between the negated fragment
        // direction and the spot direction gives the cone angle.
        let cos_angle = (-frag_dir).dot(self.direction);
        let cos_outer = self.spot_angle.cos();

        if cos_angle <= cos_outer {
            return 0.0; // Outside the cone.
        }

        // Smooth falloff from center to edge of cone.
        let t = ((cos_angle - cos_outer) / (1.0 - cos_outer)).clamp(0.0, 1.0);
        t.powf(self.spot_angle_attenuation)
    }
}

/// A vertex shader transforms per-vertex attributes into clip space.
pub trait VertexShader {
    /// Processes a single vertex and returns the transformed output.
    fn process(&self, input: &VertexInput, uniforms: &ShaderUniforms) -> VertexOutput;
}

/// A fragment shader computes the final color for a rasterized fragment.
pub trait FragmentShader {
    /// Computes the output color for a fragment.
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color;
}

// ── Built-in vertex shader ──────────────────────────────────────────

/// Standard vertex shader that applies model-view-projection transforms.
#[derive(Debug, Clone, Copy)]
pub struct StandardVertexShader;

impl StandardVertexShader {
    fn mat4_mul_vec4(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
        [
            m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
            m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
            m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
            m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
        ]
    }

    fn mat4_mul_dir(m: &[[f32; 4]; 4], v: Vector3) -> Vector3 {
        // Transform direction (w=0) — only upper-left 3x3.
        Vector3::new(
            m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
            m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
            m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
        )
    }
}

impl VertexShader for StandardVertexShader {
    fn process(&self, input: &VertexInput, uniforms: &ShaderUniforms) -> VertexOutput {
        let pos = [input.position.x, input.position.y, input.position.z, 1.0];

        // Model → world.
        let world_pos4 = Self::mat4_mul_vec4(&uniforms.model_matrix, pos);
        let world_position = Vector3::new(world_pos4[0], world_pos4[1], world_pos4[2]);

        // World → view.
        let view_pos = Self::mat4_mul_vec4(&uniforms.view_matrix, world_pos4);

        // View → clip.
        let clip_position = Self::mat4_mul_vec4(&uniforms.projection_matrix, view_pos);

        // Transform normal by model matrix (ignoring translation).
        let world_normal = Self::mat4_mul_dir(&uniforms.model_matrix, input.normal).normalized();

        VertexOutput {
            clip_position,
            world_position,
            world_normal,
            uv: input.uv,
        }
    }
}

// ── Built-in fragment shaders ───────────────────────────────────────

/// Unlit fragment shader — returns albedo color directly with emission added.
#[derive(Debug, Clone, Copy)]
pub struct UnlitFragmentShader;

impl FragmentShader for UnlitFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        // Sample albedo/emission maps at the fragment UV (flat color when none).
        let m = uniforms.sample_material(input.uv, input.world_normal);
        // Scene-referred linear HDR: no clamp, so emissive values > 1 survive
        // to feed glow; the camera post pass tonemaps and clamps at the end.
        Color::new(
            m.albedo.r + m.emission.r,
            m.albedo.g + m.emission.g,
            m.albedo.b + m.emission.b,
            m.albedo.a,
        )
    }
}

/// Shadow bias for omni cubemap depth comparison.
const OMNI_SHADOW_BIAS: f32 = 0.05;

/// Computes the shadow factor for a light at the given fragment position.
///
/// Returns a value in `[0.0, 1.0]` where 1.0 = fully lit, 0.0 = fully shadowed.
fn compute_shadow_factor(
    light: &LightUniform,
    light_index: usize,
    frag_pos: Vector3,
    uniforms: &ShaderUniforms,
) -> f32 {
    if !light.shadow_enabled {
        return 1.0;
    }
    match light.kind {
        LightKind::Directional => uniforms
            .directional_shadow_maps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|sm| 1.0 - sm.sample(frag_pos))
            .unwrap_or(1.0),
        LightKind::Point => uniforms
            .omni_shadow_cubemaps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|cm| {
                let to_frag = frag_pos - light.position;
                let dist = to_frag.length();
                let stored = cm.sample(to_frag);
                if dist > stored + OMNI_SHADOW_BIAS {
                    0.0 // in shadow
                } else {
                    1.0 // lit
                }
            })
            .unwrap_or(1.0),
        LightKind::Spot => uniforms
            .directional_shadow_maps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|sm| 1.0 - sm.sample(frag_pos))
            .unwrap_or(1.0),
    }
}

/// Lambert diffuse fragment shader.
#[derive(Debug, Clone, Copy)]
pub struct LambertFragmentShader;

impl FragmentShader for LambertFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        // Sample material textures (albedo/normal/emission) at the fragment UV.
        let m = uniforms.sample_material(input.uv, input.world_normal);
        let normal = m.normal;
        let albedo = m.albedo;

        let mut diffuse_r = uniforms.ambient.r * albedo.r;
        let mut diffuse_g = uniforms.ambient.g * albedo.g;
        let mut diffuse_b = uniforms.ambient.b * albedo.b;

        for (i, light) in uniforms.lights.iter().enumerate() {
            let (light_dir, intensity) = light.evaluate(input.world_position);
            let shadow = compute_shadow_factor(light, i, input.world_position, uniforms);
            let n_dot_l = normal.dot(light_dir).max(0.0);
            let contrib = n_dot_l * intensity * shadow;
            diffuse_r += albedo.r * light.color.r * contrib;
            diffuse_g += albedo.g * light.color.g * contrib;
            diffuse_b += albedo.b * light.color.b * contrib;
        }

        // Scene-referred linear HDR: no clamp (tonemapping clamps at the end).
        Color::new(
            diffuse_r + m.emission.r,
            diffuse_g + m.emission.g,
            diffuse_b + m.emission.b,
            albedo.a,
        )
    }
}

/// Phong mode now uses Godot's default Burley diffuse + Schlick-GGX specular
/// (pat-sf07y). Kept under the `Phong` name so existing `ShadingMode::Phong`
/// materials pick up the PBR path without a new enum variant.
#[derive(Debug, Clone, Copy)]
pub struct PhongFragmentShader;

impl FragmentShader for PhongFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        pbr_shade(input, uniforms)
    }
}

/// Burley diffuse + Schlick-GGX specular (Godot `light_compute` default).
///
/// Ported from `scene_forward_lights_inc.glsl` (Godot 4.6.1): `D_GGX`,
/// `V_GGX`, `SchlickFresnel`, Burley FD90, and metallic F0 mix.
pub fn pbr_shade(input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
    // Sample material textures (albedo/normal/roughness/metallic/emission) at UV.
    let m = uniforms.sample_material(input.uv, input.world_normal);
    let albedo = m.albedo;
    let n = m.normal;
    let v = (uniforms.camera_position - input.world_position).normalized();
    let n_dot_v = n.dot(v).max(1e-4);
    let roughness = m.roughness;
    let metallic = m.metallic;
    let alpha = (roughness * roughness).max(1e-4);
    // F0: dielectric 0.04, metals use albedo (specular amount = 0.5 → 0.16*0.25=0.04).
    let f0_d = 0.04;
    let f0 = [
        lerp_f(f0_d, albedo.r, metallic),
        lerp_f(f0_d, albedo.g, metallic),
        lerp_f(f0_d, albedo.b, metallic),
    ];
    let albedo_lin = [
        albedo.r * (1.0 - metallic),
        albedo.g * (1.0 - metallic),
        albedo.b * (1.0 - metallic),
    ];

    let mut r = uniforms.ambient.r * albedo.r;
    let mut g = uniforms.ambient.g * albedo.g;
    let mut b = uniforms.ambient.b * albedo.b;

    for (i, light) in uniforms.lights.iter().enumerate() {
        let (l, intensity) = light.evaluate(input.world_position);
        let shadow = compute_shadow_factor(light, i, input.world_position, uniforms);
        if intensity <= 0.0 || shadow <= 0.0 {
            continue;
        }
        let n_dot_l = n.dot(l).max(0.0);
        if n_dot_l <= 0.0 {
            continue;
        }
        let h = (l + v).normalized();
        let n_dot_h = n.dot(h).max(1e-4);
        let l_dot_h = l.dot(h).max(1e-4);

        // Burley diffuse (Filament / Godot).
        let fd90 = 0.5 + 2.0 * l_dot_h * l_dot_h * roughness;
        let light_scatter = 1.0 + (fd90 - 1.0) * pow5(1.0 - n_dot_l);
        let view_scatter = 1.0 + (fd90 - 1.0) * pow5(1.0 - n_dot_v);
        let burley = (1.0 / std::f32::consts::PI) * light_scatter * view_scatter;
        let diffuse = n_dot_l * intensity * shadow * burley;

        r += albedo_lin[0] * light.color.r * diffuse;
        g += albedo_lin[1] * light.color.g * diffuse;
        b += albedo_lin[2] * light.color.b * diffuse;

        // GGX D * V * F (Schlick).
        let d = d_ggx(n_dot_h, alpha);
        let vis = v_ggx(n_dot_l, n_dot_v, alpha);
        let f = [
            f0[0] + (1.0 - f0[0]) * pow5(1.0 - l_dot_h),
            f0[1] + (1.0 - f0[1]) * pow5(1.0 - l_dot_h),
            f0[2] + (1.0 - f0[2]) * pow5(1.0 - l_dot_h),
        ];
        let spec = d * vis * n_dot_l * intensity * shadow;
        r += light.color.r * f[0] * spec;
        g += light.color.g * f[1] * spec;
        b += light.color.b * f[2] * spec;
    }

    Color::new(
        r + m.emission.r,
        g + m.emission.g,
        b + m.emission.b,
        albedo.a,
    )
}

#[inline]
fn pow5(x: f32) -> f32 {
    let x2 = x * x;
    x2 * x2 * x
}

#[inline]
fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// GGX normal distribution (Godot `D_GGX`).
pub fn d_ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a = n_dot_h * alpha;
    let k = alpha / ((1.0 - n_dot_h * n_dot_h) + a * a).max(1e-6);
    let d = k * k * (1.0 / std::f32::consts::PI);
    d.max(0.0)
}

/// Height-correlated Smith GGX visibility (Godot `V_GGX`).
///
/// `0.5 / mix(2 * NdotL * NdotV, NdotL + NdotV, alpha)`.
pub fn v_ggx(n_dot_l: f32, n_dot_v: f32, alpha: f32) -> f32 {
    let a = 2.0 * n_dot_l * n_dot_v;
    let b = n_dot_l + n_dot_v;
    let denom = a * (1.0 - alpha) + b * alpha;
    (0.5 / denom.max(1e-6)).min(1.0e4)
}

/// Selects the appropriate fragment shader for a given shading mode.
pub fn fragment_shader_for_mode(
    mode: gdserver3d::material::ShadingMode,
) -> Box<dyn FragmentShader> {
    match mode {
        gdserver3d::material::ShadingMode::Unlit => Box::new(UnlitFragmentShader),
        gdserver3d::material::ShadingMode::Lambert => Box::new(LambertFragmentShader),
        gdserver3d::material::ShadingMode::Phong => Box::new(PhongFragmentShader),
    }
}

// ── Custom shader fragment shader (bridge from ShaderMaterial3D) ────

/// A fragment shader driven by a compiled [`gdserver3d::ShaderMaterial3D`].
///
/// Bridges the abstract `ShaderMaterial3D` + `CompiledShader3D` from
/// `gdserver3d` into the concrete `FragmentShader` trait used by the
/// software renderer. The compiled shader's `render_mode` flags and
/// runtime parameter values are applied during fragment processing.
pub struct CustomFragmentShader {
    /// The compiled shader program.
    compiled: gdserver3d::CompiledShader3D,
    /// Runtime parameter values from the ShaderMaterial3D.
    parameters: std::collections::HashMap<String, gdvariant::Variant>,
    /// Parsed `fragment()` body instructions (from the compiled shader) for
    /// CPU evaluation. When empty, `process` falls back to the uniform-only
    /// [`gdserver3d::ShaderProcessor3D::apply_shader`] path.
    fragment_body: Vec<gdserver3d::shader::FragmentInstruction3D>,
    /// Textures bound by `sampler2D` uniform name, sampled by the
    /// `texture(sampler, uv)` fragment expression. Populated via
    /// [`CustomFragmentShader::set_texture`] (Variants cannot carry pixels, so
    /// textures are injected separately from `parameters`).
    /// `Arc`-shared as a whole (pat-s3rrv.36), not just per-entry: the AST
    /// fragment path needs an OWNED `'static` sampler closure, and cloning the
    /// map per fragment to build one would be a per-pixel allocation. Sharing
    /// the map makes it a refcount bump. `set_texture` uses `Arc::make_mut`,
    /// so binding a texture is still a plain mutation from the caller's view.
    textures: Arc<std::collections::HashMap<String, Arc<TextureImage>>>,
    /// Shader `TIME` built-in, injectable via [`CustomFragmentShader::set_time`].
    /// Frame-loop plumbing to the RenderingServer is a documented follow-up;
    /// the setter makes time-animated shaders testable now.
    time: f32,
}

impl CustomFragmentShader {
    /// Creates a custom fragment shader from a `ShaderMaterial3D`.
    ///
    /// Compiles the attached shader (if any) and captures the runtime
    /// parameters. If no shader is attached, uses an empty compiled shader.
    pub fn from_material(material: &gdserver3d::ShaderMaterial3D) -> Self {
        // pat-s3rrv.36: `compile_cached`, not `compile`. This runs for every
        // instance on every frame (gdrender3d/src/renderer.rs, inside the entry
        // loop), so an unmemoized compile re-tokenized, re-parsed and
        // re-generated WGSL N times per frame for sources that never changed.
        let compiler = gdserver3d::ShaderCompiler3D::new();
        let compiled = if let Some(shader) = &material.shader {
            compiler.compile_cached(shader.shader_type, &shader.source_code)
        } else {
            compiler.compile_cached(gdserver3d::ShaderType3D::Spatial, "")
        };
        let fragment_body = compiled.fragment_body.clone();
        Self {
            compiled,
            parameters: material.parameters.clone(),
            fragment_body,
            textures: Arc::new(std::collections::HashMap::new()),
            time: 0.0,
        }
    }

    /// Creates a custom fragment shader from an already-compiled shader
    /// and parameter map.
    pub fn from_compiled(
        compiled: gdserver3d::CompiledShader3D,
        parameters: std::collections::HashMap<String, gdvariant::Variant>,
    ) -> Self {
        let fragment_body = compiled.fragment_body.clone();
        Self {
            compiled,
            parameters,
            fragment_body,
            textures: Arc::new(std::collections::HashMap::new()),
            time: 0.0,
        }
    }

    /// Returns a reference to the compiled shader.
    pub fn compiled(&self) -> &gdserver3d::CompiledShader3D {
        &self.compiled
    }

    /// Returns `true` if the shader compiled without errors.
    pub fn is_valid(&self) -> bool {
        !self.compiled.has_errors()
    }

    /// Sets the shader `TIME` built-in used when evaluating the fragment body.
    ///
    /// The render frame loop should call this each frame with the elapsed
    /// time so time-animated shaders advance; without it, `TIME` reads as 0.
    pub fn set_time(&mut self, t: f32) {
        self.time = t;
    }

    /// Binds a texture to a `sampler2D` uniform by name for the CPU
    /// `texture(sampler, uv)` fragment expression.
    pub fn set_texture(&mut self, name: &str, texture: Arc<TextureImage>) {
        // Copy-on-write: clones the map only if a sampler built earlier still
        // holds a reference to it, which is what keeps an in-flight render
        // seeing a consistent set of bindings.
        Arc::make_mut(&mut self.textures).insert(name.to_string(), texture);
    }
}

impl FragmentShader for CustomFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        let ctx = gdserver3d::FragmentContext3D {
            albedo: uniforms.albedo,
            normal: input.world_normal,
            world_position: input.world_position,
            uv: (input.uv[0], input.uv[1]),
            view_dir: (uniforms.camera_position - input.world_position).normalized(),
            roughness: uniforms.roughness,
            metallic: uniforms.metallic,
            time: self.time,
        };

        // If the shader has a parsed fragment body, evaluate it (real AST +
        // UV math + TIME + texture sampling). Otherwise fall back to the
        // uniform-only path so uniform-driven shaders don't regress.
        let color = if self.fragment_body.is_empty() {
            let processor = gdserver3d::ShaderProcessor3D::new();
            processor.apply_shader(&self.compiled, &self.parameters, &ctx)
        } else {
            // Sample bound textures by sampler2D uniform name; a missing
            // binding yields opaque white (a neutral placeholder). The closure
            // is passed inline so it coerces to the higher-ranked `&dyn Fn`.
            // pat-s3rrv.36: the legacy instruction arms take the borrowed
            // closure below; the AST arm needs an OWNED `'static` sampler, so
            // it captures a clone of the Arc-shared texture map — a refcount
            // bump, not a copy of the bindings.
            let env = gdserver3d::shader::ShaderEnv3D {
                sampler: Some({
                    let textures = Arc::clone(&self.textures);
                    gdserver3d::shader::Sampler::new(move |name: &str, uv: [f32; 2]| {
                        match textures.get(name) {
                            Some(tex) => {
                                let c = tex.sample_bilinear(uv[0], uv[1]);
                                [c.r, c.g, c.b, c.a]
                            }
                            // A missing binding is opaque white, the same
                            // neutral placeholder the borrowed closure uses.
                            None => [1.0, 1.0, 1.0, 1.0],
                        }
                    })
                }),
            };
            gdserver3d::shader::execute_fragment_3d_with_env(
                &self.fragment_body,
                &ctx,
                &self.parameters,
                &|name: &str, u: f32, v: f32| match self.textures.get(name) {
                    Some(tex) => tex.sample_bilinear(u, v),
                    None => Color::new(1.0, 1.0, 1.0, 1.0),
                },
                &env,
            )
        };

        // If unshaded, return the color directly (no lighting).
        if self.compiled.render_modes.unshaded {
            return color;
        }

        // Otherwise apply Lambert lighting to the shader's output color,
        // matching what LambertFragmentShader does.
        let normal = input.world_normal.normalized();
        let mut lit = Color::new(0.0, 0.0, 0.0, color.a);

        // Ambient term.
        let ambient = 0.15;
        lit.r += color.r * ambient;
        lit.g += color.g * ambient;
        lit.b += color.b * ambient;

        for light in &uniforms.lights {
            let n_dot_l = match light.kind {
                LightKind::Directional => normal.dot(light.direction).max(0.0),
                LightKind::Point | LightKind::Spot => {
                    let to_light = (light.position - input.world_position).normalized();
                    normal.dot(to_light).max(0.0)
                }
            };

            lit.r += color.r * light.color.r * n_dot_l;
            lit.g += color.g * light.color.g * n_dot_l;
            lit.b += color.b * light.color.b * n_dot_l;
        }

        // Add emission.
        if let Some(gdvariant::Variant::Color(e)) = self.parameters.get("emission") {
            lit.r = (lit.r + e.r).min(1.0);
            lit.g = (lit.g + e.g).min(1.0);
            lit.b = (lit.b + e.b).min(1.0);
        }

        lit.r = lit.r.min(1.0);
        lit.g = lit.g.min(1.0);
        lit.b = lit.b.min(1.0);

        lit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Material texture sampling ──

    fn tex(w: u32, h: u32, pixels: Vec<Color>) -> Arc<TextureImage> {
        Arc::new(TextureImage::new(w, h, pixels))
    }

    #[test]
    fn tangent_frame_is_orthonormal() {
        for n in [
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 2.0, 3.0).normalized(),
            Vector3::new(-2.0, 0.5, -1.0).normalized(),
        ] {
            let (t, b) = tangent_frame(n);
            assert!(t.dot(n).abs() < 1e-5, "tangent ⟂ normal");
            assert!(b.dot(n).abs() < 1e-5, "bitangent ⟂ normal");
            assert!(t.dot(b).abs() < 1e-5, "tangent ⟂ bitangent");
            assert!((t.length() - 1.0).abs() < 1e-5, "tangent unit-length");
            assert!((b.length() - 1.0).abs() < 1e-5, "bitangent unit-length");
        }
    }

    #[test]
    fn flat_normal_map_is_identity() {
        let n = Vector3::new(0.0, 0.0, 1.0);
        // Encoded (0.5,0.5,1.0) decodes to tangent-space (0,0,1) = no tilt.
        let out = apply_normal_map(n, Color::new(0.5, 0.5, 1.0, 1.0), 1.0);
        assert!((out.x - n.x).abs() < 1e-5);
        assert!((out.y - n.y).abs() < 1e-5);
        assert!((out.z - n.z).abs() < 1e-5);
    }

    #[test]
    fn tilted_normal_map_perturbs_normal() {
        let n = Vector3::new(0.0, 0.0, 1.0);
        // Full +X tangent tilt: (1.0,0.5,1.0) → tangent-space (1,0,1) → 45° toward +X.
        let out = apply_normal_map(n, Color::new(1.0, 0.5, 1.0, 1.0), 1.0);
        assert!(out.x > 0.5, "normal should tilt toward +X, got {out:?}");
        assert!((out.length() - 1.0).abs() < 1e-5, "result stays unit-length");
    }

    #[test]
    fn sample_material_albedo_texture_multiplies_flat_color() {
        let mut u = default_uniforms();
        u.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        // Solid green sRGB texel; sRGB(1.0)=1.0, sRGB(0.0)=0.0 → linear (0,1,0).
        u.textures.albedo = Some(tex(1, 1, vec![Color::new(0.0, 1.0, 0.0, 1.0)]));
        let m = u.sample_material([0.5, 0.5], Vector3::new(0.0, 0.0, 1.0));
        assert!(m.albedo.r < 1e-4 && (m.albedo.g - 1.0).abs() < 1e-4 && m.albedo.b < 1e-4);
    }

    #[test]
    fn sample_material_roughness_uses_green_metallic_uses_blue() {
        let mut u = default_uniforms();
        u.roughness = 1.0;
        u.metallic = 1.0;
        // roughness reads .g = 0.25, metallic reads .b = 0.5.
        u.textures.roughness = Some(tex(1, 1, vec![Color::new(0.9, 0.25, 0.9, 1.0)]));
        u.textures.metallic = Some(tex(1, 1, vec![Color::new(0.9, 0.9, 0.5, 1.0)]));
        let m = u.sample_material([0.5, 0.5], Vector3::new(0.0, 0.0, 1.0));
        assert!((m.roughness - 0.25).abs() < 1e-4, "roughness={}", m.roughness);
        assert!((m.metallic - 0.5).abs() < 1e-4, "metallic={}", m.metallic);
    }

    #[test]
    fn sample_material_emission_texture_modulates() {
        let mut u = default_uniforms();
        u.emission = Color::new(1.0, 1.0, 1.0, 1.0);
        u.textures.emission = Some(tex(1, 1, vec![Color::new(0.0, 0.0, 0.0, 1.0)]));
        let m = u.sample_material([0.5, 0.5], Vector3::new(0.0, 0.0, 1.0));
        assert!(m.emission.r < 1e-4 && m.emission.g < 1e-4 && m.emission.b < 1e-4);
    }

    #[test]
    fn sample_material_no_textures_returns_flat_values() {
        let u = default_uniforms();
        let n = Vector3::new(0.0, 1.0, 0.0);
        let m = u.sample_material([0.3, 0.7], n);
        assert_eq!(m.albedo, u.albedo);
        assert_eq!(m.emission, u.emission);
        assert!((m.roughness - u.roughness).abs() < 1e-6);
        assert!((m.metallic - u.metallic).abs() < 1e-6);
        // Normal passes through unchanged (normalized).
        assert!((m.normal.x - n.x).abs() < 1e-5 && (m.normal.y - n.y).abs() < 1e-5);
    }

    fn default_uniforms() -> ShaderUniforms {
        ShaderUniforms {
            model_matrix: identity_matrix(),
            view_matrix: identity_matrix(),
            projection_matrix: identity_matrix(),
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            ambient: Color::new(0.1, 0.1, 0.1, 1.0),
            roughness: 0.5,
            metallic: 0.0,
            camera_position: Vector3::ZERO,
            lights: vec![],
            directional_shadow_maps: vec![],
            omni_shadow_cubemaps: vec![],
            textures: MaterialTextures::default(),
        }
    }

    fn identity_matrix() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn standard_vertex_shader_identity_passthrough() {
        let vs = StandardVertexShader;
        let input = VertexInput {
            position: Vector3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            uv: [0.5, 0.5],
        };
        let uniforms = default_uniforms();
        let output = vs.process(&input, &uniforms);

        assert!((output.clip_position[0] - 1.0).abs() < 1e-5);
        assert!((output.clip_position[1] - 2.0).abs() < 1e-5);
        assert!((output.clip_position[2] - 3.0).abs() < 1e-5);
        assert!((output.clip_position[3] - 1.0).abs() < 1e-5);
        assert!((output.world_normal.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn unlit_fragment_returns_albedo() {
        let fs = UnlitFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(0.8, 0.2, 0.4, 1.0);
        let color = fs.process(&input, &uniforms);
        assert!((color.r - 0.8).abs() < 1e-5);
        assert!((color.g - 0.2).abs() < 1e-5);
        assert!((color.b - 0.4).abs() < 1e-5);
    }

    /// Fragment shaders output scene-referred linear HDR: bright emission
    /// must NOT be clamped to 1.0, or it can never cross the glow HDR
    /// threshold and bloom like Godot. (Tonemapping clamps at frame end.)
    #[test]
    fn unlit_emission_is_hdr_unclamped() {
        let fs = UnlitFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.emission = Color::new(2.0, 2.0, 2.0, 1.0);
        let color = fs.process(&input, &uniforms);
        assert!(
            (color.r - 3.0).abs() < 1e-5,
            "unlit emission must stay HDR (3.0), got {}",
            color.r
        );
    }

    #[test]
    fn lambert_with_no_lights_is_ambient_only() {
        let fs = LambertFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        let color = fs.process(&input, &uniforms);
        // Ambient = 0.1
        assert!((color.r - 0.1).abs() < 1e-5);
    }

    #[test]
    fn lambert_with_direct_light() {
        let fs = LambertFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.lights.push(LightUniform::directional(
            Vector3::UP, // Light pointing upward = facing the normal
            Color::new(1.0, 1.0, 1.0, 1.0),
        ));
        let color = fs.process(&input, &uniforms);
        // ambient(0.1) + diffuse(1.0 * 1.0 * 1.0) = 1.1 — scene-referred HDR,
        // NOT clamped to 1.0 (tonemapping clamps at the end of the frame).
        assert!((color.r - 1.1).abs() < 1e-5);
    }

    #[test]
    fn lambert_perpendicular_light_no_diffuse() {
        let fs = LambertFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.lights.push(LightUniform::directional(
            Vector3::new(1.0, 0.0, 0.0), // perpendicular
            Color::new(1.0, 1.0, 1.0, 1.0),
        ));
        let color = fs.process(&input, &uniforms);
        // Only ambient
        assert!((color.r - 0.1).abs() < 1e-5);
    }

    #[test]
    fn phong_specular_highlight() {
        let fs = PhongFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::new(0.0, 0.0, 0.0),
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.metallic = 1.0;
        uniforms.roughness = 0.05; // near-mirror GGX metal
        uniforms.camera_position = Vector3::new(0.0, 1.0, 0.0); // Above
        uniforms.lights.push(LightUniform::directional(
            Vector3::UP,
            Color::new(1.0, 1.0, 1.0, 1.0),
        ));
        let color = fs.process(&input, &uniforms);
        // Burley+GGX metal facing the light produces a strong specular lobe.
        assert!(color.r > 0.3, "PBR specular expected, got {}", color.r);
    }

    #[test]
    fn burley_ggx_rough_metal_darker_than_smooth() {
        let mut rough = default_uniforms();
        rough.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        rough.metallic = 1.0;
        rough.roughness = 0.9;
        rough.camera_position = Vector3::new(0.0, 1.0, 0.0);
        rough.lights.push(LightUniform::directional(
            Vector3::UP,
            Color::new(1.0, 1.0, 1.0, 1.0),
        ));
        let mut smooth = rough.clone();
        smooth.roughness = 0.05;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let cr = PhongFragmentShader.process(&input, &rough);
        let cs = PhongFragmentShader.process(&input, &smooth);
        assert!(
            cs.r > cr.r,
            "smooth metal should peak higher than rough: smooth={} rough={}",
            cs.r,
            cr.r
        );
    }

    #[test]
    fn d_ggx_peaks_at_normal_incidence() {
        let alpha = 0.25f32;
        assert!(d_ggx(1.0, alpha) > d_ggx(0.5, alpha));
    }

    #[test]
    fn vertex_output_lerp() {
        let a = VertexOutput {
            clip_position: [0.0, 0.0, 0.0, 1.0],
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
        };
        let b = VertexOutput {
            clip_position: [2.0, 4.0, 6.0, 1.0],
            world_position: Vector3::new(10.0, 20.0, 30.0),
            world_normal: Vector3::UP,
            uv: [1.0, 1.0],
        };
        let mid = VertexOutput::lerp(&a, &b, 0.5);
        assert!((mid.clip_position[0] - 1.0).abs() < 1e-5);
        assert!((mid.world_position.x - 5.0).abs() < 1e-5);
        assert!((mid.uv[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn vertex_output_barycentric() {
        let a = VertexOutput {
            clip_position: [1.0, 0.0, 0.0, 1.0],
            world_position: Vector3::new(1.0, 0.0, 0.0),
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
        };
        let b = VertexOutput {
            clip_position: [0.0, 1.0, 0.0, 1.0],
            world_position: Vector3::new(0.0, 1.0, 0.0),
            world_normal: Vector3::UP,
            uv: [1.0, 0.0],
        };
        let c = VertexOutput {
            clip_position: [0.0, 0.0, 1.0, 1.0],
            world_position: Vector3::new(0.0, 0.0, 1.0),
            world_normal: Vector3::UP,
            uv: [0.0, 1.0],
        };
        // Equal weights = centroid
        let center = VertexOutput::barycentric(&a, &b, &c, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
        assert!((center.world_position.x - 1.0 / 3.0).abs() < 1e-5);
        assert!((center.world_position.y - 1.0 / 3.0).abs() < 1e-5);
        assert!((center.world_position.z - 1.0 / 3.0).abs() < 1e-5);
    }

    #[test]
    fn fragment_shader_for_mode_returns_correct_type() {
        let _ = fragment_shader_for_mode(gdserver3d::material::ShadingMode::Unlit);
        let _ = fragment_shader_for_mode(gdserver3d::material::ShadingMode::Lambert);
        let _ = fragment_shader_for_mode(gdserver3d::material::ShadingMode::Phong);
    }

    #[test]
    fn emission_adds_to_output() {
        let fs = UnlitFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(0.5, 0.5, 0.5, 1.0);
        uniforms.emission = Color::new(0.3, 0.1, 0.0, 1.0);
        let color = fs.process(&input, &uniforms);
        assert!((color.r - 0.8).abs() < 1e-5);
        assert!((color.g - 0.6).abs() < 1e-5);
        assert!((color.b - 0.5).abs() < 1e-5);
    }

    // -- SpotLight3D shader tests -------------------------------------------

    fn make_spot_light(pos: Vector3, dir: Vector3, angle_rad: f32, range: f32) -> LightUniform {
        LightUniform {
            kind: LightKind::Spot,
            direction: dir,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            position: pos,
            range,
            attenuation: 1.0,
            spot_angle: angle_rad,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
        }
    }

    #[test]
    fn spot_light_center_of_cone_full_intensity() {
        // Fragment directly below a downward-pointing spot light.
        let light = make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
            20.0,
        );
        let frag_pos = Vector3::new(0.0, 0.0, 0.0);
        let (dir, intensity) = light.evaluate(frag_pos);
        // Direction should point up toward light.
        assert!(dir.y > 0.9);
        // Should have significant intensity (not zero).
        assert!(intensity > 0.5, "intensity = {intensity}");
    }

    #[test]
    fn spot_light_outside_cone_zero_intensity() {
        // Fragment far to the side of a narrow downward spot.
        let light = make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            0.1, // ~5.7 degree cone
            20.0,
        );
        let frag_pos = Vector3::new(10.0, 0.0, 0.0); // Way off to the side.
        let (_dir, intensity) = light.evaluate(frag_pos);
        assert!(intensity < 0.01, "intensity = {intensity}");
    }

    #[test]
    fn spot_light_beyond_range_zero_intensity() {
        let light = make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
            3.0, // Range 3, but fragment is 5 units away.
        );
        let frag_pos = Vector3::new(0.0, 0.0, 0.0);
        let (_dir, intensity) = light.evaluate(frag_pos);
        assert!(intensity < 0.01, "intensity = {intensity}");
    }

    #[test]
    fn spot_light_lambert_shading() {
        let fs = LambertFragmentShader;
        let input = FragmentInput {
            world_position: Vector3::new(0.0, 0.0, 0.0),
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.lights.push(make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
            20.0,
        ));
        let color = fs.process(&input, &uniforms);
        // Should be brighter than ambient (0.1).
        assert!(color.r > 0.2, "r = {}", color.r);
    }

    #[test]
    fn point_light_distance_attenuation() {
        let light = LightUniform {
            kind: LightKind::Point,
            direction: Vector3::ZERO,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            position: Vector3::new(0.0, 10.0, 0.0),
            range: 10.0,
            attenuation: 1.0,
            spot_angle: 0.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: false,
        };
        // Fragment right at the light position → full intensity.
        let (_dir, near_i) = light.evaluate(Vector3::new(0.0, 9.0, 0.0));
        assert!(near_i > 0.8, "near_i = {near_i}");

        // Fragment at max range → zero intensity.
        let (_dir, far_i) = light.evaluate(Vector3::new(0.0, 0.0, 0.0));
        assert!(far_i < 0.01, "far_i = {far_i}");
    }

    #[test]
    fn directional_light_evaluate_unchanged() {
        let light = LightUniform::directional(Vector3::UP, Color::new(1.0, 1.0, 1.0, 1.0));
        let (dir, intensity) = light.evaluate(Vector3::new(100.0, 200.0, 300.0));
        assert!((dir.y - 1.0).abs() < 1e-5);
        assert!((intensity - 1.0).abs() < 1e-5);
    }

    // ── CustomFragmentShader: real fragment-body evaluation ──

    /// Builds a `CustomFragmentShader` from an unshaded spatial shader whose
    /// `fragment()` body is `src`.
    fn custom_shader(src: &str) -> CustomFragmentShader {
        let shader = gdserver3d::Shader3D::new(gdserver3d::ShaderType3D::Spatial, src);
        let mut mat = gdserver3d::ShaderMaterial3D::new();
        mat.shader = Some(shader);
        CustomFragmentShader::from_material(&mat)
    }

    fn frag_at(uv: [f32; 2]) -> FragmentInput {
        FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv,
            depth: 0.5,
        }
    }

    #[test]
    fn custom_fragment_uv_gradient() {
        // ALBEDO = vec3(UV.x, UV.y, 0.0) — output varies across the surface.
        let fs = custom_shader(
            "shader_type spatial;\nrender_mode unshaded;\nvoid fragment() {\n    ALBEDO = vec3(UV.x, UV.y, 0.0);\n}",
        );
        let uniforms = default_uniforms();

        let a = fs.process(&frag_at([0.25, 0.75]), &uniforms);
        assert!((a.r - 0.25).abs() < 1e-5, "r={}", a.r);
        assert!((a.g - 0.75).abs() < 1e-5, "g={}", a.g);
        assert!(a.b.abs() < 1e-5, "b={}", a.b);

        // A different UV yields a different color (gradient varies with UV).
        let b = fs.process(&frag_at([0.9, 0.1]), &uniforms);
        assert!((b.r - 0.9).abs() < 1e-5, "r={}", b.r);
        assert!((b.g - 0.1).abs() < 1e-5, "g={}", b.g);
        assert!(b.r > a.r, "output must vary with UV");
    }

    #[test]
    fn custom_fragment_time_animates() {
        // ALPHA = sin(TIME) — output alpha animates as time advances.
        let mut fs = custom_shader(
            "shader_type spatial;\nrender_mode unshaded;\nvoid fragment() {\n    ALBEDO = vec3(0.5, 0.5, 0.5);\n    ALPHA = sin(TIME);\n}",
        );
        let uniforms = default_uniforms();

        fs.set_time(0.0);
        let at0 = fs.process(&frag_at([0.0, 0.0]), &uniforms);
        assert!(at0.a.abs() < 1e-5, "sin(0) alpha={}", at0.a);

        fs.set_time(std::f32::consts::FRAC_PI_2);
        let at_half_pi = fs.process(&frag_at([0.0, 0.0]), &uniforms);
        assert!(
            (at_half_pi.a - 1.0).abs() < 1e-5,
            "sin(pi/2) alpha={}",
            at_half_pi.a
        );
        assert!(at_half_pi.a > at0.a, "alpha must animate with TIME");
        // RGB is unaffected by the time change.
        assert!((at_half_pi.r - 0.5).abs() < 1e-5);
    }

    #[test]
    fn custom_fragment_texture_sample() {
        // ALBEDO = texture(tex, UV) — samples the bound solid-color texture.
        let mut fs = custom_shader(
            "shader_type spatial;\nrender_mode unshaded;\nuniform sampler2D tex;\nvoid fragment() {\n    ALBEDO = texture(tex, UV);\n}",
        );
        fs.set_texture("tex", tex(1, 1, vec![Color::new(0.2, 0.4, 0.6, 1.0)]));
        let uniforms = default_uniforms();

        let out = fs.process(&frag_at([0.3, 0.7]), &uniforms);
        assert!((out.r - 0.2).abs() < 1e-5, "r={}", out.r);
        assert!((out.g - 0.4).abs() < 1e-5, "g={}", out.g);
        assert!((out.b - 0.6).abs() < 1e-5, "b={}", out.b);
    }

    #[test]
    fn custom_fragment_missing_texture_is_white_placeholder() {
        // No texture bound for `tex` → neutral opaque-white placeholder.
        let fs = custom_shader(
            "shader_type spatial;\nrender_mode unshaded;\nuniform sampler2D tex;\nvoid fragment() {\n    ALBEDO = texture(tex, UV);\n}",
        );
        let uniforms = default_uniforms();
        let out = fs.process(&frag_at([0.3, 0.7]), &uniforms);
        assert!((out.r - 1.0).abs() < 1e-5);
        assert!((out.g - 1.0).abs() < 1e-5);
        assert!((out.b - 1.0).abs() < 1e-5);
    }

    #[test]
    fn custom_fragment_empty_body_falls_back_to_apply_shader() {
        // No fragment() body → fragment_body is empty → uniform-only path
        // (apply_shader) still applies albedo_color, so nothing regresses.
        let shader = gdserver3d::Shader3D::new(
            gdserver3d::ShaderType3D::Spatial,
            "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;",
        );
        let mut mat = gdserver3d::ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter(
            "albedo_color",
            gdvariant::Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let fs = CustomFragmentShader::from_material(&mat);
        let uniforms = default_uniforms();
        let out = fs.process(&frag_at([0.5, 0.5]), &uniforms);
        assert!((out.r - 1.0).abs() < 1e-5, "r={}", out.r);
        assert!(out.g.abs() < 1e-5, "g={}", out.g);
        assert!(out.b.abs() < 1e-5, "b={}", out.b);
    }
}
