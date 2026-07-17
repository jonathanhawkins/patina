//! Shader and shader material definitions.
//!
//! Provides types for Godot-compatible shaders: shader source parsing,
//! uniform extraction, a basic token lexer, and `ShaderMaterial` for
//! binding parameter values at runtime.

use gdvariant::variant::Variant;
use std::collections::HashMap;

/// The type of shader program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderType {
    /// 3D surface / spatial shader.
    Spatial,
    /// 2D / canvas item shader.
    CanvasItem,
    /// GPU particle shader.
    Particles,
    /// Sky / environment shader.
    Sky,
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
pub struct ShaderUniform {
    /// The uniform variable name.
    pub name: String,
    /// The data type.
    pub uniform_type: UniformType,
    /// Optional default value.
    pub default_value: Variant,
    /// Optional hint string (e.g. "source_color").
    pub hint: Option<String>,
}

/// A parsed shader program.
#[derive(Debug, Clone, PartialEq)]
pub struct Shader {
    /// The shader pipeline stage.
    pub shader_type: ShaderType,
    /// Raw shader source code.
    pub source_code: String,
    /// Uniforms extracted from the source.
    pub uniforms: Vec<ShaderUniform>,
}

impl Shader {
    /// Create a new shader, parsing uniforms from `source`.
    pub fn new(shader_type: ShaderType, source: &str) -> Self {
        let uniforms = parse_uniforms(source);
        Self {
            shader_type,
            source_code: source.to_string(),
            uniforms,
        }
    }

    /// Look up a uniform by name.
    pub fn get_uniform(&self, name: &str) -> Option<&ShaderUniform> {
        self.uniforms.iter().find(|u| u.name == name)
    }
}

/// Parse `uniform TYPE name [: hint] [= default];` lines from shader source.
///
/// This is intentionally simplified — it handles the most common Godot
/// shader uniform declarations without a full GLSL/Godot-shading-language parser.
pub fn parse_uniforms(source: &str) -> Vec<ShaderUniform> {
    let mut uniforms = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("uniform ") {
            continue;
        }
        // Strip trailing semicolon
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

        // Split remainder into name, optional hint, optional default
        let (name_hint, default_value) = if let Some(eq_pos) = remainder.find('=') {
            let name_part = remainder[..eq_pos].trim();
            let val_part = remainder[eq_pos + 1..].trim();
            (name_part, parse_default_value(val_part, uniform_type))
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

        uniforms.push(ShaderUniform {
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
fn parse_default_value(s: &str, _utype: UniformType) -> Variant {
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
// ShaderMaterial
// ---------------------------------------------------------------------------

/// A material driven by a custom `Shader` with runtime parameter overrides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShaderMaterial {
    /// The attached shader (if any).
    pub shader: Option<Shader>,
    /// Runtime parameter overrides keyed by uniform name.
    pub parameters: HashMap<String, Variant>,
}

impl ShaderMaterial {
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
}

// ---------------------------------------------------------------------------
// Shader compiler and processor
// ---------------------------------------------------------------------------

/// A compiled shader program ready for execution.
///
/// Stores the parsed uniforms, the original source, an optional WGSL
/// translation (for `canvas_item` shaders — the GPU-runnable subset), and the
/// list of source constructs that fell outside the translated subset.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledShader {
    /// The shader pipeline stage.
    pub shader_type: ShaderType,
    /// Parsed uniform declarations.
    pub uniforms: Vec<ShaderUniform>,
    /// The compiled program representation (the original source).
    pub program: String,
    /// Generated WGSL fragment program for GPU execution.
    ///
    /// `Some` for `canvas_item` shaders (the supported subset); `None` for
    /// shader stages this backend does not translate to WGSL.
    pub wgsl_program: Option<String>,
    /// Human-readable list of source constructs that are *not* represented in
    /// the WGSL translation (see [`unsupported_features`]). Empty when the
    /// whole shader lies within the supported subset.
    pub unsupported: Vec<String>,
}

impl CompiledShader {
    /// Returns the number of uniforms in this shader.
    pub fn uniform_count(&self) -> usize {
        self.uniforms.len()
    }

    /// Looks up a uniform by name.
    pub fn get_uniform(&self, name: &str) -> Option<&ShaderUniform> {
        self.uniforms.iter().find(|u| u.name == name)
    }
}

/// Compiles shader source into a [`CompiledShader`].
///
/// Currently this parses uniforms and stores the source as the "program".
/// A future implementation would produce bytecode or an intermediate representation.
#[derive(Debug, Default)]
pub struct ShaderCompiler;

impl ShaderCompiler {
    /// Creates a new shader compiler.
    pub fn new() -> Self {
        Self
    }

    /// Compiles shader source into a [`CompiledShader`].
    ///
    /// For `canvas_item` shaders this also translates the recognised
    /// `fragment()` subset into a runnable WGSL fragment program (see
    /// [`generate_canvas_item_wgsl`]) and records any source constructs that
    /// fell outside the supported subset (see [`unsupported_features`]).
    pub fn compile(&self, shader_type: ShaderType, source: &str) -> CompiledShader {
        let uniforms = parse_uniforms(source);
        let unsupported = unsupported_features(source, shader_type);
        let wgsl_program = match shader_type {
            ShaderType::CanvasItem => Some(generate_canvas_item_wgsl(&uniforms, source)),
            // Spatial shaders are translated by `gdserver3d`; particle/sky
            // stages are not part of this backend's supported subset.
            _ => None,
        };
        CompiledShader {
            shader_type,
            uniforms,
            program: source.to_string(),
            wgsl_program,
            unsupported,
        }
    }
}

/// Evaluates compiled shaders against pixel data.
///
/// Currently supports a minimal evaluation model: if the shader has an
/// `albedo_color` uniform and a matching value is provided, that color is
/// returned. Otherwise the input pixel color is passed through unchanged.
#[derive(Debug, Default)]
pub struct ShaderProcessor;

impl ShaderProcessor {
    /// Creates a new shader processor.
    pub fn new() -> Self {
        Self
    }

    /// Applies a compiled shader to a single pixel.
    ///
    /// `uniforms` maps uniform names to runtime values.
    /// `pixel` is the current fragment color.
    ///
    /// Returns the output color after "executing" the shader.
    pub fn apply_shader(
        &self,
        shader: &CompiledShader,
        uniforms: &HashMap<String, Variant>,
        pixel: gdcore::math::Color,
    ) -> gdcore::math::Color {
        // Check for albedo_color uniform override.
        if let Some(Variant::Color(c)) = uniforms.get("albedo_color") {
            return *c;
        }
        // Check for color_override uniform.
        if let Some(Variant::Color(c)) = uniforms.get("color_override") {
            return *c;
        }
        // Check shader's own default albedo_color.
        if let Some(u) = shader.get_uniform("albedo_color") {
            if let Variant::Color(c) = &u.default_value {
                return *c;
            }
        }
        // Passthrough: return the input pixel unchanged.
        pixel
    }
}

// ---------------------------------------------------------------------------
// Fragment function parsing and execution
// ---------------------------------------------------------------------------

/// Built-in variables available to a fragment shader during execution.
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentContext {
    /// The current fragment color (Godot's `COLOR` built-in).
    pub color: gdcore::math::Color,
    /// Normalised UV coordinate of the current fragment (0..1).
    pub uv: (f32, f32),
    /// Normalised screen-space UV (0..1).
    pub screen_uv: (f32, f32),
    /// Time elapsed in seconds (for animated shaders).
    pub time: f32,
    /// The sky stage's view direction (Godot's `EYEDIR`), as a unit vector
    /// (pat-nzgdu.139). Unused — and left at zero — by canvas_item shaders,
    /// which have no such built-in.
    pub eyedir: (f32, f32, f32),
}

impl Default for FragmentContext {
    fn default() -> Self {
        Self {
            color: gdcore::math::Color::WHITE,
            uv: (0.0, 0.0),
            screen_uv: (0.0, 0.0),
            time: 0.0,
            eyedir: (0.0, 0.0, 0.0),
        }
    }
}

/// A simple instruction extracted from a Godot shader fragment body.
///
/// This is not a full AST — it captures the most common Godot shader
/// patterns so the software fallback can evaluate them.
#[derive(Debug, Clone, PartialEq)]
pub enum FragmentInstruction {
    /// `COLOR = <expr>` — sets the output color.
    SetColor(ColorExpr),
    /// `COLOR.rgb = <expr>` — sets only the RGB channels.
    SetColorRgb(ColorExpr),
    /// `COLOR.a = <expr>` — sets only the alpha channel.
    SetColorAlpha(FloatExpr),
    /// `COLOR.rgb *= <float>` — multiplies RGB by a scalar.
    MultiplyColorRgb(FloatExpr),
    /// `COLOR *= <float>` — multiplies all channels by a scalar.
    MultiplyColor(FloatExpr),
}

/// An expression that evaluates to a color value.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorExpr {
    /// A `vec4(r, g, b, a)` literal.
    Literal(f32, f32, f32, f32),
    /// A `vec4(v)` shorthand (all components the same).
    Splat(f32),
    /// Reference to a uniform by name.
    Uniform(String),
    /// The built-in `COLOR` variable (passthrough).
    BuiltinColor,
    /// `mix(a, b, t)` — linear interpolation of two colors.
    Mix(Box<ColorExpr>, Box<ColorExpr>, FloatExpr),
    /// A `vec3`/`vec4` built from per-component expressions, e.g.
    /// `vec3(EYEDIR.y, 0.0, 1.0)` (pat-nzgdu.139).
    ///
    /// [`Literal`](Self::Literal) only covers constant components; this covers
    /// the case where a component references a built-in or uniform. A `vec3`
    /// parses with an alpha of `1.0`.
    Components(
        Box<FloatExpr>,
        Box<FloatExpr>,
        Box<FloatExpr>,
        Box<FloatExpr>,
    ),
}

/// An expression that evaluates to a float value.
#[derive(Debug, Clone, PartialEq)]
pub enum FloatExpr {
    /// A literal float.
    Literal(f32),
    /// A reference to a uniform.
    Uniform(String),
    /// The built-in `TIME` variable.
    BuiltinTime,
    /// `UV.x` or `UV.y`.
    UvComponent(UvAxis),
    /// `EYEDIR.x`, `.y` or `.z` — the sky stage's view direction
    /// (pat-nzgdu.139). Evaluates to `0.0` outside a sky shader, where Godot
    /// does not expose this built-in.
    EyeDirComponent(Vec3Axis),
    /// `sin(expr)`.
    Sin(Box<FloatExpr>),
}

/// Which component of UV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvAxis {
    X,
    Y,
}

/// Which component of a `vec3` built-in (e.g. `EYEDIR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vec3Axis {
    /// The `x` component.
    X,
    /// The `y` component.
    Y,
    /// The `z` component.
    Z,
}

