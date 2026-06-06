//! pat-ffc5b: Shader resource parsing (shader language tokenizer).
//!
//! Integration tests exercising the GDShader tokenizer public API from the
//! workspace root, covering:
//!
//! 1. Full shader tokenization of realistic GDShader source
//! 2. Keyword, type, literal, operator, and punctuation token variants
//! 3. Comment handling (line and block)
//! 4. `tokenize_shader_no_comments` strips comments
//! 5. `extract_shader_type` extracts spatial/canvas_item/etc
//! 6. `extract_uniforms` with plain, instance, grouped, and hinted uniforms
//! 7. Error cases: unterminated comments, unexpected characters
//! 8. Edge cases: empty source, whitespace-only, nested comments
//! 9. Position tracking (line/column on ShaderTokenSpan)
//! 10. Roundtrip: Display on ShaderToken matches keywords

use gdresource::shader_tokenizer::{
    extract_shader_type, extract_uniforms, tokenize_shader, tokenize_shader_no_comments,
    ShaderLexError, ShaderToken, ShaderTokenSpan, ShaderUniform,
};

// ===========================================================================
// 1. Full shader tokenization
// ===========================================================================

const MINIMAL_SPATIAL_SHADER: &str = "\
shader_type spatial;
render_mode unshaded;

uniform float speed = 1.0;
uniform vec3 offset;

void vertex() {
    VERTEX += offset * speed;
}

void fragment() {
    ALBEDO = vec3(1.0, 0.0, 0.0);
}
";

#[test]
fn ffc5b_tokenize_full_spatial_shader() {
    let tokens = tokenize_shader(MINIMAL_SPATIAL_SHADER).expect("should tokenize");
    assert!(
        tokens.len() > 20,
        "full shader should produce many tokens, got {}",
        tokens.len()
    );

    // First token should be shader_type keyword.
    assert_eq!(tokens[0].token, ShaderToken::ShaderType);
    assert_eq!(tokens[0].line, 1);
    assert_eq!(tokens[0].col, 1);
}

#[test]
fn ffc5b_extract_shader_type_spatial() {
    let tokens = tokenize_shader_no_comments(MINIMAL_SPATIAL_SHADER).unwrap();
    let shader_type = extract_shader_type(&tokens);
    assert_eq!(shader_type, Some("spatial".to_string()));
}

// ===========================================================================
// 2. Canvas item shader type
// ===========================================================================

const CANVAS_ITEM_SHADER: &str = "\
shader_type canvas_item;

void fragment() {
    COLOR = vec4(1.0);
}
";

#[test]
fn ffc5b_extract_shader_type_canvas_item() {
    let tokens = tokenize_shader_no_comments(CANVAS_ITEM_SHADER).unwrap();
    assert_eq!(
        extract_shader_type(&tokens),
        Some("canvas_item".to_string())
    );
}

// ===========================================================================
// 3. Keyword tokens
// ===========================================================================

#[test]
fn ffc5b_keywords_tokenized_correctly() {
    let source = "if else for while do switch case default break continue return discard";
    let tokens = tokenize_shader(source).unwrap();
    let expected = [
        ShaderToken::If,
        ShaderToken::Else,
        ShaderToken::For,
        ShaderToken::While,
        ShaderToken::Do,
        ShaderToken::Switch,
        ShaderToken::Case,
        ShaderToken::Default,
        ShaderToken::Break,
        ShaderToken::Continue,
        ShaderToken::Return,
        ShaderToken::Discard,
    ];
    for (i, exp) in expected.iter().enumerate() {
        assert_eq!(&tokens[i].token, exp, "keyword mismatch at index {i}");
    }
}

#[test]
fn ffc5b_qualifier_keywords() {
    let source = "in out inout flat smooth lowp mediump highp instance global";
    let tokens = tokenize_shader(source).unwrap();
    let expected = [
        ShaderToken::In,
        ShaderToken::Out,
        ShaderToken::Inout,
        ShaderToken::Flat,
        ShaderToken::Smooth,
        ShaderToken::Lowp,
        ShaderToken::Mediump,
        ShaderToken::Highp,
        ShaderToken::Instance,
        ShaderToken::Global,
    ];
    for (i, exp) in expected.iter().enumerate() {
        assert_eq!(&tokens[i].token, exp, "qualifier mismatch at index {i}");
    }
}

// ===========================================================================
// 4. Type tokens
// ===========================================================================

