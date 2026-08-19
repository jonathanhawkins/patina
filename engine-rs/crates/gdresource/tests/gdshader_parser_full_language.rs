//! pat-0q2kx.142 — a real GDShader parser and CPU evaluator.
//!
//! Godot's `shader_language.cpp` implements a full GLSL-ES-3 subset and reports
//! invalid source with a line number. Patina's shader support was a
//! six-instruction line matcher (duplicated in `gdserver2d` and `gdserver3d`)
//! that **silently skipped** any line it could not classify — so a shader using
//! a local variable, an `if`, or a loop rendered wrong output with no
//! diagnostic at all.
//!
//! These assert the language features the bead names, with golden values
//! computed by hand from the GLSL definitions, plus the line-numbered error.

use std::collections::HashMap;

use gdresource::shader_ast::{eval_function, parse_shader, ShaderValue, StageContext};

/// Builds a context with the given uniforms and `UV`.
fn ctx(uv: [f32; 2], uniforms: &[(&str, ShaderValue)]) -> StageContext {
    let mut u = HashMap::new();
    for (k, v) in uniforms {
        u.insert((*k).to_string(), *v);
    }
    let mut builtins = HashMap::new();
    builtins.insert("UV".to_string(), ShaderValue::Vec2(uv));
    builtins.insert("TIME".to_string(), ShaderValue::Float(0.0));
    builtins.insert("ALBEDO".to_string(), ShaderValue::Vec3([0.0, 0.0, 0.0]));
    builtins.insert("ALPHA".to_string(), ShaderValue::Float(1.0));
    StageContext::new(u, builtins)
}

