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

/// Re-exported so renderers can build a sampler without depending on
/// `gdresource` themselves (pat-s3rrv.36).
pub use gdresource::shader_ast::Sampler;

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

/// Where a uniform's value comes from (pat-0q2kx.225).
///
/// Godot 4 prefixes a declaration to widen its scope: `global uniform vec4 t;`
/// reads a project-wide value shared by every shader that names it, and
/// `instance uniform` gives each instance its own. A plain `uniform` is
/// per-material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UniformScope {
    /// Plain `uniform` — per-material, set through the ShaderMaterial.
    #[default]
    Local,
    /// `global uniform` — resolved from the project-wide global store.
    Global,
    /// `instance uniform` — per-instance override.
    Instance,
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
    /// Plain, `global`, or `instance` (pat-0q2kx.225).
    pub scope: UniformScope,
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
    /// Loaded images for `sampler2D` uniforms, keyed by uniform name
    /// (pat-s3rrv.122).
    ///
    /// 🔑 SEPARATE FROM `parameters` BECAUSE A VARIANT CANNOT CARRY PIXELS. A
    /// `shader_parameter/<sampler>` value is a resource path; something has to
    /// load it into a [`TextureImage`] and put it here, which is what makes
    /// `texture(tex, UV)` sample anything at all. Without this the fragment
    /// path's `sample_texture` callback has nothing to look up and returns the
    /// white placeholder — the reason custom-shader texturing was reachable
    /// only from tests.
    pub textures: HashMap<String, std::sync::Arc<crate::material::TextureImage>>,
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
    /// Binds a loaded image to a `sampler2D` uniform by name.
    pub fn set_texture(
        &mut self,
        name: &str,
        image: std::sync::Arc<crate::material::TextureImage>,
    ) {
        self.textures.insert(name.to_string(), image);
    }

    /// The image bound to a `sampler2D` uniform, if any.
    pub fn get_texture(
        &self,
        name: &str,
    ) -> Option<&std::sync::Arc<crate::material::TextureImage>> {
        self.textures.get(name)
    }

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
        // Godot widens a uniform's scope with a prefix. Matching only
        // `uniform ` silently DROPPED every `global uniform` / `instance
        // uniform` line, so those declarations vanished rather than erroring
        // (pat-0q2kx.225). Order matters: the scoped forms must be tested
        // first, since they also end in `uniform `.
        let (scope, decl) = if let Some(rest) = trimmed.strip_prefix("global uniform ") {
            (UniformScope::Global, rest)
        } else if let Some(rest) = trimmed.strip_prefix("instance uniform ") {
            (UniformScope::Instance, rest)
        } else if let Some(rest) = trimmed.strip_prefix("uniform ") {
            (UniformScope::Local, rest)
        } else {
            continue;
        };
        // Strip trailing semicolon. "TYPE name [: hint] [= default]"
        let rest = decl.trim_end_matches(';').trim();
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
            scope,
        });
    }
    uniforms
}

/// A parsed shader uniform hint — the structured interpretation of the raw
/// `hint` string on a [`ShaderUniform3D`] (Godot's uniform hint grammar).
#[derive(Debug, Clone, PartialEq)]
pub enum ShaderHint {
    /// `source_color` / `hint_color`: the value is authored in sRGB and should
    /// be decoded to linear before use.
    SourceColor,
    /// `hint_range(min, max[, step])`: clamps the value and drives an inspector
    /// slider.
    Range {
        /// Inclusive minimum.
        min: f32,
        /// Inclusive maximum.
        max: f32,
        /// Optional step increment.
        step: Option<f32>,
    },
    /// `hint_normal`: the sampler texture is a normal map.
    Normal,
    /// `filter_*` sampler filtering mode (the raw keyword, e.g. `filter_linear`).
    Filter(String),
    /// `repeat_enable` (true) / `repeat_disable` (false) sampler wrap mode.
    Repeat(bool),
}

/// Parses a raw shader uniform hint string into a structured [`ShaderHint`], or
/// `None` when the hint is empty or unrecognized.
pub fn parse_shader_hint(hint: &str) -> Option<ShaderHint> {
    let hint = hint.trim();
    match hint {
        "source_color" | "hint_color" => return Some(ShaderHint::SourceColor),
        "hint_normal" => return Some(ShaderHint::Normal),
        "repeat_enable" => return Some(ShaderHint::Repeat(true)),
        "repeat_disable" => return Some(ShaderHint::Repeat(false)),
        _ => {}
    }
    if hint.starts_with("filter_") {
        return Some(ShaderHint::Filter(hint.to_string()));
    }
    if let Some(rest) = hint.strip_prefix("hint_range") {
        let inner = rest.trim().strip_prefix('(')?.strip_suffix(')')?;
        let parts: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        if parts.len() >= 2 {
            return Some(ShaderHint::Range {
                min: parts[0],
                max: parts[1],
                step: parts.get(2).copied(),
            });
        }
    }
    None
}

