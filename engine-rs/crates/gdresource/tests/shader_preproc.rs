//! pat-0q2kx.222 — the shader preprocessor pass.
//!
//! Godot ground truth: `servers/rendering/shader_preprocessor.cpp`.
//!
//! Before this, Patina had no preprocessor: `#include` was reported as
//! unsupported and every other directive was skipped as a junk line. The
//! silent part is what mattered — an `#ifdef`-guarded shader compiled with
//! BOTH branches spliced together, which is a wrong render rather than an
//! error.
//!
//! ⚠️ SCOPE: this is the pass itself plus `ShaderIncludeDB` registration. It is
//! NOT yet called from the three compile entry points (gdserver3d shader.rs:416,
//! gdserver2d shader.rs:269, gdeditor shader_editor.rs:492), so the acceptance
//! test `shader_include_preprocessor` — which renders a tinted scene shader —
//! does not exist. See the completion report.

use gdresource::shader_preprocessor::preprocess;

/// Resolver over a fixed set of includes.
fn resolver(
    files: Vec<(&'static str, &'static str)>,
) -> impl FnMut(&str) -> Result<String, String> {
    move |path: &str| {
        files
            .iter()
            .find(|(n, _)| *n == path)
            .map(|(_, body)| body.to_string())
            .ok_or_else(|| "not found".to_string())
    }
}

fn run(src: &str, files: Vec<(&'static str, &'static str)>) -> String {
    let mut r = resolver(files);
    preprocess(src, "main.gdshader", &mut r)
        .unwrap_or_else(|e| panic!("preprocess failed: {e}"))
        .code
}

/// `#include` splices the file in, so a function defined there is usable.
#[test]
fn include_splices_the_referenced_file() {
    let out = run(
        "#include \"common.gdshaderinc\"\nvoid fragment() { COLOR = tint(); }",
        vec![("common.gdshaderinc", "vec4 tint() { return vec4(1.0); }")],
    );
    assert!(
        out.contains("vec4 tint()"),
        "the include body must appear in the output, got:\n{out}"
    );
    assert!(
        out.contains("COLOR = tint();"),
        "the including file survives"
    );
    assert!(
        !out.contains("#include"),
        "the directive itself must be consumed, not passed through"
    );
}

/// `#ifdef` actually excludes the dead branch. This is the case that was
/// silently wrong before: both arms used to reach the compiler.
#[test]
fn ifdef_excludes_the_untaken_branch() {
    let src = "#ifdef FANCY\nfancy();\n#else\nplain();\n#endif";

    let off = run(src, vec![]);
    assert!(
        off.contains("plain();"),
        "undefined -> #else branch, got:\n{off}"
    );
    assert!(
        !off.contains("fancy();"),
        "the #ifdef branch must be EXCLUDED, not merely appended, got:\n{off}"
    );

    let on = run(&format!("#define FANCY\n{src}"), vec![]);
    assert!(on.contains("fancy();"), "defined -> #ifdef branch");
    assert!(!on.contains("plain();"), "and the #else branch is dropped");
}

/// `#ifndef` is the inverse, and nesting resolves independently.
#[test]
fn ifndef_and_nesting_resolve_correctly() {
    let out = run(
        "#define A\n#ifdef A\nouter();\n#ifndef B\ninner();\n#endif\n#ifdef B\nskipped();\n#endif\n#endif",
        vec![],
    );
    assert!(out.contains("outer();"));
    assert!(
        out.contains("inner();"),
        "#ifndef B is true when B is undefined"
    );
    assert!(
        !out.contains("skipped();"),
        "a nested #ifdef on an undefined name stays excluded, got:\n{out}"
    );

    // A nested conditional inside a DEAD outer branch must stay dead even when
    // its own condition is true — the bug a flat bool instead of a stack gives.
    let dead = run(
        "#ifdef NOPE\n#define X\n#ifndef Q\nleaked();\n#endif\n#endif",
        vec![],
    );
    assert!(
        !dead.contains("leaked();"),
        "a true inner condition inside a dead outer branch must stay dead, got:\n{dead}"
    );
}

/// A `#define` inside a dead branch must not take effect.
#[test]
fn defines_in_dead_branches_do_not_apply() {
    let out = run(
        "#ifdef NEVER\n#define SIZE 99\n#endif\n#ifdef SIZE\nbad();\n#endif\nfloat s = SIZE;",
        vec![],
    );
    assert!(!out.contains("bad();"), "SIZE must never have been defined");
    assert!(
        out.contains("float s = SIZE;"),
        "an undefined identifier is left alone, got:\n{out}"
    );
}