fn albedo(c: &StageContext) -> [f32; 3] {
    match c.builtins.get("ALBEDO") {
        Some(ShaderValue::Vec3(v)) => *v,
        other => panic!("ALBEDO should be a vec3, got {other:?}"),
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// **The acceptance test.** A shader using a local variable, `if`/`else` on a
/// uniform, a `for` loop, a user function, and `smoothstep`/`clamp`/`pow`
/// evaluates to mathematically correct values.
#[test]
fn gdshader_parser_full_language() {
    const SRC: &str = r#"
shader_type spatial;

uniform float threshold;

float ramp(float x) {
    return pow(x, 2.0);
}

void fragment() {
    float t = UV.x;
    float shaped = ramp(t);

    if (threshold > 0.5) {
        shaped = shaped + 0.25;
    } else {
        shaped = shaped - 0.25;
    }

    float acc = 0.0;
    for (int i = 0; i < 4; i++) {
        acc = acc + 0.1;
    }

    float s = smoothstep(0.0, 1.0, t);
    ALBEDO = vec3(clamp(shaped, 0.0, 1.0), acc, s);
}
"#;

    let program = parse_shader(SRC).expect("shader parses");
    assert_eq!(program.shader_type.as_deref(), Some("spatial"));
    assert!(
        program.function("ramp").is_some() && program.function("fragment").is_some(),
        "both the user function and the stage entry must be parsed",
    );
    assert_eq!(program.uniforms, vec!["threshold".to_string()]);

    // -- branch taken: threshold > 0.5 ------------------------------------
    // UV.x = 0.5 -> ramp = 0.5^2 = 0.25 -> +0.25 = 0.5
    // acc = 4 * 0.1 = 0.4
    // smoothstep(0,1,0.5) = 0.5*0.5*(3-2*0.5) = 0.25 * 2 = 0.5
    let mut c = ctx([0.5, 0.0], &[("threshold", ShaderValue::Float(1.0))]);
    eval_function(&program, "fragment", &mut c).expect("fragment evaluates");
    let out = albedo(&c);
    assert!(approx(out[0], 0.5), "pow+if branch: got {}", out[0]);
    assert!(
        approx(out[1], 0.4),
        "the for loop must run exactly 4 times, got {}",
        out[1],
    );
    assert!(approx(out[2], 0.5), "smoothstep(0,1,0.5): got {}", out[2]);

    // -- else branch: threshold <= 0.5 -------------------------------------
    // ramp(0.5) = 0.25 -> -0.25 = 0.0, clamped to 0.0
    let mut c = ctx([0.5, 0.0], &[("threshold", ShaderValue::Float(0.1))]);
    eval_function(&program, "fragment", &mut c).expect("fragment evaluates");
    assert!(
        approx(albedo(&c)[0], 0.0),
        "the else branch must be taken, got {}",
        albedo(&c)[0],
    );

    // -- a different UV proves the maths is evaluated, not memoised --------
    // UV.x = 0.25 -> ramp = 0.0625 -> +0.25 = 0.3125
    // smoothstep(0,1,0.25) = 0.0625 * (3 - 0.5) = 0.15625
    let mut c = ctx([0.25, 0.0], &[("threshold", ShaderValue::Float(1.0))]);
    eval_function(&program, "fragment", &mut c).expect("fragment evaluates");
    let out = albedo(&c);
    assert!(approx(out[0], 0.3125), "got {}", out[0]);
    assert!(approx(out[2], 0.15625), "got {}", out[2]);
}

/// An undeclared identifier is a compile error carrying the line it appears on
/// — the old matcher silently skipped the line and rendered anyway.
#[test]
fn undeclared_identifier_reports_its_line() {
    const SRC: &str = r#"shader_type spatial;

void fragment() {
    float a = 1.0;
    ALBEDO = vec3(a, missing_value, 0.0);
}
"#;

    // Parsing succeeds — the identifier is syntactically valid; it is the
    // evaluation that cannot resolve it, which is where the name check lives.
    let program = parse_shader(SRC).expect("shader parses");
    let mut c = ctx([0.0, 0.0], &[]);
    let err = eval_function(&program, "fragment", &mut c)
        .expect_err("an undeclared identifier must be an error, not silently skipped");

    assert!(
        err.message.contains("missing_value"),
        "the error must name the identifier, got: {}",
        err.message,
    );
    assert_eq!(
        err.line, 5,
        "`missing_value` is on line 5; got line {} ({})",
        err.line, err.message,
    );
}

/// A syntax error is reported with its line rather than being dropped.
#[test]
fn syntax_error_reports_its_line() {
    const SRC: &str = r#"shader_type spatial;

void fragment() {
    float a = ;
}
"#;
    let err = parse_shader(SRC).expect_err("a malformed statement must fail to parse");
    assert_eq!(err.line, 4, "got line {} ({})", err.line, err.message);
}

/// Swizzles read and write individual lanes.
#[test]
fn swizzles_read_and_write_components() {
    const SRC: &str = r#"shader_type spatial;

void fragment() {
    vec3 c = vec3(0.1, 0.2, 0.3);
    ALBEDO = c;
    ALBEDO.rg = UV;
    ALPHA = c.b;
}
"#;
    let program = parse_shader(SRC).expect("shader parses");
    let mut c = ctx([0.7, 0.8], &[]);
    eval_function(&program, "fragment", &mut c).expect("fragment evaluates");

    let out = albedo(&c);
    assert!(
        approx(out[0], 0.7) && approx(out[1], 0.8),
        "`.rg = UV` must write the first two lanes, got {out:?}",
    );
    assert!(
        approx(out[2], 0.3),
        "the untouched lane must keep its value, got {out:?}",
    );
    match c.builtins.get("ALPHA") {
        Some(ShaderValue::Float(a)) => assert!(approx(*a, 0.3), "`.b` read gave {a}"),
        other => panic!("ALPHA should be a float, got {other:?}"),
    }
}

/// `discard` stops the shader and is reported to the caller.
#[test]
fn discard_halts_evaluation() {
    const SRC: &str = r#"shader_type spatial;

void fragment() {
    ALBEDO = vec3(1.0, 1.0, 1.0);
    discard;
    ALBEDO = vec3(0.0, 0.0, 0.0);
}
"#;
    let program = parse_shader(SRC).expect("shader parses");
    let mut c = ctx([0.0, 0.0], &[]);
    eval_function(&program, "fragment", &mut c).expect("fragment evaluates");

    assert!(c.discarded, "discard must be reported");
    assert!(
        approx(albedo(&c)[0], 1.0),
        "statements after `discard` must not run, got {:?}",
        albedo(&c),
    );
}

// ---------------------------------------------------------------------------
// pat-s3rrv.36 — a LEXICAL failure must name its line, like a parse failure does
// ---------------------------------------------------------------------------

/// 🔑 `parse_shader` reported every lexical error as **line 0**.
///
/// The tests above already pin that a *parse* error names its line (5 and 4).
/// A *lex* error did not: `parse_shader` mapped the lexer's failure into
/// `ShaderError { line: 0, col: 0 }` and discarded the position the lexer had
/// already computed — even though every `ShaderLexError` variant carries
/// `line`/`col` and its own `Display` prints them.
///
/// ⚠️ Line 0 is not merely imprecise, it is OUT OF RANGE: `ShaderError.line` is
/// 1-based (the tokenizer's counter starts at 1), so 0 is a line no editor can
/// highlight and no author can find. This bead's acceptance asks for a
/// diagnostic "naming the offending line", and a lexical typo is the most common
/// way to earn one.
#[test]
fn a_lexical_error_names_its_line_and_column() {
    // `#` is not a GDShader character — the tokenizer's own test pins it as
    // `ShaderLexError::UnexpectedChar { ch: '#', .. }`. Here it sits on line 3.
    let src = "shader_type canvas_item;\nvoid fragment() {\n    COLOR = #invalid;\n}\n";

    let err = parse_shader(src).expect_err("an unexpected character must fail to parse");

    assert_ne!(
        err.line, 0,
        "line 0 is out of range for a 1-based line number — this is the old \
         behaviour, where the lexer's position was thrown away"
    );
    assert_eq!(
        err.line, 3,
        "the error must name the line the bad character is on; got line {} ({})",
        err.line, err.message
    );
    assert!(
        err.col > 0,
        "the column is 1-based too and must survive the same trip; got {}",
        err.col
    );
    assert!(
        err.message.contains('#'),
        "the message must name the offending character, got {:?}",
        err.message
    );
}

/// The position must survive for EVERY lexical variant, not just the one above.
///
/// An unterminated block comment reports where the comment STARTED, which is the
/// line an author needs — the failure is only detected at end-of-input.
#[test]
fn an_unterminated_block_comment_names_the_line_it_opened_on() {
    let src = "shader_type canvas_item;\nvoid fragment() {\n    /* never closed\n}\n";

    let err = parse_shader(src).expect_err("an unterminated block comment must fail to parse");

    assert_ne!(err.line, 0, "the lexer's position must not be discarded");
    assert_eq!(
        err.line, 3,
        "the comment opens on line 3, which is where the author must look — \
         reporting the end-of-input line would point at the wrong place; got \
         line {} ({})",
        err.line, err.message
    );
}