/// Parses a Godot shader's `fragment()` function body into instructions.
///
/// Returns an empty vec if no `fragment()` body is found or if the body
/// contains constructs beyond what this parser supports.
pub fn parse_fragment_body(source: &str) -> Vec<FragmentInstruction> {
    let mut instructions = Vec::new();

    // Find fragment() body.
    let body = match extract_function_body(source, "fragment") {
        Some(b) => b,
        None => return instructions,
    };

    // Parse statements line-by-line (simplified).
    for line in body.lines() {
        let trimmed = line.trim().trim_end_matches(';').trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        if let Some(inst) = parse_fragment_statement(trimmed) {
            instructions.push(inst);
        }
    }

    instructions
}

/// Parses a `shader_type sky` program's `sky()` function body into the same
/// instruction set as [`parse_fragment_body`] (pat-nzgdu.139).
///
/// Godot's sky stage writes `COLOR` (a `vec3`) from built-ins such as `EYEDIR`.
/// Execute the result with [`execute_fragment`], passing a [`FragmentContext`]
/// whose [`eyedir`](FragmentContext::eyedir) is the view direction for the pixel
/// being shaded.
///
/// Returns an empty vec when the source has no `sky()` body — so a canvas_item
/// shader parsed as sky (or vice versa) yields nothing rather than misfiring.
pub fn parse_sky_body(source: &str) -> Vec<FragmentInstruction> {
    let mut instructions = Vec::new();

    let body = match extract_function_body(source, "sky") {
        Some(b) => b,
        None => return instructions,
    };

    for line in body.lines() {
        let trimmed = line.trim().trim_end_matches(';').trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(inst) = parse_fragment_statement(trimmed) {
            instructions.push(inst);
        }
    }

    instructions
}

/// Extracts the body of a named function from shader source.
fn extract_function_body<'a>(source: &'a str, func_name: &str) -> Option<&'a str> {
    let pattern = format!("{func_name}(");
    let func_pos = source.find(&pattern)?;

    // Find the opening brace.
    let after_sig = &source[func_pos..];
    let brace_pos = after_sig.find('{')?;
    let body_start = func_pos + brace_pos + 1;

    // Find matching closing brace.
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

/// Parses a single fragment statement.
fn parse_fragment_statement(s: &str) -> Option<FragmentInstruction> {
    // COLOR.a = <expr>
    if let Some(rest) = s.strip_prefix("COLOR.a") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix('=') {
            let val = val.trim();
            return Some(FragmentInstruction::SetColorAlpha(parse_float_expr(val)));
        }
    }

    // COLOR.rgb *= <expr>
    if let Some(rest) = s.strip_prefix("COLOR.rgb") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            let val = val.trim();
            return Some(FragmentInstruction::MultiplyColorRgb(parse_float_expr(val)));
        }
        if let Some(val) = rest.strip_prefix('=') {
            let val = val.trim();
            return Some(FragmentInstruction::SetColorRgb(parse_color_expr(val)));
        }
    }

    // COLOR *= <expr>
    if let Some(rest) = s.strip_prefix("COLOR") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("*=") {
            let val = val.trim();
            return Some(FragmentInstruction::MultiplyColor(parse_float_expr(val)));
        }
        if let Some(val) = rest.strip_prefix('=') {
            let val = val.trim();
            return Some(FragmentInstruction::SetColor(parse_color_expr(val)));
        }
    }

    None
}

/// Parses a color expression from source text.
fn parse_color_expr(s: &str) -> ColorExpr {
    let s = s.trim();

    // vec4(r, g, b, a) or vec4(v)
    if let Some(inner) = strip_func_call(s, "vec4") {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 4 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                parts[0].parse::<f32>(),
                parts[1].parse::<f32>(),
                parts[2].parse::<f32>(),
                parts[3].parse::<f32>(),
            ) {
                return ColorExpr::Literal(r, g, b, a);
            }
        } else if parts.len() == 1 {
            if let Ok(v) = parts[0].parse::<f32>() {
                return ColorExpr::Splat(v);
            }
        }
    }

    // mix(a, b, t)
    if let Some(inner) = strip_func_call(s, "mix") {
        let parts = split_top_level_commas(inner);
        if parts.len() == 3 {
            let a = parse_color_expr(parts[0]);
            let b = parse_color_expr(parts[1]);
            let t = parse_float_expr(parts[2]);
            return ColorExpr::Mix(Box::new(a), Box::new(b), t);
        }
    }

    if s == "COLOR" {
        return ColorExpr::BuiltinColor;
    }

    // Uniform reference.
    if s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return ColorExpr::Uniform(s.to_string());
    }

    // pat-nzgdu.139: constructors whose components are not all literals, e.g.
    // `vec3(EYEDIR.y, 0.0, 1.0)`. The all-literal `vec4` forms above already
    // returned, so reaching here means at least one component is an expression.
    // `split_top_level_commas` (not `split(',')`) keeps nested calls intact.
    if let Some(inner) = strip_func_call(s, "vec4") {
        let parts = split_top_level_commas(inner);
        if parts.len() == 4 {
            return ColorExpr::Components(
                Box::new(parse_float_expr(parts[0])),
                Box::new(parse_float_expr(parts[1])),
                Box::new(parse_float_expr(parts[2])),
                Box::new(parse_float_expr(parts[3])),
            );
        }
    }

    // `vec3(r, g, b)` — the sky stage's COLOR is a vec3, so alpha is 1.0.
    if let Some(inner) = strip_func_call(s, "vec3") {
        let parts = split_top_level_commas(inner);
        if parts.len() == 3 {
            return ColorExpr::Components(
                Box::new(parse_float_expr(parts[0])),
                Box::new(parse_float_expr(parts[1])),
                Box::new(parse_float_expr(parts[2])),
                Box::new(FloatExpr::Literal(1.0)),
            );
        }
        if parts.len() == 1 {
            // `vec3(v)` splat.
            let c = parse_float_expr(parts[0]);
            return ColorExpr::Components(
                Box::new(c.clone()),
                Box::new(c.clone()),
                Box::new(c),
                Box::new(FloatExpr::Literal(1.0)),
            );
        }
    }

    ColorExpr::BuiltinColor
}

/// Parses a float expression.
fn parse_float_expr(s: &str) -> FloatExpr {
    let s = s.trim();

    if let Ok(v) = s.parse::<f32>() {
        return FloatExpr::Literal(v);
    }

    if s == "TIME" {
        return FloatExpr::BuiltinTime;
    }

    if s == "UV.x" {
        return FloatExpr::UvComponent(UvAxis::X);
    }
    if s == "UV.y" {
        return FloatExpr::UvComponent(UvAxis::Y);
    }

    // Sky stage view direction (pat-nzgdu.139).
    if s == "EYEDIR.x" {
        return FloatExpr::EyeDirComponent(Vec3Axis::X);
    }
    if s == "EYEDIR.y" {
        return FloatExpr::EyeDirComponent(Vec3Axis::Y);
    }
    if s == "EYEDIR.z" {
        return FloatExpr::EyeDirComponent(Vec3Axis::Z);
    }

    if let Some(inner) = strip_func_call(s, "sin") {
        return FloatExpr::Sin(Box::new(parse_float_expr(inner)));
    }

    if s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return FloatExpr::Uniform(s.to_string());
    }

    FloatExpr::Literal(0.0)
}