impl ShaderUniform3D {
    /// Parses this uniform's raw `hint` into a structured [`ShaderHint`], or
    /// `None` when there is no hint or it is unrecognized.
    pub fn parsed_hint(&self) -> Option<ShaderHint> {
        self.hint.as_deref().and_then(parse_shader_hint)
    }
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

/// Best-effort default value parse for uniform literals (pat-s3rrv.36).
///
/// Delegates to the shared parser in `gdresource`, which both renderers now
/// use. This copy understood only `true`/`false`/int/float, so every vector
/// default became `Variant::Nil` and the uniform read as zero — and the 2D
/// renderer carried a byte-identical copy with the same hole.
fn parse_default_value(s: &str) -> Variant {
    gdresource::shader_tokenizer::parse_shader_default_value(s)
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
    /// Parsed `fragment()` body instructions for CPU (software) evaluation.
    ///
    /// Populated by [`parse_fragment_body_3d`]; empty when the shader has no
    /// recognised fragment statements (uniform-only shaders still evaluate via
    /// [`ShaderProcessor3D::apply_shader`]).
    pub fragment_body: Vec<FragmentInstruction3D>,
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

/// Memo for [`ShaderCompiler3D::compile_cached`] (pat-s3rrv.36).
type CompileCacheKey = (ShaderType3D, String);
static COMPILE_CACHE: std::sync::Mutex<Option<HashMap<CompileCacheKey, CompiledShader3D>>> =
    std::sync::Mutex::new(None);

fn compile_cache(
) -> std::sync::MutexGuard<'static, Option<HashMap<CompileCacheKey, CompiledShader3D>>> {
    COMPILE_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Empties the compile cache -- for tests, and for an editor reloading a shader
/// whose source changed on disk under the same identity (pat-s3rrv.36).
pub fn clear_compile_cache() {
    *compile_cache() = None;
}

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
    /// [`compile`](Self::compile), memoized on `(shader_type, source)`
    /// (pat-s3rrv.36).
    ///
    /// `CustomFragmentShader::from_material` (gdrender3d) calls `compile` for
    /// EVERY instance on EVERY frame, so a scene with N shader-material
    /// instances re-tokenized, re-parsed and re-generated WGSL N times per
    /// frame for sources that never changed. This returns a clone of the cached
    /// result instead, which still copies the struct but skips all of that work.
    ///
    /// Keyed by the full source text rather than a hash: shader sources are
    /// small, and a hash collision would silently hand back the wrong program --
    /// a bug that would be near-impossible to diagnose from the symptom.
    pub fn compile_cached(&self, shader_type: ShaderType3D, source: &str) -> CompiledShader3D {
        let key = (shader_type, source.to_string());
        {
            let guard = compile_cache();
            if let Some(hit) = guard.as_ref().and_then(|m| m.get(&key)) {
                return hit.clone();
            }
        }
        // Compiled OUTSIDE the lock: compilation is the expensive part, and
        // holding the mutex across it would serialize every renderer thread on
        // the first frame. A concurrent duplicate compile is harmless -- both
        // produce the same value and the second simply overwrites.
        let compiled = self.compile(shader_type, source);
        compile_cache()
            .get_or_insert_with(HashMap::new)
            .insert(key, compiled.clone());
        compiled
    }

    pub fn compile(&self, shader_type: ShaderType3D, source: &str) -> CompiledShader3D {
        let uniforms = parse_uniforms(source);
        let render_modes = parse_render_modes(source);
        let mut diagnostics = validate_shader(source, shader_type);

        // pat-s3rrv.36: a source the GDShader front end cannot parse becomes a
        // line-numbered diagnostic here, in the channel the editor already
        // surfaces. `validate_shader` above only checks a handful of shapes, so
        // before this a syntactically broken shader compiled "successfully",
        // produced an empty fragment_body, and rendered as plain albedo with
        // nothing to tell the author why. Godot reports these at compile time.
        //
        // Reported as a WARNING, not an Error: `has_errors()` gates WGSL
        // generation and the GPU path, and the line matcher can still handle
        // sources the AST rejects. Demoting a working shader to no-GPU because
        // a stricter front end disliked it would be a regression.
        if let Err(e) = gdresource::shader_ast::parse_shader(source) {
            diagnostics.push(ShaderDiagnostic {
                // `ShaderError::line` is 1-based; this field is 0-based.
                line: e.line.saturating_sub(1),
                severity: DiagnosticSeverity::Warning,
                message: format!("shader language: {}", e.message),
            });
        }

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
            fragment_body: parse_fragment_body_3d(source),
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
    source: &str,
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

    // Prefer real fragment-body execution: if the shader's `fragment()` body
    // transpiles to WGSL (UV math, arithmetic, mix, vec constructors), emit that
    // so `color` reflects the actual shader source — not a hardcoded set of
    // magic uniform names. Shaders whose body references custom uniforms,
    // textures, or `TIME` (none of which the base pipeline binds) fall back to
    // the uniform-override path below, preserving the previous behaviour.
    let body = parse_fragment_body_3d(source);
    let transpiled = if body.is_empty() {
        None
    } else {
        fragment_body_to_wgsl_statements(&body)
    };
    if let Some(stmts) = transpiled {
        for s in &stmts {
            wgsl.push_str("    ");
            wgsl.push_str(s);
            wgsl.push('\n');
        }
    } else {
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
                wgsl.push_str(
                    "    color = mix(color, custom.color_mix, custom.color_mix_factor);\n",
                );
            } else {
                wgsl.push_str("    color = mix(color, custom.color_mix, 0.5);\n");
            }
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

// ---------------------------------------------------------------------------
// 3D fragment AST, parser, and evaluation
// ---------------------------------------------------------------------------

/// A single instruction extracted from a Godot spatial (3D) shader fragment
/// body.
///
/// This is a small but real AST: it captures the most common Godot spatial
/// shader assignment patterns (writing `ALBEDO`, `ALPHA`, `COLOR`) so the
/// software fallback can evaluate them per-fragment. Right-hand sides are
/// [`ColorExpr3D`] / [`FloatExpr3D`] trees supporting UV math, `TIME`, and
/// texture sampling.
#[derive(Debug, Clone, PartialEq)]
pub enum FragmentInstruction3D {
    /// `COLOR = <expr>` — sets the full RGBA output color.
    SetColor(ColorExpr3D),
    /// `COLOR.rgb = <expr>` — sets only the RGB channels of the output.
    SetColorRgb(ColorExpr3D),
    /// `COLOR.a = <expr>` — sets only the alpha channel of the output.
    SetColorAlpha(FloatExpr3D),
    /// `ALBEDO = <expr>` — sets the surface albedo RGB (alpha preserved).
    SetAlbedo(ColorExpr3D),
    /// `ALPHA = <expr>` — sets the surface alpha (Godot's scalar `ALPHA`).
    SetAlpha(FloatExpr3D),
    /// `ALBEDO.rgb *= <float>` / `COLOR.rgb *= <float>` — scales RGB.
    MultiplyColorRgb(FloatExpr3D),
    /// The whole shader, parsed by the real GDShader front end (pat-s3rrv.36).
    ///
    /// The variants above are a six-shape line matcher that silently skips any
    /// statement it cannot classify. When `gdresource::shader_ast` can parse the
    /// source, the entire program is carried here and evaluated per fragment,
    /// mirroring `FragmentInstruction::Program` on the 2D side.
    Program(std::sync::Arc<gdresource::shader_ast::ShaderProgram>),
}

/// An expression that evaluates to a color value in a 3D fragment shader.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorExpr3D {
    /// A `vec4(r, g, b, a)` literal.
    Literal(f32, f32, f32, f32),
    /// A `vec4(v)` shorthand (all four components the same).
    Splat(f32),
    /// A `vec3(a, b, c)` with float-expression components (alpha = 1.0).
    Vec3(FloatExpr3D, FloatExpr3D, FloatExpr3D),
    /// Reference to a uniform by name.
    Uniform(String),
    /// The built-in `COLOR` variable.
    BuiltinColor,
    /// The built-in `ALBEDO` variable.
    BuiltinAlbedo,
    /// `mix(a, b, t)` — linear interpolation of two colors by `t`.
    Mix(Box<ColorExpr3D>, Box<ColorExpr3D>, FloatExpr3D),
    /// `texture(sampler, uv)` — sampler name plus the `u` and `v` coordinates.
    Texture(String, Box<FloatExpr3D>, Box<FloatExpr3D>),
}

/// An expression that evaluates to a float value in a 3D fragment shader.
#[derive(Debug, Clone, PartialEq)]
pub enum FloatExpr3D {
    /// A literal float.
    Literal(f32),
    /// A component of the sky pass's `EYEDIR` built-in — the normalized
    /// direction from the camera through the pixel being shaded (pat-0q2kx.224).
    ///
    /// Read from [`FragmentContext3D::view_dir`], which the sky renderer sets to
    /// the sample direction. Outside a sky shader that field is the ordinary
    /// view vector, so `EYEDIR` in a `fragment` body resolves to it rather than
    /// to zero — the same latitude Godot takes with cross-stage built-ins.
    EyeDir(Vec3Axis3D),
    /// A reference to a uniform.
    Uniform(String),
    /// The built-in `TIME` variable.
    BuiltinTime,
    /// `UV.x` or `UV.y`.
    UvComponent(UvAxis3D),
    /// `sin(expr)`.
    Sin(Box<FloatExpr3D>),
    /// `cos(expr)`.
    Cos(Box<FloatExpr3D>),
    /// `fract(expr)` — fractional part (`x - floor(x)`).
    Fract(Box<FloatExpr3D>),
    /// `a + b`.
    Add(Box<FloatExpr3D>, Box<FloatExpr3D>),
    /// `a - b`.
    Sub(Box<FloatExpr3D>, Box<FloatExpr3D>),
    /// `a * b`.
    Mul(Box<FloatExpr3D>, Box<FloatExpr3D>),
    /// `-a`.
    Neg(Box<FloatExpr3D>),
}

/// Which component of `UV` (a spatial shader's `vec2` texture coordinate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvAxis3D {
    /// The `x` (horizontal) component.
    X,
    /// The `y` (vertical) component.
    Y,
}

/// Which component of a `vec3` built-in such as `EYEDIR` (pat-0q2kx.224).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vec3Axis3D {
    /// The `x` component.
    X,
    /// The `y` component.
    Y,
    /// The `z` component.
    Z,
}

/// Parses a Godot spatial shader's `fragment()` function body into
/// instructions for software evaluation.
///
/// Returns an empty vec if no `fragment()` body is found. Statements the
/// parser does not recognise are skipped rather than causing a panic.
/// Parses a Godot **sky** shader's `sky()` function body into the same
/// instruction set the `fragment()` body uses (pat-0q2kx.224).
///
/// Godot ground truth: `shader_types.cpp`'s sky section — a `shader_type sky`
/// shader writes `COLOR` per direction from `EYEDIR`/`TIME`/uniforms. The
/// statement grammar is shared with `fragment()`, so only the function name
/// differs; `COLOR` is the output in both.
///
/// (`gdserver2d::parse_sky_body` also exists but has no runtime callers and a
/// narrower expression set — no `vec3(a, b, c)` components — and lives in a crate
/// `gdserver3d` does not depend on.)
pub fn parse_sky_body_3d(source: &str) -> Vec<FragmentInstruction3D> {
    parse_named_body_3d(source, "sky")
}

pub fn parse_fragment_body_3d(source: &str) -> Vec<FragmentInstruction3D> {
    parse_named_body_3d(source, "fragment")
}

