//! GDShader tokenizer (lexer).
//!
//! Converts GDShader source text into a stream of [`ShaderTokenSpan`]s,
//! handling keywords, built-in types, literals, operators, punctuation,
//! and both line (`//`) and block (`/* */`) comments.
//!
//! GDShader is a GLSL-like language with Godot-specific extensions such as
//! `shader_type`, `render_mode`, uniform hints, and `instance` qualifiers.

use std::fmt;

/// A lexical token produced by the GDShader tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum ShaderToken {
    // ── Shader-specific keywords ──────────────────────────────────────
    /// `shader_type`
    ShaderType,
    /// `render_mode`
    RenderMode,
    /// `uniform`
    Uniform,
    /// `varying`
    Varying,
    /// `const`
    Const,
    /// `struct`
    Struct,
    /// `void`
    Void,
    /// `if`
    If,
    /// `else`
    Else,
    /// `for`
    For,
    /// `while`
    While,
    /// `do`
    Do,
    /// `switch`
    Switch,
    /// `case`
    Case,
    /// `default`
    Default,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `return`
    Return,
    /// `discard`
    Discard,
    /// `in`
    In,
    /// `out`
    Out,
    /// `inout`
    Inout,
    /// `flat`
    Flat,
    /// `smooth`
    Smooth,
    /// `lowp`
    Lowp,
    /// `mediump`
    Mediump,
    /// `highp`
    Highp,
    /// `instance`
    Instance,
    /// `global`
    Global,
    /// `group_uniforms`
    GroupUniforms,

    // ── Built-in types ────────────────────────────────────────────────
    /// `bool`
    Bool,
    /// `int`
    Int,
    /// `uint`
    Uint,
    /// `float`
    Float,
    /// `vec2`
    Vec2,
    /// `vec3`
    Vec3,
    /// `vec4`
    Vec4,
    /// `ivec2`
    Ivec2,
    /// `ivec3`
    Ivec3,
    /// `ivec4`
    Ivec4,
    /// `uvec2`
    Uvec2,
    /// `uvec3`
    Uvec3,
    /// `uvec4`
    Uvec4,
    /// `bvec2`
    Bvec2,
    /// `bvec3`
    Bvec3,
    /// `bvec4`
    Bvec4,
    /// `mat2`
    Mat2,
    /// `mat3`
    Mat3,
    /// `mat4`
    Mat4,
    /// `sampler2D`
    Sampler2D,
    /// `isampler2D`
    Isampler2D,
    /// `usampler2D`
    Usampler2D,
    /// `sampler2DArray`
    Sampler2DArray,
    /// `sampler3D`
    Sampler3D,
    /// `samplerCube`
    SamplerCube,
    /// `samplerCubeArray`
    SamplerCubeArray,
    /// `samplerExternalOES`
    SamplerExternalOES,

    // ── Literals ──────────────────────────────────────────────────────
    /// Integer literal (decimal, hex, octal, or binary).
    IntLit(i64),
    /// Unsigned integer literal (trailing `u`).
    UintLit(u64),
    /// Float literal.
    FloatLit(f64),
    /// Boolean literal (`true` / `false`).
    BoolLit(bool),

    // ── Identifiers ───────────────────────────────────────────────────
    /// An identifier (variable name, function name, hint name, etc.).
    Ident(String),

    // ── Operators ─────────────────────────────────────────────────────
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `=`
    Assign,
    /// `+=`
    PlusAssign,
    /// `-=`
    MinusAssign,
    /// `*=`
    StarAssign,
    /// `/=`
    SlashAssign,
    /// `%=`
    PercentAssign,
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `!`
    Bang,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    LtLt,
    /// `>>`
    GtGt,
    /// `<<=`
    LtLtAssign,
    /// `>>=`
    GtGtAssign,
    /// `&=`
    AmpAssign,
    /// `|=`
    PipeAssign,
    /// `^=`
    CaretAssign,
    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,
    /// `?` (ternary)
    Question,

    // ── Punctuation ───────────────────────────────────────────────────
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `.`
    Dot,

    // ── Special ───────────────────────────────────────────────────────
    /// A line comment (`// ...`). Contents exclude the `//` prefix.
    LineComment(String),
    /// A block comment (`/* ... */`). Contents exclude delimiters.
    BlockComment(String),
    /// End of file.
    Eof,
}

/// A token together with its source location.
#[derive(Debug, Clone)]
pub struct ShaderTokenSpan {
    /// The token.
    pub token: ShaderToken,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
}

/// An error produced during shader lexical analysis.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ShaderLexError {
    /// An unterminated block comment was found.
    #[error("unterminated block comment at line {line}, column {col}")]
    UnterminatedBlockComment {
        /// Line where the comment started.
        line: usize,
        /// Column where the comment started.
        col: usize,
    },

    /// An unexpected character was encountered.
    #[error("unexpected character '{ch}' at line {line}, column {col}")]
    UnexpectedChar {
        /// The unexpected character.
        ch: char,
        /// Line of the character.
        line: usize,
        /// Column of the character.
        col: usize,
    },

    /// An invalid numeric literal.
    #[error("invalid numeric literal at line {line}, column {col}: {detail}")]
    InvalidNumber {
        /// Description of why the number is invalid.
        detail: String,
        /// Line of the literal.
        line: usize,
        /// Column of the literal.
        col: usize,
    },
}