/// Strips a function call wrapper like `func(inner)` and returns `inner`.
fn strip_func_call<'a>(s: &'a str, func: &str) -> Option<&'a str> {
    let prefix = format!("{func}(");
    if s.starts_with(&prefix) && s.ends_with(')') {
        Some(&s[prefix.len()..s.len() - 1])
    } else {
        None
    }
}

/// Splits a string by commas, but only at the top level (respecting parentheses).
fn split_top_level_commas(s: &str) -> Vec<&str> {
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

/// Evaluates a float expression against a fragment context and uniforms.
pub fn eval_float_expr(
    expr: &FloatExpr,
    ctx: &FragmentContext,
    uniforms: &HashMap<String, Variant>,
) -> f32 {
    match expr {
        FloatExpr::Literal(v) => *v,
        FloatExpr::BuiltinTime => ctx.time,
        FloatExpr::UvComponent(UvAxis::X) => ctx.uv.0,
        FloatExpr::UvComponent(UvAxis::Y) => ctx.uv.1,
        FloatExpr::EyeDirComponent(Vec3Axis::X) => ctx.eyedir.0,
        FloatExpr::EyeDirComponent(Vec3Axis::Y) => ctx.eyedir.1,
        FloatExpr::EyeDirComponent(Vec3Axis::Z) => ctx.eyedir.2,
        FloatExpr::Uniform(name) => match uniforms.get(name) {
            Some(Variant::Float(f)) => *f as f32,
            Some(Variant::Int(i)) => *i as f32,
            _ => 0.0,
        },
        FloatExpr::Sin(inner) => eval_float_expr(inner, ctx, uniforms).sin(),
    }
}

/// Evaluates a color expression against a fragment context and uniforms.
pub fn eval_color_expr(
    expr: &ColorExpr,
    ctx: &FragmentContext,
    uniforms: &HashMap<String, Variant>,
) -> gdcore::math::Color {
    match expr {
        ColorExpr::Literal(r, g, b, a) => gdcore::math::Color::new(*r, *g, *b, *a),
        ColorExpr::Splat(v) => gdcore::math::Color::new(*v, *v, *v, *v),
        ColorExpr::BuiltinColor => ctx.color,
        ColorExpr::Uniform(name) => match uniforms.get(name) {
            Some(Variant::Color(c)) => *c,
            _ => ctx.color,
        },
        ColorExpr::Components(r, g, b, a) => gdcore::math::Color::new(
            eval_float_expr(r, ctx, uniforms),
            eval_float_expr(g, ctx, uniforms),
            eval_float_expr(b, ctx, uniforms),
            eval_float_expr(a, ctx, uniforms),
        ),
        ColorExpr::Mix(a, b, t) => {
            let ca = eval_color_expr(a, ctx, uniforms);
            let cb = eval_color_expr(b, ctx, uniforms);
            let t = eval_float_expr(t, ctx, uniforms).clamp(0.0, 1.0);
            gdcore::math::Color::new(
                ca.r * (1.0 - t) + cb.r * t,
                ca.g * (1.0 - t) + cb.g * t,
                ca.b * (1.0 - t) + cb.b * t,
                ca.a * (1.0 - t) + cb.a * t,
            )
        }
    }
}

/// Executes parsed fragment instructions against a context.
///
/// Returns the resulting output color.
pub fn execute_fragment(
    instructions: &[FragmentInstruction],
    ctx: &FragmentContext,
    uniforms: &HashMap<String, Variant>,
) -> gdcore::math::Color {
    let mut color = ctx.color;

    for inst in instructions {
        match inst {
            FragmentInstruction::SetColor(expr) => {
                color = eval_color_expr(expr, ctx, uniforms);
            }
            FragmentInstruction::SetColorRgb(expr) => {
                let c = eval_color_expr(expr, ctx, uniforms);
                color.r = c.r;
                color.g = c.g;
                color.b = c.b;
            }
            FragmentInstruction::SetColorAlpha(expr) => {
                color.a = eval_float_expr(expr, ctx, uniforms);
            }
            FragmentInstruction::MultiplyColorRgb(expr) => {
                let f = eval_float_expr(expr, ctx, uniforms);
                color.r *= f;
                color.g *= f;
                color.b *= f;
            }
            FragmentInstruction::MultiplyColor(expr) => {
                let f = eval_float_expr(expr, ctx, uniforms);
                color.r *= f;
                color.g *= f;
                color.b *= f;
                color.a *= f;
            }
        }
    }

    color
}

// ---------------------------------------------------------------------------
// GDShader (canvas_item) -> WGSL translation
// ---------------------------------------------------------------------------

/// Maps a [`UniformType`] to the WGSL type used in the generated
/// `MaterialUniforms` struct / texture bindings.
pub fn uniform_type_to_wgsl(ty: UniformType) -> &'static str {
    match ty {
        UniformType::Float => "f32",
        UniformType::Vec2 => "vec2<f32>",
        UniformType::Vec3 => "vec3<f32>",
        UniformType::Vec4 => "vec4<f32>",
        UniformType::Color => "vec4<f32>",
        UniformType::Int => "i32",
        UniformType::Bool => "u32", // WGSL uniform buffers cannot hold `bool`.
        UniformType::Sampler2D => "texture_2d<f32>",
        UniformType::Mat4 => "mat4x4<f32>",
    }
}

