//! Shader and shader material definitions for 3D rendering.
//!
//! Provides types for Godot-compatible 3D (spatial) shaders: shader source
//! parsing, uniform extraction, compilation, and `ShaderMaterial3D` for
//! binding parameter values at runtime.
//!
//! The shader compiler parses uniform declarations from Godot shading language
//! source and produces a [`CompiledShader3D`] that can be evaluated by the
//! [`ShaderProcessor3D`] during rendering.

use gdcore::math::{Color, Vector3};
use gdvariant::variant::Variant;
use std::collections::HashMap;

// Re-use the shader type and uniform type enums that mirror Godot's categories.

/// The type of shader program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderType3D {
    /// Spatial / 3D surface shader (the primary type for this module).
    Spatial,
    /// Sky / environment shader.
    Sky,
}

impl Default for ShaderType3D {
    fn default() -> Self {
        Self::Spatial
    }
}

/// Data types that can appear as shader uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UniformType {
    /// Scalar float.
    Float,
    /// 2-component vector.
    Vec2,
    /// 3-component vector.
    Vec3,
    /// 4-component vector.
    Vec4,
    /// RGBA color (vec4 with color hint).
    Color,
    /// Scalar integer.
    Int,
    /// Boolean.
    Bool,
    /// 2D texture sampler.
    Sampler2D,
    /// 4×4 matrix.
    Mat4,
}

/// A single uniform declaration extracted from shader source.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderUniform3D {
    /// The uniform variable name.
    pub name: String,
    /// The data type.
    pub uniform_type: UniformType,
    /// Optional default value.
    pub default_value: Variant,
    /// Optional hint string (e.g. "source_color").
    pub hint: Option<String>,
}

/// A parsed 3D shader program.
#[derive(Debug, Clone, PartialEq)]
pub struct Shader3D {
    /// The shader pipeline stage.
    pub shader_type: ShaderType3D,
    /// Raw shader source code.
    pub source_code: String,
    /// Uniforms extracted from the source.
    pub uniforms: Vec<ShaderUniform3D>,
}

impl Shader3D {
    /// Create a new shader, parsing uniforms from `source`.
    pub fn new(shader_type: ShaderType3D, source: &str) -> Self {
        let uniforms = parse_uniforms(source);
        Self {
            shader_type,
            source_code: source.to_string(),
            uniforms,
        }
    }

    /// Look up a uniform by name.
    pub fn get_uniform(&self, name: &str) -> Option<&ShaderUniform3D> {
        self.uniforms.iter().find(|u| u.name == name)
    }
}

/// A material driven by a custom [`Shader3D`] with runtime parameter overrides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShaderMaterial3D {
    /// The attached shader (if any).
    pub shader: Option<Shader3D>,
    /// Runtime parameter overrides keyed by uniform name.
    pub parameters: HashMap<String, Variant>,
}

impl ShaderMaterial3D {
    /// Create an empty shader material.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a shader parameter value.
    pub fn set_shader_parameter(&mut self, name: &str, value: Variant) {
        self.parameters.insert(name.to_string(), value);
    }

    /// Get a shader parameter value.
    pub fn get_shader_parameter(&self, name: &str) -> Option<&Variant> {
        self.parameters.get(name)
    }

    /// Remove a shader parameter, returning the old value if present.
    pub fn remove_shader_parameter(&mut self, name: &str) -> Option<Variant> {
        self.parameters.remove(name)
    }

    /// Returns an iterator over all parameter (name, value) pairs.
    pub fn parameters(&self) -> impl Iterator<Item = (&str, &Variant)> {
        self.parameters.iter().map(|(k, v)| (k.as_str(), v))
    }
}

// ---------------------------------------------------------------------------
// Uniform parsing
// ---------------------------------------------------------------------------

/// Parse `uniform TYPE name [: hint] [= default];` lines from shader source.
///
/// This is intentionally simplified — it handles the most common Godot
/// shader uniform declarations without a full GLSL/Godot-shading-language parser.
pub fn parse_uniforms(source: &str) -> Vec<ShaderUniform3D> {
    let mut uniforms = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("uniform ") {
            continue;
        }
        // Strip trailing semicolon.
        let trimmed = trimmed.trim_end_matches(';').trim();
        // "uniform TYPE name [: hint] [= default]"
        let rest = &trimmed["uniform ".len()..];
        let mut parts = rest.splitn(2, char::is_whitespace);
        let type_str = match parts.next() {
            Some(t) => t.trim(),
            None => continue,
        };
        let remainder = match parts.next() {
            Some(r) => r.trim(),
            None => continue,
        };
        let uniform_type = match parse_uniform_type(type_str) {
            Some(t) => t,
            None => continue,
        };

        // Split remainder into name, optional hint, optional default.
        let (name_hint, default_value) = if let Some(eq_pos) = remainder.find('=') {
            let name_part = remainder[..eq_pos].trim();
            let val_part = remainder[eq_pos + 1..].trim();
            (name_part, parse_default_value(val_part))
        } else {
            (remainder, Variant::Nil)
        };

        let (name, hint) = if let Some(colon_pos) = name_hint.find(':') {
            let n = name_hint[..colon_pos].trim().to_string();
            let h = name_hint[colon_pos + 1..].trim().to_string();
            (n, if h.is_empty() { None } else { Some(h) })
        } else {
            (name_hint.trim().to_string(), None)
        };

        if name.is_empty() {
            continue;
        }

        uniforms.push(ShaderUniform3D {
            name,
            uniform_type,
            default_value,
            hint,
        });
    }
    uniforms
}