/// Macro substitution replaces whole identifiers only.
#[test]
fn macros_substitute_whole_identifiers_only() {
    let out = run(
        "#define A 1\nfloat x = A;\nfloat y = MAX_A;\nfloat z = ALBEDO;",
        vec![],
    );
    assert!(
        out.contains("float x = 1;"),
        "the macro expands, got:\n{out}"
    );
    assert!(
        out.contains("float y = MAX_A;"),
        "a substring match must NOT corrupt MAX_A — a plain string replace would \
         make this `MAX_1`, got:\n{out}"
    );
    assert!(out.contains("float z = ALBEDO;"), "nor ALBEDO, got:\n{out}");

    // #undef stops expansion again.
    let un = run("#define A 1\n#undef A\nfloat x = A;", vec![]);
    assert!(un.contains("float x = A;"), "after #undef, got:\n{un}");
}

/// A macro whose body names another macro resolves, without hanging on a
/// self-referential pair.
#[test]
fn macro_expansion_is_recursive_but_bounded() {
    let out = run(
        "#define INNER 2\n#define OUTER INNER\nfloat x = OUTER;",
        vec![],
    );
    assert!(out.contains("float x = 2;"), "nested macro, got:\n{out}");

    // Mutually recursive defines must terminate rather than hang the compile.
    let cyclic = run("#define P Q\n#define Q P\nfloat x = P;", vec![]);
    assert!(
        cyclic.contains("float x ="),
        "a self-referential macro pair must terminate, got:\n{cyclic}"
    );
}

/// A cyclic include is a located error, not a hang or a stack overflow.
#[test]
fn cyclic_include_is_a_located_error() {
    let mut r = resolver(vec![
        ("a.gdshaderinc", "#include \"b.gdshaderinc\""),
        ("b.gdshaderinc", "#include \"a.gdshaderinc\""),
    ]);
    let err = preprocess("#include \"a.gdshaderinc\"", "main.gdshader", &mut r)
        .expect_err("a cycle must be rejected");

    assert!(
        err.message.contains("cyclic"),
        "the message must name the cycle, got: {err}"
    );
    assert!(
        err.to_string().contains("a.gdshaderinc"),
        "and identify a file in it, got: {err}"
    );
}

/// A missing include reports the line of the `#include`, not of the missing file.
#[test]
fn a_missing_include_is_reported_at_its_use_site() {
    let mut r = resolver(vec![]);
    let err = preprocess(
        "void fragment() {}\n#include \"gone.gdshaderinc\"",
        "main.gdshader",
        &mut r,
    )
    .expect_err("missing include must error");

    assert_eq!(err.file, "main.gdshader");
    assert_eq!(err.line, 2, "the error belongs to the #include line");
    assert!(err.message.contains("gone.gdshaderinc"));
}

/// An unterminated conditional is an error rather than silently swallowing the
/// rest of the shader.
#[test]
fn unterminated_ifdef_errors() {
    let mut r = resolver(vec![]);
    let err = preprocess("#ifdef A\nx();", "main.gdshader", &mut r)
        .expect_err("missing #endif must error");
    assert!(err.message.contains("#endif"), "got: {err}");

    let mut r2 = resolver(vec![]);
    assert!(
        preprocess("#endif", "main.gdshader", &mut r2).is_err(),
        "a stray #endif must error too"
    );
}

/// The line map points at the ORIGINAL file and line, which is the whole reason
/// to build one — after splicing, output positions are meaningless to an author.
#[test]
fn the_line_map_survives_an_include() {
    let mut r = resolver(vec![("inc.gdshaderinc", "// a\n// b")]);
    let out = preprocess(
        "// root0\n#include \"inc.gdshaderinc\"\n// root2",
        "main.gdshader",
        &mut r,
    )
    .expect("preprocesses");

    assert_eq!(out.line_map.len(), out.code.lines().count());
    assert_eq!(out.origin(1), Some(("main.gdshader", 1)));
    // Lines 2-3 come from the include, at ITS line numbers.
    assert_eq!(out.origin(2), Some(("inc.gdshaderinc", 1)));
    assert_eq!(out.origin(3), Some(("inc.gdshaderinc", 2)));
    // The line after the include is line 3 of the root, not line 4 of output.
    assert_eq!(
        out.origin(4),
        Some(("main.gdshader", 3)),
        "an author reading a diagnostic must be sent to their own line 3"
    );
}

/// Unknown directives pass through, so `#version` and `#pragma` still reach the
/// compiler. Erroring on them would break shaders that work today.
#[test]
fn unknown_directives_pass_through() {
    let out = run(
        "#version 450\n#pragma disable_preprocessor\nvoid fragment() {}",
        vec![],
    );
    assert!(out.contains("#version 450"));
    assert!(out.contains("#pragma disable_preprocessor"));
}