/// Formats an `f32` as a WGSL float literal (always carrying a decimal point,
/// so integer-valued constants are not parsed as abstract ints).
fn wgsl_f32(v: f32) -> String {
    if !v.is_finite() {
        // WGSL has no inf/NaN literal; fall back to 0.0 to keep output valid.
        return "0.0".to_string();
    }
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Translates a [`FloatExpr`] to a WGSL `f32` expression.
///
/// `uniforms` is the set of non-sampler uniform names; a reference to an
/// undeclared name degrades to `0.0` so the generated module stays valid.
fn float_expr_to_wgsl(expr: &FloatExpr, uniforms: &[&str]) -> String {
    match expr {
        FloatExpr::Literal(v) => wgsl_f32(*v),
        FloatExpr::BuiltinTime => "globals.time".to_string(),
        FloatExpr::UvComponent(UvAxis::X) => "UV.x".to_string(),
        FloatExpr::UvComponent(UvAxis::Y) => "UV.y".to_string(),
        // EYEDIR is a sky-stage built-in with no canvas_item counterpart. This
        // emitter only generates canvas_item programs, so degrade to 0.0 —
        // matching what the CPU interpreter yields outside a sky shader — rather
        // than emitting an undefined WGSL identifier (pat-nzgdu.139).
        FloatExpr::EyeDirComponent(_) => "0.0".to_string(),
        FloatExpr::Uniform(name) => {
            if uniforms.contains(&name.as_str()) {
                format!("material.{name}")
            } else {
                "0.0".to_string()
            }
        }
        FloatExpr::Sin(inner) => format!("sin({})", float_expr_to_wgsl(inner, uniforms)),
    }
}

/// Translates a [`ColorExpr`] to a WGSL `vec4<f32>` expression.
///
/// A reference to an undeclared uniform degrades to the built-in `COLOR`
/// (passthrough) so the generated module stays valid.
fn color_expr_to_wgsl(expr: &ColorExpr, uniforms: &[&str]) -> String {
    match expr {
        ColorExpr::Literal(r, g, b, a) => format!(
            "vec4<f32>({}, {}, {}, {})",
            wgsl_f32(*r),
            wgsl_f32(*g),
            wgsl_f32(*b),
            wgsl_f32(*a)
        ),
        ColorExpr::Splat(v) => format!("vec4<f32>({})", wgsl_f32(*v)),
        ColorExpr::BuiltinColor => "COLOR".to_string(),
        ColorExpr::Uniform(name) => {
            if uniforms.contains(&name.as_str()) {
                format!("material.{name}")
            } else {
                "COLOR".to_string()
            }
        }
        // Per-component constructor, e.g. `vec4(UV.x, 0.0, 0.0, 1.0)`
        // (pat-nzgdu.139) — translate each component in turn.
        ColorExpr::Components(r, g, b, a) => format!(
            "vec4<f32>({}, {}, {}, {})",
            float_expr_to_wgsl(r, uniforms),
            float_expr_to_wgsl(g, uniforms),
            float_expr_to_wgsl(b, uniforms),
            float_expr_to_wgsl(a, uniforms)
        ),
        ColorExpr::Mix(a, b, t) => format!(
            "mix({}, {}, {})",
            color_expr_to_wgsl(a, uniforms),
            color_expr_to_wgsl(b, uniforms),
            float_expr_to_wgsl(t, uniforms)
        ),
    }
}

/// Generates a runnable WGSL fragment program from a Godot `canvas_item`
/// shader source.
///
/// The generated module mirrors the CPU interpreter ([`execute_fragment`]):
/// `COLOR` is initialised to `textureSample(TEXTURE, ..., UV) * MODULATE`, then
/// the recognised `fragment()` statements are applied in order. Non-sampler
/// uniforms are collected into a `MaterialUniforms` struct (bind group 2); the
/// `TIME` built-in, when used, is sourced from a `Globals` uniform (group 0).
///
/// Constructs outside the supported subset are silently skipped here — call
/// [`unsupported_features`] to enumerate them.
pub fn generate_canvas_item_wgsl(uniforms: &[ShaderUniform], source: &str) -> String {
    let instructions = parse_fragment_body(source);

    let buffer_uniforms: Vec<&ShaderUniform> = uniforms
        .iter()
        .filter(|u| u.uniform_type != UniformType::Sampler2D)
        .collect();
    let uni_names: Vec<&str> = buffer_uniforms.iter().map(|u| u.name.as_str()).collect();

    // Translate the fragment body first so we can tell which built-ins are used.
    let mut body = String::new();
    for inst in &instructions {
        match inst {
            FragmentInstruction::SetColor(e) => {
                body.push_str(&format!(
                    "    COLOR = {};\n",
                    color_expr_to_wgsl(e, &uni_names)
                ));
            }
            FragmentInstruction::SetColorRgb(e) => {
                body.push_str(&format!(
                    "    COLOR = vec4<f32>(({}).rgb, COLOR.a);\n",
                    color_expr_to_wgsl(e, &uni_names)
                ));
            }
            FragmentInstruction::SetColorAlpha(e) => {
                body.push_str(&format!(
                    "    COLOR.a = {};\n",
                    float_expr_to_wgsl(e, &uni_names)
                ));
            }
            FragmentInstruction::MultiplyColorRgb(e) => {
                body.push_str(&format!(
                    "    COLOR = vec4<f32>(COLOR.rgb * ({}), COLOR.a);\n",
                    float_expr_to_wgsl(e, &uni_names)
                ));
            }
            FragmentInstruction::MultiplyColor(e) => {
                body.push_str(&format!(
                    "    COLOR = COLOR * ({});\n",
                    float_expr_to_wgsl(e, &uni_names)
                ));
            }
        }
    }
    let uses_time = body.contains("globals.time");

    let mut wgsl = String::with_capacity(1024);
    wgsl.push_str("// Auto-generated WGSL from Godot `canvas_item` shader source.\n");
    wgsl.push_str("// Fragment-stage subset — regenerated on each compilation.\n\n");

    if uses_time {
        wgsl.push_str("struct Globals {\n    time: f32,\n}\n");
        wgsl.push_str("@group(0) @binding(0)\nvar<uniform> globals: Globals;\n\n");
    }

    if !buffer_uniforms.is_empty() {
        wgsl.push_str("struct MaterialUniforms {\n");
        for u in &buffer_uniforms {
            wgsl.push_str(&format!(
                "    {}: {},\n",
                u.name,
                uniform_type_to_wgsl(u.uniform_type)
            ));
        }
        wgsl.push_str("}\n");
        wgsl.push_str("@group(2) @binding(0)\nvar<uniform> material: MaterialUniforms;\n\n");
    }

    // A canvas_item always exposes the item's texture + sampler.
    wgsl.push_str("@group(2) @binding(1) var TEXTURE: texture_2d<f32>;\n");
    wgsl.push_str("@group(2) @binding(2) var TEXTURE_sampler: sampler;\n\n");

    wgsl.push_str("struct FragmentInput {\n");
    wgsl.push_str("    @location(0) uv: vec2<f32>,\n");
    wgsl.push_str("    @location(1) modulate: vec4<f32>,\n");
    wgsl.push_str("}\n\n");

    wgsl.push_str("@fragment\n");
    wgsl.push_str("fn fragment_main(in: FragmentInput) -> @location(0) vec4<f32> {\n");
    wgsl.push_str("    let UV: vec2<f32> = in.uv;\n");
    wgsl.push_str(
        "    var COLOR: vec4<f32> = textureSample(TEXTURE, TEXTURE_sampler, UV) * in.modulate;\n",
    );
    wgsl.push_str(&body);
    wgsl.push_str("    return COLOR;\n");
    wgsl.push_str("}\n");

    wgsl
}

/// Returns a WGSL-facing name for a shader stage.
fn shader_type_name(ty: ShaderType) -> &'static str {
    match ty {
        ShaderType::Spatial => "spatial",
        ShaderType::CanvasItem => "canvas_item",
        ShaderType::Particles => "particles",
        ShaderType::Sky => "sky",
    }
}