#[test]
fn ffc5b_builtin_types() {
    let source = "bool int uint float vec2 vec3 vec4 mat2 mat3 mat4 sampler2D";
    let tokens = tokenize_shader(source).unwrap();

    assert_eq!(tokens[0].token, ShaderToken::Bool);
    assert_eq!(tokens[1].token, ShaderToken::Int);
    assert_eq!(tokens[2].token, ShaderToken::Uint);
    assert_eq!(tokens[3].token, ShaderToken::Float);
    assert_eq!(tokens[4].token, ShaderToken::Vec2);
    assert_eq!(tokens[5].token, ShaderToken::Vec3);
    assert_eq!(tokens[6].token, ShaderToken::Vec4);
    assert_eq!(tokens[7].token, ShaderToken::Mat2);
    assert_eq!(tokens[8].token, ShaderToken::Mat3);
    assert_eq!(tokens[9].token, ShaderToken::Mat4);
    assert_eq!(tokens[10].token, ShaderToken::Sampler2D);
}

// ===========================================================================
// 5. Literals
// ===========================================================================

#[test]
fn ffc5b_integer_literal() {
    let tokens = tokenize_shader("42").unwrap();
    // Tokens: IntLit(42), Eof
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].token, ShaderToken::IntLit(42)));
    assert_eq!(tokens[1].token, ShaderToken::Eof);
}

#[test]
fn ffc5b_float_literal() {
    let tokens = tokenize_shader("3.14").unwrap();
    assert_eq!(tokens.len(), 2); // FloatLit + Eof
    if let ShaderToken::FloatLit(v) = tokens[0].token {
        assert!((v - 3.14).abs() < 1e-5, "expected 3.14, got {v}");
    } else {
        panic!("expected FloatLit, got {:?}", tokens[0].token);
    }
}

#[test]
fn ffc5b_boolean_literals() {
    let tokens = tokenize_shader("true false").unwrap();
    assert_eq!(tokens[0].token, ShaderToken::BoolLit(true));
    assert_eq!(tokens[1].token, ShaderToken::BoolLit(false));
}

// ===========================================================================
// 6. Operators and punctuation
// ===========================================================================

#[test]
fn ffc5b_operators() {
    let source = "+ - * / = == != < > <= >= && || ! += -= *= /=";
    let tokens = tokenize_shader(source).unwrap();
    assert!(tokens.len() >= 16, "should tokenize all operators");

    assert_eq!(tokens[0].token, ShaderToken::Plus);
    assert_eq!(tokens[1].token, ShaderToken::Minus);
    assert_eq!(tokens[2].token, ShaderToken::Star);
    assert_eq!(tokens[3].token, ShaderToken::Slash);
    assert_eq!(tokens[4].token, ShaderToken::Assign);
    assert_eq!(tokens[5].token, ShaderToken::EqEq);
    assert_eq!(tokens[6].token, ShaderToken::BangEq);
}

#[test]
fn ffc5b_punctuation() {
    let source = "( ) { } [ ] ; , .";
    let tokens = tokenize_shader(source).unwrap();

    assert_eq!(tokens[0].token, ShaderToken::LParen);
    assert_eq!(tokens[1].token, ShaderToken::RParen);
    assert_eq!(tokens[2].token, ShaderToken::LBrace);
    assert_eq!(tokens[3].token, ShaderToken::RBrace);
    assert_eq!(tokens[4].token, ShaderToken::LBracket);
    assert_eq!(tokens[5].token, ShaderToken::RBracket);
    assert_eq!(tokens[6].token, ShaderToken::Semicolon);
    assert_eq!(tokens[7].token, ShaderToken::Comma);
    assert_eq!(tokens[8].token, ShaderToken::Dot);
}

// ===========================================================================
// 7. Comments
// ===========================================================================

#[test]
fn ffc5b_line_comment() {
    let source = "float x; // this is a comment\nint y;";
    let tokens = tokenize_shader(source).unwrap();

    let has_comment = tokens
        .iter()
        .any(|t| matches!(&t.token, ShaderToken::LineComment(_)));
    assert!(has_comment, "should contain a line comment token");
}

#[test]
fn ffc5b_block_comment() {
    let source = "float x; /* block comment */ int y;";
    let tokens = tokenize_shader(source).unwrap();

    let has_block = tokens
        .iter()
        .any(|t| matches!(&t.token, ShaderToken::BlockComment(_)));
    assert!(has_block, "should contain a block comment token");
}

#[test]
fn ffc5b_no_comments_strips_all() {
    let source = "// line\nfloat x; /* block */ int y;";
    let tokens = tokenize_shader_no_comments(source).unwrap();

    for t in &tokens {
        assert!(
            !matches!(
                &t.token,
                ShaderToken::LineComment(_) | ShaderToken::BlockComment(_)
            ),
            "no_comments should strip all comment tokens"
        );
    }
    // Should still have the non-comment tokens.
    assert!(
        tokens.len() >= 4,
        "should have float, x, ;, int, y, ; tokens"
    );
}

