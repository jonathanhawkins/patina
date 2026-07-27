//! pat-0q2kx.225 (slice 2) — `global uniform` / `instance uniform` survive parsing.
//!
//! `parse_uniforms` gated on `trimmed.starts_with("uniform ")`, so every
//! `global uniform vec4 tint;` and `instance uniform float x;` line was
//! **silently dropped** — not rejected, just absent from the parsed uniform
//! list. A shader referencing a project-wide global therefore behaved as if the
//! uniform had never been declared.
//!
//! Godot 4 widens scope with a declaration prefix: `global uniform` reads a
//! project-wide value shared by every shader naming it (settable via
//! `RenderingServer.global_shader_parameter_set`), `instance uniform` gives each
//! instance its own, and a bare `uniform` is per-material.

use gdserver3d::shader::{parse_uniforms, UniformScope, UniformType};

/// One shader declaring all three scopes, so a fix that handled only the
/// prefixed forms — or only the bare one — would fail.
const SOURCE: &str = "\
shader_type spatial;
uniform vec4 albedo : source_color = vec4(1.0, 0.0, 0.0, 1.0);
global uniform vec4 tint;
instance uniform float wobble = 2.5;
";

#[test]
fn all_three_uniform_scopes_are_parsed() {
    let uniforms = parse_uniforms(SOURCE);
    assert_eq!(
        uniforms.len(),
        3,
        "every declaration must survive; got {:?}",
        uniforms.iter().map(|u| &u.name).collect::<Vec<_>>(),
    );

    let by = |n: &str| {
        uniforms
            .iter()
            .find(|u| u.name == n)
            .unwrap_or_else(|| panic!("`{n}` missing from {uniforms:?}"))
    };

    assert_eq!(by("albedo").scope, UniformScope::Local);
    assert_eq!(by("tint").scope, UniformScope::Global);
    assert_eq!(by("wobble").scope, UniformScope::Instance);
}

/// The prefix must not eat the type: a scoped declaration has to parse its
/// type, name, hint and default exactly like a plain one.
#[test]
fn scoped_declarations_still_parse_type_name_and_default() {
    let uniforms = parse_uniforms(
        "global uniform float strength : hint_range(0, 1) = 0.75;\ninstance uniform vec4 col;\n",
    );
    assert_eq!(uniforms.len(), 2);

    let strength = &uniforms[0];
    assert_eq!(strength.name, "strength");
    assert_eq!(strength.uniform_type, UniformType::Float);
    assert_eq!(strength.scope, UniformScope::Global);
    assert_eq!(
        strength.hint.as_deref(),
        Some("hint_range(0, 1)"),
        "the hint must survive the scope prefix",
    );
    assert!(
        matches!(strength.default_value, gdvariant::Variant::Float(v) if (v - 0.75).abs() < 1e-9),
        "the default must survive too, got {:?}",
        strength.default_value,
    );

    assert_eq!(uniforms[1].name, "col");
    assert_eq!(uniforms[1].uniform_type, UniformType::Vec4);
    assert_eq!(uniforms[1].scope, UniformScope::Instance);
}

/// A bare `uniform` keeps defaulting to Local — the prefixes must not have
/// changed existing behaviour, since every current shader relies on it.
#[test]
fn plain_uniforms_are_unaffected() {
    let uniforms = parse_uniforms("uniform float speed = 3.0;\n");
    assert_eq!(uniforms.len(), 1);
    assert_eq!(uniforms[0].scope, UniformScope::Local);
    assert_eq!(uniforms[0].name, "speed");
}

/// ⚠️ Order matters in the prefix test: `global uniform ` also ends in
/// `uniform `, so testing the bare form first would classify every scoped
/// declaration as Local while still "parsing" it — a silent wrong answer rather
/// than a visible failure.
#[test]
fn scoped_prefixes_are_not_misread_as_local() {
    for (src, want) in [
        ("global uniform vec3 g;\n", UniformScope::Global),
        ("instance uniform vec3 i;\n", UniformScope::Instance),
    ] {
        let u = parse_uniforms(src);
        assert_eq!(u.len(), 1, "parsed {src:?} -> {u:?}");
        assert_eq!(u[0].scope, want, "wrong scope for {src:?}");
    }
}

/// A word merely *containing* "uniform" is not a declaration — guards against
/// a prefix check loose enough to match identifiers or comments.
#[test]
fn non_declarations_are_ignored() {
    let uniforms = parse_uniforms(
        "// uniform vec4 commented;\nvec4 uniformish = vec4(0.0);\nvoid fragment() {}\n",
    );
    assert!(
        uniforms.is_empty(),
        "comments and lookalike identifiers must not parse as uniforms, got {uniforms:?}",
    );
}

// ===========================================================================
// Slice 4: a global uniform actually reads from the store during evaluation
// ===========================================================================

/// **The wiring that matters.** A `global uniform` never appears in a
/// material's parameter map, so evaluation must fall through to the
/// project-wide store — otherwise the declaration parses (slice 2) and the
/// store holds a value (slice 3) while the shader still renders with nothing.
#[test]
fn global_uniform_resolves_from_the_store_during_evaluation() {
    gdvariant::shader_globals::clear();
    gdvariant::shader_globals::add("wind_strength", "float", gdvariant::Variant::Float(4.5));

    let ctx = gdserver3d::shader::FragmentContext3D::default();
    // No material parameters at all — the value can only come from the store.
    let params = std::collections::HashMap::new();

    let value = gdserver3d::shader::eval_float_expr_3d(
        &gdserver3d::shader::FloatExpr3D::Uniform("wind_strength".to_string()),
        &ctx,
        &params,
    );
    assert!(
        (value - 4.5).abs() < 1e-6,
        "a global uniform must evaluate to the store's value, got {value}",
    );
}

/// A material parameter of the same name shadows the global — the more
/// specific binding wins, as in Godot.
#[test]
fn material_parameter_shadows_the_global() {
    gdvariant::shader_globals::clear();
    gdvariant::shader_globals::add("speed", "float", gdvariant::Variant::Float(1.0));

    let ctx = gdserver3d::shader::FragmentContext3D::default();
    let mut params = std::collections::HashMap::new();
    params.insert("speed".to_string(), gdvariant::Variant::Float(9.0));

    let value = gdserver3d::shader::eval_float_expr_3d(
        &gdserver3d::shader::FloatExpr3D::Uniform("speed".to_string()),
        &ctx,
        &params,
    );
    assert!(
        (value - 9.0).abs() < 1e-6,
        "the material parameter must win over the global, got {value}",
    );
}

/// An undeclared name still yields the old default rather than reading
/// whatever happens to be in the store under a different key.
#[test]
fn unknown_uniform_still_defaults() {
    gdvariant::shader_globals::clear();
    let ctx = gdserver3d::shader::FragmentContext3D::default();
    let params = std::collections::HashMap::new();

    let value = gdserver3d::shader::eval_float_expr_3d(
        &gdserver3d::shader::FloatExpr3D::Uniform("nonexistent".to_string()),
        &ctx,
        &params,
    );
    assert_eq!(value, 0.0, "an unknown uniform keeps its 0.0 fallback");
}