/// Returns true if `source` contains a call/definition of `name(` at a word
/// boundary (so `light(` does not match inside `highlight(`).
fn contains_call(source: &str, name: &str) -> bool {
    let pat = format!("{name}(");
    let bytes = source.as_bytes();
    let mut start = 0;
    while let Some(rel) = source[start..].find(pat.as_str()) {
        let abs = start + rel;
        let boundary = abs == 0 || {
            let prev = bytes[abs - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        if boundary {
            return true;
        }
        start = abs + 1;
    }
    false
}

/// Enumerates Godot shading-language constructs in `source` that fall *outside*
/// the translated WGSL subset (see [`generate_canvas_item_wgsl`]).
///
/// The returned strings are human-readable and intended to be surfaced to the
/// user (editor diagnostics, logs). An empty vector means the whole shader is
/// within the supported subset. This is best-effort, not a full validator.
pub fn unsupported_features(source: &str, shader_type: ShaderType) -> Vec<String> {
    let mut out = Vec::new();

    if shader_type != ShaderType::CanvasItem {
        out.push(format!(
            "shader_type `{}` is not translated to WGSL (canvas_item only)",
            shader_type_name(shader_type)
        ));
    }

    if source.contains("#include") {
        out.push("`#include` preprocessor directive".to_string());
    }
    if contains_call(source, "vertex") {
        out.push("`vertex()` stage (only `fragment()` is translated)".to_string());
    }
    if contains_call(source, "light") {
        out.push("`light()` custom lighting processor".to_string());
    }
    if source.contains("SCREEN_TEXTURE")
        || source.contains("hint_screen_texture")
        || source.contains("screen_texture")
    {
        out.push("`SCREEN_TEXTURE` / screen-buffer sampling".to_string());
    }
    if source.contains("discard") {
        out.push("`discard` statement".to_string());
    }
    if source.contains("struct ") {
        out.push("user-defined `struct` types".to_string());
    }

    // Any statement inside fragment() the line parser cannot classify (control
    // flow, local variables, unhandled built-ins, ...).
    if let Some(body) = extract_function_body(source, "fragment") {
        for line in body.lines() {
            let t = line.trim().trim_end_matches(';').trim();
            if t.is_empty() || t.starts_with("//") || t == "{" || t == "}" {
                continue;
            }
            if parse_fragment_statement(t).is_none() {
                out.push(format!("unrecognised fragment statement: `{t}`"));
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Basic shader-language tokenizer
// ---------------------------------------------------------------------------

/// A token from the Godot shading language.
#[derive(Debug, Clone, PartialEq)]
pub enum ShaderToken {
    /// A reserved keyword.
    Keyword(ShaderKeyword),
    /// An identifier (variable / function name).
    Identifier(String),
    /// An integer literal.
    IntLiteral(i64),
    /// A float literal.
    FloatLiteral(f64),
    /// A single-character or two-character operator / punctuation.
    Operator(String),
    /// Unrecognised character sequence.
    Unknown(String),
}

/// Recognised shader keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderKeyword {
    Uniform,
    Varying,
    Void,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat4,
    Sampler2D,
    Int,
    Bool,
    If,
    Else,
    For,
    While,
    Return,
    Struct,
}

/// Map a word to a keyword, if it is one.
fn keyword_from_str(s: &str) -> Option<ShaderKeyword> {
    match s {
        "uniform" => Some(ShaderKeyword::Uniform),
        "varying" => Some(ShaderKeyword::Varying),
        "void" => Some(ShaderKeyword::Void),
        "float" => Some(ShaderKeyword::Float),
        "vec2" => Some(ShaderKeyword::Vec2),
        "vec3" => Some(ShaderKeyword::Vec3),
        "vec4" => Some(ShaderKeyword::Vec4),
        "mat4" => Some(ShaderKeyword::Mat4),
        "sampler2D" => Some(ShaderKeyword::Sampler2D),
        "int" => Some(ShaderKeyword::Int),
        "bool" => Some(ShaderKeyword::Bool),
        "if" => Some(ShaderKeyword::If),
        "else" => Some(ShaderKeyword::Else),
        "for" => Some(ShaderKeyword::For),
        "while" => Some(ShaderKeyword::While),
        "return" => Some(ShaderKeyword::Return),
        "struct" => Some(ShaderKeyword::Struct),
        _ => None,
    }
}

/// Two-character operators we recognise.
const TWO_CHAR_OPS: &[&str] = &["==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/="];

/// Tokenize shader source into a flat list of tokens.
///
/// This is a basic lexer — it handles keywords, identifiers, numeric
/// literals, and common operators/punctuation. It does *not* build an
/// AST or validate the shader program.
pub fn tokenize_shader(source: &str) -> Vec<ShaderToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // Skip whitespace
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Skip line comments
        if ch == '/' && i + 1 < len && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Skip block comments
        if ch == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        // Identifiers and keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if let Some(kw) = keyword_from_str(&word) {
                tokens.push(ShaderToken::Keyword(kw));
            } else {
                tokens.push(ShaderToken::Identifier(word));
            }
            continue;
        }

        // Numeric literals
        if ch.is_ascii_digit() || (ch == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            let mut has_dot = false;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                if chars[i] == '.' {
                    if has_dot {
                        break;
                    }
                    has_dot = true;
                }
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if has_dot {
                if let Ok(f) = num_str.parse::<f64>() {
                    tokens.push(ShaderToken::FloatLiteral(f));
                } else {
                    tokens.push(ShaderToken::Unknown(num_str));
                }
            } else if let Ok(n) = num_str.parse::<i64>() {
                tokens.push(ShaderToken::IntLiteral(n));
            } else {
                tokens.push(ShaderToken::Unknown(num_str));
            }
            continue;
        }

        // Two-char operators
        if i + 1 < len {
            let two: String = chars[i..=i + 1].iter().collect();
            if TWO_CHAR_OPS.contains(&two.as_str()) {
                tokens.push(ShaderToken::Operator(two));
                i += 2;
                continue;
            }
        }

        // Single-char operators / punctuation
        if "+-*/%=<>!&|^~(){}[];,.:?".contains(ch) {
            tokens.push(ShaderToken::Operator(ch.to_string()));
            i += 1;
            continue;
        }

        tokens.push(ShaderToken::Unknown(ch.to_string()));
        i += 1;
    }

    tokens
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_type_equality() {
        assert_eq!(ShaderType::Spatial, ShaderType::Spatial);
        assert_ne!(ShaderType::Spatial, ShaderType::CanvasItem);
    }

    #[test]
    fn parse_uniform_float() {
        let src = "uniform float speed = 1.5;";
        let uniforms = parse_uniforms(src);
        assert_eq!(uniforms.len(), 1);
        assert_eq!(uniforms[0].name, "speed");
        assert_eq!(uniforms[0].uniform_type, UniformType::Float);
        assert_eq!(uniforms[0].default_value, Variant::Float(1.5));
        assert!(uniforms[0].hint.is_none());
    }

    #[test]
    fn parse_uniform_with_hint() {
        let src = "uniform vec4 albedo : source_color = 1.0;";
        let uniforms = parse_uniforms(src);
        assert_eq!(uniforms.len(), 1);
        assert_eq!(uniforms[0].name, "albedo");
        assert_eq!(uniforms[0].uniform_type, UniformType::Vec4);
        assert_eq!(uniforms[0].hint, Some("source_color".to_string()));
    }

    #[test]
    fn parse_uniform_no_default() {
        let src = "uniform sampler2D tex;";
        let uniforms = parse_uniforms(src);
        assert_eq!(uniforms.len(), 1);
        assert_eq!(uniforms[0].name, "tex");
        assert_eq!(uniforms[0].uniform_type, UniformType::Sampler2D);
        assert_eq!(uniforms[0].default_value, Variant::Nil);
    }

    #[test]
    fn parse_multiple_uniforms() {
        let src = "\
uniform float speed = 2.0;
uniform int count = 5;
void fragment() {}
uniform bool enabled = true;
";
        let uniforms = parse_uniforms(src);
        assert_eq!(uniforms.len(), 3);
        assert_eq!(uniforms[0].name, "speed");
        assert_eq!(uniforms[1].name, "count");
        assert_eq!(uniforms[1].default_value, Variant::Int(5));
        assert_eq!(uniforms[2].name, "enabled");
        assert_eq!(uniforms[2].default_value, Variant::Bool(true));
    }

    #[test]
    fn shader_get_uniform() {
        let shader = Shader::new(ShaderType::Spatial, "uniform float speed = 3.0;");
        assert!(shader.get_uniform("speed").is_some());
        assert!(shader.get_uniform("missing").is_none());
    }

    #[test]
    fn shader_material_set_get() {
        let mut mat = ShaderMaterial::new();
        assert!(mat.get_shader_parameter("foo").is_none());
        mat.set_shader_parameter("foo", Variant::Float(42.0));
        assert_eq!(mat.get_shader_parameter("foo"), Some(&Variant::Float(42.0)));
    }

    #[test]
    fn shader_material_default() {
        let mat = ShaderMaterial::default();
        assert!(mat.shader.is_none());
        assert!(mat.parameters.is_empty());
    }

    #[test]
    fn tokenize_empty() {
        assert!(tokenize_shader("").is_empty());
    }

    #[test]
    fn tokenize_keywords() {
        let tokens = tokenize_shader("uniform float void return");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0], ShaderToken::Keyword(ShaderKeyword::Uniform));
        assert_eq!(tokens[1], ShaderToken::Keyword(ShaderKeyword::Float));
        assert_eq!(tokens[2], ShaderToken::Keyword(ShaderKeyword::Void));
        assert_eq!(tokens[3], ShaderToken::Keyword(ShaderKeyword::Return));
    }

    #[test]
    fn tokenize_identifiers_and_literals() {
        let tokens = tokenize_shader("my_var = 42 + 3.14");
        assert_eq!(tokens[0], ShaderToken::Identifier("my_var".to_string()));
        assert_eq!(tokens[1], ShaderToken::Operator("=".to_string()));
        assert_eq!(tokens[2], ShaderToken::IntLiteral(42));
        assert_eq!(tokens[3], ShaderToken::Operator("+".to_string()));
        assert_eq!(tokens[4], ShaderToken::FloatLiteral(3.14));
    }

    #[test]
    fn tokenize_two_char_operators() {
        let tokens = tokenize_shader("a == b && c != d");
        assert!(tokens.contains(&ShaderToken::Operator("==".to_string())));
        assert!(tokens.contains(&ShaderToken::Operator("&&".to_string())));
        assert!(tokens.contains(&ShaderToken::Operator("!=".to_string())));
    }

    #[test]
    fn tokenize_line_comment() {
        let tokens = tokenize_shader("a // this is a comment\nb");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], ShaderToken::Identifier("a".to_string()));
        assert_eq!(tokens[1], ShaderToken::Identifier("b".to_string()));
    }

    #[test]
    fn tokenize_block_comment() {
        let tokens = tokenize_shader("a /* block */ b");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], ShaderToken::Identifier("a".to_string()));
        assert_eq!(tokens[1], ShaderToken::Identifier("b".to_string()));
    }

    #[test]
    fn tokenize_full_fragment() {
        let src = "void fragment() { COLOR = vec4(1.0); }";
        let tokens = tokenize_shader(src);
        assert!(tokens.len() > 5);
        assert_eq!(tokens[0], ShaderToken::Keyword(ShaderKeyword::Void));
        assert_eq!(tokens[1], ShaderToken::Identifier("fragment".to_string()));
    }

    // -- ShaderCompiler tests -----------------------------------------------

    #[test]
    fn compiler_compiles_source() {
        let compiler = ShaderCompiler::new();
        let src = "uniform float speed = 2.0;\nvoid fragment() {}";
        let compiled = compiler.compile(ShaderType::CanvasItem, src);
        assert_eq!(compiled.shader_type, ShaderType::CanvasItem);
        assert_eq!(compiled.program, src);
        assert_eq!(compiled.uniform_count(), 1);
        assert!(compiled.get_uniform("speed").is_some());
    }

    #[test]
    fn compiler_no_uniforms() {
        let compiler = ShaderCompiler::new();
        let compiled = compiler.compile(ShaderType::Spatial, "void fragment() {}");
        assert_eq!(compiled.uniform_count(), 0);
        assert!(compiled.get_uniform("anything").is_none());
    }

    #[test]
    fn compiler_multiple_uniforms() {
        let compiler = ShaderCompiler::new();
        let src = "uniform float a;\nuniform int b;\nuniform vec4 c;";
        let compiled = compiler.compile(ShaderType::CanvasItem, src);
        assert_eq!(compiled.uniform_count(), 3);
        assert!(compiled.get_uniform("a").is_some());
        assert!(compiled.get_uniform("b").is_some());
        assert!(compiled.get_uniform("c").is_some());
    }

    // -- ShaderProcessor tests ----------------------------------------------

    #[test]
    fn processor_passthrough_no_uniforms() {
        let compiler = ShaderCompiler::new();
        let processor = ShaderProcessor::new();
        let compiled = compiler.compile(ShaderType::CanvasItem, "void fragment() {}");
        let pixel = gdcore::math::Color::rgb(0.5, 0.3, 0.1);
        let result = processor.apply_shader(&compiled, &HashMap::new(), pixel);
        assert_eq!(result, pixel);
    }

    #[test]
    fn processor_albedo_color_override() {
        let compiler = ShaderCompiler::new();
        let processor = ShaderProcessor::new();
        let compiled = compiler.compile(ShaderType::CanvasItem, "uniform vec4 albedo_color;");
        let red = gdcore::math::Color::rgb(1.0, 0.0, 0.0);
        let mut uniforms = HashMap::new();
        uniforms.insert("albedo_color".to_string(), Variant::Color(red));
        let result = processor.apply_shader(&compiled, &uniforms, gdcore::math::Color::BLACK);
        assert_eq!(result, red);
    }

    #[test]
    fn processor_color_override_uniform() {
        let compiler = ShaderCompiler::new();
        let processor = ShaderProcessor::new();
        let compiled = compiler.compile(ShaderType::CanvasItem, "void fragment() {}");
        let green = gdcore::math::Color::rgb(0.0, 1.0, 0.0);
        let mut uniforms = HashMap::new();
        uniforms.insert("color_override".to_string(), Variant::Color(green));
        let result = processor.apply_shader(&compiled, &uniforms, gdcore::math::Color::BLACK);
        assert_eq!(result, green);
    }

    #[test]
    fn processor_non_color_uniform_passthrough() {
        let compiler = ShaderCompiler::new();
        let processor = ShaderProcessor::new();
        let compiled = compiler.compile(ShaderType::CanvasItem, "uniform float speed = 1.0;");
        let mut uniforms = HashMap::new();
        uniforms.insert("speed".to_string(), Variant::Float(5.0));
        let pixel = gdcore::math::Color::rgb(0.2, 0.4, 0.6);
        let result = processor.apply_shader(&compiled, &uniforms, pixel);
        assert_eq!(result, pixel);
    }

    #[test]
    fn compiled_shader_equality() {
        let compiler = ShaderCompiler::new();
        let a = compiler.compile(ShaderType::CanvasItem, "void fragment() {}");
        let b = compiler.compile(ShaderType::CanvasItem, "void fragment() {}");
        assert_eq!(a, b);
        let c = compiler.compile(ShaderType::Spatial, "void fragment() {}");
        assert_ne!(a, c);
    }

    // -- existing tests below -----------------------------------------------

    #[test]
    fn uniform_type_variants() {
        // Ensure all UniformType variants can be created and compared
        let types = [
            UniformType::Float,
            UniformType::Vec2,
            UniformType::Vec3,
            UniformType::Vec4,
            UniformType::Color,
            UniformType::Int,
            UniformType::Bool,
            UniformType::Sampler2D,
            UniformType::Mat4,
        ];
        for (i, a) in types.iter().enumerate() {
            for (j, b) in types.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // -- Fragment parsing and execution tests --------------------------------

    #[test]
    fn parse_fragment_set_color_literal() {
        let src =
            "shader_type canvas_item;\nvoid fragment() {\n    COLOR = vec4(1.0, 0.0, 0.0, 1.0);\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColor(ColorExpr::Literal(1.0, 0.0, 0.0, 1.0))
        );
    }

    #[test]
    fn parse_fragment_set_color_splat() {
        let src = "void fragment() {\n    COLOR = vec4(0.5);\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColor(ColorExpr::Splat(0.5))
        );
    }

    #[test]
    fn parse_fragment_set_alpha() {
        let src = "void fragment() {\n    COLOR.a = 0.5;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColorAlpha(FloatExpr::Literal(0.5))
        );
    }

    #[test]
    fn parse_fragment_multiply_rgb() {
        let src = "void fragment() {\n    COLOR.rgb *= 0.8;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::MultiplyColorRgb(FloatExpr::Literal(0.8))
        );
    }

    #[test]
    fn parse_fragment_multiply_color() {
        let src = "void fragment() {\n    COLOR *= brightness;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::MultiplyColor(FloatExpr::Uniform("brightness".to_string()))
        );
    }

    #[test]
    fn parse_fragment_color_uniform_reference() {
        let src = "void fragment() {\n    COLOR = my_color;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColor(ColorExpr::Uniform("my_color".to_string()))
        );
    }

    #[test]
    fn parse_fragment_color_passthrough() {
        let src = "void fragment() {\n    COLOR = COLOR;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColor(ColorExpr::BuiltinColor)
        );
    }

    #[test]
    fn parse_fragment_no_fragment_func() {
        let src = "shader_type canvas_item;\nuniform float speed;";
        let instructions = parse_fragment_body(src);
        assert!(instructions.is_empty());
    }

    #[test]
    fn parse_fragment_empty_body() {
        let src = "void fragment() {}";
        let instructions = parse_fragment_body(src);
        assert!(instructions.is_empty());
    }

    #[test]
    fn parse_fragment_multiple_statements() {
        let src = "void fragment() {\n    COLOR.rgb *= 0.5;\n    COLOR.a = 0.8;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 2);
    }

    #[test]
    fn parse_fragment_mix_expression() {
        let src = "void fragment() {\n    COLOR = mix(COLOR, vec4(1.0, 0.0, 0.0, 1.0), 0.5);\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        match &instructions[0] {
            FragmentInstruction::SetColor(ColorExpr::Mix(a, b, t)) => {
                assert_eq!(**a, ColorExpr::BuiltinColor);
                assert_eq!(**b, ColorExpr::Literal(1.0, 0.0, 0.0, 1.0));
                assert_eq!(*t, FloatExpr::Literal(0.5));
            }
            other => panic!("expected SetColor(Mix), got {:?}", other),
        }
    }

    #[test]
    fn parse_float_expr_sin_time() {
        let src = "void fragment() {\n    COLOR.a = sin(TIME);\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColorAlpha(FloatExpr::Sin(Box::new(FloatExpr::BuiltinTime)))
        );
    }

    #[test]
    fn parse_float_expr_uv() {
        let src = "void fragment() {\n    COLOR.a = UV.x;\n}";
        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 1);
        assert_eq!(
            instructions[0],
            FragmentInstruction::SetColorAlpha(FloatExpr::UvComponent(UvAxis::X))
        );
    }

    #[test]
    fn execute_fragment_set_color() {
        let instructions = vec![FragmentInstruction::SetColor(ColorExpr::Literal(
            1.0, 0.0, 0.0, 1.0,
        ))];
        let ctx = FragmentContext::default();
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert_eq!(result, gdcore::math::Color::rgb(1.0, 0.0, 0.0));
    }

    // -- pat-nzgdu.139: shader_type sky stage + EYEDIR built-in ---------------

    const SKY_SRC: &str = "shader_type sky;\nvoid sky() {\n    COLOR = vec3(EYEDIR.y, 0.0, 1.0);\n}";

    #[test]
    fn sky_shader_computes_color_from_eyedir() {
        let instructions = parse_sky_body(SKY_SRC);
        assert_eq!(instructions.len(), 1, "sky() body parsed");

        // Looking up: EYEDIR.y = 0.25 drives the red channel.
        let ctx = FragmentContext {
            eyedir: (0.0, 0.25, 0.0),
            ..Default::default()
        };
        let out = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert_eq!(out.r, 0.25);
        assert_eq!(out.g, 0.0);
        assert_eq!(out.b, 1.0);
        // vec3 COLOR implies opaque alpha.
        assert_eq!(out.a, 1.0);

        // A different view direction yields a different colour — i.e. the sky
        // is actually a function of EYEDIR rather than a constant.
        let ctx2 = FragmentContext {
            eyedir: (0.0, 0.9, 0.0),
            ..Default::default()
        };
        let out2 = execute_fragment(&instructions, &ctx2, &HashMap::new());
        assert_eq!(out2.r, 0.9);
        assert_ne!(out.r, out2.r);
    }

    #[test]
    fn sky_shader_reads_every_eyedir_axis() {
        let src = "shader_type sky;\nvoid sky() {\n    COLOR = vec3(EYEDIR.x, EYEDIR.y, EYEDIR.z);\n}";
        let ctx = FragmentContext {
            eyedir: (0.1, 0.2, 0.3),
            ..Default::default()
        };
        let out = execute_fragment(&parse_sky_body(src), &ctx, &HashMap::new());
        assert_eq!((out.r, out.g, out.b), (0.1, 0.2, 0.3));
    }

    #[test]
    fn parse_sky_body_ignores_a_canvas_item_shader() {
        // Stage mismatch yields nothing rather than misfiring on fragment().
        let canvas = "shader_type canvas_item;\nvoid fragment() {\n    COLOR = vec4(1.0, 0.0, 0.0, 1.0);\n}";
        assert!(parse_sky_body(canvas).is_empty());
        // ...and the converse: a sky program has no fragment() body.
        assert!(parse_fragment_body(SKY_SRC).is_empty());
    }

    #[test]
    fn eyedir_is_zero_outside_a_sky_shader() {
        // canvas_item has no EYEDIR built-in; it must read as 0, not panic.
        let instructions = vec![FragmentInstruction::SetColorAlpha(
            FloatExpr::EyeDirComponent(Vec3Axis::Y),
        )];
        let out = execute_fragment(&instructions, &FragmentContext::default(), &HashMap::new());
        assert_eq!(out.a, 0.0);
    }

    #[test]
    fn vec3_and_non_literal_vec4_parse_as_components() {
        // vec3 splat -> rgb all equal, alpha 1.
        let src = "shader_type sky;\nvoid sky() {\n    COLOR = vec3(0.5);\n}";
        let out = execute_fragment(
            &parse_sky_body(src),
            &FragmentContext::default(),
            &HashMap::new(),
        );
        assert_eq!((out.r, out.g, out.b, out.a), (0.5, 0.5, 0.5, 1.0));

        // vec4 with an expression component still evaluates per-component.
        let src4 = "void fragment() {\n    COLOR = vec4(UV.x, 0.0, 0.0, 1.0);\n}";
        let ctx = FragmentContext {
            uv: (0.75, 0.0),
            ..Default::default()
        };
        let out4 = execute_fragment(&parse_fragment_body(src4), &ctx, &HashMap::new());
        assert_eq!(out4.r, 0.75);
        assert_eq!(out4.a, 1.0);
    }

    #[test]
    fn all_literal_vec4_still_parses_as_a_literal() {
        // Regression: the existing all-literal fast path must keep its
        // ColorExpr::Literal representation, not fall through to Components.
        let src = "void fragment() {\n    COLOR = vec4(0.1, 0.2, 0.3, 0.4);\n}";
        assert_eq!(
            parse_fragment_body(src),
            vec![FragmentInstruction::SetColor(ColorExpr::Literal(
                0.1, 0.2, 0.3, 0.4
            ))],
        );
    }

    #[test]
    fn sky_shader_uniform_and_time_built_ins_work() {
        let src = "shader_type sky;\nvoid sky() {\n    COLOR = vec3(horizon, TIME, EYEDIR.y);\n}";
        let mut uniforms = HashMap::new();
        uniforms.insert("horizon".to_string(), Variant::Float(0.4));
        let ctx = FragmentContext {
            time: 0.5,
            eyedir: (0.0, 0.6, 0.0),
            ..Default::default()
        };
        let out = execute_fragment(&parse_sky_body(src), &ctx, &uniforms);
        assert_eq!(out.r, 0.4, "uniform");
        assert_eq!(out.g, 0.5, "TIME");
        assert_eq!(out.b, 0.6, "EYEDIR.y");
    }

    #[test]
    fn execute_fragment_set_alpha() {
        let instructions = vec![FragmentInstruction::SetColorAlpha(FloatExpr::Literal(0.5))];
        let ctx = FragmentContext {
            color: gdcore::math::Color::rgb(1.0, 0.0, 0.0),
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert_eq!(result.r, 1.0);
        assert_eq!(result.a, 0.5);
    }

    #[test]
    fn execute_fragment_multiply_rgb() {
        let instructions = vec![FragmentInstruction::MultiplyColorRgb(FloatExpr::Literal(
            0.5,
        ))];
        let ctx = FragmentContext {
            color: gdcore::math::Color::new(1.0, 0.8, 0.6, 1.0),
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.r - 0.5).abs() < 0.001);
        assert!((result.g - 0.4).abs() < 0.001);
        assert!((result.b - 0.3).abs() < 0.001);
        assert_eq!(result.a, 1.0); // alpha unchanged
    }

    #[test]
    fn execute_fragment_multiply_all() {
        let instructions = vec![FragmentInstruction::MultiplyColor(FloatExpr::Literal(0.5))];
        let ctx = FragmentContext {
            color: gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.r - 0.5).abs() < 0.001);
        assert!((result.a - 0.5).abs() < 0.001);
    }

    #[test]
    fn execute_fragment_uniform_color() {
        let instructions = vec![FragmentInstruction::SetColor(ColorExpr::Uniform(
            "tint".to_string(),
        ))];
        let ctx = FragmentContext::default();
        let mut uniforms = HashMap::new();
        let green = gdcore::math::Color::rgb(0.0, 1.0, 0.0);
        uniforms.insert("tint".to_string(), Variant::Color(green));
        let result = execute_fragment(&instructions, &ctx, &uniforms);
        assert_eq!(result, green);
    }

    #[test]
    fn execute_fragment_mix() {
        let instructions = vec![FragmentInstruction::SetColor(ColorExpr::Mix(
            Box::new(ColorExpr::Literal(1.0, 0.0, 0.0, 1.0)),
            Box::new(ColorExpr::Literal(0.0, 0.0, 1.0, 1.0)),
            FloatExpr::Literal(0.5),
        ))];
        let ctx = FragmentContext::default();
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.r - 0.5).abs() < 0.001);
        assert!((result.b - 0.5).abs() < 0.001);
    }

    #[test]
    fn execute_fragment_sin_time() {
        let instructions = vec![FragmentInstruction::SetColorAlpha(FloatExpr::Sin(
            Box::new(FloatExpr::BuiltinTime),
        ))];
        let ctx = FragmentContext {
            time: std::f32::consts::FRAC_PI_2,
            color: gdcore::math::Color::WHITE,
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.a - 1.0).abs() < 0.001); // sin(PI/2) = 1.0
    }

    #[test]
    fn execute_fragment_uv_gradient() {
        let instructions = vec![FragmentInstruction::SetColorAlpha(FloatExpr::UvComponent(
            UvAxis::X,
        ))];
        let ctx = FragmentContext {
            color: gdcore::math::Color::WHITE,
            uv: (0.75, 0.25),
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.a - 0.75).abs() < 0.001);
    }

    #[test]
    fn execute_fragment_empty_passthrough() {
        let ctx = FragmentContext {
            color: gdcore::math::Color::rgb(0.3, 0.6, 0.9),
            ..Default::default()
        };
        let result = execute_fragment(&[], &ctx, &HashMap::new());
        assert_eq!(result, ctx.color);
    }

    #[test]
    fn execute_fragment_multiple_instructions() {
        let instructions = vec![
            FragmentInstruction::SetColor(ColorExpr::Literal(1.0, 1.0, 1.0, 1.0)),
            FragmentInstruction::MultiplyColorRgb(FloatExpr::Literal(0.5)),
            FragmentInstruction::SetColorAlpha(FloatExpr::Literal(0.8)),
        ];
        let ctx = FragmentContext::default();
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.r - 0.5).abs() < 0.001);
        assert!((result.g - 0.5).abs() < 0.001);
        assert!((result.b - 0.5).abs() < 0.001);
        assert!((result.a - 0.8).abs() < 0.001);
    }

    #[test]
    fn execute_fragment_set_color_rgb() {
        let instructions = vec![FragmentInstruction::SetColorRgb(ColorExpr::Literal(
            0.2, 0.4, 0.6, 1.0,
        ))];
        let ctx = FragmentContext {
            color: gdcore::math::Color::new(0.0, 0.0, 0.0, 0.9),
            ..Default::default()
        };
        let result = execute_fragment(&instructions, &ctx, &HashMap::new());
        assert!((result.r - 0.2).abs() < 0.001);
        assert!((result.g - 0.4).abs() < 0.001);
        assert!((result.b - 0.6).abs() < 0.001);
        assert!((result.a - 0.9).abs() < 0.001); // alpha unchanged
    }

    #[test]
    fn fragment_context_default() {
        let ctx = FragmentContext::default();
        assert_eq!(ctx.color, gdcore::math::Color::WHITE);
        assert_eq!(ctx.uv, (0.0, 0.0));
        assert_eq!(ctx.screen_uv, (0.0, 0.0));
        assert_eq!(ctx.time, 0.0);
    }

    #[test]
    fn parse_full_shader_end_to_end() {
        // Parse + execute a complete shader.
        let src = r#"
shader_type canvas_item;
uniform vec4 tint_color;
uniform float alpha_mult = 1.0;

void fragment() {
    COLOR = tint_color;
    COLOR.a = alpha_mult;
}
"#;
        let uniforms_parsed = parse_uniforms(src);
        assert_eq!(uniforms_parsed.len(), 2);

        let instructions = parse_fragment_body(src);
        assert_eq!(instructions.len(), 2);

        let mut uniforms = HashMap::new();
        let red = gdcore::math::Color::rgb(1.0, 0.0, 0.0);
        uniforms.insert("tint_color".to_string(), Variant::Color(red));
        uniforms.insert("alpha_mult".to_string(), Variant::Float(0.7));

        let ctx = FragmentContext::default();
        let result = execute_fragment(&instructions, &ctx, &uniforms);
        assert_eq!(result.r, 1.0);
        assert_eq!(result.g, 0.0);
        assert!((result.a - 0.7).abs() < 0.001);
    }

    #[test]
    fn eval_float_uniform_fallback() {
        let expr = FloatExpr::Uniform("missing".to_string());
        let ctx = FragmentContext::default();
        let result = eval_float_expr(&expr, &ctx, &HashMap::new());
        assert_eq!(result, 0.0);
    }

    #[test]
    fn eval_color_uniform_fallback() {
        let expr = ColorExpr::Uniform("missing".to_string());
        let ctx = FragmentContext::default();
        let result = eval_color_expr(&expr, &ctx, &HashMap::new());
        assert_eq!(result, ctx.color);
    }

    // -----------------------------------------------------------------------
    // canvas_item -> WGSL translation (pat-p0s82.21)
    // -----------------------------------------------------------------------

    /// Braces in the generated WGSL must be balanced — a cheap structural
    /// sanity check that the module is well-formed.
    fn assert_brace_balanced(wgsl: &str) {
        let open = wgsl.chars().filter(|c| *c == '{').count();
        let close = wgsl.chars().filter(|c| *c == '}').count();
        assert_eq!(open, close, "unbalanced braces in generated WGSL:\n{wgsl}");
    }

    #[test]
    fn canvas_item_compile_emits_wgsl() {
        let compiled = ShaderCompiler::new().compile(
            ShaderType::CanvasItem,
            "shader_type canvas_item;\nvoid fragment() {}\n",
        );
        let wgsl = compiled.wgsl_program.expect("canvas_item should emit WGSL");
        assert!(wgsl.contains("@fragment"), "missing @fragment: {wgsl}");
        assert!(wgsl.contains("fn fragment_main"), "missing entry point");
        // The default COLOR samples the item texture.
        assert!(
            wgsl.contains("textureSample(TEXTURE, TEXTURE_sampler, UV)"),
            "default COLOR should sample TEXTURE: {wgsl}"
        );
        assert!(compiled.unsupported.is_empty());
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn canvas_item_wgsl_translates_color_literal() {
        let src =
            "shader_type canvas_item;\nvoid fragment() {\n    COLOR = vec4(1.0, 0.0, 0.0, 1.0);\n}\n";
        let wgsl = generate_canvas_item_wgsl(&[], src);
        assert!(
            wgsl.contains("COLOR = vec4<f32>(1.0, 0.0, 0.0, 1.0);"),
            "color literal not translated: {wgsl}"
        );
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn canvas_item_wgsl_uniform_struct_and_reference() {
        let src =
            "shader_type canvas_item;\nuniform vec4 tint;\nvoid fragment() {\n    COLOR = tint;\n}\n";
        let uniforms = parse_uniforms(src);
        let wgsl = generate_canvas_item_wgsl(&uniforms, src);
        assert!(
            wgsl.contains("struct MaterialUniforms"),
            "no uniform struct"
        );
        assert!(
            wgsl.contains("tint: vec4<f32>,"),
            "tint field missing: {wgsl}"
        );
        assert!(
            wgsl.contains("var<uniform> material: MaterialUniforms;"),
            "material binding missing"
        );
        assert!(
            wgsl.contains("COLOR = material.tint;"),
            "uniform reference not translated: {wgsl}"
        );
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn canvas_item_wgsl_uv_and_time_builtins() {
        let src = "shader_type canvas_item;\nvoid fragment() {\n    COLOR.a = UV.x;\n    COLOR.rgb *= sin(TIME);\n}\n";
        let wgsl = generate_canvas_item_wgsl(&[], src);
        assert!(
            wgsl.contains("COLOR.a = UV.x;"),
            "UV.x not translated: {wgsl}"
        );
        assert!(
            wgsl.contains("struct Globals"),
            "TIME needs Globals: {wgsl}"
        );
        assert!(
            wgsl.contains("sin(globals.time)"),
            "TIME builtin not translated: {wgsl}"
        );
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn canvas_item_wgsl_no_globals_without_time() {
        let src = "shader_type canvas_item;\nvoid fragment() {\n    COLOR.a = UV.x;\n}\n";
        let wgsl = generate_canvas_item_wgsl(&[], src);
        assert!(
            !wgsl.contains("struct Globals"),
            "Globals emitted without TIME use: {wgsl}"
        );
    }

    #[test]
    fn canvas_item_wgsl_mix_expression() {
        let src = "shader_type canvas_item;\nuniform vec4 tint;\nvoid fragment() {\n    COLOR = mix(vec4(0.0, 0.0, 0.0, 1.0), tint, 0.5);\n}\n";
        let uniforms = parse_uniforms(src);
        let wgsl = generate_canvas_item_wgsl(&uniforms, src);
        assert!(
            wgsl.contains("mix(vec4<f32>(0.0, 0.0, 0.0, 1.0), material.tint, 0.5)"),
            "mix() not translated: {wgsl}"
        );
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn shader_material_compiles_to_wgsl() {
        // A ShaderMaterial carrying a canvas_item shader yields runnable WGSL.
        let mut mat = ShaderMaterial::new();
        mat.shader = Some(Shader::new(
            ShaderType::CanvasItem,
            "shader_type canvas_item;\nuniform vec4 modulate_color;\nvoid fragment() {\n    COLOR = modulate_color;\n}\n",
        ));
        let shader = mat.shader.as_ref().unwrap();
        let compiled = ShaderCompiler::new().compile(shader.shader_type, &shader.source_code);
        let wgsl = compiled
            .wgsl_program
            .expect("material shader should emit WGSL");
        assert!(wgsl.contains("modulate_color: vec4<f32>,"));
        assert!(wgsl.contains("COLOR = material.modulate_color;"));
        assert_brace_balanced(&wgsl);
    }

    #[test]
    fn non_canvas_item_has_no_wgsl_and_is_flagged() {
        let compiled = ShaderCompiler::new().compile(
            ShaderType::Spatial,
            "shader_type spatial;\nvoid fragment() {}\n",
        );
        assert!(compiled.wgsl_program.is_none());
        assert!(
            compiled.unsupported.iter().any(|s| s.contains("spatial")),
            "spatial stage should be flagged unsupported: {:?}",
            compiled.unsupported
        );
    }

    #[test]
    fn unsupported_features_flags_known_constructs() {
        let src = "shader_type canvas_item;\n#include \"lib.gdshaderinc\"\nvoid vertex() {\n    VERTEX.x += 1.0;\n}\nvoid fragment() {\n    for (int i = 0; i < 4; i++) {\n        COLOR.rgb *= 0.5;\n    }\n    discard;\n}\n";
        let flags = unsupported_features(src, ShaderType::CanvasItem);
        assert!(flags.iter().any(|s| s.contains("#include")), "{flags:?}");
        assert!(flags.iter().any(|s| s.contains("vertex()")), "{flags:?}");
        assert!(flags.iter().any(|s| s.contains("discard")), "{flags:?}");
        // The `for` loop header is an unrecognised fragment statement.
        assert!(
            flags.iter().any(|s| s.contains("for (int i")),
            "for-loop should be flagged: {flags:?}"
        );
    }

    #[test]
    fn unsupported_features_empty_for_supported_shader() {
        let src =
            "shader_type canvas_item;\nuniform vec4 tint;\nvoid fragment() {\n    COLOR = tint;\n}\n";
        let flags = unsupported_features(src, ShaderType::CanvasItem);
        assert!(
            flags.is_empty(),
            "expected no unsupported flags, got {flags:?}"
        );
    }

    #[test]
    fn uniform_type_wgsl_mapping() {
        assert_eq!(uniform_type_to_wgsl(UniformType::Float), "f32");
        assert_eq!(uniform_type_to_wgsl(UniformType::Vec2), "vec2<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Color), "vec4<f32>");
        assert_eq!(uniform_type_to_wgsl(UniformType::Mat4), "mat4x4<f32>");
        assert_eq!(
            uniform_type_to_wgsl(UniformType::Sampler2D),
            "texture_2d<f32>"
        );
    }
}