/// Shared body walk for [`parse_fragment_body_3d`] and [`parse_sky_body_3d`].
fn parse_named_body_3d(source: &str, func: &str) -> Vec<FragmentInstruction3D> {
    let mut instructions = parse_named_body_3d_matched(source, func);
    // pat-s3rrv.36: when the line matcher understood NOTHING, hand the whole
    // source to the real front end. A fallback, not a replacement — a shader the
    // matcher already handles keeps its exact path, so nothing that renders
    // correctly today can regress. Mirrors the 2D side.
    //
    // Restricted to `fragment` on purpose: `parse_sky_body_3d` shares this
    // function, but `execute_fragment_3d` evaluates a Program by calling its
    // `fragment()` entry. A sky Program would therefore look for a function it
    // does not have, silently produce nothing, and be harder to diagnose than
    // the matcher's current behaviour. Extending the AST path to the sky stage
    // means carrying the entry-point name in the variant.
    if instructions.is_empty() && func == "fragment" {
        if let Ok(program) = gdresource::shader_ast::parse_shader(source) {
            if program.function(func).is_some() {
                return vec![FragmentInstruction3D::Program(std::sync::Arc::new(program))];
            }
        }
    }
    instructions.shrink_to_fit();
    instructions
}

fn parse_named_body_3d_matched(source: &str, func: &str) -> Vec<FragmentInstruction3D> {
    let mut instructions = Vec::new();

    let body = match extract_function_body_3d(source, func) {
        Some(b) => b,
        None => return instructions,
    };

    for line in body.lines() {
        let trimmed = line.trim().trim_end_matches(';').trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(inst) = parse_fragment_statement_3d(trimmed) {
            instructions.push(inst);
        }
    }

    instructions
}

/// Extracts the body of a named function from shader source (private copy for
/// this crate — mirrors the 2D helper).
fn extract_function_body_3d<'a>(source: &'a str, func_name: &str) -> Option<&'a str> {
    let pattern = format!("{func_name}(");
    let func_pos = source.find(&pattern)?;

    let after_sig = &source[func_pos..];
    let brace_pos = after_sig.find('{')?;
    let body_start = func_pos + brace_pos + 1;

    let mut depth = 1;
    let mut end = body_start;
    for ch in source[body_start..].chars() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[body_start..end]);
                }
            }
            _ => {}
        }
        end += ch.len_utf8();
    }
    None
}

/// Parses a single fragment statement into an instruction, or `None` if the
/// statement is not recognised.
fn parse_fragment_statement_3d(s: &str) -> Option<FragmentInstruction3D> {
    // ALBEDO.rgb = / *=  (checked before plain ALBEDO).
    if let Some(rest) = s.strip_prefix("ALBEDO.rgb") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            return Some(FragmentInstruction3D::MultiplyColorRgb(
                parse_float_expr_3d(val.trim()),
            ));
        }
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetColorRgb(parse_color_expr_3d(
                val.trim(),
            )));
        }
    }

    // ALBEDO = / *=
    if let Some(rest) = s.strip_prefix("ALBEDO") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            return Some(FragmentInstruction3D::MultiplyColorRgb(
                parse_float_expr_3d(val.trim()),
            ));
        }
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetAlbedo(parse_color_expr_3d(
                val.trim(),
            )));
        }
    }

    // ALPHA = <float>
    if let Some(rest) = s.strip_prefix("ALPHA") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetAlpha(parse_float_expr_3d(
                val.trim(),
            )));
        }
    }

    // COLOR.a = <float>
    if let Some(rest) = s.strip_prefix("COLOR.a") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetColorAlpha(parse_float_expr_3d(
                val.trim(),
            )));
        }
    }

    // COLOR.rgb = / *=
    if let Some(rest) = s.strip_prefix("COLOR.rgb") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            return Some(FragmentInstruction3D::MultiplyColorRgb(
                parse_float_expr_3d(val.trim()),
            ));
        }
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetColorRgb(parse_color_expr_3d(
                val.trim(),
            )));
        }
    }

    // COLOR = / *=
    if let Some(rest) = s.strip_prefix("COLOR") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            return Some(FragmentInstruction3D::MultiplyColorRgb(
                parse_float_expr_3d(val.trim()),
            ));
        }
        if let Some(val) = rest.strip_prefix('=') {
            return Some(FragmentInstruction3D::SetColor(parse_color_expr_3d(
                val.trim(),
            )));
        }
    }

    None
}

/// Parses a color expression from source text.
fn parse_color_expr_3d(s: &str) -> ColorExpr3D {
    let s = strip_color_swizzle_3d(s.trim());

    // vec4(r, g, b, a) or vec4(v) — literal components only.
    if let Some(inner) = strip_func_call_3d(s, "vec4") {
        let parts = split_top_level_commas_3d(inner);
        if parts.len() == 4 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                parts[0].trim().parse::<f32>(),
                parts[1].trim().parse::<f32>(),
                parts[2].trim().parse::<f32>(),
                parts[3].trim().parse::<f32>(),
            ) {
                return ColorExpr3D::Literal(r, g, b, a);
            }
        } else if parts.len() == 1 {
            if let Ok(v) = parts[0].trim().parse::<f32>() {
                return ColorExpr3D::Splat(v);
            }
        }
    }

    // vec3(a, b, c) with float-expression components (alpha = 1.0).
    if let Some(inner) = strip_func_call_3d(s, "vec3") {
        let parts = split_top_level_commas_3d(inner);
        if parts.len() == 3 {
            return ColorExpr3D::Vec3(
                parse_float_expr_3d(parts[0]),
                parse_float_expr_3d(parts[1]),
                parse_float_expr_3d(parts[2]),
            );
        } else if parts.len() == 1 {
            // vec3(x) shorthand → (x, x, x).
            let e = parse_float_expr_3d(parts[0]);
            return ColorExpr3D::Vec3(e.clone(), e.clone(), e);
        }
    }

    // mix(a, b, t)
    if let Some(inner) = strip_func_call_3d(s, "mix") {
        let parts = split_top_level_commas_3d(inner);
        if parts.len() == 3 {
            let a = parse_color_expr_3d(parts[0]);
            let b = parse_color_expr_3d(parts[1]);
            let t = parse_float_expr_3d(parts[2]);
            return ColorExpr3D::Mix(Box::new(a), Box::new(b), t);
        }
    }

    // texture(sampler, uvexpr) where uvexpr is `UV` or `vec2(u, v)`.
    if let Some(inner) = strip_func_call_3d(s, "texture") {
        let parts = split_top_level_commas_3d(inner);
        if parts.len() == 2 {
            let sampler = parts[0].trim().to_string();
            let uv = parts[1].trim();
            let (u, v) = if uv == "UV" {
                (
                    FloatExpr3D::UvComponent(UvAxis3D::X),
                    FloatExpr3D::UvComponent(UvAxis3D::Y),
                )
            } else if let Some(vec_inner) = strip_func_call_3d(uv, "vec2") {
                let uv_parts = split_top_level_commas_3d(vec_inner);
                if uv_parts.len() == 2 {
                    (
                        parse_float_expr_3d(uv_parts[0]),
                        parse_float_expr_3d(uv_parts[1]),
                    )
                } else {
                    (
                        FloatExpr3D::UvComponent(UvAxis3D::X),
                        FloatExpr3D::UvComponent(UvAxis3D::Y),
                    )
                }
            } else {
                // Best-effort default: sample at the interpolated UV.
                (
                    FloatExpr3D::UvComponent(UvAxis3D::X),
                    FloatExpr3D::UvComponent(UvAxis3D::Y),
                )
            };
            return ColorExpr3D::Texture(sampler, Box::new(u), Box::new(v));
        }
    }

    if s == "ALBEDO" {
        return ColorExpr3D::BuiltinAlbedo;
    }
    if s == "COLOR" {
        return ColorExpr3D::BuiltinColor;
    }

    // Uniform reference (a bare identifier).
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return ColorExpr3D::Uniform(s.to_string());
    }

    // Unrecognised → treat as an albedo passthrough.
    ColorExpr3D::BuiltinAlbedo
}

