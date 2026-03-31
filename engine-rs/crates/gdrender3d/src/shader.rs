//! Vertex and fragment shader abstractions for the 3D render pipeline.
//!
//! Provides trait-based shader stages that transform vertices and compute
//! per-fragment colors. Built-in implementations cover unlit, Lambert diffuse,
//! and Phong specular shading models matching [`gdserver3d::ShadingMode`].

use std::sync::Arc;

use gdcore::math::{Color, Vector3};

use gdserver3d::light::ShadowCubemap;

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
    fn process(&self, _input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        Color::new(
            (uniforms.albedo.r + uniforms.emission.r).min(1.0),
            (uniforms.albedo.g + uniforms.emission.g).min(1.0),
            (uniforms.albedo.b + uniforms.emission.b).min(1.0),
            uniforms.albedo.a,
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
        LightKind::Spot => {
            // Spot light shadows not yet implemented — treat as lit.
            1.0
        }
    }
}

/// Lambert diffuse fragment shader.
#[derive(Debug, Clone, Copy)]
pub struct LambertFragmentShader;

impl FragmentShader for LambertFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        let normal = input.world_normal.normalized();
        let ambient = 0.1_f32;

        let mut diffuse_r = ambient * uniforms.albedo.r;
        let mut diffuse_g = ambient * uniforms.albedo.g;
        let mut diffuse_b = ambient * uniforms.albedo.b;

        for (i, light) in uniforms.lights.iter().enumerate() {
            let (light_dir, intensity) = light.evaluate(input.world_position);
            let shadow = compute_shadow_factor(light, i, input.world_position, uniforms);
            let n_dot_l = normal.dot(light_dir).max(0.0);
            let contrib = n_dot_l * intensity * shadow;
            diffuse_r += uniforms.albedo.r * light.color.r * contrib;
            diffuse_g += uniforms.albedo.g * light.color.g * contrib;
            diffuse_b += uniforms.albedo.b * light.color.b * contrib;
        }

        Color::new(
            (diffuse_r + uniforms.emission.r).min(1.0),
            (diffuse_g + uniforms.emission.g).min(1.0),
            (diffuse_b + uniforms.emission.b).min(1.0),
            uniforms.albedo.a,
        )
    }
}

/// Phong specular fragment shader (diffuse + specular).
#[derive(Debug, Clone, Copy)]
pub struct PhongFragmentShader;

impl FragmentShader for PhongFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        let normal = input.world_normal.normalized();
        let view_dir = (uniforms.camera_position - input.world_position).normalized();
        let ambient = 0.1_f32;
        let shininess = ((1.0 - uniforms.roughness) * 128.0).max(1.0);

        let mut r = ambient * uniforms.albedo.r;
        let mut g = ambient * uniforms.albedo.g;
        let mut b = ambient * uniforms.albedo.b;

        for (i, light) in uniforms.lights.iter().enumerate() {
            let (light_dir, intensity) = light.evaluate(input.world_position);
            let shadow = compute_shadow_factor(light, i, input.world_position, uniforms);
            let n_dot_l = normal.dot(light_dir).max(0.0);
            let contrib = n_dot_l * intensity * shadow;

            // Diffuse.
            r += uniforms.albedo.r * light.color.r * contrib;
            g += uniforms.albedo.g * light.color.g * contrib;
            b += uniforms.albedo.b * light.color.b * contrib;

            // Specular (Blinn-Phong half-vector).
            if n_dot_l > 0.0 && intensity > 0.0 && shadow > 0.0 {
                let half_dir = (light_dir + view_dir).normalized();
                let spec = normal.dot(half_dir).max(0.0).powf(shininess);
                let spec_strength = uniforms.metallic * 0.5 + 0.5;
                r += light.color.r * spec * spec_strength * intensity * shadow;
                g += light.color.g * spec * spec_strength * intensity * shadow;
                b += light.color.b * spec * spec_strength * intensity * shadow;
            }
        }

        Color::new(
            (r + uniforms.emission.r).min(1.0),
            (g + uniforms.emission.g).min(1.0),
            (b + uniforms.emission.b).min(1.0),
            uniforms.albedo.a,
        )
    }
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
}

impl CustomFragmentShader {
    /// Creates a custom fragment shader from a `ShaderMaterial3D`.
    ///
    /// Compiles the attached shader (if any) and captures the runtime
    /// parameters. If no shader is attached, uses an empty compiled shader.
    pub fn from_material(material: &gdserver3d::ShaderMaterial3D) -> Self {
        let compiler = gdserver3d::ShaderCompiler3D::new();
        let compiled = if let Some(shader) = &material.shader {
            compiler.compile(shader.shader_type, &shader.source_code)
        } else {
            compiler.compile(gdserver3d::ShaderType3D::Spatial, "")
        };
        Self {
            compiled,
            parameters: material.parameters.clone(),
        }
    }

    /// Creates a custom fragment shader from an already-compiled shader
    /// and parameter map.
    pub fn from_compiled(
        compiled: gdserver3d::CompiledShader3D,
        parameters: std::collections::HashMap<String, gdvariant::Variant>,
    ) -> Self {
        Self {
            compiled,
            parameters,
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
}

impl FragmentShader for CustomFragmentShader {
    fn process(&self, input: &FragmentInput, uniforms: &ShaderUniforms) -> Color {
        let processor = gdserver3d::ShaderProcessor3D::new();
        let ctx = gdserver3d::FragmentContext3D {
            albedo: uniforms.albedo,
            normal: input.world_normal,
            world_position: input.world_position,
            uv: (input.uv[0], input.uv[1]),
            view_dir: (uniforms.camera_position - input.world_position).normalized(),
            roughness: uniforms.roughness,
            metallic: uniforms.metallic,
            time: 0.0,
        };

        let color = processor.apply_shader(&self.compiled, &self.parameters, &ctx);

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

    fn default_uniforms() -> ShaderUniforms {
        ShaderUniforms {
            model_matrix: identity_matrix(),
            view_matrix: identity_matrix(),
            projection_matrix: identity_matrix(),
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            roughness: 0.5,
            metallic: 0.0,
            camera_position: Vector3::ZERO,
            lights: vec![],
            directional_shadow_maps: vec![],
            omni_shadow_cubemaps: vec![],
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
        // ambient(0.1) + diffuse(1.0 * 1.0 * 1.0) = 1.1, clamped to 1.0
        assert!((color.r - 1.0).abs() < 1e-5);
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
        uniforms.roughness = 0.0; // Very shiny
        uniforms.camera_position = Vector3::new(0.0, 1.0, 0.0); // Above
        uniforms.lights.push(LightUniform::directional(
            Vector3::UP,
            Color::new(1.0, 1.0, 1.0, 1.0),
        ));
        let color = fs.process(&input, &uniforms);
        // Should have significant brightness from specular
        assert!(color.r > 0.5);
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
}