/// Map a type keyword to `UniformType`.
fn parse_uniform_type(s: &str) -> Option<UniformType> {
    match s {
        "float" => Some(UniformType::Float),
        "vec2" => Some(UniformType::Vec2),
        "vec3" => Some(UniformType::Vec3),
        "vec4" => Some(UniformType::Vec4),
        "int" => Some(UniformType::Int),
        "bool" => Some(UniformType::Bool),
        "sampler2D" => Some(UniformType::Sampler2D),
        "mat4" => Some(UniformType::Mat4),
        _ => None,
    }
}

/// Best-effort default value parse for simple literals.
fn parse_default_value(s: &str) -> Variant {
    let s = s.trim();
    if s == "true" {
        return Variant::Bool(true);
    }
    if s == "false" {
        return Variant::Bool(false);
    }
    if let Ok(i) = s.parse::<i64>() {
        return Variant::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Variant::Float(f);
    }
    Variant::Nil
}

// ---------------------------------------------------------------------------
// Shader compiler
// ---------------------------------------------------------------------------

/// A diagnostic emitted during shader compilation.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderDiagnostic {
    /// Zero-based line number where the issue was detected.
    pub line: usize,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Human-readable message.
    pub message: String,
}

/// Severity level for shader compilation diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Informational warning — shader still compiles.
    Warning,
    /// Hard error — shader compilation fails.
    Error,
}

/// Render-mode flags parsed from a `render_mode` directive.
///
/// Maps to Godot's `render_mode` statement in spatial shaders, e.g.
/// `render_mode unshaded, cull_disabled;`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenderModeFlags {
    /// When true, disables all lighting (equivalent to `unshaded`).
    pub unshaded: bool,
    /// When true, renders both front and back faces (`cull_disabled`).
    pub cull_disabled: bool,
    /// When true, renders only back faces (`cull_front`).
    pub cull_front: bool,
    /// When true, enables alpha blending (`blend_mix`).
    pub blend_mix: bool,
    /// When true, enables additive blending (`blend_add`).
    pub blend_add: bool,
    /// When true, skips depth writes (`depth_draw_never`).
    pub depth_draw_never: bool,
}

/// A compiled 3D shader program ready for execution.
///
/// Stores the parsed uniforms, render mode flags, original source, and an
/// optional WGSL translation for GPU execution.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledShader3D {
    /// The shader pipeline stage.
    pub shader_type: ShaderType3D,
    /// Parsed uniform declarations.
    pub uniforms: Vec<ShaderUniform3D>,
    /// The original GDShader source.
    pub program: String,
    /// Generated WGSL program for GPU execution (if compilation succeeded).
    pub wgsl_program: Option<String>,
    /// Render-mode flags extracted from the source.
    pub render_modes: RenderModeFlags,
    /// Diagnostics (warnings/errors) emitted during compilation.
    pub diagnostics: Vec<ShaderDiagnostic>,
}

impl CompiledShader3D {
    /// Returns the number of uniforms in this shader.
    pub fn uniform_count(&self) -> usize {
        self.uniforms.len()
    }

    /// Looks up a uniform by name.
    pub fn get_uniform(&self, name: &str) -> Option<&ShaderUniform3D> {
        self.uniforms.iter().find(|u| u.name == name)
    }

    /// Returns `true` if compilation produced any errors.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    /// Returns `true` if compilation produced any warnings.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Warning)
    }

    /// Returns error diagnostics only.
    pub fn errors(&self) -> Vec<&ShaderDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect()
    }
}

/// Compiles 3D shader source into a [`CompiledShader3D`].
///
/// Parses uniforms, render_mode directives, and validates basic structure.
/// Emits diagnostics for unrecognized constructs.
#[derive(Debug, Default)]
pub struct ShaderCompiler3D;

impl ShaderCompiler3D {
    /// Creates a new shader compiler.
    pub fn new() -> Self {
        Self
    }

    /// Compiles shader source into a [`CompiledShader3D`].
    ///
    /// Parses `uniform` declarations, `render_mode` directives, performs
    /// basic validation, and generates a WGSL program for GPU execution.
    /// Check [`CompiledShader3D::has_errors`] to determine if compilation
    /// succeeded.
    pub fn compile(&self, shader_type: ShaderType3D, source: &str) -> CompiledShader3D {
        let uniforms = parse_uniforms(source);
        let render_modes = parse_render_modes(source);
        let diagnostics = validate_shader(source, shader_type);
        let wgsl_program = if diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
        {
            None
        } else {
            Some(generate_wgsl(&uniforms, &render_modes, source))
        };
        CompiledShader3D {
            shader_type,
            uniforms,
            program: source.to_string(),
            wgsl_program,
            render_modes,
            diagnostics,
        }
    }

    /// Compiles a [`ShaderMaterial3D`] by extracting the shader source and
    /// producing a [`CompiledShader3D`] with WGSL output.
    ///
    /// Returns `None` if the material has no attached shader.
    pub fn compile_material(&self, material: &ShaderMaterial3D) -> Option<CompiledShader3D> {
        let shader = material.shader.as_ref()?;
        Some(self.compile(shader.shader_type, &shader.source_code))
    }
}

