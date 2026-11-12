//! pat-s3rrv.36 (slice 6): the 3D fragment path evaluates the real GDShader
//! language.
//!
//! `parse_fragment_body_3d` was a six-shape line matcher that silently skipped
//! any statement it could not classify, and `execute_fragment_3d` collapsed
//! unparsed expressions to `ColorExpr3D::BuiltinAlbedo`. A spatial shader with
//! a local variable or an `if` therefore rendered as plain albedo with no
//! diagnostic.
//!
//! The bead's 3D acceptance case is exactly this: a `fragment()` body that
//! declares a local `vec3` and branches on `if (UV.x > 0.5)` must produce two
//! distinct colours across the quad.

use std::collections::HashMap;

use gdcore::math::Color;
use gdserver3d::shader::{
    execute_fragment_3d, execute_fragment_3d_with_env, parse_fragment_body_3d, FragmentContext3D,
    FragmentInstruction3D, Sampler, ShaderEnv3D,
};
use gdvariant::variant::Variant;

/// A local `vec3` plus a branch on UV — neither expressible in the line matcher.
const BRANCHING: &str = "shader_type spatial;

void fragment() {
\tvec3 tint = vec3(0.0, 0.0, 1.0);
\tif (UV.x > 0.5) {
\t\ttint = vec3(1.0, 0.0, 0.0);
\t}
\tALBEDO = tint;
}
";

fn ctx_at_u(u: f32) -> FragmentContext3D {
    FragmentContext3D {
        uv: (u, 0.5),
        ..Default::default()
    }
}

fn shade(instructions: &[FragmentInstruction3D], u: f32) -> Color {
    execute_fragment_3d(
        instructions,
        &ctx_at_u(u),
        &HashMap::new(),
        &|_n, _u, _v| Color::new(1.0, 1.0, 1.0, 1.0),
    )
}

/// The branching shader yields instructions at all — before this slice the
/// matcher returned an empty vec, which sent the renderer down its
/// uniform-only fallback.
#[test]
fn a_branching_spatial_shader_yields_instructions() {
    let instructions = parse_fragment_body_3d(BRANCHING);
    assert!(
        !instructions.is_empty(),
        "a fragment() body with a local and an `if` must parse; an empty vec is \
         the silent skip this bead is about"
    );
    assert!(
        matches!(
            instructions.first(),
            Some(FragmentInstruction3D::Program(_))
        ),
        "…and it must come back as a parsed Program, not a matched line"
    );
}

/// **The bead's 3D acceptance case.** Two distinct colours across the quad.
#[test]
fn the_branch_produces_two_colours_across_the_quad() {
    let instructions = parse_fragment_body_3d(BRANCHING);

    let left = shade(&instructions, 0.25);
    let right = shade(&instructions, 0.75);

    assert!(
        left.b > 0.9 && left.r < 0.1,
        "UV.x = 0.25 takes the else branch and stays blue, got {left:?}"
    );
    assert!(
        right.r > 0.9 && right.b < 0.1,
        "UV.x = 0.75 takes the if branch and becomes red, got {right:?}"
    );
    assert_ne!(
        (left.r, left.g, left.b),
        (right.r, right.g, right.b),
        "the two halves must differ — identical halves mean the branch never ran"
    );
}

/// A shader the line matcher DOES understand keeps its existing path, so this
/// routing change cannot alter anything that renders correctly today.
#[test]
fn a_matcher_shaped_shader_still_uses_the_matcher() {
    let simple = "shader_type spatial;\nvoid fragment() {\n\tALBEDO = vec3(0.0, 1.0, 0.0);\n}\n";
    let instructions = parse_fragment_body_3d(simple);
    assert!(!instructions.is_empty(), "the matcher still handles this");
    assert!(
        !matches!(
            instructions.first(),
            Some(FragmentInstruction3D::Program(_))
        ),
        "a source the matcher understands must NOT be rerouted through the AST"
    );
}

/// `texture(TEX, UV)` reaches the renderer-supplied sampler through the env.
/// Without this the AST path would sample transparent black and a textured
/// spatial shader would render darker than before — a regression, not a fix.
#[test]
fn the_ast_path_samples_through_the_supplied_sampler() {
    let src = "shader_type spatial;

uniform sampler2D tex;

void fragment() {
\tvec4 c = texture(tex, UV);
\tALBEDO = c.rgb;
}
";
    let instructions = parse_fragment_body_3d(src);
    assert!(matches!(
        instructions.first(),
        Some(FragmentInstruction3D::Program(_))
    ));

    let env = ShaderEnv3D {
        sampler: Some(Sampler::new(|name, _uv| {
            assert_eq!(name, "tex", "the sampler is asked for the uniform's name");
            [0.0, 1.0, 0.0, 1.0]
        })),
    };
    let got = execute_fragment_3d_with_env(
        &instructions,
        &ctx_at_u(0.5),
        &HashMap::new(),
        &|_n, _u, _v| Color::new(1.0, 1.0, 1.0, 1.0),
        &env,
    );
    assert!(
        got.g > 0.9 && got.r < 0.1,
        "the sampled green must reach ALBEDO; got {got:?}"
    );
}

/// A uniform declared in the material reaches the AST path.
#[test]
fn material_parameters_are_visible_as_uniforms() {
    let src = "shader_type spatial;

uniform vec4 tint;

void fragment() {
\tALBEDO = tint.rgb;
}
";
    let instructions = parse_fragment_body_3d(src);
    let mut params = HashMap::new();
    params.insert(
        "tint".to_string(),
        Variant::Color(Color::new(1.0, 0.0, 1.0, 1.0)),
    );

    let got = execute_fragment_3d(&instructions, &ctx_at_u(0.5), &params, &|_n, _u, _v| {
        Color::new(1.0, 1.0, 1.0, 1.0)
    });
    assert!(
        got.r > 0.9 && got.b > 0.9 && got.g < 0.1,
        "the material's magenta tint must reach ALBEDO; got {got:?}"
    );
}

/// A syntactically broken spatial shader produces a line-numbered diagnostic in
/// the channel the editor already surfaces (pat-s3rrv.36).
///
/// `validate_shader` only checks a handful of shapes, so before this a broken
/// shader compiled "successfully", produced an empty fragment_body, and
/// rendered as plain albedo with nothing to tell the author why.
#[test]
fn a_broken_spatial_shader_reports_a_diagnostic() {
    use gdserver3d::{ShaderCompiler3D, ShaderType3D};

    // Unbalanced parenthesis on line 4.
    let broken = "shader_type spatial;\n\nvoid fragment() {\n\tALBEDO = vec3(1.0, 0.0;\n}\n";
    let compiled = ShaderCompiler3D::new().compile(ShaderType3D::Spatial, broken);

    let found = compiled
        .diagnostics
        .iter()
        .find(|d| d.message.contains("shader language"));
    let d = found.unwrap_or_else(|| {
        panic!(
            "a malformed shader must report a diagnostic; got {:?}",
            compiled.diagnostics
        )
    });
    assert!(
        !d.message.is_empty(),
        "the diagnostic must carry the parser's message"
    );

    // A VALID shader must not gain a spurious one — otherwise every shader in
    // the project would light up the editor's warning channel.
    let ok = ShaderCompiler3D::new().compile(ShaderType3D::Spatial, BRANCHING);
    assert!(
        !ok.diagnostics
            .iter()
            .any(|d| d.message.contains("shader language")),
        "a valid shader must not report a parse diagnostic; got {:?}",
        ok.diagnostics
    );
}
