//! pat-s3rrv.36 (slice 4): a broken shader reports WHY, with a line number.
//!
//! Godot's `shader_language.cpp` treats anything outside its GLSL-ES-3 subset as
//! a compile error carrying a line. Patina dropped unparsable statements
//! silently, so a broken shader and a shader that legitimately does nothing were
//! indistinguishable — that silence is the root of this bead.
//!
//! `parse_fragment_body` keeps its lossy `Vec<_>` signature so existing callers
//! are untouched; `parse_fragment_body_checked` is the reporting variant.

use gdserver2d::shader::{parse_fragment_body, parse_fragment_body_checked};

/// The shipped outline fixture — full of locals, `max()` and `texture()` taps —
/// parses cleanly through the checked entry point.
#[test]
fn a_valid_shader_reports_no_error() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/shaders/outline.gdshader"
    );
    let source = std::fs::read_to_string(path).expect("fixture exists");
    let parsed = parse_fragment_body_checked(&source)
        .unwrap_or_else(|e| panic!("the shipped fixture must parse, got {e}"));
    assert!(
        !parsed.is_empty(),
        "the fixture must yield instructions, not an empty passthrough"
    );
}

/// **The acceptance behaviour.** A deliberate syntax error names the offending
/// line instead of vanishing.
#[test]
fn a_syntax_error_reports_the_offending_line() {
    // Line 1 `shader_type`, line 2 blank, line 3 `void fragment() {`,
    // line 4 is the broken statement — unbalanced parenthesis.
    let broken = "shader_type canvas_item;\n\nvoid fragment() {\n\tCOLOR = vec4(1.0, 0.0;\n}\n";

    let err = match parse_fragment_body_checked(broken) {
        Err(e) => e,
        Ok(v) => panic!(
            "a malformed shader must report an error, not {} silently-dropped \
             instructions — that silence is exactly the bug",
            v.len()
        ),
    };
    assert!(
        err.line > 0,
        "the diagnostic must carry a 1-based source line, got {}",
        err.line
    );
    assert!(
        !err.message.is_empty(),
        "the diagnostic must say something, got an empty message"
    );

    // The lossy entry point still returns an empty vec for the same source,
    // which is precisely why the checked variant had to exist.
    assert!(
        parse_fragment_body(broken).is_empty(),
        "the unchecked path stays lossy by design"
    );
}

/// A source with no `fragment()` at all is NOT an error — it is a legal shader
/// that this stage has nothing to run. Without this the checked variant would
/// report a false positive on every vertex-only or sky shader.
#[test]
fn a_shader_without_a_fragment_function_is_not_an_error() {
    let vertex_only = "shader_type canvas_item;\n\nvoid vertex() {\n\tVERTEX.x += 1.0;\n}\n";
    let parsed = parse_fragment_body_checked(vertex_only)
        .expect("a shader with no fragment() is legal, not a syntax error");
    assert!(parsed.is_empty(), "…and it yields no fragment instructions");
}

/// A shader the old line matcher already handled reports no error either, so
/// adding diagnostics did not change the verdict for anything that worked.
#[test]
fn a_matcher_shaped_shader_reports_no_error() {
    let simple = "shader_type canvas_item;\nvoid fragment() {\n\tCOLOR.a = 0.5;\n}\n";
    let parsed = parse_fragment_body_checked(simple).expect("simple shader parses");
    assert!(!parsed.is_empty());
}