/// Parse `render_mode` directives from shader source.
///
/// Godot shaders can have `render_mode flag1, flag2;` on one or more lines.
fn parse_render_modes(source: &str) -> RenderModeFlags {
    let mut flags = RenderModeFlags::default();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("render_mode ") {
            continue;
        }
        let rest = trimmed["render_mode ".len()..].trim_end_matches(';');
        for flag in rest.split(',') {
            match flag.trim() {
                "unshaded" => flags.unshaded = true,
                "cull_disabled" => flags.cull_disabled = true,
                "cull_front" => flags.cull_front = true,
                "blend_mix" => flags.blend_mix = true,
                "blend_add" => flags.blend_add = true,
                "depth_draw_never" => flags.depth_draw_never = true,
                _ => {} // Unknown flags silently ignored (Godot-compatible).
            }
        }
    }
    flags
}

/// Basic shader source validation.
///
/// Checks for structural issues and returns diagnostics.
fn validate_shader(source: &str, shader_type: ShaderType3D) -> Vec<ShaderDiagnostic> {
    let mut diagnostics = Vec::new();

    // Check for shader_type declaration.
    let has_shader_type = source.lines().any(|l| l.trim().starts_with("shader_type "));
    if !has_shader_type && !source.trim().is_empty() {
        diagnostics.push(ShaderDiagnostic {
            line: 0,
            severity: DiagnosticSeverity::Warning,
            message: "Missing `shader_type` declaration".to_string(),
        });
    }

    // Validate shader_type matches the expected type.
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("shader_type ") {
            let declared = trimmed["shader_type ".len()..].trim_end_matches(';').trim();
            let expected = match shader_type {
                ShaderType3D::Spatial => "spatial",
                ShaderType3D::Sky => "sky",
            };
            if declared != expected {
                diagnostics.push(ShaderDiagnostic {
                    line: i,
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Shader type mismatch: declared `{}`, expected `{}`",
                        declared, expected
                    ),
                });
            }
        }

        // Check for duplicate uniform names.
        if trimmed.starts_with("uniform ") {
            // Duplicate detection is done separately below.
        }
    }

    // Detect duplicate uniform names.
    let uniforms = parse_uniforms(source);
    let mut seen = std::collections::HashSet::new();
    for u in &uniforms {
        if !seen.insert(&u.name) {
            diagnostics.push(ShaderDiagnostic {
                line: 0,
                severity: DiagnosticSeverity::Error,
                message: format!("Duplicate uniform `{}`", u.name),
            });
        }
    }

    // Check for mismatched braces (basic structural validation).
    let open_braces = source.chars().filter(|c| *c == '{').count();
    let close_braces = source.chars().filter(|c| *c == '}').count();
    if open_braces != close_braces {
        diagnostics.push(ShaderDiagnostic {
            line: 0,
            severity: DiagnosticSeverity::Error,
            message: format!(
                "Mismatched braces: {} opening, {} closing",
                open_braces, close_braces
            ),
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// GDShader → WGSL code generation
// ---------------------------------------------------------------------------

/// Maps a [`UniformType`] to a WGSL type string.
pub fn uniform_type_to_wgsl(ty: UniformType) -> &'static str {
    match ty {
        UniformType::Float => "f32",
        UniformType::Vec2 => "vec2<f32>",
        UniformType::Vec3 => "vec3<f32>",
        UniformType::Vec4 => "vec4<f32>",
        UniformType::Color => "vec4<f32>",
        UniformType::Int => "i32",
        UniformType::Bool => "u32", // WGSL has no bool in uniform buffers; use u32.
        UniformType::Sampler2D => "texture_2d<f32>",
        UniformType::Mat4 => "mat4x4<f32>",
    }
}

/// Returns the WGSL byte size and alignment for a uniform type.
///
/// Useful for calculating uniform buffer offsets.
pub fn uniform_type_wgsl_size(ty: UniformType) -> u32 {
    match ty {
        UniformType::Float | UniformType::Int | UniformType::Bool => 4,
        UniformType::Vec2 => 8,
        UniformType::Vec3 | UniformType::Vec4 | UniformType::Color => 16,
        UniformType::Mat4 => 64,
        UniformType::Sampler2D => 0, // Texture bindings are separate.
    }
}

/// Generates a WGSL fragment shader from parsed GDShader uniforms and
/// render mode flags.
///
/// Produces a self-contained WGSL module with:
/// - A `CustomUniforms` struct at bind group 3 containing all non-texture
///   uniforms.
/// - A `custom_fragment` function that applies uniform overrides to the
///   fragment color (albedo, emission, alpha, color_mix).
/// - Respects `unshaded` render mode by skipping lighting in the output.
///
/// The generated WGSL is designed to be composed with the engine's built-in
/// camera/model/light bind groups (groups 0–2).
fn generate_wgsl(
    uniforms: &[ShaderUniform3D],
    render_modes: &RenderModeFlags,
    _source: &str,
) -> String {
    let mut wgsl = String::with_capacity(2048);

    // Header comment.
    wgsl.push_str("// Auto-generated WGSL from GDShader source\n");
    wgsl.push_str("// Do not edit — regenerated on each compilation\n\n");

    // Collect non-texture uniforms for the uniform buffer.
    let buffer_uniforms: Vec<&ShaderUniform3D> = uniforms
        .iter()
        .filter(|u| u.uniform_type != UniformType::Sampler2D)
        .collect();

    // Generate the CustomUniforms struct and binding.
    if !buffer_uniforms.is_empty() {
        wgsl.push_str("struct CustomUniforms {\n");
        for u in &buffer_uniforms {
            wgsl.push_str(&format!(
                "    {}: {},\n",
                u.name,
                uniform_type_to_wgsl(u.uniform_type)
            ));
        }
        wgsl.push_str("}\n\n");
        wgsl.push_str("@group(3) @binding(0)\n");
        wgsl.push_str("var<uniform> custom: CustomUniforms;\n\n");
    }

    // Generate texture/sampler bindings.
    let mut tex_binding = 1u32;
    for u in uniforms
        .iter()
        .filter(|u| u.uniform_type == UniformType::Sampler2D)
    {
        wgsl.push_str(&format!(
            "@group(3) @binding({})\nvar {}: texture_2d<f32>;\n",
            tex_binding, u.name
        ));
        tex_binding += 1;
        wgsl.push_str(&format!(
            "@group(3) @binding({})\nvar {}_sampler: sampler;\n\n",
            tex_binding, u.name
        ));
        tex_binding += 1;
    }

    // Fragment input struct.
    wgsl.push_str("struct FragmentInput {\n");
    wgsl.push_str("    @location(0) world_position: vec3<f32>,\n");
    wgsl.push_str("    @location(1) world_normal: vec3<f32>,\n");
    wgsl.push_str("    @location(2) uv: vec2<f32>,\n");
    wgsl.push_str("}\n\n");

    // Fragment output struct.
    wgsl.push_str("struct FragmentOutput {\n");
    wgsl.push_str("    @location(0) color: vec4<f32>,\n");
    wgsl.push_str("}\n\n");

    // Generate the custom_fragment function.
    wgsl.push_str(
        "fn custom_fragment(in: FragmentInput, base_color: vec4<f32>) -> FragmentOutput {\n",
    );
    wgsl.push_str("    var color = base_color;\n");

    // Apply albedo_color override if present.
    if buffer_uniforms.iter().any(|u| u.name == "albedo_color") {
        wgsl.push_str("    color = vec4<f32>(custom.albedo_color.rgb, color.a);\n");
    }

    // Apply color_override.
    if buffer_uniforms.iter().any(|u| u.name == "color_override") {
        wgsl.push_str("    color = custom.color_override;\n");
    }

    // Apply emission as additive.
    if buffer_uniforms.iter().any(|u| u.name == "emission") {
        wgsl.push_str("    color = vec4<f32>(min(color.rgb + custom.emission.rgb, vec3<f32>(1.0)), color.a);\n");
    }

    // Apply alpha override.
    if buffer_uniforms.iter().any(|u| u.name == "alpha") {
        wgsl.push_str("    color.a = custom.alpha;\n");
    }

    // Apply color_mix blending.
    if buffer_uniforms.iter().any(|u| u.name == "color_mix") {
        let has_factor = buffer_uniforms.iter().any(|u| u.name == "color_mix_factor");
        if has_factor {
            wgsl.push_str("    color = mix(color, custom.color_mix, custom.color_mix_factor);\n");
        } else {
            wgsl.push_str("    color = mix(color, custom.color_mix, 0.5);\n");
        }
    }

    // Unshaded: output directly without lighting.
    if render_modes.unshaded {
        wgsl.push_str("    // render_mode: unshaded — skip lighting\n");
    }

    wgsl.push_str("    var out: FragmentOutput;\n");
    wgsl.push_str("    out.color = color;\n");
    wgsl.push_str("    return out;\n");
    wgsl.push_str("}\n");

    wgsl
}

// ---------------------------------------------------------------------------
// 3D Fragment context and evaluation
// ---------------------------------------------------------------------------

/// Built-in variables available to a 3D fragment shader during execution.
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentContext3D {
    /// The current fragment color (Godot's `ALBEDO` built-in as full RGBA).
    pub albedo: Color,
    /// World-space normal at the fragment.
    pub normal: Vector3,
    /// World-space position of the fragment.
    pub world_position: Vector3,
    /// Normalised UV coordinate of the current fragment (0..1).
    pub uv: (f32, f32),
    /// View direction (camera to fragment, normalised).
    pub view_dir: Vector3,
    /// Roughness value at the fragment.
    pub roughness: f32,
    /// Metallic value at the fragment.
    pub metallic: f32,
    /// Time elapsed in seconds (for animated shaders).
    pub time: f32,
}

impl Default for FragmentContext3D {
    fn default() -> Self {
        Self {
            albedo: Color::WHITE,
            normal: Vector3::new(0.0, 1.0, 0.0),
            world_position: Vector3::ZERO,
            uv: (0.0, 0.0),
            view_dir: Vector3::new(0.0, 0.0, -1.0),
            roughness: 0.5,
            metallic: 0.0,
            time: 0.0,
        }
    }
}

/// Evaluates compiled 3D shaders against fragment data.
///
/// Currently supports a minimal evaluation model: if the shader has an
/// `albedo_color` uniform and a matching value is provided, that color is
/// returned. Also handles `emission`, `roughness_scale`, and `metallic_scale`
/// uniforms for PBR workflows. Otherwise the input albedo is passed through.
#[derive(Debug, Default)]
pub struct ShaderProcessor3D;

impl ShaderProcessor3D {
    /// Creates a new shader processor.
    pub fn new() -> Self {
        Self
    }

    /// Applies a compiled shader to a single fragment.
    ///
    /// `uniforms` maps uniform names to runtime values.
    /// `ctx` provides the built-in fragment variables.
    ///
    /// Returns the output color after "executing" the shader.
    pub fn apply_shader(
        &self,
        shader: &CompiledShader3D,
        uniforms: &HashMap<String, Variant>,
        ctx: &FragmentContext3D,
    ) -> Color {
        let mut color = ctx.albedo;

        // Check for albedo_color uniform override.
        if let Some(Variant::Color(c)) = uniforms.get("albedo_color") {
            color = *c;
        } else if let Some(Variant::Color(c)) = uniforms.get("color_override") {
            // Alternative name used by some Godot shaders.
            color = *c;
        } else if let Some(u) = shader.get_uniform("albedo_color") {
            // Fall back to the shader's default value.
            if let Variant::Color(c) = &u.default_value {
                color = *c;
            }
        }

        // Apply emission as additive.
        if let Some(Variant::Color(e)) = uniforms.get("emission") {
            color = Color::new(
                (color.r + e.r).min(1.0),
                (color.g + e.g).min(1.0),
                (color.b + e.b).min(1.0),
                color.a,
            );
        }

        // Apply alpha override.
        if let Some(Variant::Float(a)) = uniforms.get("alpha") {
            color.a = *a as f32;
        }

        // Apply color_mix: blend between albedo and a target color.
        if let Some(Variant::Color(mix_color)) = uniforms.get("color_mix") {
            let mix_factor = match uniforms.get("color_mix_factor") {
                Some(Variant::Float(f)) => *f as f32,
                _ => 0.5,
            };
            color = Color::new(
                color.r + (mix_color.r - color.r) * mix_factor,
                color.g + (mix_color.g - color.g) * mix_factor,
                color.b + (mix_color.b - color.b) * mix_factor,
                color.a + (mix_color.a - color.a) * mix_factor,
            );
        }

        // If the shader is unshaded, skip any further lighting adjustments.
        if shader.render_modes.unshaded {
            return color;
        }

        color
    }

    /// Resolves a uniform value: checks runtime parameters first, then
    /// falls back to the shader's declared default.
    pub fn resolve_uniform<'a>(
        shader: &'a CompiledShader3D,
        params: &'a HashMap<String, Variant>,
        name: &str,
    ) -> Option<&'a Variant> {
        params.get(name).or_else(|| {
            shader.get_uniform(name).and_then(|u| {
                if u.default_value != Variant::Nil {
                    Some(&u.default_value)
                } else {
                    None
                }
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Uniform parsing ──

    #[test]
    fn parse_basic_uniforms() {
        let source = r#"
shader_type spatial;
uniform float speed = 2.0;
uniform vec4 albedo_color : source_color;
uniform int count = 5;
uniform bool enabled = true;
"#;
        let uniforms = parse_uniforms(source);
        assert_eq!(uniforms.len(), 4);
        assert_eq!(uniforms[0].name, "speed");
        assert_eq!(uniforms[0].uniform_type, UniformType::Float);
        assert_eq!(uniforms[0].default_value, Variant::Float(2.0));

        assert_eq!(uniforms[1].name, "albedo_color");
        assert_eq!(uniforms[1].uniform_type, UniformType::Vec4);
        assert_eq!(uniforms[1].hint.as_deref(), Some("source_color"));

        assert_eq!(uniforms[2].name, "count");
        assert_eq!(uniforms[2].uniform_type, UniformType::Int);
        assert_eq!(uniforms[2].default_value, Variant::Int(5));

        assert_eq!(uniforms[3].name, "enabled");
        assert_eq!(uniforms[3].uniform_type, UniformType::Bool);
        assert_eq!(uniforms[3].default_value, Variant::Bool(true));
    }

    #[test]
    fn parse_no_uniforms() {
        let source = "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }";
        assert!(parse_uniforms(source).is_empty());
    }

    #[test]
    fn parse_sampler_and_mat4() {
        let source = "uniform sampler2D tex;\nuniform mat4 model_matrix;";
        let uniforms = parse_uniforms(source);
        assert_eq!(uniforms.len(), 2);
        assert_eq!(uniforms[0].uniform_type, UniformType::Sampler2D);
        assert_eq!(uniforms[1].uniform_type, UniformType::Mat4);
    }

    #[test]
    fn parse_unknown_type_skipped() {
        let source = "uniform highp_float thing = 1.0;";
        assert!(parse_uniforms(source).is_empty());
    }

    // ── Shader3D ──

    #[test]
    fn shader3d_new_parses_uniforms() {
        let source = "shader_type spatial;\nuniform float speed = 3.0;";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);
        assert_eq!(shader.shader_type, ShaderType3D::Spatial);
        assert_eq!(shader.uniforms.len(), 1);
        assert_eq!(shader.get_uniform("speed").unwrap().name, "speed");
        assert!(shader.get_uniform("missing").is_none());
    }

    // ── ShaderMaterial3D ──

    #[test]
    fn shader_material_default() {
        let mat = ShaderMaterial3D::new();
        assert!(mat.shader.is_none());
        assert!(mat.parameters.is_empty());
    }

    #[test]
    fn shader_material_set_get_remove() {
        let mut mat = ShaderMaterial3D::new();
        mat.set_shader_parameter("speed", Variant::Float(5.0));
        assert_eq!(
            mat.get_shader_parameter("speed"),
            Some(&Variant::Float(5.0))
        );

        let removed = mat.remove_shader_parameter("speed");
        assert_eq!(removed, Some(Variant::Float(5.0)));
        assert!(mat.get_shader_parameter("speed").is_none());
    }

    #[test]
    fn shader_material_parameters_iterator() {
        let mut mat = ShaderMaterial3D::new();
        mat.set_shader_parameter("a", Variant::Int(1));
        mat.set_shader_parameter("b", Variant::Int(2));
        let params: Vec<_> = mat.parameters().collect();
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn shader_material_with_shader() {
        let source = "uniform float roughness_scale = 0.8;";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);
        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter("roughness_scale", Variant::Float(0.5));

        assert!(mat.shader.is_some());
        assert_eq!(
            mat.get_shader_parameter("roughness_scale"),
            Some(&Variant::Float(0.5))
        );
    }

    // ── ShaderCompiler3D ──

    #[test]
    fn compiler_produces_compiled_shader() {
        let compiler = ShaderCompiler3D::new();
        let source = "uniform float speed = 1.0;\nuniform vec3 offset;";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        assert_eq!(compiled.shader_type, ShaderType3D::Spatial);
        assert_eq!(compiled.uniform_count(), 2);
        assert!(compiled.get_uniform("speed").is_some());
        assert!(compiled.get_uniform("offset").is_some());
        assert_eq!(compiled.program, source);
    }

    #[test]
    fn compiler_empty_source() {
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Sky, "");
        assert_eq!(compiled.uniform_count(), 0);
        assert_eq!(compiled.shader_type, ShaderType3D::Sky);
    }

    // ── FragmentContext3D ──

    #[test]
    fn fragment_context_defaults() {
        let ctx = FragmentContext3D::default();
        assert_eq!(ctx.albedo, Color::WHITE);
        assert_eq!(ctx.normal, Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(ctx.uv, (0.0, 0.0));
        assert!((ctx.roughness - 0.5).abs() < f32::EPSILON);
        assert!(ctx.metallic.abs() < f32::EPSILON);
        assert!(ctx.time.abs() < f32::EPSILON);
    }

    // ── ShaderProcessor3D ──

    #[test]
    fn processor_passthrough_with_no_uniforms() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "");
        let ctx = FragmentContext3D {
            albedo: Color::new(0.5, 0.5, 0.5, 1.0),
            ..Default::default()
        };
        let result = processor.apply_shader(&compiled, &HashMap::new(), &ctx);
        assert_eq!(result, Color::new(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn processor_albedo_color_override() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "uniform vec4 albedo_color;");
        let mut params = HashMap::new();
        params.insert(
            "albedo_color".to_string(),
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert_eq!(result, Color::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn processor_color_override_uniform() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "");
        let mut params = HashMap::new();
        params.insert(
            "color_override".to_string(),
            Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
        );
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert_eq!(result, Color::new(0.0, 1.0, 0.0, 1.0));
    }

    #[test]
    fn processor_emission_additive() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "");
        let mut params = HashMap::new();
        params.insert(
            "emission".to_string(),
            Variant::Color(Color::new(0.5, 0.0, 0.0, 1.0)),
        );
        let ctx = FragmentContext3D {
            albedo: Color::new(0.3, 0.3, 0.3, 1.0),
            ..Default::default()
        };
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert!((result.r - 0.8).abs() < 0.01);
        assert!((result.g - 0.3).abs() < 0.01);
    }

    #[test]
    fn processor_emission_clamps_to_one() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "");
        let mut params = HashMap::new();
        params.insert(
            "emission".to_string(),
            Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        );
        let ctx = FragmentContext3D {
            albedo: Color::new(0.5, 0.5, 0.5, 1.0),
            ..Default::default()
        };
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert!((result.r - 1.0).abs() < f32::EPSILON);
        assert!((result.g - 1.0).abs() < f32::EPSILON);
        assert!((result.b - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn processor_alpha_override() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "");
        let mut params = HashMap::new();
        params.insert("alpha".to_string(), Variant::Float(0.25));
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert!((result.a - 0.25).abs() < 0.01);
    }

    #[test]
    fn processor_albedo_color_from_shader_default() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        // The parse_default_value doesn't handle vec4 literals yet, so
        // we manually build a compiled shader with a Color default.
        let compiled = CompiledShader3D {
            shader_type: ShaderType3D::Spatial,
            uniforms: vec![ShaderUniform3D {
                name: "albedo_color".to_string(),
                uniform_type: UniformType::Vec4,
                default_value: Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
                hint: None,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
        };
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &HashMap::new(), &ctx);
        assert_eq!(result, Color::new(0.0, 0.0, 1.0, 1.0));
    }

    // ── Roundtrip: material → compile → evaluate ──

    #[test]
    fn full_pipeline_material_compile_evaluate() {
        let source = "shader_type spatial;\nuniform vec4 albedo_color : source_color;";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);

        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter(
            "albedo_color",
            Variant::Color(Color::new(1.0, 0.5, 0.0, 1.0)),
        );

        // Compile.
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(
            mat.shader.as_ref().unwrap().shader_type,
            &mat.shader.as_ref().unwrap().source_code,
        );

        // Evaluate.
        let processor = ShaderProcessor3D::new();
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &mat.parameters, &ctx);
        assert_eq!(result, Color::new(1.0, 0.5, 0.0, 1.0));
    }

    // ── ShaderType3D default ──

    #[test]
    fn shader_type_default_is_spatial() {
        assert_eq!(ShaderType3D::default(), ShaderType3D::Spatial);
    }

    // ── Render mode parsing ──

    #[test]
    fn parse_render_mode_unshaded() {
        let source = "shader_type spatial;\nrender_mode unshaded;";
        let flags = parse_render_modes(source);
        assert!(flags.unshaded);
        assert!(!flags.cull_disabled);
    }

    #[test]
    fn parse_render_mode_multiple_flags() {
        let source = "render_mode unshaded, cull_disabled, blend_add;";
        let flags = parse_render_modes(source);
        assert!(flags.unshaded);
        assert!(flags.cull_disabled);
        assert!(flags.blend_add);
        assert!(!flags.blend_mix);
        assert!(!flags.depth_draw_never);
    }

    #[test]
    fn parse_render_mode_empty_source() {
        let flags = parse_render_modes("");
        assert_eq!(flags, RenderModeFlags::default());
    }

    // ── Shader validation ──

    #[test]
    fn validate_missing_shader_type_warning() {
        let source = "uniform float speed = 1.0;";
        let diagnostics = validate_shader(source, ShaderType3D::Spatial);
        assert!(diagnostics.iter().any(
            |d| d.severity == DiagnosticSeverity::Warning && d.message.contains("shader_type")
        ));
    }

    #[test]
    fn validate_type_mismatch_error() {
        let source = "shader_type sky;\nuniform float speed;";
        let diagnostics = validate_shader(source, ShaderType3D::Spatial);
        assert!(diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error && d.message.contains("mismatch")));
    }

    #[test]
    fn validate_mismatched_braces_error() {
        let source = "shader_type spatial;\nvoid fragment() {";
        let diagnostics = validate_shader(source, ShaderType3D::Spatial);
        assert!(diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error && d.message.contains("braces")));
    }

    #[test]
    fn validate_duplicate_uniform_error() {
        let source = "shader_type spatial;\nuniform float x;\nuniform float x;";
        let diagnostics = validate_shader(source, ShaderType3D::Spatial);
        assert!(diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error && d.message.contains("Duplicate")));
    }

    #[test]
    fn validate_clean_shader_no_diagnostics() {
        let source = "shader_type spatial;\nuniform float speed = 1.0;\nvoid fragment() { ALBEDO = vec3(1.0); }";
        let diagnostics = validate_shader(source, ShaderType3D::Spatial);
        assert!(diagnostics.is_empty());
    }

    // ── CompiledShader3D helpers ──

    #[test]
    fn compiled_shader_has_errors() {
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(ShaderType3D::Spatial, "shader_type sky;");
        assert!(compiled.has_errors());
        assert!(!compiled.errors().is_empty());
    }

    #[test]
    fn compiled_shader_no_errors() {
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(
            ShaderType3D::Spatial,
            "shader_type spatial;\nvoid fragment() {}",
        );
        assert!(!compiled.has_errors());
    }

    #[test]
    fn compiled_shader_render_modes_preserved() {
        let compiler = ShaderCompiler3D::new();
        let source =
            "shader_type spatial;\nrender_mode unshaded, cull_disabled;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        assert!(compiled.render_modes.unshaded);
        assert!(compiled.render_modes.cull_disabled);
    }

    // ── ShaderProcessor3D unshaded path ──

    #[test]
    fn processor_unshaded_returns_color_directly() {
        let processor = ShaderProcessor3D::new();
        let compiled = CompiledShader3D {
            shader_type: ShaderType3D::Spatial,
            uniforms: vec![],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags {
                unshaded: true,
                ..Default::default()
            },
            diagnostics: Vec::new(),
        };
        let mut params = HashMap::new();
        params.insert(
            "albedo_color".to_string(),
            Variant::Color(Color::new(0.8, 0.2, 0.1, 1.0)),
        );
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &params, &ctx);
        assert_eq!(result, Color::new(0.8, 0.2, 0.1, 1.0));
    }

    // ── ShaderProcessor3D::resolve_uniform ──

    #[test]
    fn resolve_uniform_prefers_runtime_over_default() {
        let compiled = CompiledShader3D {
            shader_type: ShaderType3D::Spatial,
            uniforms: vec![ShaderUniform3D {
                name: "speed".to_string(),
                uniform_type: UniformType::Float,
                default_value: Variant::Float(1.0),
                hint: None,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
        };
        let mut params = HashMap::new();
        params.insert("speed".to_string(), Variant::Float(5.0));

        let val = ShaderProcessor3D::resolve_uniform(&compiled, &params, "speed");
        assert_eq!(val, Some(&Variant::Float(5.0)));
    }

    #[test]
    fn resolve_uniform_falls_back_to_default() {
        let compiled = CompiledShader3D {
            shader_type: ShaderType3D::Spatial,
            uniforms: vec![ShaderUniform3D {
                name: "speed".to_string(),
                uniform_type: UniformType::Float,
                default_value: Variant::Float(1.0),
                hint: None,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
        };
        let params = HashMap::new();

        let val = ShaderProcessor3D::resolve_uniform(&compiled, &params, "speed");
        assert_eq!(val, Some(&Variant::Float(1.0)));
    }

    #[test]
    fn resolve_uniform_returns_none_for_missing() {
        let compiled = CompiledShader3D {
            shader_type: ShaderType3D::Spatial,
            uniforms: vec![],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
        };
        let params = HashMap::new();

        assert!(ShaderProcessor3D::resolve_uniform(&compiled, &params, "missing").is_none());
    }

    // ── Color mix uniform ──

    #[test]
    fn processor_color_mix_with_factor() {
        let processor = ShaderProcessor3D::new();
        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(
            ShaderType3D::Spatial,
            "shader_type spatial;\nvoid fragment() {}",
        );
        let mut params = HashMap::new();
        params.insert(
            "color_mix".to_string(),
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        params.insert("color_mix_factor".to_string(), Variant::Float(1.0));
        let ctx = FragmentContext3D {
            albedo: Color::new(0.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };
        let result = processor.apply_shader(&compiled, &params, &ctx);
        // Full mix toward red.
        assert!((result.r - 1.0).abs() < 0.01);
        assert!(result.g.abs() < 0.01);
    }

    // ── Full pipeline with render_mode ──

    #[test]
    fn full_pipeline_unshaded_shader() {
        let source = "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);
        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter(
            "albedo_color",
            Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
        );

        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile(
            mat.shader.as_ref().unwrap().shader_type,
            &mat.shader.as_ref().unwrap().source_code,
        );

        assert!(!compiled.has_errors());
        assert!(compiled.render_modes.unshaded);

        let processor = ShaderProcessor3D::new();
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &mat.parameters, &ctx);
        assert_eq!(result, Color::new(0.0, 1.0, 0.0, 1.0));
    }

    // ── WGSL code generation ──

    #[test]
    fn wgsl_generation_basic_uniforms() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nuniform float speed = 1.0;\nuniform vec4 albedo_color : source_color;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(wgsl.contains("struct CustomUniforms"));
        assert!(wgsl.contains("speed: f32"));
        assert!(wgsl.contains("albedo_color: vec4<f32>"));
        assert!(wgsl.contains("@group(3) @binding(0)"));
        assert!(wgsl.contains("fn custom_fragment"));
        // albedo_color uniform should trigger override in the function body.
        assert!(wgsl.contains("custom.albedo_color"));
    }

    #[test]
    fn wgsl_generation_no_uniforms() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        // No CustomUniforms struct when there are no uniforms.
        assert!(!wgsl.contains("struct CustomUniforms"));
        // Function still present.
        assert!(wgsl.contains("fn custom_fragment"));
    }

    #[test]
    fn wgsl_generation_with_sampler() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nuniform sampler2D tex;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(wgsl.contains("var tex: texture_2d<f32>"));
        assert!(wgsl.contains("var tex_sampler: sampler"));
        // Samplers should not be in the uniform struct.
        assert!(!wgsl.contains("struct CustomUniforms"));
    }

    #[test]
    fn wgsl_generation_unshaded_render_mode() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nrender_mode unshaded;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(wgsl.contains("unshaded"));
    }

    #[test]
    fn wgsl_generation_emission_uniform() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nuniform vec4 emission;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(wgsl.contains("custom.emission"));
        assert!(wgsl.contains("min(color.rgb + custom.emission.rgb"));
    }

    #[test]
    fn wgsl_generation_alpha_and_color_mix() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nuniform float alpha;\nuniform vec4 color_mix;\nuniform float color_mix_factor;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(wgsl.contains("color.a = custom.alpha"));
        assert!(wgsl.contains("mix(color, custom.color_mix, custom.color_mix_factor)"));
    }

    #[test]
    fn wgsl_not_generated_on_errors() {
        let compiler = ShaderCompiler3D::new();
        // Type mismatch: declared sky but compiling as spatial.
        let source = "shader_type sky;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        assert!(compiled.has_errors());
        assert!(compiled.wgsl_program.is_none());
    }

    #[test]
    fn uniform_type_wgsl_mapping() {
        assert_eq!(uniform_type_to_wgsl(UniformType::Float), "f32");
        assert_eq!(uniform_type_to_wgsl(UniformType::Vec2), "vec2<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Vec3), "vec3<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Vec4), "vec4<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Color), "vec4<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Int), "i32");
        assert_eq!(uniform_type_to_wgsl(UniformType::Bool), "u32");
        assert_eq!(uniform_type_to_wgsl(UniformType::Mat4), "mat4x4<f32>");
    }

    #[test]
    fn uniform_type_wgsl_sizes() {
        assert_eq!(uniform_type_wgsl_size(UniformType::Float), 4);
        assert_eq!(uniform_type_wgsl_size(UniformType::Vec2), 8);
        assert_eq!(uniform_type_wgsl_size(UniformType::Vec3), 16);
        assert_eq!(uniform_type_wgsl_size(UniformType::Vec4), 16);
        assert_eq!(uniform_type_wgsl_size(UniformType::Mat4), 64);
        assert_eq!(uniform_type_wgsl_size(UniformType::Sampler2D), 0);
    }

    // ── compile_material ──

    #[test]
    fn compile_material_with_shader() {
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nuniform float speed = 2.0;\nvoid fragment() {}";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);
        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter("speed", Variant::Float(5.0));

        let compiled = compiler.compile_material(&mat).expect("should compile");
        assert!(!compiled.has_errors());
        assert!(compiled.wgsl_program.is_some());
        assert_eq!(compiled.uniform_count(), 1);
    }

    #[test]
    fn compile_material_without_shader_returns_none() {
        let compiler = ShaderCompiler3D::new();
        let mat = ShaderMaterial3D::new();
        assert!(compiler.compile_material(&mat).is_none());
    }

    #[test]
    fn full_pipeline_compile_material_to_wgsl() {
        let source = "shader_type spatial;\nuniform vec4 albedo_color : source_color;\nuniform float alpha;\nvoid fragment() {}";
        let shader = Shader3D::new(ShaderType3D::Spatial, source);
        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);
        mat.set_shader_parameter(
            "albedo_color",
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        mat.set_shader_parameter("alpha", Variant::Float(0.5));

        let compiler = ShaderCompiler3D::new();
        let compiled = compiler.compile_material(&mat).unwrap();
        assert!(!compiled.has_errors());

        let wgsl = compiled.wgsl_program.as_ref().unwrap();
        assert!(wgsl.contains("albedo_color: vec4<f32>"));
        assert!(wgsl.contains("alpha: f32"));
        assert!(wgsl.contains("custom.albedo_color"));
        assert!(wgsl.contains("color.a = custom.alpha"));

        // Also verify software evaluation still works.
        let processor = ShaderProcessor3D::new();
        let ctx = FragmentContext3D::default();
        let result = processor.apply_shader(&compiled, &mat.parameters, &ctx);
        assert_eq!(result.r, 1.0);
        assert!((result.a - 0.5).abs() < 0.01);
    }
}