impl fmt::Display for ShaderToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShaderToken::ShaderType => write!(f, "shader_type"),
            ShaderToken::RenderMode => write!(f, "render_mode"),
            ShaderToken::Uniform => write!(f, "uniform"),
            ShaderToken::Varying => write!(f, "varying"),
            ShaderToken::Const => write!(f, "const"),
            ShaderToken::Struct => write!(f, "struct"),
            ShaderToken::Void => write!(f, "void"),
            ShaderToken::If => write!(f, "if"),
            ShaderToken::Else => write!(f, "else"),
            ShaderToken::For => write!(f, "for"),
            ShaderToken::While => write!(f, "while"),
            ShaderToken::Do => write!(f, "do"),
            ShaderToken::Switch => write!(f, "switch"),
            ShaderToken::Case => write!(f, "case"),
            ShaderToken::Default => write!(f, "default"),
            ShaderToken::Break => write!(f, "break"),
            ShaderToken::Continue => write!(f, "continue"),
            ShaderToken::Return => write!(f, "return"),
            ShaderToken::Discard => write!(f, "discard"),
            ShaderToken::In => write!(f, "in"),
            ShaderToken::Out => write!(f, "out"),
            ShaderToken::Inout => write!(f, "inout"),
            ShaderToken::Flat => write!(f, "flat"),
            ShaderToken::Smooth => write!(f, "smooth"),
            ShaderToken::Lowp => write!(f, "lowp"),
            ShaderToken::Mediump => write!(f, "mediump"),
            ShaderToken::Highp => write!(f, "highp"),
            ShaderToken::Instance => write!(f, "instance"),
            ShaderToken::Global => write!(f, "global"),
            ShaderToken::GroupUniforms => write!(f, "group_uniforms"),
            ShaderToken::Bool => write!(f, "bool"),
            ShaderToken::Int => write!(f, "int"),
            ShaderToken::Uint => write!(f, "uint"),
            ShaderToken::Float => write!(f, "float"),
            ShaderToken::Vec2 => write!(f, "vec2"),
            ShaderToken::Vec3 => write!(f, "vec3"),
            ShaderToken::Vec4 => write!(f, "vec4"),
            ShaderToken::Ivec2 => write!(f, "ivec2"),
            ShaderToken::Ivec3 => write!(f, "ivec3"),
            ShaderToken::Ivec4 => write!(f, "ivec4"),
            ShaderToken::Uvec2 => write!(f, "uvec2"),
            ShaderToken::Uvec3 => write!(f, "uvec3"),
            ShaderToken::Uvec4 => write!(f, "uvec4"),
            ShaderToken::Bvec2 => write!(f, "bvec2"),
            ShaderToken::Bvec3 => write!(f, "bvec3"),
            ShaderToken::Bvec4 => write!(f, "bvec4"),
            ShaderToken::Mat2 => write!(f, "mat2"),
            ShaderToken::Mat3 => write!(f, "mat3"),
            ShaderToken::Mat4 => write!(f, "mat4"),
            ShaderToken::Sampler2D => write!(f, "sampler2D"),
            ShaderToken::Isampler2D => write!(f, "isampler2D"),
            ShaderToken::Usampler2D => write!(f, "usampler2D"),
            ShaderToken::Sampler2DArray => write!(f, "sampler2DArray"),
            ShaderToken::Sampler3D => write!(f, "sampler3D"),
            ShaderToken::SamplerCube => write!(f, "samplerCube"),
            ShaderToken::SamplerCubeArray => write!(f, "samplerCubeArray"),
            ShaderToken::SamplerExternalOES => write!(f, "samplerExternalOES"),
            ShaderToken::IntLit(v) => write!(f, "{v}"),
            ShaderToken::UintLit(v) => write!(f, "{v}u"),
            ShaderToken::FloatLit(v) => write!(f, "{v}"),
            ShaderToken::BoolLit(v) => write!(f, "{v}"),
            ShaderToken::Ident(name) => write!(f, "{name}"),
            ShaderToken::Plus => write!(f, "+"),
            ShaderToken::Minus => write!(f, "-"),
            ShaderToken::Star => write!(f, "*"),
            ShaderToken::Slash => write!(f, "/"),
            ShaderToken::Percent => write!(f, "%"),
            ShaderToken::Assign => write!(f, "="),
            ShaderToken::PlusAssign => write!(f, "+="),
            ShaderToken::MinusAssign => write!(f, "-="),
            ShaderToken::StarAssign => write!(f, "*="),
            ShaderToken::SlashAssign => write!(f, "/="),
            ShaderToken::PercentAssign => write!(f, "%="),
            ShaderToken::EqEq => write!(f, "=="),
            ShaderToken::BangEq => write!(f, "!="),
            ShaderToken::Lt => write!(f, "<"),
            ShaderToken::Gt => write!(f, ">"),
            ShaderToken::LtEq => write!(f, "<="),
            ShaderToken::GtEq => write!(f, ">="),
            ShaderToken::AmpAmp => write!(f, "&&"),
            ShaderToken::PipePipe => write!(f, "||"),
            ShaderToken::Bang => write!(f, "!"),
            ShaderToken::Amp => write!(f, "&"),
            ShaderToken::Pipe => write!(f, "|"),
            ShaderToken::Caret => write!(f, "^"),
            ShaderToken::Tilde => write!(f, "~"),
            ShaderToken::LtLt => write!(f, "<<"),
            ShaderToken::GtGt => write!(f, ">>"),
            ShaderToken::LtLtAssign => write!(f, "<<="),
            ShaderToken::GtGtAssign => write!(f, ">>="),
            ShaderToken::AmpAssign => write!(f, "&="),
            ShaderToken::PipeAssign => write!(f, "|="),
            ShaderToken::CaretAssign => write!(f, "^="),
            ShaderToken::PlusPlus => write!(f, "++"),
            ShaderToken::MinusMinus => write!(f, "--"),
            ShaderToken::Question => write!(f, "?"),
            ShaderToken::LParen => write!(f, "("),
            ShaderToken::RParen => write!(f, ")"),
            ShaderToken::LBracket => write!(f, "["),
            ShaderToken::RBracket => write!(f, "]"),
            ShaderToken::LBrace => write!(f, "{{"),
            ShaderToken::RBrace => write!(f, "}}"),
            ShaderToken::Colon => write!(f, ":"),
            ShaderToken::Semicolon => write!(f, ";"),
            ShaderToken::Comma => write!(f, ","),
            ShaderToken::Dot => write!(f, "."),
            ShaderToken::LineComment(s) => write!(f, "//{s}"),
            ShaderToken::BlockComment(s) => write!(f, "/*{s}*/"),
            ShaderToken::Eof => write!(f, "EOF"),
        }
    }
}

