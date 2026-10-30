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
    /// The reflection probe covering this draw, if any (pat-s3rrv.206).
    ///
    /// Selected per instance (a point query, matching how probe ambient was
    /// already resolved) but sampled per fragment, because a mirror reflects
    /// different things at different pixels only if the reflection direction
    /// varies across it. `None` leaves the specular term exactly as it was.
    pub probe: Option<gdserver3d::reflection_probe::ProbeReflection>,
    /// The render layers of the instance being shaded — Godot's
    /// `VisualInstance3D.layers`, 1-based, default 1.
    ///
    /// ⚠️ Per-DRAW state, not per-scene: it is the missing half of
    /// `light_cull_mask`. The shading loop previously had no idea which
    /// instance it was shading, so a light could not be skipped for a
    /// non-matching mesh however the mask was set.
    pub layers: u32,
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
    /// Godot `Light3D.shadow_blur`, in SHADOW-MAP TEXELS: the radius the PCF
    /// kernel spreads over.
    pub shadow_blur: f32,
    /// Tap count from Godot's `shadow_filter_quality`
    /// (`ShadowFilterQuality::taps()`): 1 / 4 / 8 / 16.
    ///
    /// ⚠️ **1 is the default and means no filtering at all** — the filtered
    /// sampler returns the plain sample bit-for-bit, so nothing softens until a
    /// scene asks for it.
    pub shadow_taps: u32,
    /// Godot `DirectionalLight3D.directional_shadow_max_distance` — how far
    /// from the camera this light's shadows are drawn. Inert for positional
    /// lights.
    pub directional_shadow_max_distance: f32,
    /// Godot `DirectionalLight3D.directional_shadow_fade_start`, as authored.
    /// The 0.999 clamp is applied at the point of use, not here.
    pub directional_shadow_fade_start: f32,
    /// Godot `Light3D.light_specular`, as authored — the type factor is applied
    /// at the point of use.
    ///
    /// ⚠️ Distinct from the MATERIAL's specular, which feeds the Fresnel F0.
    pub specular: f32,
    /// Godot `Light3D.shadow_normal_bias`, in SHADOW-MAP TEXELS.
    ///
    /// ⚠️ A different unit system from [`shadow_bias`](Self::shadow_bias) —
    /// see `Light3D::shadow_normal_bias`.
    pub shadow_normal_bias: f32,
    /// Godot `Light3D.shadow_bias`, in WORLD UNITS.
    ///
    /// ⚠️ World units, not the normalized epsilon the 2D depth compare wants —
    /// `ShadowMap::sample_with_bias` does that conversion. The omni path below
    /// compares a world-space DISTANCE, so it takes this value directly. Those
    /// two comparisons being in different spaces is why one authored property
    /// cannot simply replace both constants.
    pub shadow_bias: f32,
    /// Godot `Light3D.light_cull_mask`: which render layers this light lights.
    ///
    /// Intersected with [`ShaderUniforms::layers`] by
    /// `gdserver3d::light::cull_mask_matches_layers`, the same predicate
    /// `Light3D::affects_layers` uses — the test is not re-derived here.
    pub cull_mask: u32,
    /// Godot `Light3D.shadow_opacity`: how dark this light's shadow is.
    ///
    /// `1.0` is a fully opaque shadow (Godot's default) and `0.0` means the
    /// light casts no visible shadow at all. Scales the OCCLUSION, not the
    /// visibility — see [`compute_shadow_factor`].
    pub shadow_opacity: f32,
}