/// Parses a float expression, supporting left-associative binary arithmetic
/// (`+`, `-`, `*`) with `*` binding tighter than `+`/`-`, unary functions
/// (`sin`, `cos`, `fract`), unary negation, `TIME`, `UV.x`/`UV.y`, numeric
/// literals, and uniform identifiers.
fn parse_float_expr_3d(s: &str) -> FloatExpr3D {
    let s = strip_outer_parens_3d(s.trim());

    // A bare numeric literal (also handles negatives / scientific notation).
    if let Ok(v) = s.parse::<f32>() {
        return FloatExpr3D::Literal(v);
    }

    // Top-level binary operator (lowest precedence, rightmost occurrence).
    if let Some((idx, op)) = find_top_level_binop_3d(s) {
        let left = parse_float_expr_3d(&s[..idx]);
        let right = parse_float_expr_3d(&s[idx + 1..]);
        return match op {
            '+' => FloatExpr3D::Add(Box::new(left), Box::new(right)),
            '-' => FloatExpr3D::Sub(Box::new(left), Box::new(right)),
            _ => FloatExpr3D::Mul(Box::new(left), Box::new(right)),
        };
    }

    // Unary negation (e.g. `-UV.x`, `-sin(TIME)`).
    if let Some(rest) = s.strip_prefix('-') {
        return FloatExpr3D::Neg(Box::new(parse_float_expr_3d(rest)));
    }

    // Unary functions.
    if let Some(inner) = strip_func_call_3d(s, "sin") {
        return FloatExpr3D::Sin(Box::new(parse_float_expr_3d(inner)));
    }
    if let Some(inner) = strip_func_call_3d(s, "cos") {
        return FloatExpr3D::Cos(Box::new(parse_float_expr_3d(inner)));
    }
    if let Some(inner) = strip_func_call_3d(s, "fract") {
        return FloatExpr3D::Fract(Box::new(parse_float_expr_3d(inner)));
    }

    if s == "TIME" {
        return FloatExpr3D::BuiltinTime;
    }
    if s == "UV.x" {
        return FloatExpr3D::UvComponent(UvAxis3D::X);
    }
    if s == "UV.y" {
        return FloatExpr3D::UvComponent(UvAxis3D::Y);
    }
    // `EYEDIR` — the sky pass's per-pixel eye direction (pat-0q2kx.224).
    if s == "EYEDIR.x" {
        return FloatExpr3D::EyeDir(Vec3Axis3D::X);
    }
    if s == "EYEDIR.y" {
        return FloatExpr3D::EyeDir(Vec3Axis3D::Y);
    }
    if s == "EYEDIR.z" {
        return FloatExpr3D::EyeDir(Vec3Axis3D::Z);
    }

    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return FloatExpr3D::Uniform(s.to_string());
    }

    FloatExpr3D::Literal(0.0)
}

/// Drops a trailing `.rgb` / `.rgba` swizzle from a colour-valued expression.
///
/// Godot writes `ALBEDO = texture(tex, UV).rgb` as readily as
/// `ALBEDO = texture(tex, UV)`, and both mean the same thing to an ALBEDO
/// assignment: [`ColorExpr3D`] already carries RGBA, and ALPHA is a separate
/// builtin that an ALBEDO write does not touch. So the swizzle is dropped
/// rather than modelled.
///
/// ⚠️ Without this, the swizzled form parsed to **nothing usable**. Every
/// branch of [`parse_color_expr_3d`] is anchored on the whole string —
/// `strip_func_call_3d` requires `s.ends_with(')')`, and the bare-identifier
/// branch rejects `(`, `,` and `.` — so `texture(tex, UV).rgb` fell through to
/// the `BuiltinAlbedo` catch-all. That is a silent passthrough of the material
/// albedo, so a bound texture rendered as flat WHITE and looked exactly like an
/// unbound sampler hitting the placeholder (pat-s3rrv.122).
fn strip_color_swizzle_3d(s: &str) -> &str {
    // `.rgba` first: a plain `.rgb` prefix test would leave a stray `a`.
    for suffix in [".rgba", ".rgb"] {
        if let Some(base) = s.strip_suffix(suffix) {
            let base = base.trim_end();
            if !base.is_empty() {
                return base;
            }
        }
    }
    s
}

/// Strips a function call wrapper like `func(inner)` and returns `inner`.
fn strip_func_call_3d<'a>(s: &'a str, func: &str) -> Option<&'a str> {
    let prefix = format!("{func}(");
    if s.starts_with(&prefix) && s.ends_with(')') {
        Some(&s[prefix.len()..s.len() - 1])
    } else {
        None
    }
}

/// Splits a string by commas, but only at the top level (respecting parens).
fn split_top_level_commas_3d(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Removes one or more layers of fully-enclosing parentheses from `s`.
///
/// Only strips when the leading `(` matches the trailing `)` (i.e. they wrap
/// the entire expression), so `(a+b)*(c+d)` is left untouched.
fn strip_outer_parens_3d(s: &str) -> &str {
    let s = s.trim();
    if !(s.starts_with('(') && s.ends_with(')')) {
        return s;
    }
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    // If the matching close paren is the last char, strip and recurse.
                    if i == s.len() - ch.len_utf8() {
                        return strip_outer_parens_3d(&s[1..s.len() - 1]);
                    }
                    return s;
                }
            }
            _ => {}
        }
    }
    s
}

/// Finds the split point for a top-level binary operator in `s`.
///
/// Returns `(byte_index, operator)` for the rightmost top-level additive
/// operator (`+`/`-`); if none is present, the rightmost top-level `*`.
/// Unary signs (an operator with no preceding operand) are ignored so
/// expressions like `-UV.x + 1.0` split at the `+`.
fn find_top_level_binop_3d(s: &str) -> Option<(usize, char)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut depth = 0i32;
    let mut add_split: Option<(usize, char)> = None;
    let mut mul_split: Option<usize> = None;

    for k in 0..chars.len() {
        let (i, ch) = chars[k];
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '+' | '-' if depth == 0 && is_binary_op_position_3d(&chars, k) => {
                add_split = Some((i, ch)); // rightmost wins
            }
            '*' if depth == 0 && is_binary_op_position_3d(&chars, k) => {
                mul_split = Some(i); // rightmost wins
            }
            _ => {}
        }
    }

    if let Some(split) = add_split {
        return Some(split);
    }
    mul_split.map(|i| (i, '*'))
}

/// Returns `true` if the operator at index `k` is a binary operator (has a
/// preceding operand) rather than a unary sign.
fn is_binary_op_position_3d(chars: &[(usize, char)], k: usize) -> bool {
    let mut j = k;
    while j > 0 {
        j -= 1;
        let c = chars[j].1;
        if c.is_whitespace() {
            continue;
        }
        return !matches!(c, '+' | '-' | '*' | '/' | '(' | ',');
    }
    false
}

/// Evaluates a float expression against a fragment context and uniforms.
/// A uniform's value for evaluation: the material's own parameter first, then
/// the project-wide global store (pat-0q2kx.225).
///
/// A `global uniform` never appears in a material's parameter map — that map
/// holds what the ShaderMaterial itself sets — so falling through to the store
/// is what makes `global uniform vec4 tint;` actually read the project value.
/// A material parameter of the same name deliberately shadows the global, which
/// matches Godot: the more specific binding wins.
fn uniform_value(uniforms: &HashMap<String, Variant>, name: &str) -> Option<Variant> {
    if let Some(v) = uniforms.get(name) {
        return Some(v.clone());
    }
    let global = gdvariant::shader_globals::get(name);
    (global != Variant::Nil).then_some(global)
}

pub fn eval_float_expr_3d(
    expr: &FloatExpr3D,
    ctx: &FragmentContext3D,
    uniforms: &HashMap<String, Variant>,
) -> f32 {
    match expr {
        FloatExpr3D::Literal(v) => *v,
        FloatExpr3D::BuiltinTime => ctx.time,
        FloatExpr3D::UvComponent(UvAxis3D::X) => ctx.uv.0,
        FloatExpr3D::UvComponent(UvAxis3D::Y) => ctx.uv.1,
        // The sky renderer puts the sample direction in `view_dir`.
        FloatExpr3D::EyeDir(Vec3Axis3D::X) => ctx.view_dir.x,
        FloatExpr3D::EyeDir(Vec3Axis3D::Y) => ctx.view_dir.y,
        FloatExpr3D::EyeDir(Vec3Axis3D::Z) => ctx.view_dir.z,
        FloatExpr3D::Uniform(name) => match uniform_value(uniforms, name) {
            Some(Variant::Float(f)) => f as f32,
            Some(Variant::Int(i)) => i as f32,
            _ => 0.0,
        },
        FloatExpr3D::Sin(inner) => eval_float_expr_3d(inner, ctx, uniforms).sin(),
        FloatExpr3D::Cos(inner) => eval_float_expr_3d(inner, ctx, uniforms).cos(),
        FloatExpr3D::Fract(inner) => {
            let x = eval_float_expr_3d(inner, ctx, uniforms);
            x - x.floor()
        }
        FloatExpr3D::Add(a, b) => {
            eval_float_expr_3d(a, ctx, uniforms) + eval_float_expr_3d(b, ctx, uniforms)
        }
        FloatExpr3D::Sub(a, b) => {
            eval_float_expr_3d(a, ctx, uniforms) - eval_float_expr_3d(b, ctx, uniforms)
        }
        FloatExpr3D::Mul(a, b) => {
            eval_float_expr_3d(a, ctx, uniforms) * eval_float_expr_3d(b, ctx, uniforms)
        }
        FloatExpr3D::Neg(inner) => -eval_float_expr_3d(inner, ctx, uniforms),
    }
}