/// Maps an identifier string to the corresponding keyword or type token,
/// or returns `None` if it is a plain identifier.
fn keyword_or_type(word: &str) -> Option<ShaderToken> {
    match word {
        // Keywords
        "shader_type" => Some(ShaderToken::ShaderType),
        "render_mode" => Some(ShaderToken::RenderMode),
        "uniform" => Some(ShaderToken::Uniform),
        "varying" => Some(ShaderToken::Varying),
        "const" => Some(ShaderToken::Const),
        "struct" => Some(ShaderToken::Struct),
        "void" => Some(ShaderToken::Void),
        "if" => Some(ShaderToken::If),
        "else" => Some(ShaderToken::Else),
        "for" => Some(ShaderToken::For),
        "while" => Some(ShaderToken::While),
        "do" => Some(ShaderToken::Do),
        "switch" => Some(ShaderToken::Switch),
        "case" => Some(ShaderToken::Case),
        "default" => Some(ShaderToken::Default),
        "break" => Some(ShaderToken::Break),
        "continue" => Some(ShaderToken::Continue),
        "return" => Some(ShaderToken::Return),
        "discard" => Some(ShaderToken::Discard),
        "in" => Some(ShaderToken::In),
        "out" => Some(ShaderToken::Out),
        "inout" => Some(ShaderToken::Inout),
        "flat" => Some(ShaderToken::Flat),
        "smooth" => Some(ShaderToken::Smooth),
        "lowp" => Some(ShaderToken::Lowp),
        "mediump" => Some(ShaderToken::Mediump),
        "highp" => Some(ShaderToken::Highp),
        "true" => Some(ShaderToken::BoolLit(true)),
        "false" => Some(ShaderToken::BoolLit(false)),
        "instance" => Some(ShaderToken::Instance),
        "global" => Some(ShaderToken::Global),
        "group_uniforms" => Some(ShaderToken::GroupUniforms),
        // Built-in types
        "bool" => Some(ShaderToken::Bool),
        "int" => Some(ShaderToken::Int),
        "uint" => Some(ShaderToken::Uint),
        "float" => Some(ShaderToken::Float),
        "vec2" => Some(ShaderToken::Vec2),
        "vec3" => Some(ShaderToken::Vec3),
        "vec4" => Some(ShaderToken::Vec4),
        "ivec2" => Some(ShaderToken::Ivec2),
        "ivec3" => Some(ShaderToken::Ivec3),
        "ivec4" => Some(ShaderToken::Ivec4),
        "uvec2" => Some(ShaderToken::Uvec2),
        "uvec3" => Some(ShaderToken::Uvec3),
        "uvec4" => Some(ShaderToken::Uvec4),
        "bvec2" => Some(ShaderToken::Bvec2),
        "bvec3" => Some(ShaderToken::Bvec3),
        "bvec4" => Some(ShaderToken::Bvec4),
        "mat2" => Some(ShaderToken::Mat2),
        "mat3" => Some(ShaderToken::Mat3),
        "mat4" => Some(ShaderToken::Mat4),
        "sampler2D" => Some(ShaderToken::Sampler2D),
        "isampler2D" => Some(ShaderToken::Isampler2D),
        "usampler2D" => Some(ShaderToken::Usampler2D),
        "sampler2DArray" => Some(ShaderToken::Sampler2DArray),
        "sampler3D" => Some(ShaderToken::Sampler3D),
        "samplerCube" => Some(ShaderToken::SamplerCube),
        "samplerCubeArray" => Some(ShaderToken::SamplerCubeArray),
        "samplerExternalOES" => Some(ShaderToken::SamplerExternalOES),
        _ => None,
    }
}

/// Tokenizes GDShader source code into a sequence of [`ShaderTokenSpan`]s.
///
/// Comments are preserved as [`ShaderToken::LineComment`] and
/// [`ShaderToken::BlockComment`] tokens so downstream consumers can
/// optionally strip or retain them.
pub fn tokenize_shader(source: &str) -> Result<Vec<ShaderTokenSpan>, ShaderLexError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;
    let mut line: usize = 1;
    let mut col: usize = 1;

    while pos < len {
        let ch = chars[pos];

        // Skip whitespace (but track newlines for location)
        if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            pos += 1;
            continue;
        }

        let start_line = line;
        let start_col = col;

        // ── Comments and division ─────────────────────────────────────
        if ch == '/' {
            if pos + 1 < len && chars[pos + 1] == '/' {
                // Line comment
                pos += 2;
                col += 2;
                let content_start = pos;
                while pos < len && chars[pos] != '\n' {
                    pos += 1;
                    col += 1;
                }
                let content: String = chars[content_start..pos].iter().collect();
                tokens.push(ShaderTokenSpan {
                    token: ShaderToken::LineComment(content),
                    line: start_line,
                    col: start_col,
                });
                continue;
            }
            if pos + 1 < len && chars[pos + 1] == '*' {
                // Block comment
                pos += 2;
                col += 2;
                let content_start = pos;
                loop {
                    if pos >= len {
                        return Err(ShaderLexError::UnterminatedBlockComment {
                            line: start_line,
                            col: start_col,
                        });
                    }
                    if chars[pos] == '*' && pos + 1 < len && chars[pos + 1] == '/' {
                        let content: String = chars[content_start..pos].iter().collect();
                        tokens.push(ShaderTokenSpan {
                            token: ShaderToken::BlockComment(content),
                            line: start_line,
                            col: start_col,
                        });
                        pos += 2;
                        col += 2;
                        break;
                    }
                    if chars[pos] == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    pos += 1;
                }
                continue;
            }
            // /= or plain /
            if pos + 1 < len && chars[pos + 1] == '=' {
                tokens.push(ShaderTokenSpan {
                    token: ShaderToken::SlashAssign,
                    line: start_line,
                    col: start_col,
                });
                pos += 2;
                col += 2;
            } else {
                tokens.push(ShaderTokenSpan {
                    token: ShaderToken::Slash,
                    line: start_line,
                    col: start_col,
                });
                pos += 1;
                col += 1;
            }
            continue;
        }

        // ── Numeric literals ──────────────────────────────────────────
        if ch.is_ascii_digit() || (ch == '.' && pos + 1 < len && chars[pos + 1].is_ascii_digit()) {
            let (tok, new_pos) = lex_number(&chars, pos, len, line, col)?;
            let advance = new_pos - pos;
            tokens.push(ShaderTokenSpan {
                token: tok,
                line: start_line,
                col: start_col,
            });
            col += advance;
            pos = new_pos;
            continue;
        }

        // ── Identifiers & keywords ────────────────────────────────────
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = pos;
            while pos < len && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_') {
                pos += 1;
                col += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let tok = keyword_or_type(&word).unwrap_or(ShaderToken::Ident(word));
            tokens.push(ShaderTokenSpan {
                token: tok,
                line: start_line,
                col: start_col,
            });
            continue;
        }

        // ── Multi-character operators ─────────────────────────────────
        let next = if pos + 1 < len {
            Some(chars[pos + 1])
        } else {
            None
        };
        let next2 = if pos + 2 < len {
            Some(chars[pos + 2])
        } else {
            None
        };

        let (tok, advance) = match ch {
            '+' => match next {
                Some('+') => (ShaderToken::PlusPlus, 2),
                Some('=') => (ShaderToken::PlusAssign, 2),
                _ => (ShaderToken::Plus, 1),
            },
            '-' => match next {
                Some('-') => (ShaderToken::MinusMinus, 2),
                Some('=') => (ShaderToken::MinusAssign, 2),
                _ => (ShaderToken::Minus, 1),
            },
            '*' => match next {
                Some('=') => (ShaderToken::StarAssign, 2),
                _ => (ShaderToken::Star, 1),
            },
            '%' => match next {
                Some('=') => (ShaderToken::PercentAssign, 2),
                _ => (ShaderToken::Percent, 1),
            },
            '=' => match next {
                Some('=') => (ShaderToken::EqEq, 2),
                _ => (ShaderToken::Assign, 1),
            },
            '!' => match next {
                Some('=') => (ShaderToken::BangEq, 2),
                _ => (ShaderToken::Bang, 1),
            },
            '<' => match next {
                Some('<') => match next2 {
                    Some('=') => (ShaderToken::LtLtAssign, 3),
                    _ => (ShaderToken::LtLt, 2),
                },
                Some('=') => (ShaderToken::LtEq, 2),
                _ => (ShaderToken::Lt, 1),
            },
            '>' => match next {
                Some('>') => match next2 {
                    Some('=') => (ShaderToken::GtGtAssign, 3),
                    _ => (ShaderToken::GtGt, 2),
                },
                Some('=') => (ShaderToken::GtEq, 2),
                _ => (ShaderToken::Gt, 1),
            },
            '&' => match next {
                Some('&') => (ShaderToken::AmpAmp, 2),
                Some('=') => (ShaderToken::AmpAssign, 2),
                _ => (ShaderToken::Amp, 1),
            },
            '|' => match next {
                Some('|') => (ShaderToken::PipePipe, 2),
                Some('=') => (ShaderToken::PipeAssign, 2),
                _ => (ShaderToken::Pipe, 1),
            },
            '^' => match next {
                Some('=') => (ShaderToken::CaretAssign, 2),
                _ => (ShaderToken::Caret, 1),
            },
            '~' => (ShaderToken::Tilde, 1),
            '?' => (ShaderToken::Question, 1),

            // Punctuation
            '(' => (ShaderToken::LParen, 1),
            ')' => (ShaderToken::RParen, 1),
            '[' => (ShaderToken::LBracket, 1),
            ']' => (ShaderToken::RBracket, 1),
            '{' => (ShaderToken::LBrace, 1),
            '}' => (ShaderToken::RBrace, 1),
            ':' => (ShaderToken::Colon, 1),
            ';' => (ShaderToken::Semicolon, 1),
            ',' => (ShaderToken::Comma, 1),
            '.' => (ShaderToken::Dot, 1),

            _ => {
                return Err(ShaderLexError::UnexpectedChar {
                    ch,
                    line: start_line,
                    col: start_col,
                });
            }
        };

        tokens.push(ShaderTokenSpan {
            token: tok,
            line: start_line,
            col: start_col,
        });
        pos += advance;
        col += advance;
    }

    tokens.push(ShaderTokenSpan {
        token: ShaderToken::Eof,
        line,
        col,
    });

    Ok(tokens)
}

