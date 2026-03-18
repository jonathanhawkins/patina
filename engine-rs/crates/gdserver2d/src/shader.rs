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
}