/// Evaluates a color expression against a fragment context and uniforms.
///
/// `sample_texture` abstracts texture lookup (name, u, v) → color so this
/// crate needs no image dependencies; the software renderer supplies a
/// closure that samples the bound texture maps.
pub fn eval_color_expr_3d(
    expr: &ColorExpr3D,
    ctx: &FragmentContext3D,
    uniforms: &HashMap<String, Variant>,
    sample_texture: &dyn Fn(&str, f32, f32) -> Color,
) -> Color {
    match expr {
        ColorExpr3D::Literal(r, g, b, a) => Color::new(*r, *g, *b, *a),
        ColorExpr3D::Splat(v) => Color::new(*v, *v, *v, *v),
        ColorExpr3D::Vec3(a, b, c) => Color::new(
            eval_float_expr_3d(a, ctx, uniforms),
            eval_float_expr_3d(b, ctx, uniforms),
            eval_float_expr_3d(c, ctx, uniforms),
            1.0,
        ),
        ColorExpr3D::BuiltinColor | ColorExpr3D::BuiltinAlbedo => ctx.albedo,
        ColorExpr3D::Uniform(name) => match uniform_value(uniforms, name) {
            Some(Variant::Color(c)) => c,
            _ => ctx.albedo,
        },
        ColorExpr3D::Mix(a, b, t) => {
            let ca = eval_color_expr_3d(a, ctx, uniforms, sample_texture);
            let cb = eval_color_expr_3d(b, ctx, uniforms, sample_texture);
            let t = eval_float_expr_3d(t, ctx, uniforms).clamp(0.0, 1.0);
            Color::new(
                ca.r * (1.0 - t) + cb.r * t,
                ca.g * (1.0 - t) + cb.g * t,
                ca.b * (1.0 - t) + cb.b * t,
                ca.a * (1.0 - t) + cb.a * t,
            )
        }
        ColorExpr3D::Texture(name, u, v) => {
            let uu = eval_float_expr_3d(u, ctx, uniforms);
            let vv = eval_float_expr_3d(v, ctx, uniforms);
            sample_texture(name.as_str(), uu, vv)
        }
    }
}

/// Executes parsed 3D fragment instructions against a context, returning the
/// resulting output color.
///
/// The base color starts from `ctx.albedo`. Texture sampling is delegated to
/// `sample_texture` so this crate carries no image dependencies.
pub fn execute_fragment_3d(
    instructions: &[FragmentInstruction3D],
    ctx: &FragmentContext3D,
    uniforms: &HashMap<String, Variant>,
    sample_texture: &dyn Fn(&str, f32, f32) -> Color,
) -> Color {
    execute_fragment_3d_with_env(
        instructions,
        ctx,
        uniforms,
        sample_texture,
        &ShaderEnv3D::default(),
    )
}

/// Converts a material parameter into a shader value (pat-s3rrv.36).
fn uniform_to_shader_value_3d(v: &Variant) -> Option<gdresource::shader_ast::ShaderValue> {
    use gdresource::shader_ast::ShaderValue as SV;
    Some(match v {
        Variant::Bool(b) => SV::Bool(*b),
        Variant::Int(i) => SV::Int(*i),
        Variant::Float(f) => SV::Float(*f as f32),
        Variant::Vector2(v) => SV::Vec2([v.x, v.y]),
        Variant::Vector3(v) => SV::Vec3([v.x, v.y, v.z]),
        Variant::Color(c) => SV::Vec4([c.r, c.g, c.b, c.a]),
        _ => return None,
    })
}

/// Runs a parsed spatial shader's `fragment()` for one fragment (pat-s3rrv.36).
///
/// Returns `None` when evaluation fails or the shader `discard`s, leaving the
/// caller's colour untouched.
///
/// Godot's spatial fragment stage writes ALBEDO (vec3) and ALPHA (float)
/// separately, unlike canvas_item's single vec4 COLOR — so both are seeded and
/// both are read back, with ALPHA winning over the albedo's own alpha when the
/// shader set it.
fn eval_ast_fragment_3d(
    program: &gdresource::shader_ast::ShaderProgram,
    ctx: &FragmentContext3D,
    uniforms: &HashMap<String, Variant>,
    env: &ShaderEnv3D,
) -> Option<Color> {
    use gdresource::shader_ast::{eval_function, ShaderValue as SV, StageContext};

    let uniform_values: HashMap<String, SV> = uniforms
        .iter()
        .filter_map(|(k, v)| uniform_to_shader_value_3d(v).map(|sv| (k.clone(), sv)))
        .collect();

    let mut builtins: HashMap<String, SV> = HashMap::new();
    builtins.insert(
        "ALBEDO".to_string(),
        SV::Vec3([ctx.albedo.r, ctx.albedo.g, ctx.albedo.b]),
    );
    builtins.insert("ALPHA".to_string(), SV::Float(ctx.albedo.a));
    builtins.insert(
        "COLOR".to_string(),
        SV::Vec4([ctx.albedo.r, ctx.albedo.g, ctx.albedo.b, ctx.albedo.a]),
    );
    builtins.insert("UV".to_string(), SV::Vec2([ctx.uv.0, ctx.uv.1]));
    builtins.insert(
        "NORMAL".to_string(),
        SV::Vec3([ctx.normal.x, ctx.normal.y, ctx.normal.z]),
    );
    builtins.insert(
        "VERTEX".to_string(),
        SV::Vec3([
            ctx.world_position.x,
            ctx.world_position.y,
            ctx.world_position.z,
        ]),
    );
    builtins.insert(
        "VIEW".to_string(),
        SV::Vec3([ctx.view_dir.x, ctx.view_dir.y, ctx.view_dir.z]),
    );
    builtins.insert("ROUGHNESS".to_string(), SV::Float(ctx.roughness));
    builtins.insert("METALLIC".to_string(), SV::Float(ctx.metallic));
    builtins.insert("TIME".to_string(), SV::Float(ctx.time));

    let mut stage = StageContext::new(uniform_values, builtins);
    if let Some(sampler) = &env.sampler {
        stage = stage.with_sampler(sampler.clone());
    }

    eval_function(program, "fragment", &mut stage).ok()?;
    if stage.discarded {
        return None;
    }

    // ALPHA is authoritative when written; otherwise fall back to whatever the
    // vec4 COLOR path produced, then to the incoming alpha.
    let alpha = match stage.builtins.get("ALPHA") {
        Some(v) => v.as_f32(),
        None => ctx.albedo.a,
    };
    match stage.builtins.get("ALBEDO") {
        Some(SV::Vec3(c)) => Some(Color::new(c[0], c[1], c[2], alpha)),
        _ => match stage.builtins.get("COLOR") {
            Some(SV::Vec4(c)) => Some(Color::new(c[0], c[1], c[2], c[3])),
            _ => None,
        },
    }
}

/// Renderer-supplied environment for the AST fragment path (pat-s3rrv.36).
///
/// The legacy instruction arms take their texture lookup as a borrowed
/// `&dyn Fn`, but `shader_ast::Sampler` is `'static`, so the AST path needs an
/// OWNED closure. The renderer builds one cheaply by capturing an
/// `Arc`-shared texture map — see `CustomFragmentShader::shade`.
#[derive(Debug, Clone, Default)]
pub struct ShaderEnv3D {
    /// Resolves `texture(SAMPLER, uv)` for the AST path. `None` samples
    /// transparent black.
    pub sampler: Option<gdresource::shader_ast::Sampler>,
}

