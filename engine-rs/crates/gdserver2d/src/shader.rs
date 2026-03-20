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
/// Stores the parsed uniforms and the shader "program" (currently the source
/// string — a future implementation would hold bytecode or an IR).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledShader {
    /// The shader pipeline stage.
    pub shader_type: ShaderType,
    /// Parsed uniform declarations.
    pub uniforms: Vec<ShaderUniform>,
    /// The compiled program representation (currently just the source).
    pub program: String,
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
    pub fn compile(&self, shader_type: ShaderType, source: &str) -> CompiledShader {
        let uniforms = parse_uniforms(source);
        CompiledShader {
            shader_type,
            uniforms,
            program: source.to_string(),
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
}

impl Default for FragmentContext {
    fn default() -> Self {
        Self {
            color: gdcore::math::Color::WHITE,
            uv: (0.0, 0.0),
            screen_uv: (0.0, 0.0),
            time: 0.0,
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
    /// `sin(expr)`.
    Sin(Box<FloatExpr>),
}

/// Which component of UV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvAxis {
    X,
    Y,
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
}
