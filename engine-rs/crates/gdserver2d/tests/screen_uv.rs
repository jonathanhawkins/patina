//! pat-0q2kx.223 (partial) — `SCREEN_UV` is readable from a canvas shader.
//!
//! Godot ground truth: `shader_types.cpp` canvas fragment built-ins (:300-302).
//!
//! The producer was already live and the consumer was dead: the software
//! renderer fills `FragmentContext::screen_uv` for every fragment
//! (`gdrender2d/src/draw.rs:870`), but the expression parser had no `SCREEN_UV`
//! case, so no shader could reference it. The value was computed and thrown
//! away on every pixel.
//!
//! ⚠️ SCOPE: this is `SCREEN_UV` only. The bead's headline feature —
//! `hint_screen_texture`, snapshotting the framebuffer and binding it as a
//! sampler so a shader can composite what is behind it — is NOT delivered, and
//! the acceptance test `shader_screen_texture_sampling` does not exist. Screen
//! *texture* sampling is still correctly reported as unsupported. See the
//! completion report.

use std::collections::HashMap;

use gdvariant::Variant;

use gdserver2d::shader::{
    eval_float_expr, parse_fragment_body, FloatExpr, FragmentContext, FragmentInstruction, UvAxis,
};

/// `parse_fragment_body` runs `extract_function_body(source, "fragment")` and
/// returns EMPTY for anything that is not a whole shader — a bare statement
/// silently parses to nothing. Wrap every body, as the in-module tests do.
fn parse_body(stmt: &str) -> Vec<FragmentInstruction> {
    parse_fragment_body(&format!(
        "shader_type canvas_item;\nvoid fragment() {{\n    {stmt}\n}}"
    ))
}

/// `eval_float_expr` takes a uniform MAP, not a slice.
fn no_uniforms() -> HashMap<String, Variant> {
    HashMap::new()
}

fn ctx_with_screen_uv(u: f32, v: f32) -> FragmentContext {
    FragmentContext {
        screen_uv: (u, v),
        ..FragmentContext::default()
    }
}

/// `SCREEN_UV.x` / `.y` parse to their own expression, not to `UV`.
#[test]
fn screen_uv_parses_as_a_distinct_builtin() {
    let body = "COLOR.a = SCREEN_UV.x;";
    let instrs = parse_body(body);
    assert_eq!(
        instrs,
        vec![FragmentInstruction::SetColorAlpha(
            FloatExpr::ScreenUvComponent(UvAxis::X)
        )],
        "SCREEN_UV.x must parse to ScreenUvComponent, not UvComponent — \
         conflating them silently substitutes the item's own UV"
    );

    let y = parse_body("COLOR.a = SCREEN_UV.y;");
    assert_eq!(
        y,
        vec![FragmentInstruction::SetColorAlpha(
            FloatExpr::ScreenUvComponent(UvAxis::Y)
        )]
    );
}

/// The distinction is load-bearing: plain `UV` must still parse to `UV`.
#[test]
fn plain_uv_is_unaffected() {
    assert_eq!(
        parse_body("COLOR.a = UV.x;"),
        vec![FragmentInstruction::SetColorAlpha(FloatExpr::UvComponent(
            UvAxis::X
        ))],
        "adding SCREEN_UV must not capture the existing UV case"
    );
}

/// Evaluation reads `screen_uv`, and — the point of the change — reads it
/// SEPARATELY from `uv`.
#[test]
fn screen_uv_evaluates_from_the_screen_uv_field() {
    // Deliberately different from `uv`, so a wire-up to the wrong field fails.
    let ctx = FragmentContext {
        uv: (0.1, 0.2),
        screen_uv: (0.75, 0.25),
        ..FragmentContext::default()
    };

    assert_eq!(
        eval_float_expr(
            &FloatExpr::ScreenUvComponent(UvAxis::X),
            &ctx,
            &no_uniforms()
        ),
        0.75,
        "SCREEN_UV.x must read screen_uv.0, not uv.0 (0.1)"
    );
    assert_eq!(
        eval_float_expr(
            &FloatExpr::ScreenUvComponent(UvAxis::Y),
            &ctx,
            &no_uniforms()
        ),
        0.25
    );

    // And the original UV still reads its own field.
    assert_eq!(
        eval_float_expr(&FloatExpr::UvComponent(UvAxis::X), &ctx, &no_uniforms()),
        0.1
    );
    assert_eq!(
        eval_float_expr(&FloatExpr::UvComponent(UvAxis::Y), &ctx, &no_uniforms()),
        0.2
    );
}

/// A fragment at the right edge of the screen reads ~1.0 regardless of its own
/// UV — the case a distortion shader depends on.
#[test]
fn screen_uv_tracks_screen_position_not_item_uv() {
    let left = ctx_with_screen_uv(0.0, 0.5);
    let right = ctx_with_screen_uv(1.0, 0.5);
    let expr = FloatExpr::ScreenUvComponent(UvAxis::X);

    assert_eq!(eval_float_expr(&expr, &left, &no_uniforms()), 0.0);
    assert_eq!(eval_float_expr(&expr, &right, &no_uniforms()), 1.0);
    assert_ne!(
        eval_float_expr(&expr, &left, &no_uniforms()),
        eval_float_expr(&expr, &right, &no_uniforms()),
        "two fragments at different screen positions must differ"
    );
}

/// End to end through the parser and evaluator, which is what a real shader
/// body exercises.
#[test]
fn a_parsed_body_evaluates_screen_uv() {
    let instrs = parse_body("COLOR.a = SCREEN_UV.y;");
    let ctx = ctx_with_screen_uv(0.3, 0.6);

    let alpha = match &instrs[0] {
        FragmentInstruction::SetColorAlpha(e) => eval_float_expr(e, &ctx, &no_uniforms()),
        other => panic!("expected SetColorAlpha, got {other:?}"),
    };
    assert!(
        (alpha - 0.6).abs() < 1e-6,
        "a shader body reading SCREEN_UV.y must see 0.6, got {alpha}"
    );
}