/// [`execute_fragment_3d`] with the environment the AST path needs
/// (pat-s3rrv.36).
pub fn execute_fragment_3d_with_env(
    instructions: &[FragmentInstruction3D],
    ctx: &FragmentContext3D,
    uniforms: &HashMap<String, Variant>,
    sample_texture: &dyn Fn(&str, f32, f32) -> Color,
    env: &ShaderEnv3D,
) -> Color {
    let mut color = ctx.albedo;

    for inst in instructions {
        match inst {
            FragmentInstruction3D::Program(program) => {
                if let Some(c) = eval_ast_fragment_3d(program, ctx, uniforms, env) {
                    color = c;
                }
            }
            FragmentInstruction3D::SetColor(expr) => {
                color = eval_color_expr_3d(expr, ctx, uniforms, sample_texture);
            }
            FragmentInstruction3D::SetColorRgb(expr) | FragmentInstruction3D::SetAlbedo(expr) => {
                let c = eval_color_expr_3d(expr, ctx, uniforms, sample_texture);
                color.r = c.r;
                color.g = c.g;
                color.b = c.b;
            }
            FragmentInstruction3D::SetColorAlpha(expr) | FragmentInstruction3D::SetAlpha(expr) => {
                color.a = eval_float_expr_3d(expr, ctx, uniforms);
            }
            FragmentInstruction3D::MultiplyColorRgb(expr) => {
                let f = eval_float_expr_3d(expr, ctx, uniforms);
                color.r *= f;
                color.g *= f;
                color.b *= f;
            }
        }
    }

    color
}

// ---------------------------------------------------------------------------
// 3D fragment AST → WGSL transpilation (GPU path)
// ---------------------------------------------------------------------------

/// Formats an `f32` as a WGSL float literal.
///
/// Rust's `{:?}` for floats always emits a decimal point or exponent (`1.0`,
/// `0.5`, `1e-5`), all of which are valid WGSL float literals — so the value is
/// typed `f32` rather than an abstract int. Non-finite values collapse to `0.0`
/// since WGSL has no `inf`/`nan` literals.
/// A GDShader uniform name as a WGSL struct field (pat-s3rrv.122).
///
/// Both languages take `[A-Za-z_][A-Za-z0-9_]*`, so this only has to defend
/// against a leading digit and stray characters — it is a rename, not an
/// escape, and the emitted struct field must be generated from the SAME
/// function so the reference and the declaration cannot disagree.
pub fn wgsl_uniform_field(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 1);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if i == 0 && c.is_ascii_digit() {
                out.push('_');
            }
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn wgsl_f32(v: f32) -> String {
    if !v.is_finite() {
        return "0.0".to_string();
    }
    format!("{:?}", v)
}

/// Transpiles a [`FloatExpr3D`] to a WGSL expression string, or `None` when the
/// expression references a feature the base spatial pipeline does not bind
/// (custom uniforms or the `TIME` built-in). `UV` maps to the interpolated
/// `in.uv` fragment input.
fn float_expr_to_wgsl(expr: &FloatExpr3D) -> Option<String> {
    Some(match expr {
        FloatExpr3D::Literal(v) => wgsl_f32(*v),
        FloatExpr3D::UvComponent(UvAxis3D::X) => "in.uv.x".to_string(),
        FloatExpr3D::UvComponent(UvAxis3D::Y) => "in.uv.y".to_string(),
        // TIME and custom uniforms read from the per-material uniform block
        // the custom pipeline binds as group 3 (pat-s3rrv.122). Every field is
        // a `vec4<f32>` so the struct needs no alignment padding; a scalar
        // takes `.x`.
        FloatExpr3D::BuiltinTime => "custom.time.x".to_string(),
        FloatExpr3D::Uniform(n) => format!("custom.{}.x", wgsl_uniform_field(n)),
        FloatExpr3D::Sin(e) => format!("sin({})", float_expr_to_wgsl(e)?),
        FloatExpr3D::Cos(e) => format!("cos({})", float_expr_to_wgsl(e)?),
        FloatExpr3D::Fract(e) => format!("fract({})", float_expr_to_wgsl(e)?),
        FloatExpr3D::Add(a, b) => {
            format!("({} + {})", float_expr_to_wgsl(a)?, float_expr_to_wgsl(b)?)
        }
        FloatExpr3D::Sub(a, b) => {
            format!("({} - {})", float_expr_to_wgsl(a)?, float_expr_to_wgsl(b)?)
        }
        FloatExpr3D::Mul(a, b) => {
            format!("({} * {})", float_expr_to_wgsl(a)?, float_expr_to_wgsl(b)?)
        }
        FloatExpr3D::Neg(e) => format!("(-{})", float_expr_to_wgsl(e)?),
        // `EYEDIR` belongs to the sky pass, which is a separate WGSL module from
        // the spatial fragment shader this transpiles. Declining (as with TIME
        // and custom uniforms above) is right; emitting a stand-in vector would
        // silently render the wrong sky on the GPU path (pat-0q2kx.224).
        FloatExpr3D::EyeDir(_) => return None,
    })
}

/// Transpiles a [`ColorExpr3D`] to a WGSL `vec4<f32>` expression string, or
/// `None` when it references a custom uniform or texture sample (GPU custom
/// uniform/texture binding is a follow-up). `COLOR`/`ALBEDO` refer to the
/// running `color` accumulator.
fn color_expr_to_wgsl(expr: &ColorExpr3D) -> Option<String> {
    Some(match expr {
        ColorExpr3D::Literal(r, g, b, a) => format!(
            "vec4<f32>({}, {}, {}, {})",
            wgsl_f32(*r),
            wgsl_f32(*g),
            wgsl_f32(*b),
            wgsl_f32(*a)
        ),
        ColorExpr3D::Splat(v) => format!("vec4<f32>({})", wgsl_f32(*v)),
        ColorExpr3D::Vec3(a, b, c) => format!(
            "vec4<f32>({}, {}, {}, 1.0)",
            float_expr_to_wgsl(a)?,
            float_expr_to_wgsl(b)?,
            float_expr_to_wgsl(c)?
        ),
        ColorExpr3D::BuiltinColor | ColorExpr3D::BuiltinAlbedo => "color".to_string(),
        ColorExpr3D::Uniform(n) => format!("custom.{}", wgsl_uniform_field(n)),
        ColorExpr3D::Mix(a, b, t) => format!(
            "mix({}, {}, {})",
            color_expr_to_wgsl(a)?,
            color_expr_to_wgsl(b)?,
            float_expr_to_wgsl(t)?
        ),
        // `texture(sampler, uv)` reads the group-3 sampler pair the custom
        // pipeline binds. Named from the SAME `wgsl_uniform_field` the uniform
        // block uses, so a sampler and a buffer field can never collide
        // (pat-s3rrv.122).
        ColorExpr3D::Texture(name, u, v) => format!(
            "textureSample(t_{0}, s_{0}, vec2<f32>({1}, {2}))",
            wgsl_uniform_field(name),
            float_expr_to_wgsl(u)?,
            float_expr_to_wgsl(v)?
        ),
    })
}