impl LightUniform {
    /// Whether this light lights a mesh whose render-layer MASK is `layers` —
    /// Godot's `light_cull_mask` test.
    ///
    /// 🔑 Both sides are bitmasks, so this is an intersection. Delegates to
    /// `gdserver3d::light::cull_mask_matches_layers`, the same function
    /// `Light3D::affects_layers` calls, so the two cannot drift.
    pub fn affects_layers(&self, layers: u32) -> bool {
        gdserver3d::light::cull_mask_matches_layers(self.cull_mask, layers)
    }

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
            shadow_blur: 1.0,
            shadow_taps: 1,
            directional_shadow_max_distance: 100.0,
            directional_shadow_fade_start: 0.8,
            specular: 1.0,
            shadow_normal_bias: 2.0,
            shadow_bias: 0.1,
            cull_mask: u32::MAX,
            shadow_opacity: 1.0,
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

/// World size of one directional shadow-map texel.
///
/// The orthographic frustum spans `[-SHADOW_EXTENT, SHADOW_EXTENT]` across
/// `SHADOW_MAP_SIZE` texels, so one texel covers `2 * EXTENT / SIZE` world
/// units. `shadow_normal_bias` is authored relative to this.
fn shadow_texel_size() -> f32 {
    2.0 * crate::shadow_map::SHADOW_EXTENT / crate::shadow_map::SHADOW_MAP_SIZE as f32
}

/// How much a directional light's shadow has faded out at a given VIEW-SPACE
/// depth: 0.0 = full shadow, 1.0 = shadow entirely gone.
///
/// Godot dissolves the last cascade instead of ending it on a hard edge
/// (`light_storage.cpp:753-754` + `scene_forward_clustered.glsl:2471`):
///
/// ```text
/// fade_to   = -max_distance
/// fade_from = -max_distance * min(fade_start, 0.999)
/// shadow    = mix(shadow, 1.0, smoothstep(fade_from, fade_to, view_z))
/// ```
///
/// ⚠️ `view_z` is NEGATIVE in front of the camera, which is why both edges are
/// negated and why `fade_from > fade_to`. The ratio still comes out right —
/// the interval is simply traversed downwards — but reading the formula as if
/// the edges were distances gets the direction backwards.
///
/// ⚠️ The `min(fade_start, 0.999)` is Godot's, and its own comment says why:
/// "using 1.0 would break smoothstep". At exactly 1.0 the two edges coincide
/// and the ratio divides by zero. The clamp lives here, at the point of use,
/// so the authored value still round-trips through the property.
///
/// ⚠️ The `max_distance <= 0` guard is OURS, not Godot's: a `.tscn` may author
/// zero, which collapses both edges to 0.0 and would otherwise put a NaN into
/// every fragment's shadow. Zero shadow distance is read as "nothing to fade".
fn directional_shadow_fade(view_z: f32, max_distance: f32, fade_start: f32) -> f32 {
    if max_distance <= 0.0 {
        return 0.0;
    }
    let fade_to = -max_distance;
    let fade_from = -max_distance * fade_start.min(0.999);
    let t = ((view_z - fade_from) / (fade_to - fade_from)).clamp(0.0, 1.0);
    // smoothstep's cubic, once the edges have been normalized away.
    t * t * (3.0 - 2.0 * t)
}

/// The vector added to a fragment's position before the shadow-map lookup
/// (Godot's normal-offset shadows).
///
/// Magnitude follows `light_storage.cpp`: the authored bias is in SHADOW-MAP
/// TEXELS, so it is scaled by the texel size, positional lights carry an extra
/// x10 (`:968`, which sits above Godot's spot/omni split and so covers both)
/// while directional does not (`:724`), and the `1 - |dot|` term grows the
/// offset as the surface turns edge-on to the light — where acne happens — and
/// vanishes where it faces the light squarely.
///
/// 🔑 **For a DIRECTIONAL light the result is then made perpendicular to the
/// light ray**, matching the directional `BIAS_FUNC`
/// (`scene_forward_clustered.glsl:2287-2290`, `normal_bias -= light_dir *
/// dot(light_dir, normal_bias)`). A normal offset is meant to slide the lookup
/// ACROSS the shadow map; any component along the ray changes the sampled depth
/// instead, which double-counts with `shadow_bias` and lifts the shadow off its
/// caster.
///
/// ⚠️ Positional lights are deliberately NOT projected: Godot's omni and spot
/// paths (`scene_forward_lights_inc.glsl:489`, `:780`) offset along the raw
/// normal. The asymmetry is upstream's, not an oversight here.
///
/// ⓘ The projection is a no-op at both extremes — edge-on, the normal is
/// already perpendicular to the ray, and face-on the `1 - |dot|` term has
/// already zeroed the offset — so it only bites at intermediate angles.
fn shadow_normal_offset(
    kind: LightKind,
    normal: Vector3,
    light_dir: Vector3,
    shadow_normal_bias: f32,
) -> Vector3 {
    let positional_scale = match kind {
        LightKind::Directional => 1.0,
        LightKind::Point | LightKind::Spot => 10.0,
    };
    let magnitude = shadow_normal_bias
        * shadow_texel_size()
        * positional_scale
        * (1.0 - normal.dot(light_dir).abs());
    let offset = normal * magnitude;
    match kind {
        LightKind::Directional => {
            // Normalize defensively: the projection is only exact for a unit
            // ray, and a zero-length direction must not produce NaNs.
            let len = light_dir.length();
            if len < 1e-6 {
                return offset;
            }
            let dir = light_dir * (1.0 / len);
            offset - dir * dir.dot(offset)
        }
        LightKind::Point | LightKind::Spot => offset,
    }
}

/// Computes the shadow factor for a light at the given fragment position.
///
/// Returns a value in `[0.0, 1.0]` where 1.0 = fully lit, 0.0 = fully shadowed.
fn compute_shadow_factor(
    light: &LightUniform,
    light_index: usize,
    frag_pos: Vector3,
    normal: Vector3,
    light_dir: Vector3,
    uniforms: &ShaderUniforms,
) -> f32 {
    if !light.shadow_enabled {
        return 1.0;
    }
    // Godot's normal-offset shadows: nudge the lookup position before
    // sampling. See `shadow_normal_offset` for the scaling rules and for
    // why the directional case is projected perpendicular to the ray.
    let normal_offset_vec =
        shadow_normal_offset(light.kind, normal, light_dir, light.shadow_normal_bias);
    // View-space depth of the TRUE fragment position, for the directional
    // shadow fade below. Taken before `frag_pos` is shadowed by the
    // normal-offset rebinding on the next line.
    let view_z = StandardVertexShader::mat4_mul_vec4(
        &uniforms.view_matrix,
        [frag_pos.x, frag_pos.y, frag_pos.z, 1.0],
    )[2];
    let frag_pos = frag_pos + normal_offset_vec;
    // Godot's `shadow_opacity` scales how dark the shadow is, so it multiplies
    // the OCCLUSION before it is turned into visibility: opacity 0 leaves the
    // fragment fully lit, opacity 1 is the old behaviour (pat-s3rrv.90).
    let opacity = light.shadow_opacity.clamp(0.0, 1.0);
    match light.kind {
        LightKind::Directional => uniforms
            .directional_shadow_maps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|sm| {
                // PCF: `shadow_taps` of 1 (Godot's `Hard` default) makes this
                // the exact single sample it replaced.
                let occlusion = sm.sample_with_bias_filtered(
                    frag_pos,
                    light.shadow_bias,
                    light.shadow_taps,
                    light.shadow_blur,
                );
                let shadow = 1.0 - occlusion * opacity;
                // Fade the shadow out as it approaches the light's shadow
                // distance. ⚠️ Measured from the UNOFFSET position: the normal
                // offset above is a shadow-map lookup nudge, not where the
                // fragment actually is, and using it here would make the fade
                // depend on the surface's angle to the light.
                let fade = directional_shadow_fade(
                    view_z,
                    light.directional_shadow_max_distance,
                    light.directional_shadow_fade_start,
                );
                shadow + (1.0 - shadow) * fade
            })
            .unwrap_or(1.0),
        LightKind::Point => uniforms
            .omni_shadow_cubemaps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|cm| {
                let to_frag = frag_pos - light.position;
                let dist = to_frag.length();
                let stored = cm.sample(to_frag);
                // ⚠️ Directly, with no conversion: this comparison is already
                // in WORLD units, unlike the 2D one above.
                if dist > stored + light.shadow_bias {
                    // The omni test is binary, so opacity is applied directly
                    // to the shadowed result rather than to a soft factor.
                    1.0 - opacity
                } else {
                    1.0 // lit
                }
            })
            .unwrap_or(1.0),
        LightKind::Spot => uniforms
            .directional_shadow_maps
            .get(light_index)
            .and_then(|s| s.as_ref())
            .map(|sm| 1.0 - sm.sample_with_bias(frag_pos, light.shadow_bias) * opacity)
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
            // Godot's `light_cull_mask`: a light lights this mesh only when the
            // mesh's render layer is in the mask. ⚠️ Applied in EVERY shading
            // loop in this file — a mask honoured in one path and ignored in
            // another is worse than ignoring it everywhere, because the mesh is
            // then lit or unlit depending on which shading mode it happens to
            // take.
            if !light.affects_layers(uniforms.layers) {
                continue;
            }
            let (light_dir, intensity) = light.evaluate(input.world_position);
            let shadow =
                compute_shadow_factor(light, i, input.world_position, normal, light_dir, uniforms);
            let n_dot_l = normal.dot(light_dir).max(0.0);
            let contrib = n_dot_l * intensity * shadow;
            diffuse_r += albedo.r * light.color.r * contrib;
            diffuse_g += albedo.g * light.color.g * contrib;
            diffuse_b += albedo.b * light.color.b * contrib;
        }

        // Image-based specular from a captured ReflectionProbe (pat-s3rrv.206).
        //
        // 🔑 `uniforms.probe` was read in exactly ONE place in the renderer —
        // inside `pbr_shade`, which only `PhongFragmentShader` reaches. But
        // `ShadingMode::default()` is `Lambert` and `StandardMaterial3D` passes
        // that straight through, so a `.tscn` mesh took THIS path and the probe
        // could not reach a single pixel however perfect the capture was. The
        // capture side and the sample side were each correct in isolation; the
        // consumer simply sat in a branch the mesh never entered.
        //
        // ⚠️ The `sharpness > 0.0` guard is what makes this invisible to
        // existing scenes, and it must stay. A DEFAULT material is
        // `roughness = 1.0`, so `sharpness` is exactly 0 and this adds exactly
        // nothing — every frame that does not author a shiny material is
        // byte-identical. Only a smooth surface gains the reflection, which is
        // the feature. Dropping the guard would nudge every rough surface in
        // the engine and turn a targeted fix into a wide, confusing diff.
        //
        // ⚠️ Deliberately DUPLICATED from `pbr_shade` rather than extracted
        // into a shared helper: this file's PBR path is currently green and
        // reachable from `PhongFragmentShader`, and an additive change here
        // cannot regress it, where a refactor of both could. If the two ever
        // need to change together, extract then — and keep the F0 and
        // `sharpness` terms identical to `pbr_shade`'s until someone does.
        if let Some(probe) = uniforms.probe.as_ref() {
            let n = normal;
            let v = (uniforms.camera_position - input.world_position).normalized();
            let n_dot_v = n.dot(v).max(1e-4);
            let sharpness = 1.0 - m.roughness.clamp(0.0, 1.0);
            if sharpness > 0.0 {
                // F0: dielectric 0.04, metals use albedo — the same split
                // `pbr_shade` makes.
                let f0_d = 0.04;
                let f0 = [
                    lerp_f(f0_d, albedo.r, m.metallic),
                    lerp_f(f0_d, albedo.g, m.metallic),
                    lerp_f(f0_d, albedo.b, m.metallic),
                ];
                let reflection = n * (2.0 * n_dot_v) - v;
                let env = probe.sample(input.world_position, reflection);

                // 🔑 Metals have no diffuse. At `metallic = 1.0` the surface
                // must show the reflection INSTEAD of a Lambert term, not on
                // top of one. Adding a red capture to a lit white plane gives
                // `(W + 1, W, W)` — pink, not red — so a mirror read as
                // "reflecting" while failing any strict red-dominance test.
                // `pbr_shade` avoids this because its diffuse is already
                // energy-split by metallic before the same `+=` runs;
                // duplicating that block without the split was the defect.
                //
                // ⚠️ Scoped INSIDE the probe/sharpness guard deliberately, so
                // the "invisible to existing frames" property is preserved: a
                // probe-less frame never reaches here, and neither does a
                // default `roughness = 1.0` material. A dielectric is
                // unaffected either way — `keep` is 1.0 at `metallic = 0.0`.
                let keep = 1.0 - m.metallic.clamp(0.0, 1.0) * sharpness;
                diffuse_r *= keep;
                diffuse_g *= keep;
                diffuse_b *= keep;

                diffuse_r += env.r * f0[0] * sharpness;
                diffuse_g += env.g * f0[1] * sharpness;
                diffuse_b += env.b * f0[2] * sharpness;
            }
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

    // Image-based specular from a captured ReflectionProbe (pat-s3rrv.206).
    //
    // Reflect the view vector about the shading normal and look the result up
    // in the probe's cubemap. This is what lets a probe MIRROR the scene, as
    // opposed to the flat `ambient_color` term that was the only thing a probe
    // could contribute before — `sample_probes_at` folds that in through
    // `uniforms.ambient` above, and it carries no direction at all.
    //
    // ⚠️ Scaled by F0 and faded out with roughness. A rough surface has no
    // single coherent mirror direction, so one cubemap texel is simply the
    // wrong answer for it; a prefiltered mip chain is what Godot uses and is
    // not built here. metallic = 1 / roughness = 0 — the mirror case the
    // acceptance exercises — takes the reflection at full strength.
    if let Some(probe) = uniforms.probe.as_ref() {
        let reflection = n * (2.0 * n_dot_v) - v;
        let sharpness = 1.0 - roughness.clamp(0.0, 1.0);
        if sharpness > 0.0 {
            let env = probe.sample(input.world_position, reflection);
            r += env.r * f0[0] * sharpness;
            g += env.g * f0[1] * sharpness;
            b += env.b * f0[2] * sharpness;
        }
    }

    for (i, light) in uniforms.lights.iter().enumerate() {
        // Godot's `light_cull_mask`: a light lights this mesh only when the
        // mesh's render layer is in the mask. ⚠️ Applied in EVERY shading
        // loop in this file — a mask honoured in one path and ignored in
        // another is worse than ignoring it everywhere, because the mesh is
        // then lit or unlit depending on which shading mode it happens to
        // take.
        if !light.affects_layers(uniforms.layers) {
            continue;
        }
        let (l, intensity) = light.evaluate(input.world_position);
        let shadow = compute_shadow_factor(light, i, input.world_position, n, l, uniforms);
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
        // Godot's `light_specular` scales the light's specular contribution.
        //
        // ⚠️ POSITIONAL lights are multiplied by 2.0 (`light_storage.cpp:891`)
        // while directional uses the value raw (`:665`). Combined with the
        // defaults — 1.0 base/directional, 0.5 omni/spot — every light type
        // comes out at an effective 1.0, so this is INVISIBLE at default values
        // and only bites when a value is authored. Wiring the 0.5 without the
        // x2 would halve every positional light's highlights.
        let specular_type_scale = match light.kind {
            LightKind::Directional => 1.0,
            LightKind::Point | LightKind::Spot => 2.0,
        };
        let spec = d * vis * n_dot_l * intensity * shadow * light.specular * specular_type_scale;
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
            // pat-s3rrv.122: carry the material's bound sampler images through.
            // This was `HashMap::new()` unconditionally, so `texture(tex, UV)`
            // always hit the missing-binding placeholder and custom-shader
            // texturing was reachable only from this file's own tests — the
            // engine had a sampler with nothing ever bound to it.
            textures: Arc::new(material.textures.clone()),
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
            // Godot's `light_cull_mask`: a light lights this mesh only when the
            // mesh's render layer is in the mask. ⚠️ Applied in EVERY shading
            // loop in this file — a mask honoured in one path and ignored in
            // another is worse than ignoring it everywhere, because the mesh is
            // then lit or unlit depending on which shading mode it happens to
            // take.
            if !light.affects_layers(uniforms.layers) {
                continue;
            }
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
            probe: None,
            lights: vec![],
            directional_shadow_maps: vec![],
            layers: 1,
            omni_shadow_cubemaps: vec![],
            textures: MaterialTextures::default(),
        }
    }

    /// 🔑 THE NORMAL OFFSET SCALES WITH `1 - |dot(normal, light_dir)|`, AND
    /// THAT TERM IS THE WHOLE POINT.
    ///
    /// Godot pushes the sample point along the surface normal by
    /// `shadow_normal_bias * texel_size * (1 - |dot(n, l)|)`
    /// (`scene_forward_lights_inc.glsl:489`). The offset grows as the surface
    /// turns EDGE-ON to the light — where acne actually happens — and vanishes
    /// where it faces the light squarely.
    ///
    /// ⚠️ Without that term it is just a second constant bias, which can only
    /// fix grazing acne by over-biasing everything and detaching contact
    /// shadows. So this pins the RATIO between two angles rather than an
    /// absolute: a face-on surface must be offset strictly less than a
    /// grazing one.
    #[test]
    fn the_normal_offset_grows_as_the_surface_turns_edge_on() {
        let texel = shadow_texel_size();
        assert!(
            (texel - 2.0 * 20.0 / 256.0).abs() < 1e-6,
            "texel size is 2*SHADOW_EXTENT/SHADOW_MAP_SIZE, got {texel}"
        );

        let bias = 1.0f32;
        let n = Vector3::UP;
        // Face-on: dot == 1, so the offset is zero.
        let face_on = bias * texel * (1.0 - n.dot(Vector3::UP).abs());
        // Grazing: dot == 0, so the offset is the full texel-scaled bias.
        let grazing = bias * texel * (1.0 - n.dot(Vector3::new(1.0, 0.0, 0.0)).abs());

        assert!(
            face_on.abs() < 1e-6,
            "a surface facing the light needs no normal offset, got {face_on}"
        );
        assert!(
            (grazing - texel).abs() < 1e-6,
            "a grazing surface gets the full texel-scaled bias, got {grazing}"
        );
        assert!(grazing > face_on, "the offset must grow toward edge-on");
    }

    /// ⚠️ POSITIONAL LIGHTS CARRY AN EXTRA ×10 ON THE NORMAL OFFSET, AND
    /// DIRECTIONAL DOES NOT.
    ///
    /// Godot applies `param * texel_size * 10.0` at `light_storage.cpp:968`,
    /// which sits ABOVE its `if (type == SPOT) … else //omni` split — so it
    /// covers spot and omni alike. Directional uses the unscaled form at `:724`.
    ///
    /// I shipped this without the ×10 first time and caught it re-reading the
    /// source: every positional light's normal bias was ten times too small,
    /// which reads as "normal bias does nothing" rather than as a wrong number.
    /// The two are asserted against EACH OTHER so the ratio is pinned even if
    /// the texel size changes.
    #[test]
    fn positional_lights_get_ten_times_the_normal_offset() {
        let texel = shadow_texel_size();
        let bias = 1.0f32;
        // Grazing incidence, so the (1 - |dot|) term is 1 and only the type
        // scale distinguishes the two.
        let grazing = 1.0f32;

        let directional = bias * texel * 1.0 * grazing;
        let positional = bias * texel * 10.0 * grazing;

        assert!(
            (positional / directional - 10.0).abs() < 1e-4,
            "a positional light's normal offset is 10x a directional's, got {}",
            positional / directional
        );
        assert!(
            (directional - texel).abs() < 1e-6,
            "a directional light's offset is the bare texel-scaled bias"
        );
    }

    /// 🔑 A DIRECTIONAL light's normal offset must be PERPENDICULAR to the
    /// light ray.
    ///
    /// Godot's directional `BIAS_FUNC` strips the along-ray component
    /// (`scene_forward_clustered.glsl:2287-2290`). The offset exists to slide
    /// the lookup ACROSS the shadow map; a component along the ray changes the
    /// sampled depth instead, double-counting with `shadow_bias` and lifting
    /// the shadow off its caster.
    ///
    /// ⚠️ The fixture angle matters. At grazing incidence the normal is already
    /// perpendicular to the ray, and face-on the `1 - |dot|` term has already
    /// zeroed the offset — at BOTH extremes the projection is a no-op and this
    /// test would pass without it. 45 degrees is where the two versions differ,
    /// so the assertion below is paired with a check that the unprojected
    /// offset really would have failed.
    #[test]
    fn a_directional_normal_offset_is_perpendicular_to_the_light_ray() {
        let n = Vector3::UP;
        let inv = 1.0 / 2.0f32.sqrt();
        let light_dir = Vector3::new(inv, inv, 0.0); // 45 degrees to the normal

        let offset = shadow_normal_offset(LightKind::Directional, n, light_dir, 1.0);
        assert!(
            offset.dot(light_dir).abs() < 1e-6,
            "offset must have no component along the ray, got {}",
            offset.dot(light_dir)
        );

        // The fixture is discriminating: without the projection the offset is
        // `n * magnitude`, which at this angle is well off perpendicular.
        let magnitude = shadow_texel_size() * (1.0 - n.dot(light_dir).abs());
        let unprojected = n * magnitude;
        assert!(
            unprojected.dot(light_dir).abs() > 1e-4,
            "fixture is not discriminating: the unprojected offset is already \
             perpendicular"
        );
        // And the projection only removes that component — it does not flip or
        // rescale the rest, so the result still points along the normal side.
        assert!(offset.dot(n) > 0.0, "the offset must still push outward");
    }

    /// ⚠️ POSITIONAL lights are deliberately NOT projected.
    ///
    /// Godot's omni and spot paths offset along the raw normal
    /// (`scene_forward_lights_inc.glsl:489`, `:780`) with no projection step.
    /// The asymmetry with directional is upstream's; this pins it so the
    /// projection is not "helpfully" generalised to every light later.
    #[test]
    fn a_positional_normal_offset_keeps_its_along_ray_component() {
        let n = Vector3::UP;
        let inv = 1.0 / 2.0f32.sqrt();
        let light_dir = Vector3::new(inv, inv, 0.0);

        for kind in [LightKind::Point, LightKind::Spot] {
            let offset = shadow_normal_offset(kind, n, light_dir, 1.0);
            assert!(
                offset.dot(light_dir).abs() > 1e-6,
                "{kind:?} must not be projected perpendicular to the ray"
            );
            // It is exactly the raw normal times the x10-scaled magnitude.
            let expected = n * (shadow_texel_size() * 10.0 * (1.0 - n.dot(light_dir).abs()));
            assert!((offset - expected).length() < 1e-6);
        }
    }

    /// The projection is a no-op at both extremes, which is what makes it safe
    /// to add: it changes nothing where the offset was already correct.
    #[test]
    fn the_normal_offset_ray_projection_changes_nothing_at_extremes() {
        let n = Vector3::UP;

        // Face-on: the `1 - |dot|` term zeroes the offset entirely.
        let face_on = shadow_normal_offset(LightKind::Directional, n, Vector3::UP, 1.0);
        assert!(face_on.length() < 1e-6, "got {face_on:?}");

        // Grazing: the normal is already perpendicular, so the full
        // texel-scaled offset survives the projection intact.
        let grazing =
            shadow_normal_offset(LightKind::Directional, n, Vector3::new(1.0, 0.0, 0.0), 1.0);
        assert!(
            (grazing.length() - shadow_texel_size()).abs() < 1e-6,
            "grazing keeps the full texel-scaled bias, got {}",
            grazing.length()
        );
    }

    /// The x10 positional split, asserted through the production function
    /// rather than by re-deriving the arithmetic in the test.
    #[test]
    fn the_positional_normal_offset_ten_times_scale_holds_via_the_helper() {
        let n = Vector3::UP;
        let grazing_dir = Vector3::new(1.0, 0.0, 0.0);
        let directional =
            shadow_normal_offset(LightKind::Directional, n, grazing_dir, 1.0).length();
        let positional = shadow_normal_offset(LightKind::Point, n, grazing_dir, 1.0).length();
        assert!(
            (positional / directional - 10.0).abs() < 1e-4,
            "got {}",
            positional / directional
        );
    }

    /// The per-class defaults, which are the opposite shape to `shadow_bias`.
    ///
    /// ⚠️ `shadow_normal_bias` is **2.0** on the base `Light3D` (inherited by
    /// `DirectionalLight3D`) and **1.0** on both omni and spot — whereas
    /// `shadow_bias` is 0.1 on base AND omni with only spot overriding to 0.03.
    /// Copying one's shape onto the other gets two of the six values wrong.
    #[test]
    fn the_two_bias_defaults_have_different_per_class_shapes() {
        use gdserver3d::light::{Light3D as SceneLight, Light3DId};
        let dir = SceneLight::directional(Light3DId(1));
        let omni = SceneLight::point(Light3DId(2), Vector3::ZERO);
        let spot = SceneLight::spot(Light3DId(3), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));

        assert_eq!((dir.shadow_normal_bias, dir.shadow_bias), (2.0, 0.1));
        assert_eq!((omni.shadow_normal_bias, omni.shadow_bias), (1.0, 0.1));
        assert_eq!((spot.shadow_normal_bias, spot.shadow_bias), (1.0, 0.03));
    }

    /// 🔑 `shadow_opacity` lightens the shadow rather than switching it off
    /// (pat-s3rrv.90).
    ///
    /// ⚠️ Three points, not one. A single "opacity 0.5 is brighter than 1.0"
    /// assertion is also satisfied by an implementation that treats any opacity
    /// below 1 as "no shadow", so the 0.0 endpoint pins the far end and the 1.0
    /// endpoint pins that the old fully-opaque behaviour is unchanged — which
    /// matters because 1.0 is Godot's default and every existing shadow test
    /// renders through this path.
    #[test]
    fn shadow_opacity_scales_how_dark_the_shadow_is() {
        use crate::shadow_map::generate_shadow_maps;
        use gdcore::math3d::{Basis, Transform3D};
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::{Light3D as CasterLight, Light3DId};
        use gdserver3d::mesh::Mesh3D;

        let mut caster = CasterLight::directional(Light3DId(1));
        caster.shadow_enabled = true;
        caster.direction = Vector3::new(0.0, -1.0, 0.0);

        // An occluder above the origin, so the sampled point is in its shadow.
        let mut occluder = Instance3D::new(Instance3DId(1));
        occluder.mesh = Some(Mesh3D::cube(4.0));
        occluder.visible = true;
        occluder.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        let map = generate_shadow_maps(&[caster], &[occluder])
            .into_iter()
            .next()
            .expect("a shadow-enabled directional light produces one map");
        let mut uniforms = default_uniforms();
        uniforms.directional_shadow_maps = vec![Some(Arc::new(map))];

        let mut light =
            LightUniform::directional(Vector3::new(0.0, -1.0, 0.0), Color::new(1.0, 1.0, 1.0, 1.0));
        light.shadow_enabled = true;

        light.shadow_opacity = 1.0;
        let opaque = compute_shadow_factor(
            &light,
            0,
            Vector3::ZERO,
            Vector3::UP,
            light.direction,
            &uniforms,
        );
        light.shadow_opacity = 0.5;
        let half = compute_shadow_factor(
            &light,
            0,
            Vector3::ZERO,
            Vector3::UP,
            light.direction,
            &uniforms,
        );
        light.shadow_opacity = 0.0;
        let off = compute_shadow_factor(
            &light,
            0,
            Vector3::ZERO,
            Vector3::UP,
            light.direction,
            &uniforms,
        );

        assert!(
            opaque < 0.5,
            "the point must actually be in shadow at full opacity, or this test \
             compares three lit values and proves nothing; got {opaque}",
        );
        assert!(
            half > opaque,
            "halving the opacity must lighten the shadow: got {half} at 0.5 vs \
             {opaque} at 1.0",
        );
        assert!(
            (off - 1.0).abs() < 1e-6,
            "opacity 0 means the light casts no visible shadow at all, so the \
             fragment is fully lit; got {off}",
        );
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
            shadow_blur: 1.0,
            shadow_taps: 1,
            directional_shadow_max_distance: 100.0,
            directional_shadow_fade_start: 0.8,
            specular: 0.5,
            shadow_normal_bias: 2.0,
            shadow_bias: 0.1,
            cull_mask: u32::MAX,
            shadow_opacity: 1.0,
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
            shadow_blur: 1.0,
            shadow_taps: 1,
            directional_shadow_max_distance: 100.0,
            directional_shadow_fade_start: 0.8,
            specular: 0.5,
            shadow_normal_bias: 2.0,
            shadow_bias: 0.1,
            cull_mask: u32::MAX,
            shadow_opacity: 1.0,
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
    // -- pat-s3rrv.90: light_cull_mask ---------------------------------------

    /// A white directional light and a fragment facing straight at it — so any
    /// brightness in the result came from the light.
    ///
    /// ⚠️ `LightUniform.direction` points **toward the light source**, not the
    /// way the light travels: `evaluate` hands it straight to
    /// `normal.dot(light_dir)`. Seeding it with the travel direction `(0,-1,0)`
    /// gives `n · l == -1`, clamps to 0, and makes the fragment black for a
    /// reason that has nothing to do with the cull mask — every "must be dark"
    /// assertion below would then pass vacuously.
    fn lit_setup() -> (FragmentInput, ShaderUniforms, LightUniform) {
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.ambient = Color::new(0.0, 0.0, 0.0, 1.0);
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        let light = LightUniform::directional(Vector3::UP, Color::new(1.0, 1.0, 1.0, 1.0));
        (input, uniforms, light)
    }

    /// 🔑 A LIGHT WHOSE `light_cull_mask` EXCLUDES THE MESH'S LAYER DOES NOT
    /// LIGHT IT — and one that includes it still does.
    ///
    /// ⚠️ Both halves in one test on purpose. "The mesh is dark" alone is also
    /// satisfied by a mask check that rejects EVERYTHING, which would black out
    /// every scene in the project; the second half is what separates the fix
    /// from that.
    ///
    /// Ambient is zeroed so the only brightness available comes from the light.
    #[test]
    fn a_light_skips_a_mesh_outside_its_cull_mask() {
        let (input, mut uniforms, mut light) = lit_setup();
        uniforms.layers = 1 << 1; // the mesh's layer MASK: layer 2

        // Mask covering layer 1 only: this light must not touch a layer-2 mesh.
        light.cull_mask = 1;
        uniforms.lights = vec![light];
        let dark = LambertFragmentShader.process(&input, &uniforms);

        // The same light with layer 2 in its mask.
        uniforms.lights[0].cull_mask = 1 << 1;
        let lit = LambertFragmentShader.process(&input, &uniforms);

        assert!(
            dark.r < 1e-5,
            "a light whose mask excludes layer 2 must contribute nothing, got {}",
            dark.r
        );
        assert!(
            lit.r > 0.5,
            "the SAME light with layer 2 in its mask must light the mesh, got {}",
            lit.r
        );
    }

    /// ⚠️ EVERY shading path applies the mask, not just one.
    ///
    /// Lambert, Phong and the unshaded-with-lights path each have their own
    /// light loop. A mask honoured in one and ignored in another is worse than
    /// ignoring it everywhere: the mesh would be lit or unlit depending on which
    /// shading mode its material happened to select, which reads as a material
    /// bug rather than a masking one.
    #[test]
    fn the_cull_mask_is_applied_in_every_shading_path() {
        let (input, mut uniforms, mut light) = lit_setup();
        uniforms.layers = 1 << 1;
        light.cull_mask = 1; // layer 1 only — excludes this mesh
        uniforms.lights = vec![light];

        let lambert = LambertFragmentShader.process(&input, &uniforms);
        let phong = PhongFragmentShader.process(&input, &uniforms);
        assert!(
            lambert.r < 1e-5,
            "Lambert must skip the light, got {}",
            lambert.r
        );
        assert!(phong.r < 1e-5, "Phong must skip the light, got {}", phong.r);
    }

    /// 🔑 THE DEFAULTS MUST STAY PERMISSIVE.
    ///
    /// Godot's defaults are `layers = 1` and `light_cull_mask = all ones`, so an
    /// untouched scene is lit exactly as before. This is the regression guard
    /// for the whole change: an off-by-one in the bit shift, or a default of 0
    /// on either side, blacks out every mesh in the project — and every other
    /// test here would still pass, because they all set the mask explicitly.
    #[test]
    fn default_layers_and_cull_mask_still_light_the_mesh() {
        let (input, mut uniforms, light) = lit_setup();
        assert_eq!(uniforms.layers, 1, "Godot's VisualInstance3D default");
        assert_eq!(light.cull_mask, u32::MAX, "Godot's Light3D default");
        uniforms.lights = vec![light];

        let lit = LambertFragmentShader.process(&input, &uniforms);
        assert!(
            lit.r > 0.5,
            "an untouched scene must be lit exactly as before, got {}",
            lit.r
        );
    }

    /// The renderer's light uniform and the scene light agree on the predicate.
    ///
    /// ⚠️ They call one shared function rather than each shifting their own
    /// bit; this pins that, because an off-by-one in a second copy would light
    /// the NEIGHBOURING layer — visible only in scenes that use more than one.
    #[test]
    fn the_uniform_and_the_scene_light_share_one_cull_mask_predicate() {
        use gdserver3d::light::{Light3D as SceneLight, Light3DId};
        let mut scene = SceneLight::directional(Light3DId(1));
        let (_, _, mut uniform) = lit_setup();

        for mask in [0u32, 1, 0b1010, u32::MAX] {
            scene.cull_mask = mask;
            uniform.cull_mask = mask;
            for layers in [0u32, 1, 0b10, 0b101, 1 << 31] {
                assert_eq!(
                    uniform.affects_layers(layers),
                    scene.affects_layers(layers),
                    "mask {mask:#b} layers {layers:#b}"
                );
            }
        }
    }

    /// The THIRD light loop — `CustomFragmentShader`, the `ShaderMaterial3D`
    /// bridge — honours the mask too.
    ///
    /// ⚠️ Recorded as a coverage gap when the mask landed: Lambert and Phong
    /// were tested, this one was not, because it needs a compiled shader
    /// material to construct. It does not: `custom_shader()` already exists in
    /// this module. The gap was cheaper to close than to keep describing.
    ///
    /// The shader deliberately omits `render_mode unshaded` — with it, `process`
    /// returns the raw colour before the light loop and the test would pass
    /// without exercising anything.
    ///
    /// This path applies a hardcoded `ambient = 0.15`, so a fully masked light
    /// leaves exactly `albedo * 0.15` rather than black; that is what makes the
    /// two numbers below exact rather than approximate.
    #[test]
    fn the_cull_mask_is_applied_in_the_custom_shader_path() {
        let fs = custom_shader(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = vec3(1.0, 1.0, 1.0);\n}",
        );
        let (input, mut uniforms, mut light) = lit_setup();
        uniforms.layers = 1 << 1; // the mesh is on render layer 2

        light.cull_mask = 1; // layer 1 only — excludes this mesh
        uniforms.lights = vec![light];
        let masked = fs.process(&input, &uniforms);
        assert!(
            (masked.r - 0.15).abs() < 1e-4,
            "a masked light must leave only the ambient term (0.15), got {}",
            masked.r
        );

        uniforms.lights[0].cull_mask = 1 << 1; // now it covers layer 2
        let lit = fs.process(&input, &uniforms);
        assert!(
            lit.r > 0.9,
            "the SAME light with layer 2 in its mask must light the mesh, got {}",
            lit.r
        );
    }

    /// 🔑 `layers` IS A BITMASK, NOT A LAYER INDEX — and mistaking the two is
    /// invisible in simple scenes.
    ///
    /// A mesh on layers 1 and 3 has `layers == 0b101`. Read as an INDEX that is
    /// "layer 5", so a light masked to layer 5 would light it and a light masked
    /// to layers 1+3 would not — both exactly backwards. ⚠️ The confusion agrees
    /// with the right answer for every single-layer mask, including the default
    /// `1` that almost every mesh has, so nothing else in this file can catch it.
    #[test]
    fn layers_is_a_mask_not_an_index() {
        let (input, mut uniforms, mut light) = lit_setup();
        uniforms.layers = 0b101; // layers 1 and 3

        // A light restricted to layer 3 alone must light it...
        light.cull_mask = 1 << 2;
        uniforms.lights = vec![light];
        let on_layer_3 = LambertFragmentShader.process(&input, &uniforms);
        assert!(
            on_layer_3.r > 0.5,
            "a layer-3 light must light a mesh whose mask includes layer 3, got {}",
            on_layer_3.r
        );

        // ...and one restricted to layer 5 must not — which is precisely what an
        // index reading of `0b101` would have selected.
        uniforms.lights[0].cull_mask = 1 << 4;
        let on_layer_5 = LambertFragmentShader.process(&input, &uniforms);
        assert!(
            on_layer_5.r < 1e-5,
            "a layer-5 light must not touch this mesh; an index reading of 0b101 \
             would have lit it, got {}",
            on_layer_5.r
        );
    }

    // -- Light3D.light_specular ---------------------------------------------

    /// Geometry shared by the `light_specular` tests: a flat +Y surface at the
    /// origin, lit from straight above, viewed off-axis.
    ///
    /// ⚠️ `default_uniforms` puts the camera at the ORIGIN, which is also the
    /// fragment position — the view vector is then a normalized zero and
    /// `n_dot_v` falls back to its 1e-4 clamp. That is a degenerate specular
    /// setup, so these tests move the camera. Ambient is zeroed so the only
    /// brightness is the light's.
    fn specular_setup() -> (FragmentInput, ShaderUniforms) {
        let input = FragmentInput {
            world_position: Vector3::ZERO,
            world_normal: Vector3::UP,
            uv: [0.0, 0.0],
            depth: 0.5,
        };
        let mut uniforms = default_uniforms();
        uniforms.ambient = Color::new(0.0, 0.0, 0.0, 1.0);
        uniforms.albedo = Color::new(1.0, 1.0, 1.0, 1.0);
        uniforms.metallic = 0.0; // keep the diffuse lobe alive to compare against
        uniforms.roughness = 0.3;
        uniforms.camera_position = Vector3::new(1.0, 3.0, 0.0);
        (input, uniforms)
    }

    /// Shade with one light and return the red channel.
    fn shade_with(input: &FragmentInput, uniforms: &ShaderUniforms, light: LightUniform) -> f32 {
        let mut u = uniforms.clone();
        u.lights = vec![light];
        pbr_shade(input, &u).r
    }

    /// 🔑 `light_specular` SCALES THE HIGHLIGHT ONLY — the diffuse lobe is
    /// untouched, and the scaling is a plain linear multiplier.
    ///
    /// ⚠️ The "specular 0 is dimmer" half alone is also satisfied by a bug that
    /// zeroes the whole light, which would be far worse than ignoring the
    /// property. The surviving-diffuse assertion is what separates the two, and
    /// the linearity check pins it as a multiplier rather than an on/off switch.
    #[test]
    fn light_specular_scales_the_highlight_and_leaves_diffuse_alone() {
        let (input, uniforms) = specular_setup();
        let mut light = LightUniform::directional(Vector3::UP, Color::new(1.0, 1.0, 1.0, 1.0));

        light.specular = 1.0;
        let full = shade_with(&input, &uniforms, light);
        light.specular = 0.5;
        let half = shade_with(&input, &uniforms, light);
        light.specular = 0.0;
        let none = shade_with(&input, &uniforms, light);

        assert!(
            none < full - 1e-4,
            "light_specular must reach pixels: specular=0 gave {none}, specular=1 gave {full}"
        );
        assert!(
            none > 0.05,
            "specular=0 must keep the DIFFUSE lobe — the light is not switched \
             off, only its highlight. got {none}"
        );
        // Linear multiplier: the specular part at 0.5 is half the part at 1.0.
        let part_half = half - none;
        let part_full = full - none;
        assert!(
            (part_half - 0.5 * part_full).abs() < 1e-4 * part_full.max(1.0),
            "specular must scale linearly: part(0.5)={part_half}, part(1.0)={part_full}"
        );
    }

    /// 🔑 EVERY LIGHT TYPE COMES OUT AT AN EFFECTIVE SPECULAR OF 1.0 AT ITS
    /// CLASS DEFAULT — which is the whole reason wiring this property moves no
    /// existing baseline.
    ///
    /// Godot stores `light_specular` raw for directional lights
    /// (`light_storage.cpp:665`) but doubles it for positional ones (`:891`),
    /// and pairs that with defaults of 1.0 on `Light3D`/`DirectionalLight3D`
    /// against 0.5 on `OmniLight3D`/`SpotLight3D`. 1.0x1 == 0.5x2.
    ///
    /// ⚠️ Compared as a SPECULAR-TO-DIFFUSE RATIO, not as an absolute: a
    /// positional light is attenuated by distance and cone falloff, and that
    /// factor multiplies the diffuse and specular terms alike, so it cancels in
    /// the ratio. Comparing raw colours would just measure attenuation.
    ///
    /// ⚠️ Second half is the positive control. "All three agree" is also true
    /// of a build where `light_specular` is ignored entirely — the doubled
    /// positional light is what proves the x2 is really there.
    #[test]
    fn every_light_type_has_an_effective_specular_of_one_at_its_class_default() {
        let (input, uniforms) = specular_setup();

        // Specular-to-diffuse ratio for one light, isolating the specular part
        // by re-shading the same light with its specular zeroed.
        let ratio = |mut light: LightUniform| -> f32 {
            let authored = light.specular;
            light.specular = 0.0;
            let diffuse_only = shade_with(&input, &uniforms, light);
            light.specular = authored;
            let total = shade_with(&input, &uniforms, light);
            assert!(
                diffuse_only > 1e-3,
                "fixture must produce a diffuse lobe to divide by, got {diffuse_only}"
            );
            (total - diffuse_only) / diffuse_only
        };

        // All three sit directly above the fragment, so the light direction —
        // and with it the half-vector the GGX lobe depends on — is +Y for each.
        let directional = {
            let mut l = LightUniform::directional(Vector3::UP, Color::new(1.0, 1.0, 1.0, 1.0));
            l.specular = 1.0; // Light3D / DirectionalLight3D default
            l
        };
        let mut point = make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
            100.0,
        );
        point.kind = LightKind::Point;
        point.specular = 0.5; // OmniLight3D default
        let mut spot = make_spot_light(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            std::f32::consts::FRAC_PI_4,
            100.0,
        );
        spot.specular = 0.5; // SpotLight3D default

        let r_dir = ratio(directional);
        let r_point = ratio(point);
        let r_spot = ratio(spot);

        assert!(
            r_dir > 1e-3,
            "fixture must have a highlight to compare, got {r_dir}"
        );
        assert!(
            (r_point - r_dir).abs() < 1e-3 * r_dir,
            "OmniLight3D's 0.5 default x2 must equal the directional 1.0: \
             directional={r_dir}, point={r_point}"
        );
        assert!(
            (r_spot - r_dir).abs() < 1e-3 * r_dir,
            "SpotLight3D's 0.5 default x2 must equal the directional 1.0: \
             directional={r_dir}, spot={r_spot}"
        );

        // Positive control: authoring 1.0 on a positional light really does
        // double it, so the agreement above is the x2 cancelling the 0.5 and
        // not the property being ignored.
        let mut loud = point;
        loud.specular = 1.0;
        let r_loud = ratio(loud);
        assert!(
            (r_loud - 2.0 * r_dir).abs() < 1e-3 * r_dir,
            "a positional light authored at 1.0 must be twice the directional \
             1.0 (Godot's x2): got {r_loud}, expected {}",
            2.0 * r_dir
        );
    }

    // -- DirectionalLight3D.directional_shadow_fade_start -------------------

    /// 🔑 THE FADE IS ZERO UNTIL `fade_start`, ONE AT `max_distance`, AND A
    /// SMOOTHSTEP BETWEEN — measured in NEGATIVE view-space depth.
    ///
    /// ⚠️ `view_z` is negative in front of the camera, so `fade_from` (-80 at
    /// the defaults) is GREATER than `fade_to` (-100) and the interval is
    /// walked downwards. Reading the edges as positive distances gets the
    /// direction backwards and the fade inverts — the near geometry would lose
    /// its shadows instead of the far.
    #[test]
    fn directional_shadow_fade_follows_godots_smoothstep_between_the_two_edges() {
        let (max, start) = (100.0, 0.8);

        // In front of fade_start: no fade at all.
        assert_eq!(
            directional_shadow_fade(0.0, max, start),
            0.0,
            "at the camera"
        );
        assert_eq!(
            directional_shadow_fade(-50.0, max, start),
            0.0,
            "well inside"
        );
        assert_eq!(
            directional_shadow_fade(-80.0, max, start),
            0.0,
            "exactly at fade_start"
        );

        // Beyond max_distance: fully faded, and it stays clamped.
        assert_eq!(
            directional_shadow_fade(-100.0, max, start),
            1.0,
            "at max_distance"
        );
        assert_eq!(
            directional_shadow_fade(-500.0, max, start),
            1.0,
            "past max_distance"
        );

        // Halfway between the edges is smoothstep(0.5) = 0.5, and the curve
        // is an S rather than a ramp: at the QUARTER point the cubic gives
        // 0.15625 where a linear blend would give 0.25. Checking only the
        // midpoint would not tell the two apart — smoothstep and lerp agree
        // exactly at 0.5.
        let mid = directional_shadow_fade(-90.0, max, start);
        assert!(
            (mid - 0.5).abs() < 1e-6,
            "midpoint should be 0.5, got {mid}"
        );
        let quarter = directional_shadow_fade(-85.0, max, start);
        let expected = 0.25f32 * 0.25 * (3.0 - 2.0 * 0.25);
        assert!(
            (quarter - expected).abs() < 1e-6,
            "quarter point should be the smoothstep cubic {expected}, got {quarter} \
             — a linear ramp would give 0.25"
        );

        // Monotonic across the band, which a sign error would break.
        let mut prev = 0.0;
        for step in 0..=20 {
            let z = -80.0 - step as f32;
            let f = directional_shadow_fade(z, max, start);
            assert!(f >= prev, "fade must not decrease with distance at z={z}");
            prev = f;
        }
    }

    /// 🔑 `fade_start = 1.0` MUST NOT DIVIDE BY ZERO.
    ///
    /// Godot clamps to 0.999 with the comment "using 1.0 would break
    /// smoothstep" (`light_storage.cpp:753`) — at exactly 1.0 both edges land
    /// on `-max_distance` and the ratio's denominator is zero.
    ///
    /// ⚠️ The authored 1.0 must still round-trip on the property; the clamp
    /// belongs at the point of use. Asserted here rather than on the setter.
    #[test]
    fn directional_shadow_fade_start_of_one_stays_finite() {
        for z in [-50.0, -99.0, -99.95, -100.0, -200.0] {
            let f = directional_shadow_fade(z, 100.0, 1.0);
            assert!(f.is_finite(), "fade at z={z} must be finite, got {f}");
            assert!((0.0..=1.0).contains(&f), "fade at z={z} out of range: {f}");
        }
        // The band is squeezed into the last 0.1 units, so a fragment just
        // inside max_distance is still (almost) fully shadowed...
        assert!(directional_shadow_fade(-99.0, 100.0, 1.0) < 1e-6);
        // ...and one at max_distance is fully faded. Without the clamp this
        // pair is NaN and neither assertion holds.
        assert_eq!(directional_shadow_fade(-100.0, 100.0, 1.0), 1.0);
    }

    /// 🔑 A ZERO SHADOW DISTANCE FADES NOTHING RATHER THAN NaN-ING EVERY PIXEL.
    ///
    /// ⚠️ This guard is OURS, not Godot's. A `.tscn` may author
    /// `directional_shadow_max_distance = 0`, which collapses both edges onto
    /// 0.0; the unguarded ratio is 0/0 and a NaN fade poisons the shadow of
    /// every fragment, not just the distant ones.
    #[test]
    fn a_zero_shadow_distance_fades_nothing_instead_of_nan() {
        for z in [0.0, -1.0, -100.0] {
            let f = directional_shadow_fade(z, 0.0, 0.8);
            assert!(f.is_finite() && f == 0.0, "z={z} should not fade, got {f}");
        }
        // Positive control: a real distance at the same depth DOES fade, so the
        // zero case above is the guard firing and not the fade being inert.
        assert_eq!(directional_shadow_fade(-100.0, 100.0, 0.8), 1.0);
    }

    /// 🔑 A DISTANT FRAGMENT LOSES ITS SHADOW — AT THE SAME WORLD POSITION.
    ///
    /// ⚠️ Only the VIEW MATRIX moves between the two halves. Pushing the
    /// fragment itself away would take it outside the shadow map's
    /// `[-SHADOW_EXTENT, SHADOW_EXTENT]` frustum, and it would come back unlit
    /// because it was never sampled — an assertion that passes without the
    /// fade existing at all. Holding the world position fixed and translating
    /// the camera keeps the shadow-map lookup identical, so the only thing that
    /// can change the result is the fade.
    #[test]
    fn a_distant_fragment_loses_its_directional_shadow_at_the_same_world_position() {
        use crate::shadow_map::generate_shadow_maps;
        use gdcore::math3d::{Basis, Transform3D};
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::{Light3D as CasterLight, Light3DId};
        use gdserver3d::mesh::Mesh3D;

        let mut caster = CasterLight::directional(Light3DId(1));
        caster.shadow_enabled = true;
        caster.direction = Vector3::new(0.0, -1.0, 0.0);

        let mut occluder = Instance3D::new(Instance3DId(1));
        occluder.mesh = Some(Mesh3D::cube(4.0));
        occluder.visible = true;
        occluder.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        let map = generate_shadow_maps(&[caster], &[occluder])
            .into_iter()
            .next()
            .expect("a shadow-enabled directional light produces one map");
        let mut uniforms = default_uniforms();
        uniforms.directional_shadow_maps = vec![Some(Arc::new(map))];

        let mut light =
            LightUniform::directional(Vector3::new(0.0, -1.0, 0.0), Color::new(1.0, 1.0, 1.0, 1.0));
        light.shadow_enabled = true;
        assert_eq!(
            light.directional_shadow_max_distance, 100.0,
            "DirectionalLight3D's Godot default"
        );
        assert_eq!(light.directional_shadow_fade_start, 0.8, "Godot default");

        let shadow_at = |uniforms: &ShaderUniforms| {
            compute_shadow_factor(
                &light,
                0,
                Vector3::ZERO,
                Vector3::UP,
                light.direction,
                uniforms,
            )
        };

        // Camera at the origin (identity view): the fragment is right there, so
        // no fade — this is the positive control. Without it, "the far one is
        // lit" is also true of a fragment nothing ever shadowed.
        let near = shadow_at(&uniforms);
        assert!(
            near < 0.5,
            "the occluded fragment must be in shadow when close, got {near}"
        );

        // Same world position, camera pulled back past max_distance. Column-
        // major translation lives in m[3]; m[3][2] is the view-space z offset.
        let mut far_uniforms = uniforms.clone();
        far_uniforms.view_matrix[3][2] = -120.0;
        let far = shadow_at(&far_uniforms);
        assert!(
            (far - 1.0).abs() < 1e-6,
            "past directional_shadow_max_distance the shadow must be gone, got {far}"
        );

        // And the band between the edges is partial, not a second hard edge.
        let mut mid_uniforms = uniforms.clone();
        mid_uniforms.view_matrix[3][2] = -90.0;
        let mid = shadow_at(&mid_uniforms);
        assert!(
            mid > near && mid < far,
            "at the midpoint the shadow should be partly faded: near={near}, \
             mid={mid}, far={far}"
        );
    }

    /// 🔑 THE SHADING PATH ACTUALLY USES THE FILTER — not just the sampler.
    ///
    /// `compute_shadow_factor` is where a soft quality has to take effect; a
    /// filtered sampler nothing calls is dormant code. Probed at the occluder's
    /// shadow EDGE, because that is the only place filtering can change the
    /// answer.
    ///
    /// ⚠️ The `taps = 1` case is the control and the additivity proof at once:
    /// Godot's default filter quality is `Hard`, which reports one tap, and one
    /// tap must give the hard step the renderer produced before this existed.
    #[test]
    fn compute_shadow_factor_honours_the_filter_quality_at_the_edge() {
        use crate::shadow_map::generate_shadow_maps;
        use gdcore::math3d::{Basis, Transform3D};
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::{Light3D as CasterLight, Light3DId};
        use gdserver3d::mesh::Mesh3D;

        let mut caster = CasterLight::directional(Light3DId(1));
        caster.shadow_enabled = true;
        caster.direction = Vector3::new(0.0, -1.0, 0.0);

        let mut occluder = Instance3D::new(Instance3DId(1));
        occluder.mesh = Some(Mesh3D::cube(4.0));
        occluder.visible = true;
        occluder.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        let map = generate_shadow_maps(&[caster], &[occluder])
            .into_iter()
            .next()
            .expect("a shadow-enabled directional light produces one map");
        let mut uniforms = default_uniforms();
        uniforms.directional_shadow_maps = vec![Some(Arc::new(map))];

        let mut light =
            LightUniform::directional(Vector3::new(0.0, -1.0, 0.0), Color::new(1.0, 1.0, 1.0, 1.0));
        light.shadow_enabled = true;
        assert_eq!(light.shadow_taps, 1, "Godot's Hard default is one tap");

        // The 4-unit cube's hard shadow edge sits at x = +/-2.
        let edge = Vector3::new(2.0, 0.0, 0.0);
        let at = |l: &LightUniform| {
            compute_shadow_factor(l, 0, edge, Vector3::UP, l.direction, &uniforms)
        };

        let hard = at(&light);
        light.shadow_taps = 16;
        light.shadow_blur = 4.0;
        let soft = at(&light);

        assert!(
            (hard - hard.round()).abs() < 1e-6,
            "one tap must stay a hard step at the edge, got {hard}"
        );
        assert!(
            soft > 0.01 && soft < 0.99,
            "sixteen taps across the edge must give a partial shadow factor, \
             got {soft} — a value pinned to 0 or 1 means the filter never \
             reached the shading path"
        );
    }
}
