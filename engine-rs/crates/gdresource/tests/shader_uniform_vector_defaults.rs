//! pat-s3rrv.36 — a uniform's vector default must survive the parse.
//!
//! Both renderers carried a byte-identical `parse_default_value` that
//! understood only `true`/`false`/int/float. Every vector default therefore
//! became `Variant::Nil`, and a Nil uniform reads as zero — so the authored
//! colour silently rendered as transparent black.
//!
//! The repo's own fixture is the case (`fixtures/shaders/water.gdshader:4`):
//!
//! ```text
//! uniform vec4 water_color : source_color = vec4(0.1, 0.4, 0.6, 1.0);
//! ```
//!
//! ⚠️ `vec4` yields `Variant::Color`, not a would-be `Vector4`. That is the
//! representation the evaluators already agree on — `gdserver3d::shader` maps
//! `Variant::Color -> SV::Vec4` and reads a vec4 default as `Variant::Color` —
//! and `Variant` has no `Vector4` kind at all until pat-s3rrv.59 lands. Any
//! other choice would read back as Nil exactly like the bug this replaces.

use gdcore::math::{Color, Vector2, Vector3};
use gdresource::shader_tokenizer::parse_shader_default_value as parse;
use gdvariant::Variant;

/// **The acceptance test**: the fixture's own default parses to its colour.
#[test]
fn shader_uniform_vector_defaults() {
    assert_eq!(
        parse("vec4(0.1, 0.4, 0.6, 1.0)"),
        Variant::Color(Color::new(0.1, 0.4, 0.6, 1.0)),
        "water.gdshader's authored blue must reach the uniform. `Nil` here is \
         the bug: it reads as zero, i.e. transparent black",
    );
    assert_eq!(
        parse("vec3(1.0, 0.5, 0.25)"),
        Variant::Vector3(Vector3::new(1.0, 0.5, 0.25)),
    );
    assert_eq!(
        parse("vec2(3.0, 4.0)"),
        Variant::Vector2(Vector2::new(3.0, 4.0)),
    );
}

/// GLSL broadcasts a single scalar across every component (pat-s3rrv.36).
///
/// `vec3(1.0)` is opaque white, not `(1, 0, 0)`. Reading it as "first component
/// only" is the natural wrong guess and produces a colour that is plausible
/// enough to survive review.
#[test]
fn a_single_scalar_broadcasts_across_components() {
    assert_eq!(
        parse("vec4(1.0)"),
        Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)),
        "vec4(1.0) is opaque white",
    );
    assert_eq!(
        parse("vec3(0.5)"),
        Variant::Vector3(Vector3::new(0.5, 0.5, 0.5)),
    );
    assert_eq!(parse("vec2(2.0)"), Variant::Vector2(Vector2::new(2.0, 2.0)));
}

/// The scalar forms keep working, and ints stay ints (pat-s3rrv.36).
///
/// `parse::<f64>` accepts `"1"` too, so the int branch has to come first — an
/// `int` uniform whose default arrived as `Float(1.0)` compares unequal to
/// `Int(1)` everywhere downstream.
#[test]
fn scalar_defaults_keep_their_types() {
    assert_eq!(parse("true"), Variant::Bool(true));
    assert_eq!(parse("false"), Variant::Bool(false));
    assert_eq!(parse("7"), Variant::Int(7), "an int default stays an Int");
    assert_eq!(parse("-3"), Variant::Int(-3));
    assert_eq!(parse("0.25"), Variant::Float(0.25));
    assert_eq!(parse("  1.5  "), Variant::Float(1.5), "surrounding space");
}

/// A form this parser does NOT support stays `Nil` rather than guessing
/// (pat-s3rrv.36).
///
/// A wrong colour is invisible at runtime — it just looks a bit off, with
/// nothing pointing back at the parser — so an unsupported construct must
/// remain unmistakably absent instead of being approximated.
#[test]
fn unsupported_forms_stay_nil_instead_of_guessing() {
    for src in [
        // Nested constructors are real GLSL but not parsed here.
        "vec4(vec3(1.0), 0.5)",
        // A named constant is not a literal.
        "vec3(PI, 0.0, 0.0)",
        // Wrong component count for the constructor.
        "vec4(1.0, 2.0)",
        "vec2(1.0, 2.0, 3.0)",
        // Not a constructor at all.
        "mat4(1.0)",
        "texture(TEX, UV)",
        "",
    ] {
        assert_eq!(
            parse(src),
            Variant::Nil,
            "`{src}` must stay Nil rather than being approximated",
        );
    }
}

/// Whitespace inside the constructor is tolerated (pat-s3rrv.36).
///
/// Hand-authored shaders are not normalised, and a default that parsed only
/// when written without spaces would fail on roughly half of real source.
#[test]
fn constructor_whitespace_is_tolerated() {
    assert_eq!(
        parse("vec4( 0.0 , 1.0,0.0 ,  1.0 )"),
        Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
    );
    assert_eq!(
        parse("vec3 (1.0, 2.0, 3.0)"),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        "a space between the type and its parenthesis is legal GLSL",
    );
}