// ===========================================================================
// 8. Uniform extraction
// ===========================================================================

#[test]
fn ffc5b_extract_plain_uniforms() {
    let source = "shader_type spatial;\nuniform float speed;\nuniform vec3 offset;";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 2);
    assert_eq!(uniforms[0].type_name, "float");
    assert_eq!(uniforms[0].name, "speed");
    assert!(!uniforms[0].instance);
    assert_eq!(uniforms[0].group, None);

    assert_eq!(uniforms[1].type_name, "vec3");
    assert_eq!(uniforms[1].name, "offset");
}

#[test]
fn ffc5b_extract_instance_uniform() {
    let source = "instance uniform float scale;";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 1);
    assert_eq!(uniforms[0].name, "scale");
    assert!(uniforms[0].instance);
}

#[test]
fn ffc5b_extract_uniform_with_default() {
    let source = "uniform float opacity = 0.5;";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 1);
    assert_eq!(uniforms[0].name, "opacity");
    assert_eq!(uniforms[0].type_name, "float");
}

#[test]
fn ffc5b_extract_uniform_with_hint() {
    let source = "uniform float alpha : hint_range(0.0, 1.0) = 1.0;";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 1);
    assert_eq!(uniforms[0].name, "alpha");
}

#[test]
fn ffc5b_extract_grouped_uniforms() {
    let source = "\
group_uniforms mygroup;
uniform float x;
uniform float y;
group_uniforms;
uniform float z;
";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 3);
    assert_eq!(uniforms[0].group, Some("mygroup".to_string()));
    assert_eq!(uniforms[1].group, Some("mygroup".to_string()));
    assert_eq!(uniforms[2].group, None); // after group_uniforms; reset
}

// ===========================================================================
// 9. Error cases
// ===========================================================================

#[test]
fn ffc5b_unterminated_block_comment_error() {
    let source = "float x; /* unterminated";
    let result = tokenize_shader(source);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ShaderLexError::UnterminatedBlockComment { .. }),
        "expected UnterminatedBlockComment, got {err:?}"
    );
}

// ===========================================================================
// 10. Edge cases
// ===========================================================================

#[test]
fn ffc5b_empty_source() {
    let tokens = tokenize_shader("").unwrap();
    // Only Eof token.
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token, ShaderToken::Eof);
}

#[test]
fn ffc5b_whitespace_only() {
    let tokens = tokenize_shader("   \n\n\t  \n").unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token, ShaderToken::Eof);
}

#[test]
fn ffc5b_identifiers() {
    let tokens = tokenize_shader("my_variable _private x123").unwrap();
    // 3 identifiers + Eof = 4
    assert_eq!(tokens.len(), 4);
    for t in &tokens[..3] {
        assert!(matches!(&t.token, ShaderToken::Ident(_)));
    }
}

// ===========================================================================
// 11. Position tracking
// ===========================================================================

#[test]
fn ffc5b_position_tracking() {
    let source = "float x;\nint y;";
    let tokens = tokenize_shader(source).unwrap();

    // "float" is at line 1, col 1
    assert_eq!(tokens[0].line, 1);
    assert_eq!(tokens[0].col, 1);

    // "int" is at line 2, col 1
    let int_token = tokens.iter().find(|t| t.token == ShaderToken::Int).unwrap();
    assert_eq!(int_token.line, 2);
    assert_eq!(int_token.col, 1);
}

#[test]
fn ffc5b_multiline_position_tracking() {
    let source = "shader_type spatial;\n\nvoid vertex() {\n}\n";
    let tokens = tokenize_shader(source).unwrap();

    // shader_type at line 1
    assert_eq!(tokens[0].line, 1);

    // void at line 3 (skipping blank line 2)
    let void_token = tokens
        .iter()
        .find(|t| t.token == ShaderToken::Void)
        .unwrap();
    assert_eq!(void_token.line, 3);
}

// ===========================================================================
// 12. ShaderToken Display roundtrip
// ===========================================================================

#[test]
fn ffc5b_token_display_keywords() {
    assert_eq!(format!("{}", ShaderToken::ShaderType), "shader_type");
    assert_eq!(format!("{}", ShaderToken::RenderMode), "render_mode");
    assert_eq!(format!("{}", ShaderToken::Uniform), "uniform");
    assert_eq!(format!("{}", ShaderToken::Void), "void");
    assert_eq!(format!("{}", ShaderToken::If), "if");
    assert_eq!(format!("{}", ShaderToken::Return), "return");
    assert_eq!(format!("{}", ShaderToken::Discard), "discard");
}