/// Lexes a numeric literal starting at `pos`.
///
/// Handles:
/// - Decimal integers: `42`
/// - Hex: `0xFF`
/// - Octal: `0o77`
/// - Binary: `0b1010`
/// - Floats: `3.14`, `.5`, `1e10`, `2.5e-3`
/// - Unsigned suffix: `42u`
fn lex_number(
    chars: &[char],
    start: usize,
    len: usize,
    line: usize,
    col: usize,
) -> Result<(ShaderToken, usize), ShaderLexError> {
    let mut pos = start;

    // Check for hex/octal/binary prefix
    if chars[pos] == '0' && pos + 1 < len {
        match chars[pos + 1] {
            'x' | 'X' => {
                pos += 2;
                let digit_start = pos;
                while pos < len && chars[pos].is_ascii_hexdigit() {
                    pos += 1;
                }
                if pos == digit_start {
                    return Err(ShaderLexError::InvalidNumber {
                        detail: "empty hex literal".into(),
                        line,
                        col,
                    });
                }
                let text: String = chars[digit_start..pos].iter().collect();
                let unsigned = pos < len && (chars[pos] == 'u' || chars[pos] == 'U');
                if unsigned {
                    pos += 1;
                }
                let val =
                    u64::from_str_radix(&text, 16).map_err(|e| ShaderLexError::InvalidNumber {
                        detail: e.to_string(),
                        line,
                        col,
                    })?;
                return if unsigned {
                    Ok((ShaderToken::UintLit(val), pos))
                } else {
                    Ok((ShaderToken::IntLit(val as i64), pos))
                };
            }
            'o' | 'O' => {
                pos += 2;
                let digit_start = pos;
                while pos < len && matches!(chars[pos], '0'..='7') {
                    pos += 1;
                }
                if pos == digit_start {
                    return Err(ShaderLexError::InvalidNumber {
                        detail: "empty octal literal".into(),
                        line,
                        col,
                    });
                }
                let text: String = chars[digit_start..pos].iter().collect();
                let unsigned = pos < len && (chars[pos] == 'u' || chars[pos] == 'U');
                if unsigned {
                    pos += 1;
                }
                let val =
                    u64::from_str_radix(&text, 8).map_err(|e| ShaderLexError::InvalidNumber {
                        detail: e.to_string(),
                        line,
                        col,
                    })?;
                return if unsigned {
                    Ok((ShaderToken::UintLit(val), pos))
                } else {
                    Ok((ShaderToken::IntLit(val as i64), pos))
                };
            }
            'b' | 'B' => {
                pos += 2;
                let digit_start = pos;
                while pos < len && matches!(chars[pos], '0' | '1') {
                    pos += 1;
                }
                if pos == digit_start {
                    return Err(ShaderLexError::InvalidNumber {
                        detail: "empty binary literal".into(),
                        line,
                        col,
                    });
                }
                let text: String = chars[digit_start..pos].iter().collect();
                let unsigned = pos < len && (chars[pos] == 'u' || chars[pos] == 'U');
                if unsigned {
                    pos += 1;
                }
                let val =
                    u64::from_str_radix(&text, 2).map_err(|e| ShaderLexError::InvalidNumber {
                        detail: e.to_string(),
                        line,
                        col,
                    })?;
                return if unsigned {
                    Ok((ShaderToken::UintLit(val), pos))
                } else {
                    Ok((ShaderToken::IntLit(val as i64), pos))
                };
            }
            _ => {}
        }
    }

    // Decimal integer or float
    let num_start = pos;
    while pos < len && chars[pos].is_ascii_digit() {
        pos += 1;
    }

    let mut is_float = false;

    // Fractional part
    if pos < len && chars[pos] == '.' {
        // Make sure this is a decimal point, not a method call like `1.method()`
        if pos + 1 < len
            && (chars[pos + 1].is_ascii_digit() || chars[pos + 1] == 'e' || chars[pos + 1] == 'E')
        {
            is_float = true;
            pos += 1; // skip '.'
            while pos < len && chars[pos].is_ascii_digit() {
                pos += 1;
            }
        } else if num_start == pos {
            // Started with '.', e.g. ".5"
            is_float = true;
            pos += 1; // skip '.'
            while pos < len && chars[pos].is_ascii_digit() {
                pos += 1;
            }
        }
    }

    // Exponent part
    if pos < len && (chars[pos] == 'e' || chars[pos] == 'E') {
        is_float = true;
        pos += 1;
        if pos < len && (chars[pos] == '+' || chars[pos] == '-') {
            pos += 1;
        }
        let exp_start = pos;
        while pos < len && chars[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos == exp_start {
            return Err(ShaderLexError::InvalidNumber {
                detail: "empty exponent".into(),
                line,
                col,
            });
        }
    }

    // Float suffix 'f' (optional, not standard GDShader but some GLSL habits)
    if pos < len && (chars[pos] == 'f' || chars[pos] == 'F') && is_float {
        pos += 1;
    }

    let text: String = chars[num_start..pos].iter().collect();
    // Strip trailing 'f'/'F' for parsing
    let parse_text = text.trim_end_matches(|c| c == 'f' || c == 'F');

    if is_float {
        let val: f64 = parse_text.parse().map_err(|e: std::num::ParseFloatError| {
            ShaderLexError::InvalidNumber {
                detail: e.to_string(),
                line,
                col,
            }
        })?;
        Ok((ShaderToken::FloatLit(val), pos))
    } else {
        // Check for unsigned suffix
        let unsigned = pos < len && (chars[pos] == 'u' || chars[pos] == 'U');
        if unsigned {
            pos += 1;
        }
        let val: u64 = parse_text.parse().map_err(|e: std::num::ParseIntError| {
            ShaderLexError::InvalidNumber {
                detail: e.to_string(),
                line,
                col,
            }
        })?;
        if unsigned {
            Ok((ShaderToken::UintLit(val), pos))
        } else {
            Ok((ShaderToken::IntLit(val as i64), pos))
        }
    }
}

/// Convenience: tokenize and strip all comment tokens.
pub fn tokenize_shader_no_comments(source: &str) -> Result<Vec<ShaderTokenSpan>, ShaderLexError> {
    let mut tokens = tokenize_shader(source)?;
    tokens.retain(|t| {
        !matches!(
            t.token,
            ShaderToken::LineComment(_) | ShaderToken::BlockComment(_)
        )
    });
    Ok(tokens)
}

/// Extracts the `shader_type` keyword from a token stream.
///
/// Looks for the pattern `ShaderType <Ident> Semicolon` and returns the
/// type name (e.g. `"spatial"`, `"canvas_item"`).
pub fn extract_shader_type(tokens: &[ShaderTokenSpan]) -> Option<String> {
    for window in tokens.windows(3) {
        if matches!(window[0].token, ShaderToken::ShaderType) {
            if let ShaderToken::Ident(ref name) = window[1].token {
                if matches!(window[2].token, ShaderToken::Semicolon) {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

/// A uniform declaration extracted from a token stream.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderUniform {
    /// The type of the uniform (e.g. `"float"`, `"vec3"`, `"sampler2D"`).
    pub type_name: String,
    /// The name of the uniform variable.
    pub name: String,
    /// Whether this is an `instance` uniform.
    pub instance: bool,
    /// The uniform group name, if declared inside `group_uniforms`.
    pub group: Option<String>,
}

/// Extracts all uniform declarations from a token stream (comments stripped).
///
/// Handles patterns like:
/// - `uniform float speed;`
/// - `uniform vec3 offset = vec3(0.0);`
/// - `uniform float opacity : hint_range(0.0, 1.0) = 1.0;`
/// - `instance uniform float scale;`
/// - `group_uniforms mygroup; uniform float x;`
pub fn extract_uniforms(tokens: &[ShaderTokenSpan]) -> Vec<ShaderUniform> {
    let mut uniforms = Vec::new();
    let mut current_group: Option<String> = None;
    let mut i = 0;

    while i < tokens.len() {
        // Track group_uniforms blocks
        if matches!(tokens[i].token, ShaderToken::GroupUniforms) {
            i += 1;
            if i < tokens.len() {
                if let ShaderToken::Ident(ref name) = tokens[i].token {
                    current_group = Some(name.clone());
                    i += 1;
                    continue;
                } else if matches!(tokens[i].token, ShaderToken::Semicolon) {
                    // `group_uniforms;` ends the group
                    current_group = None;
                    i += 1;
                    continue;
                }
            }
            continue;
        }

        // Check for [instance] uniform
        let is_instance = matches!(tokens[i].token, ShaderToken::Instance);
        let uni_idx = if is_instance { i + 1 } else { i };

        if uni_idx < tokens.len() && matches!(tokens[uni_idx].token, ShaderToken::Uniform) {
            // Next token should be the type
            let type_idx = uni_idx + 1;
            if type_idx < tokens.len() {
                let type_name = token_type_name(&tokens[type_idx].token);
                if let Some(tn) = type_name {
                    let name_idx = type_idx + 1;
                    if name_idx < tokens.len() {
                        if let ShaderToken::Ident(ref var_name) = tokens[name_idx].token {
                            uniforms.push(ShaderUniform {
                                type_name: tn,
                                name: var_name.clone(),
                                instance: is_instance,
                                group: current_group.clone(),
                            });
                            // Skip to semicolon
                            i = name_idx + 1;
                            while i < tokens.len()
                                && !matches!(tokens[i].token, ShaderToken::Semicolon)
                            {
                                i += 1;
                            }
                            i += 1; // skip semicolon
                            continue;
                        }
                    }
                }
            }
        }

        i += 1;
    }

    uniforms
}

/// Returns the type name string for a token if it represents a type.
fn token_type_name(token: &ShaderToken) -> Option<String> {
    match token {
        ShaderToken::Bool => Some("bool".into()),
        ShaderToken::Int => Some("int".into()),
        ShaderToken::Uint => Some("uint".into()),
        ShaderToken::Float => Some("float".into()),
        ShaderToken::Vec2 => Some("vec2".into()),
        ShaderToken::Vec3 => Some("vec3".into()),
        ShaderToken::Vec4 => Some("vec4".into()),
        ShaderToken::Ivec2 => Some("ivec2".into()),
        ShaderToken::Ivec3 => Some("ivec3".into()),
        ShaderToken::Ivec4 => Some("ivec4".into()),
        ShaderToken::Uvec2 => Some("uvec2".into()),
        ShaderToken::Uvec3 => Some("uvec3".into()),
        ShaderToken::Uvec4 => Some("uvec4".into()),
        ShaderToken::Bvec2 => Some("bvec2".into()),
        ShaderToken::Bvec3 => Some("bvec3".into()),
        ShaderToken::Bvec4 => Some("bvec4".into()),
        ShaderToken::Mat2 => Some("mat2".into()),
        ShaderToken::Mat3 => Some("mat3".into()),
        ShaderToken::Mat4 => Some("mat4".into()),
        ShaderToken::Sampler2D => Some("sampler2D".into()),
        ShaderToken::Isampler2D => Some("isampler2D".into()),
        ShaderToken::Usampler2D => Some("usampler2D".into()),
        ShaderToken::Sampler2DArray => Some("sampler2DArray".into()),
        ShaderToken::Sampler3D => Some("sampler3D".into()),
        ShaderToken::SamplerCube => Some("samplerCube".into()),
        ShaderToken::SamplerCubeArray => Some("samplerCubeArray".into()),
        ShaderToken::SamplerExternalOES => Some("samplerExternalOES".into()),
        ShaderToken::Void => Some("void".into()),
        // User-defined struct types come through as identifiers
        ShaderToken::Ident(name) => Some(name.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic tokenization ────────────────────────────────────────────

    #[test]
    fn tokenize_shader_type_declaration() {
        let tokens = tokenize_shader("shader_type spatial;").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::ShaderType);
        assert_eq!(tokens[1].token, ShaderToken::Ident("spatial".into()));
        assert_eq!(tokens[2].token, ShaderToken::Semicolon);
        assert_eq!(tokens[3].token, ShaderToken::Eof);
    }

    #[test]
    fn tokenize_render_mode() {
        let tokens = tokenize_shader("render_mode unshaded, skip_vertex_transform;").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::RenderMode);
        assert_eq!(tokens[1].token, ShaderToken::Ident("unshaded".into()));
        assert_eq!(tokens[2].token, ShaderToken::Comma);
        assert_eq!(
            tokens[3].token,
            ShaderToken::Ident("skip_vertex_transform".into())
        );
        assert_eq!(tokens[4].token, ShaderToken::Semicolon);
    }

    #[test]
    fn tokenize_uniform_with_hint() {
        let src = "uniform float opacity : hint_range(0.0, 1.0) = 1.0;";
        let tokens = tokenize_shader(src).unwrap();
        assert_eq!(tokens[0].token, ShaderToken::Uniform);
        assert_eq!(tokens[1].token, ShaderToken::Float);
        assert_eq!(tokens[2].token, ShaderToken::Ident("opacity".into()));
        assert_eq!(tokens[3].token, ShaderToken::Colon);
        assert_eq!(tokens[4].token, ShaderToken::Ident("hint_range".into()));
        assert_eq!(tokens[5].token, ShaderToken::LParen);
        assert_eq!(tokens[6].token, ShaderToken::FloatLit(0.0));
        assert_eq!(tokens[7].token, ShaderToken::Comma);
        assert_eq!(tokens[8].token, ShaderToken::FloatLit(1.0));
        assert_eq!(tokens[9].token, ShaderToken::RParen);
        assert_eq!(tokens[10].token, ShaderToken::Assign);
        assert_eq!(tokens[11].token, ShaderToken::FloatLit(1.0));
        assert_eq!(tokens[12].token, ShaderToken::Semicolon);
    }

    #[test]
    fn tokenize_function_signature() {
        let src = "void fragment() {\n\tCOLOR = vec4(1.0);\n}";
        let tokens = tokenize_shader(src).unwrap();
        assert_eq!(tokens[0].token, ShaderToken::Void);
        assert_eq!(tokens[1].token, ShaderToken::Ident("fragment".into()));
        assert_eq!(tokens[2].token, ShaderToken::LParen);
        assert_eq!(tokens[3].token, ShaderToken::RParen);
        assert_eq!(tokens[4].token, ShaderToken::LBrace);
        assert_eq!(tokens[5].token, ShaderToken::Ident("COLOR".into()));
        assert_eq!(tokens[6].token, ShaderToken::Assign);
        assert_eq!(tokens[7].token, ShaderToken::Vec4);
        assert_eq!(tokens[8].token, ShaderToken::LParen);
        assert_eq!(tokens[9].token, ShaderToken::FloatLit(1.0));
        assert_eq!(tokens[10].token, ShaderToken::RParen);
        assert_eq!(tokens[11].token, ShaderToken::Semicolon);
        assert_eq!(tokens[12].token, ShaderToken::RBrace);
    }

    // ── Comments ──────────────────────────────────────────────────────

    #[test]
    fn tokenize_line_comment() {
        let tokens = tokenize_shader("// this is a comment\nshader_type spatial;").unwrap();
        assert!(
            matches!(&tokens[0].token, ShaderToken::LineComment(s) if s.contains("this is a comment"))
        );
        assert_eq!(tokens[1].token, ShaderToken::ShaderType);
    }

    #[test]
    fn tokenize_block_comment() {
        let tokens = tokenize_shader("/* block\ncomment */\nshader_type spatial;").unwrap();
        assert!(matches!(&tokens[0].token, ShaderToken::BlockComment(s) if s.contains("block")));
        assert_eq!(tokens[1].token, ShaderToken::ShaderType);
    }

    #[test]
    fn unterminated_block_comment() {
        let err = tokenize_shader("/* never closed").unwrap_err();
        assert!(matches!(
            err,
            ShaderLexError::UnterminatedBlockComment { .. }
        ));
    }

    #[test]
    fn strip_comments() {
        let tokens = tokenize_shader_no_comments("// comment\nshader_type spatial;").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::ShaderType);
    }

    // ── Numeric literals ──────────────────────────────────────────────

    #[test]
    fn tokenize_integer_literals() {
        let tokens = tokenize_shader("42 0xFF 0o77 0b1010").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::IntLit(42));
        assert_eq!(tokens[1].token, ShaderToken::IntLit(0xFF));
        assert_eq!(tokens[2].token, ShaderToken::IntLit(0o77));
        assert_eq!(tokens[3].token, ShaderToken::IntLit(0b1010));
    }

    #[test]
    fn tokenize_unsigned_integer() {
        let tokens = tokenize_shader("42u 0xFFu").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::UintLit(42));
        assert_eq!(tokens[1].token, ShaderToken::UintLit(0xFF));
    }

    #[test]
    fn tokenize_float_literals() {
        let tokens = tokenize_shader("3.14 .5 1e10 2.5e-3").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::FloatLit(3.14));
        assert_eq!(tokens[1].token, ShaderToken::FloatLit(0.5));
        assert_eq!(tokens[2].token, ShaderToken::FloatLit(1e10));
        assert_eq!(tokens[3].token, ShaderToken::FloatLit(2.5e-3));
    }

    #[test]
    fn tokenize_negative_exponent() {
        let tokens = tokenize_shader("1.0e-5").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::FloatLit(1.0e-5));
    }

    // ── Operators ─────────────────────────────────────────────────────

    #[test]
    fn tokenize_comparison_operators() {
        let tokens = tokenize_shader("== != < > <= >=").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::EqEq);
        assert_eq!(tokens[1].token, ShaderToken::BangEq);
        assert_eq!(tokens[2].token, ShaderToken::Lt);
        assert_eq!(tokens[3].token, ShaderToken::Gt);
        assert_eq!(tokens[4].token, ShaderToken::LtEq);
        assert_eq!(tokens[5].token, ShaderToken::GtEq);
    }

    #[test]
    fn tokenize_compound_assignment() {
        let tokens = tokenize_shader("+= -= *= /= %= &= |= ^= <<= >>=").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::PlusAssign);
        assert_eq!(tokens[1].token, ShaderToken::MinusAssign);
        assert_eq!(tokens[2].token, ShaderToken::StarAssign);
        assert_eq!(tokens[3].token, ShaderToken::SlashAssign);
        assert_eq!(tokens[4].token, ShaderToken::PercentAssign);
        assert_eq!(tokens[5].token, ShaderToken::AmpAssign);
        assert_eq!(tokens[6].token, ShaderToken::PipeAssign);
        assert_eq!(tokens[7].token, ShaderToken::CaretAssign);
        assert_eq!(tokens[8].token, ShaderToken::LtLtAssign);
        assert_eq!(tokens[9].token, ShaderToken::GtGtAssign);
    }

    #[test]
    fn tokenize_logical_and_bitwise() {
        let tokens = tokenize_shader("&& || ! & | ^ ~ << >>").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::AmpAmp);
        assert_eq!(tokens[1].token, ShaderToken::PipePipe);
        assert_eq!(tokens[2].token, ShaderToken::Bang);
        assert_eq!(tokens[3].token, ShaderToken::Amp);
        assert_eq!(tokens[4].token, ShaderToken::Pipe);
        assert_eq!(tokens[5].token, ShaderToken::Caret);
        assert_eq!(tokens[6].token, ShaderToken::Tilde);
        assert_eq!(tokens[7].token, ShaderToken::LtLt);
        assert_eq!(tokens[8].token, ShaderToken::GtGt);
    }

    #[test]
    fn tokenize_increment_decrement() {
        let tokens = tokenize_shader("i++ j--").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::Ident("i".into()));
        assert_eq!(tokens[1].token, ShaderToken::PlusPlus);
        assert_eq!(tokens[2].token, ShaderToken::Ident("j".into()));
        assert_eq!(tokens[3].token, ShaderToken::MinusMinus);
    }

    // ── Keywords & types ──────────────────────────────────────────────

    #[test]
    fn tokenize_all_builtin_types() {
        let src = "bool int uint float vec2 vec3 vec4 ivec2 ivec3 ivec4 uvec2 uvec3 uvec4 bvec2 bvec3 bvec4 mat2 mat3 mat4 sampler2D samplerCube";
        let tokens = tokenize_shader(src).unwrap();
        assert_eq!(tokens[0].token, ShaderToken::Bool);
        assert_eq!(tokens[1].token, ShaderToken::Int);
        assert_eq!(tokens[2].token, ShaderToken::Uint);
        assert_eq!(tokens[3].token, ShaderToken::Float);
        assert_eq!(tokens[4].token, ShaderToken::Vec2);
        assert_eq!(tokens[5].token, ShaderToken::Vec3);
        assert_eq!(tokens[6].token, ShaderToken::Vec4);
        assert_eq!(tokens[7].token, ShaderToken::Ivec2);
        assert_eq!(tokens[8].token, ShaderToken::Ivec3);
        assert_eq!(tokens[9].token, ShaderToken::Ivec4);
        assert_eq!(tokens[10].token, ShaderToken::Uvec2);
        assert_eq!(tokens[11].token, ShaderToken::Uvec3);
        assert_eq!(tokens[12].token, ShaderToken::Uvec4);
        assert_eq!(tokens[13].token, ShaderToken::Bvec2);
        assert_eq!(tokens[14].token, ShaderToken::Bvec3);
        assert_eq!(tokens[15].token, ShaderToken::Bvec4);
        assert_eq!(tokens[16].token, ShaderToken::Mat2);
        assert_eq!(tokens[17].token, ShaderToken::Mat3);
        assert_eq!(tokens[18].token, ShaderToken::Mat4);
        assert_eq!(tokens[19].token, ShaderToken::Sampler2D);
        assert_eq!(tokens[20].token, ShaderToken::SamplerCube);
    }

    #[test]
    fn tokenize_control_flow_keywords() {
        let src = "if else for while do switch case default break continue return discard";
        let tokens = tokenize_shader(src).unwrap();
        assert_eq!(tokens[0].token, ShaderToken::If);
        assert_eq!(tokens[1].token, ShaderToken::Else);
        assert_eq!(tokens[2].token, ShaderToken::For);
        assert_eq!(tokens[3].token, ShaderToken::While);
        assert_eq!(tokens[4].token, ShaderToken::Do);
        assert_eq!(tokens[5].token, ShaderToken::Switch);
        assert_eq!(tokens[6].token, ShaderToken::Case);
        assert_eq!(tokens[7].token, ShaderToken::Default);
        assert_eq!(tokens[8].token, ShaderToken::Break);
        assert_eq!(tokens[9].token, ShaderToken::Continue);
        assert_eq!(tokens[10].token, ShaderToken::Return);
        assert_eq!(tokens[11].token, ShaderToken::Discard);
    }

    #[test]
    fn tokenize_qualifier_keywords() {
        let src = "in out inout flat smooth lowp mediump highp instance global";
        let tokens = tokenize_shader(src).unwrap();
        assert_eq!(tokens[0].token, ShaderToken::In);
        assert_eq!(tokens[1].token, ShaderToken::Out);
        assert_eq!(tokens[2].token, ShaderToken::Inout);
        assert_eq!(tokens[3].token, ShaderToken::Flat);
        assert_eq!(tokens[4].token, ShaderToken::Smooth);
        assert_eq!(tokens[5].token, ShaderToken::Lowp);
        assert_eq!(tokens[6].token, ShaderToken::Mediump);
        assert_eq!(tokens[7].token, ShaderToken::Highp);
        assert_eq!(tokens[8].token, ShaderToken::Instance);
        assert_eq!(tokens[9].token, ShaderToken::Global);
    }

    #[test]
    fn tokenize_boolean_literals() {
        let tokens = tokenize_shader("true false").unwrap();
        assert_eq!(tokens[0].token, ShaderToken::BoolLit(true));
        assert_eq!(tokens[1].token, ShaderToken::BoolLit(false));
    }

    // ── Source location tracking ──────────────────────────────────────

    #[test]
    fn line_and_column_tracking() {
        let src = "shader_type spatial;\nvoid fragment() {}";
        let tokens = tokenize_shader(src).unwrap();
        // shader_type is at line 1, col 1
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 1);
        // "void" is at line 2, col 1
        let void_tok = tokens
            .iter()
            .find(|t| t.token == ShaderToken::Void)
            .unwrap();
        assert_eq!(void_tok.line, 2);
        assert_eq!(void_tok.col, 1);
    }

    // ── extract_shader_type ───────────────────────────────────────────

    #[test]
    fn extract_type_from_tokens() {
        let tokens = tokenize_shader_no_comments("shader_type canvas_item;").unwrap();
        assert_eq!(extract_shader_type(&tokens), Some("canvas_item".into()));
    }

    #[test]
    fn extract_type_missing() {
        let tokens = tokenize_shader_no_comments("void fragment() {}").unwrap();
        assert_eq!(extract_shader_type(&tokens), None);
    }

    // ── extract_uniforms ──────────────────────────────────────────────

    #[test]
    fn extract_simple_uniforms() {
        let src = "shader_type spatial;\nuniform float speed;\nuniform vec3 offset;";
        let tokens = tokenize_shader_no_comments(src).unwrap();
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 2);
        assert_eq!(unis[0].type_name, "float");
        assert_eq!(unis[0].name, "speed");
        assert!(!unis[0].instance);
        assert_eq!(unis[1].type_name, "vec3");
        assert_eq!(unis[1].name, "offset");
    }

    #[test]
    fn extract_uniform_with_default_and_hint() {
        let src = "shader_type canvas_item;\nuniform float opacity : hint_range(0.0, 1.0) = 1.0;";
        let tokens = tokenize_shader_no_comments(src).unwrap();
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 1);
        assert_eq!(unis[0].type_name, "float");
        assert_eq!(unis[0].name, "opacity");
    }

    #[test]
    fn extract_instance_uniform() {
        let src = "shader_type spatial;\ninstance uniform float scale;";
        let tokens = tokenize_shader_no_comments(src).unwrap();
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 1);
        assert!(unis[0].instance);
        assert_eq!(unis[0].name, "scale");
    }

    #[test]
    fn extract_sampler_uniform() {
        let src = "shader_type spatial;\nuniform sampler2D albedo_tex;";
        let tokens = tokenize_shader_no_comments(src).unwrap();
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 1);
        assert_eq!(unis[0].type_name, "sampler2D");
        assert_eq!(unis[0].name, "albedo_tex");
    }

    #[test]
    fn extract_group_uniforms() {
        let src = "shader_type spatial;\ngroup_uniforms lighting;\nuniform float brightness;\nuniform float contrast;\ngroup_uniforms;\nuniform float global_val;";
        let tokens = tokenize_shader_no_comments(src).unwrap();
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 3);
        assert_eq!(unis[0].group, Some("lighting".into()));
        assert_eq!(unis[1].group, Some("lighting".into()));
        assert_eq!(unis[2].group, None);
    }

    // ── Full shader tokenization ──────────────────────────────────────

    #[test]
    fn tokenize_complete_canvas_item_shader() {
        let src = r#"shader_type canvas_item;

uniform float speed = 1.5;
uniform vec4 tint_color : source_color = vec4(1.0, 1.0, 1.0, 1.0);

void fragment() {
    float t = TIME * speed;
    COLOR = texture(TEXTURE, UV) * tint_color;
    COLOR.a *= sin(t) * 0.5 + 0.5;
}
"#;
        let tokens = tokenize_shader(src).unwrap();
        // Should tokenize without errors
        assert!(tokens.len() > 20);
        // Last token is Eof
        assert_eq!(tokens.last().unwrap().token, ShaderToken::Eof);

        // Verify shader_type extraction
        let no_comments = tokenize_shader_no_comments(src).unwrap();
        assert_eq!(
            extract_shader_type(&no_comments),
            Some("canvas_item".into())
        );

        // Verify uniform extraction
        let unis = extract_uniforms(&no_comments);
        assert_eq!(unis.len(), 2);
        assert_eq!(unis[0].type_name, "float");
        assert_eq!(unis[0].name, "speed");
        assert_eq!(unis[1].type_name, "vec4");
        assert_eq!(unis[1].name, "tint_color");
    }

    #[test]
    fn tokenize_spatial_shader_with_vertex() {
        let src = r#"shader_type spatial;
render_mode blend_mix, depth_draw_opaque;

uniform float height_scale = 0.5;
uniform sampler2D height_map;

void vertex() {
    float h = texture(height_map, UV).r * height_scale;
    VERTEX.y += h;
}

void fragment() {
    ALBEDO = vec3(0.8, 0.2, 0.1);
    ROUGHNESS = 0.9;
}
"#;
        let tokens = tokenize_shader_no_comments(src).unwrap();
        assert_eq!(extract_shader_type(&tokens), Some("spatial".into()));
        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 2);
        assert_eq!(unis[0].name, "height_scale");
        assert_eq!(unis[1].type_name, "sampler2D");
        assert_eq!(unis[1].name, "height_map");
    }

    #[test]
    fn tokenize_shader_with_struct() {
        let src = r#"shader_type spatial;

struct LightData {
    vec3 direction;
    vec4 color;
    float intensity;
};

uniform LightData main_light;
"#;
        let tokens = tokenize_shader_no_comments(src).unwrap();
        // struct keyword
        let struct_count = tokens
            .iter()
            .filter(|t| t.token == ShaderToken::Struct)
            .count();
        assert_eq!(struct_count, 1);

        let unis = extract_uniforms(&tokens);
        assert_eq!(unis.len(), 1);
        // User-defined struct type comes through as identifier
        assert_eq!(unis[0].type_name, "LightData");
        assert_eq!(unis[0].name, "main_light");
    }

    #[test]
    fn unexpected_character_error() {
        let err = tokenize_shader("shader_type spatial; #invalid").unwrap_err();
        assert!(matches!(
            err,
            ShaderLexError::UnexpectedChar { ch: '#', .. }
        ));
    }

    #[test]
    fn empty_source() {
        let tokens = tokenize_shader("").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, ShaderToken::Eof);
    }

    #[test]
    fn whitespace_only() {
        let tokens = tokenize_shader("   \n\t\n  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, ShaderToken::Eof);
    }

    #[test]
    fn ternary_operator() {
        let tokens = tokenize_shader("a ? b : c").unwrap();
        assert_eq!(tokens[1].token, ShaderToken::Question);
        assert_eq!(tokens[3].token, ShaderToken::Colon);
    }

    #[test]
    fn display_roundtrip_keywords() {
        // Verify Display produces the expected text for key tokens
        assert_eq!(format!("{}", ShaderToken::ShaderType), "shader_type");
        assert_eq!(format!("{}", ShaderToken::Uniform), "uniform");
        assert_eq!(format!("{}", ShaderToken::Vec4), "vec4");
        assert_eq!(format!("{}", ShaderToken::Sampler2D), "sampler2D");
        assert_eq!(format!("{}", ShaderToken::IntLit(42)), "42");
        assert_eq!(format!("{}", ShaderToken::UintLit(42)), "42u");
        assert_eq!(format!("{}", ShaderToken::FloatLit(3.14)), "3.14");
    }
}