/// Transpiles parsed fragment-body instructions into WGSL statements that
/// mutate a mutable `color: vec4<f32>` variable (which the caller initialises to
/// the base albedo). Returns `None` if any instruction references a feature not
/// yet supported on the GPU path (custom uniforms, textures, or `TIME`), so the
/// caller can fall back to the flat-albedo path rather than emit invalid WGSL.
///
/// This is the GPU-side counterpart to [`execute_fragment_3d`] (the CPU
/// interpreter); both consume the same [`FragmentInstruction3D`] AST so the two
/// backends agree.
pub fn fragment_body_to_wgsl_statements(
    instructions: &[FragmentInstruction3D],
) -> Option<Vec<String>> {
    let mut stmts = Vec::with_capacity(instructions.len());
    for inst in instructions {
        let stmt = match inst {
            FragmentInstruction3D::SetColor(e) => {
                format!("color = {};", color_expr_to_wgsl(e)?)
            }
            FragmentInstruction3D::SetColorRgb(e) | FragmentInstruction3D::SetAlbedo(e) => {
                format!(
                    "color = vec4<f32>(({}).rgb, color.a);",
                    color_expr_to_wgsl(e)?
                )
            }
            FragmentInstruction3D::SetColorAlpha(e) | FragmentInstruction3D::SetAlpha(e) => {
                format!("color.a = {};", float_expr_to_wgsl(e)?)
            }
            FragmentInstruction3D::MultiplyColorRgb(e) => {
                format!(
                    "color = vec4<f32>(color.rgb * {}, color.a);",
                    float_expr_to_wgsl(e)?
                )
            }
            // pat-s3rrv.36: an AST-routed shader has no WGSL translation yet.
            // Declining the whole body (`None`) is the honest answer — the
            // other arms already bail that way when an expression cannot be
            // translated, and the caller falls back to the CPU interpreter,
            // which CAN evaluate a Program. Emitting partial WGSL that silently
            // dropped the fragment body would render the wrong thing on GPU
            // while the CPU path rendered the right one.
            FragmentInstruction3D::Program(_) => return None,
        };
        stmts.push(stmt);
    }
    Some(stmts)
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

    // pat-nzgdu.88: raw uniform hint strings parse into structured ShaderHint.
    // (The hint *semantics*, global/instance uniforms, and #include are a
    // separate compiler/interpreter lane.)
    #[test]
    fn shader_uniform_hints_parse() {
        assert_eq!(
            parse_shader_hint("source_color"),
            Some(ShaderHint::SourceColor)
        );
        assert_eq!(
            parse_shader_hint("hint_color"),
            Some(ShaderHint::SourceColor)
        );
        assert_eq!(parse_shader_hint("hint_normal"), Some(ShaderHint::Normal));
        assert_eq!(
            parse_shader_hint("repeat_enable"),
            Some(ShaderHint::Repeat(true))
        );
        assert_eq!(
            parse_shader_hint("repeat_disable"),
            Some(ShaderHint::Repeat(false))
        );
        assert_eq!(
            parse_shader_hint("filter_linear"),
            Some(ShaderHint::Filter("filter_linear".to_string()))
        );
        assert_eq!(
            parse_shader_hint("hint_range(0.0, 100.0)"),
            Some(ShaderHint::Range {
                min: 0.0,
                max: 100.0,
                step: None
            })
        );
        assert_eq!(
            parse_shader_hint("hint_range(0, 10, 0.5)"),
            Some(ShaderHint::Range {
                min: 0.0,
                max: 10.0,
                step: Some(0.5)
            })
        );
        assert_eq!(parse_shader_hint("bogus"), None);
        assert_eq!(parse_shader_hint(""), None);

        // Round-trip: a `source_color`-hinted uniform parsed from shader source
        // exposes a structured SourceColor hint.
        let shader = Shader3D::new(
            ShaderType3D::Spatial,
            "shader_type spatial;\nuniform vec4 albedo : source_color = vec4(1.0);\n",
        );
        let u = shader.get_uniform("albedo").expect("albedo uniform parsed");
        assert_eq!(u.parsed_hint(), Some(ShaderHint::SourceColor));

        // A uniform with no hint yields None.
        let plain = Shader3D::new(
            ShaderType3D::Spatial,
            "shader_type spatial;\nuniform float strength = 1.0;\n",
        );
        assert_eq!(
            plain.get_uniform("strength").and_then(|u| u.parsed_hint()),
            None
        );
    }

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
                scope: UniformScope::Local,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
            fragment_body: Vec::new(),
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
            fragment_body: Vec::new(),
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
                scope: UniformScope::Local,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
            fragment_body: Vec::new(),
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
                scope: UniformScope::Local,
            }],
            program: String::new(),
            wgsl_program: None,
            render_modes: RenderModeFlags::default(),
            diagnostics: Vec::new(),
            fragment_body: Vec::new(),
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
            fragment_body: Vec::new(),
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

    // ── WGSL fragment-body transpilation (real body execution, not magic
    //    uniforms) ──

    #[test]
    fn wgsl_generation_reflects_uv_gradient_body() {
        // A `fragment()` body that writes ALBEDO from UV must transpile to WGSL
        // that reads the interpolated `in.uv` — not a hardcoded uniform override.
        let compiler = ShaderCompiler3D::new();
        let source = "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(UV.x, UV.y, 1.0); }";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled
            .wgsl_program
            .as_ref()
            .expect("WGSL should be generated");

        assert!(
            wgsl.contains("in.uv.x"),
            "WGSL should reference UV.x:\n{wgsl}"
        );
        assert!(wgsl.contains("in.uv.y"), "WGSL should reference UV.y");
        assert!(wgsl.contains("fn custom_fragment"));
    }

    #[test]
    fn fragment_body_to_wgsl_statements_uv_gradient() {
        let body = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(UV.x, UV.y, 1.0); }",
        );
        assert!(
            !body.is_empty(),
            "UV gradient body should parse to instructions"
        );
        let stmts = fragment_body_to_wgsl_statements(&body).expect("UV gradient transpiles");
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("in.uv.x"));
        assert!(stmts[0].contains("in.uv.y"));
        assert!(stmts[0].starts_with("color = vec4<f32>("));
    }

    #[test]
    fn fragment_body_to_wgsl_statements_arithmetic_and_fns() {
        // sin/fract/arithmetic over UV all transpile to WGSL intrinsics.
        let body = vec![FragmentInstruction3D::SetAlpha(FloatExpr3D::Fract(
            Box::new(FloatExpr3D::Mul(
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                Box::new(FloatExpr3D::Literal(4.0)),
            )),
        ))];
        let stmts = fragment_body_to_wgsl_statements(&body).unwrap();
        assert_eq!(stmts[0], "color.a = fract((in.uv.x * 4.0));");
    }

    #[test]
    fn fragment_body_to_wgsl_statements_bails_on_custom_uniform() {
        // A body referencing a custom uniform is not bindable in the base
        // spatial pipeline (no group(3)), so transpilation bails to None.
        let body = vec![FragmentInstruction3D::SetAlpha(FloatExpr3D::Uniform(
            "alpha".into(),
        ))];
        assert!(fragment_body_to_wgsl_statements(&body).is_none());
    }

    #[test]
    fn fragment_body_to_wgsl_statements_bails_on_time() {
        let body = vec![FragmentInstruction3D::SetAlpha(FloatExpr3D::BuiltinTime)];
        assert!(fragment_body_to_wgsl_statements(&body).is_none());
    }

    #[test]
    fn wgsl_generation_uniform_only_body_keeps_override_fallback() {
        // An empty body with an albedo_color uniform still emits the legacy
        // uniform-override WGSL (transpilation only replaces recognised bodies).
        let compiler = ShaderCompiler3D::new();
        let source =
            "shader_type spatial;\nuniform vec4 albedo_color : source_color;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        let wgsl = compiled.wgsl_program.as_ref().unwrap();
        assert!(wgsl.contains("custom.albedo_color"));
    }

    #[test]
    fn wgsl_f32_always_typed_float() {
        assert_eq!(wgsl_f32(1.0), "1.0");
        assert_eq!(wgsl_f32(0.5), "0.5");
        // Non-finite values collapse to a valid literal.
        assert_eq!(wgsl_f32(f32::NAN), "0.0");
        assert_eq!(wgsl_f32(f32::INFINITY), "0.0");
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

    // ── 3D fragment AST parsing ──

    /// A texture sampler stub that ignores its arguments and returns black.
    fn no_texture(_name: &str, _u: f32, _v: f32) -> Color {
        Color::new(0.0, 0.0, 0.0, 1.0)
    }

    #[test]
    fn parse_albedo_vec3_literal() {
        let source =
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = vec3(1.0, 0.0, 0.0);\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Vec3(
                FloatExpr3D::Literal(1.0),
                FloatExpr3D::Literal(0.0),
                FloatExpr3D::Literal(0.0),
            ))]
        );
    }

    #[test]
    fn parse_albedo_uniform() {
        let source = "shader_type spatial;\nvoid fragment() {\n    ALBEDO = my_color;\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Uniform(
                "my_color".to_string()
            ))]
        );
    }

    #[test]
    fn parse_albedo_uv_math() {
        let source =
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = vec3(UV.x, UV.y, 0.0);\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Vec3(
                FloatExpr3D::UvComponent(UvAxis3D::X),
                FloatExpr3D::UvComponent(UvAxis3D::Y),
                FloatExpr3D::Literal(0.0),
            ))]
        );
    }

    #[test]
    fn parse_alpha_sin_time() {
        let source = "shader_type spatial;\nvoid fragment() {\n    ALPHA = sin(TIME);\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlpha(FloatExpr3D::Sin(Box::new(
                FloatExpr3D::BuiltinTime
            )))]
        );
    }

    #[test]
    fn parse_texture_uv() {
        let source = "shader_type spatial;\nvoid fragment() {\n    ALBEDO = texture(tex, UV);\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Texture(
                "tex".to_string(),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::Y)),
            ))]
        );
    }

    /// pat-s3rrv.122: the swizzled form must parse to the SAME node as the
    /// bare one.
    ///
    /// ⚠️ Asserted against `parse_texture_uv`'s expected value rather than
    /// merely "not BuiltinAlbedo": the old failure WAS `BuiltinAlbedo`, which
    /// is a legitimate node, so a weaker assertion would pass on a parser that
    /// silently dropped the texture again.
    #[test]
    fn parse_texture_uv_rgb_swizzle_matches_the_bare_form() {
        let bare = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = texture(tex, UV);\n}",
        );
        let swizzled = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = texture(tex, UV).rgb;\n}",
        );
        assert_eq!(
            swizzled, bare,
            "`texture(tex, UV).rgb` must parse exactly like `texture(tex, UV)`; \
             falling through to BuiltinAlbedo renders a bound texture as flat \
             white, indistinguishable from an unbound sampler"
        );
        assert_eq!(
            swizzled,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Texture(
                "tex".to_string(),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::Y)),
            ))]
        );
    }

    /// `.rgba` too, and a swizzle on a plain uniform reference.
    #[test]
    fn parse_color_swizzle_handles_rgba_and_uniforms() {
        let rgba = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = texture(tex, UV).rgba;\n}",
        );
        assert_eq!(
            rgba,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Texture(
                "tex".to_string(),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::Y)),
            ))]
        );

        let uniform = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = tint.rgb;\n}",
        );
        assert_eq!(
            uniform,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Uniform(
                "tint".to_string()
            ))]
        );
    }

    /// ⚠️ A bare `.rgb` has no base expression and must NOT be stripped into an
    /// empty string — the guard that keeps the helper from inventing a node out
    /// of nothing.
    #[test]
    fn parse_color_swizzle_leaves_a_bare_swizzle_alone() {
        assert_eq!(strip_color_swizzle_3d(".rgb"), ".rgb");
        assert_eq!(strip_color_swizzle_3d("tint"), "tint");
        assert_eq!(
            strip_color_swizzle_3d("texture(t, UV).rgb"),
            "texture(t, UV)"
        );
    }

    #[test]
    fn parse_unsupported_statement_skipped() {
        // `NORMAL = ...` and a local `float` declaration are unsupported and
        // must be skipped, while the ALBEDO assignment is still parsed.
        let source = "shader_type spatial;\nvoid fragment() {\n    float k = 0.5;\n    NORMAL = vec3(0.0, 0.0, 1.0);\n    ALBEDO = vec3(0.2, 0.2, 0.2);\n}";
        let body = parse_fragment_body_3d(source);
        assert_eq!(
            body,
            vec![FragmentInstruction3D::SetAlbedo(ColorExpr3D::Vec3(
                FloatExpr3D::Literal(0.2),
                FloatExpr3D::Literal(0.2),
                FloatExpr3D::Literal(0.2),
            ))]
        );
    }

    #[test]
    fn parse_float_expr_precedence_and_parens() {
        // `a + b * c` must parse as Add(a, Mul(b, c)).
        assert_eq!(
            parse_float_expr_3d("UV.x + UV.y * 2.0"),
            FloatExpr3D::Add(
                Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                Box::new(FloatExpr3D::Mul(
                    Box::new(FloatExpr3D::UvComponent(UvAxis3D::Y)),
                    Box::new(FloatExpr3D::Literal(2.0)),
                )),
            )
        );
        // Parentheses override precedence.
        assert_eq!(
            parse_float_expr_3d("(UV.x + UV.y) * 2.0"),
            FloatExpr3D::Mul(
                Box::new(FloatExpr3D::Add(
                    Box::new(FloatExpr3D::UvComponent(UvAxis3D::X)),
                    Box::new(FloatExpr3D::UvComponent(UvAxis3D::Y)),
                )),
                Box::new(FloatExpr3D::Literal(2.0)),
            )
        );
    }

    // ── 3D fragment execution ──

    #[test]
    fn eval_albedo_uv_math_varies_with_uv() {
        let body = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = vec3(UV.x, UV.y, 0.0);\n}",
        );
        let ctx = FragmentContext3D {
            uv: (0.25, 0.75),
            ..Default::default()
        };
        let out = execute_fragment_3d(&body, &ctx, &HashMap::new(), &no_texture);
        assert!((out.r - 0.25).abs() < 1e-6, "r={}", out.r);
        assert!((out.g - 0.75).abs() < 1e-6, "g={}", out.g);
        assert!(out.b.abs() < 1e-6, "b={}", out.b);
        // SetAlbedo preserves the original alpha (default white = 1.0).
        assert!((out.a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn eval_time_animates_alpha() {
        let body = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALPHA = sin(TIME);\n}",
        );
        let at0 = execute_fragment_3d(
            &body,
            &FragmentContext3D {
                time: 0.0,
                ..Default::default()
            },
            &HashMap::new(),
            &no_texture,
        );
        let at_half_pi = execute_fragment_3d(
            &body,
            &FragmentContext3D {
                time: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
            &HashMap::new(),
            &no_texture,
        );
        assert!(at0.a.abs() < 1e-6, "sin(0) alpha={}", at0.a);
        assert!(
            (at_half_pi.a - 1.0).abs() < 1e-6,
            "sin(pi/2) alpha={}",
            at_half_pi.a
        );
    }

    #[test]
    fn eval_uniform_albedo() {
        let body = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = tint;\n}",
        );
        let mut uniforms = HashMap::new();
        uniforms.insert(
            "tint".to_string(),
            Variant::Color(Color::new(0.1, 0.2, 0.3, 0.5)),
        );
        let ctx = FragmentContext3D::default();
        let out = execute_fragment_3d(&body, &ctx, &uniforms, &no_texture);
        assert!((out.r - 0.1).abs() < 1e-6);
        assert!((out.g - 0.2).abs() < 1e-6);
        assert!((out.b - 0.3).abs() < 1e-6);
        // SetAlbedo keeps the context alpha, not the uniform's alpha.
        assert!((out.a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn eval_texture_sample_uses_closure() {
        let body = parse_fragment_body_3d(
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = texture(tex, UV);\n}",
        );
        let ctx = FragmentContext3D {
            uv: (0.4, 0.6),
            ..Default::default()
        };
        // Stub returns a color encoding the (name, u, v) it was called with.
        let out = execute_fragment_3d(
            &body,
            &ctx,
            &HashMap::new(),
            &|name: &str, u: f32, v: f32| {
                assert_eq!(name, "tex");
                Color::new(u, v, 0.0, 1.0)
            },
        );
        assert!((out.r - 0.4).abs() < 1e-6, "r={}", out.r);
        assert!((out.g - 0.6).abs() < 1e-6, "g={}", out.g);
    }

    #[test]
    fn eval_float_expr_precedence_matches_math() {
        // Confirm evaluation (not just parsing) respects precedence.
        let expr = parse_float_expr_3d("UV.x + UV.y * 2.0");
        let ctx = FragmentContext3D {
            uv: (0.5, 0.25),
            ..Default::default()
        };
        // 0.5 + 0.25 * 2.0 = 1.0 (not (0.5 + 0.25) * 2.0 = 1.5).
        let v = eval_float_expr_3d(&expr, &ctx, &HashMap::new());
        assert!((v - 1.0).abs() < 1e-6, "v={v}");
    }

    #[test]
    fn eval_fract_and_neg() {
        let ctx = FragmentContext3D {
            time: 2.75,
            ..Default::default()
        };
        let fract = eval_float_expr_3d(&parse_float_expr_3d("fract(TIME)"), &ctx, &HashMap::new());
        assert!((fract - 0.75).abs() < 1e-6, "fract={fract}");
        let neg = eval_float_expr_3d(&parse_float_expr_3d("-UV.x"), &ctx, &HashMap::new());
        assert!(neg.abs() < 1e-6);
    }

    #[test]
    fn compile_populates_fragment_body() {
        let compiler = ShaderCompiler3D::new();
        let source =
            "shader_type spatial;\nvoid fragment() {\n    ALBEDO = vec3(1.0, 0.0, 0.0);\n}";
        let compiled = compiler.compile(ShaderType3D::Spatial, source);
        assert_eq!(compiled.fragment_body.len(), 1);
    }
}