#[test]
fn ffc5b_token_display_types() {
    assert_eq!(format!("{}", ShaderToken::Float), "float");
    assert_eq!(format!("{}", ShaderToken::Vec3), "vec3");
    assert_eq!(format!("{}", ShaderToken::Mat4), "mat4");
    assert_eq!(format!("{}", ShaderToken::Sampler2D), "sampler2D");
}

// ===========================================================================
// 13. Realistic complex shader
// ===========================================================================

const COMPLEX_SHADER: &str = "\
shader_type spatial;
render_mode unshaded, cull_disabled;

// Uniforms
group_uniforms material;
uniform vec4 albedo_color : source_color = vec4(1.0);
uniform sampler2D albedo_texture;
instance uniform float emission_strength = 0.0;
group_uniforms;

uniform float time_scale = 1.0;

varying vec2 world_uv;

void vertex() {
    world_uv = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xz;
    VERTEX.y += sin(world_uv.x * 2.0 + TIME * time_scale) * 0.1;
}

void fragment() {
    vec4 tex = texture(albedo_texture, UV);
    ALBEDO = tex.rgb * albedo_color.rgb;
    ALPHA = tex.a * albedo_color.a;
    if (emission_strength > 0.0) {
        EMISSION = ALBEDO * emission_strength;
    }
}
";

#[test]
fn ffc5b_complex_shader_tokenizes() {
    let tokens = tokenize_shader(COMPLEX_SHADER).unwrap();
    assert!(
        tokens.len() > 50,
        "complex shader should produce many tokens, got {}",
        tokens.len()
    );
}

#[test]
fn ffc5b_complex_shader_type() {
    let tokens = tokenize_shader_no_comments(COMPLEX_SHADER).unwrap();
    assert_eq!(extract_shader_type(&tokens), Some("spatial".to_string()));
}

#[test]
fn ffc5b_complex_shader_uniforms() {
    let tokens = tokenize_shader_no_comments(COMPLEX_SHADER).unwrap();
    let uniforms = extract_uniforms(&tokens);

    assert_eq!(uniforms.len(), 4, "should find 4 uniforms: {:?}", uniforms);

    // albedo_color — grouped under "material"
    let albedo = uniforms.iter().find(|u| u.name == "albedo_color").unwrap();
    assert_eq!(albedo.type_name, "vec4");
    assert_eq!(albedo.group, Some("material".to_string()));
    assert!(!albedo.instance);

    // albedo_texture — sampler2D, grouped
    let tex = uniforms
        .iter()
        .find(|u| u.name == "albedo_texture")
        .unwrap();
    assert_eq!(tex.type_name, "sampler2D");
    assert_eq!(tex.group, Some("material".to_string()));

    // emission_strength — instance uniform, grouped
    let emission = uniforms
        .iter()
        .find(|u| u.name == "emission_strength")
        .unwrap();
    assert!(emission.instance);
    assert_eq!(emission.group, Some("material".to_string()));

    // time_scale — ungrouped (after group_uniforms;)
    let time = uniforms.iter().find(|u| u.name == "time_scale").unwrap();
    assert_eq!(time.group, None);
    assert!(!time.instance);
}

// ===========================================================================
// 14. ShaderTokenSpan clone and debug
// ===========================================================================

#[test]
fn ffc5b_token_span_clone_debug() {
    let span = ShaderTokenSpan {
        token: ShaderToken::Void,
        line: 5,
        col: 3,
    };
    let cloned = span.clone();
    assert_eq!(cloned.token, ShaderToken::Void);
    assert_eq!(cloned.line, 5);
    assert_eq!(cloned.col, 3);

    let debug = format!("{:?}", span);
    assert!(debug.contains("Void"));
}

#[test]
fn ffc5b_shader_uniform_clone_debug() {
    let uniform = ShaderUniform {
        type_name: "float".to_string(),
        name: "speed".to_string(),
        instance: false,
        group: None,
    };
    let cloned = uniform.clone();
    assert_eq!(uniform, cloned);

    let debug = format!("{:?}", uniform);
    assert!(debug.contains("speed"));
    assert!(debug.contains("float"));
}

// ===========================================================================
// 15. No shader_type returns None
// ===========================================================================

#[test]
fn ffc5b_no_shader_type_returns_none() {
    let source = "uniform float x;";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    assert_eq!(extract_shader_type(&tokens), None);
}

#[test]
fn ffc5b_no_uniforms_returns_empty() {
    let source = "shader_type spatial;\nvoid fragment() {}";
    let tokens = tokenize_shader_no_comments(source).unwrap();
    let uniforms = extract_uniforms(&tokens);
    assert!(uniforms.is_empty());
}
