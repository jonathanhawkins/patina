//! pat-0q2kx.144 (partial) — a loaded `.gdshader` carries its real source, and
//! `get_shader_uniform_list()` reports the declared uniforms with types/hints.
//!
//! ⚠️ This does NOT satisfy the whole acceptance (`shader_script_api_roundtrip`).
//! The clause it does not cover is live parameter propagation —
//! `m.set_shader_parameter("tint", Color.RED)` then a frame tick rendering
//! red-tinted pixels — which needs the render path, not the script API. See the
//! completion report.
//!
//! Before this, `load("res://f.gdshader")` returned a `ResourceRef` with an
//! EMPTY property bag: the file was never read, so `.code` was `Nil`, and
//! `get_shader_uniform_list` did not exist anywhere in the workspace (`rg` = 0
//! hits). A script could name a shader and learn nothing about it.
//!
//! An integration test, not a `--lib` unit test: `-p gdscript-interop --lib`
//! cannot pass at HEAD (a pre-existing `fuzz_deeply_nested_parens` stack
//! overflow cancels the run), so a unit test here would be unverifiable.

use std::sync::atomic::{AtomicUsize, Ordering};

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SHADER: &str = r#"shader_type canvas_item;

uniform vec4 tint : source_color = vec4(1.0, 0.0, 0.0, 1.0);
uniform float speed = 2.5;
uniform sampler2D noise_tex;
"#;

/// A unique temp dir, so concurrent copies of this binary cannot collide.
fn temp_project(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx144_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// `load("…​.gdshader").code` is the file's real source.
#[test]
fn loading_a_gdshader_exposes_its_source_as_code() {
    let dir = temp_project("code");
    let path = dir.join("f.gdshader");
    std::fs::write(&path, SHADER).unwrap();
    let path_str = path.to_string_lossy().replace('\\', "/");

    let code = run_val(&format!("var s = load(\"{path_str}\")\nreturn s.code\n"));
    match code {
        Variant::String(c) => {
            assert!(
                c.contains("uniform vec4 tint"),
                "`.code` must be the file's source, got:\n{c}"
            );
            assert_eq!(c, SHADER, "`.code` must be the source verbatim");
        }
        other => panic!("`.code` should be a String, got {other:?}"),
    }

    // The resource still identifies as a Shader.
    assert_eq!(
        run_val(&format!(
            "var s = load(\"{path_str}\")\nreturn s.get_class()\n"
        )),
        Variant::String("Shader".to_string())
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `get_shader_uniform_list()` reports every declared uniform with its type and
/// hint — the metadata an inspector or a script needs to drive the shader.
#[test]
fn get_shader_uniform_list_reports_types_and_hints() {
    let dir = temp_project("uniforms");
    let path = dir.join("f.gdshader");
    std::fs::write(&path, SHADER).unwrap();
    let path_str = path.to_string_lossy().replace('\\', "/");

    let list = run_val(&format!(
        "var s = load(\"{path_str}\")\nreturn s.get_shader_uniform_list()\n"
    ));
    let entries = match list {
        Variant::Array(a) => a,
        other => panic!("get_shader_uniform_list() should return an Array, got {other:?}"),
    };

    let field = |i: usize, key: &str| -> String {
        match &entries.to_vec()[i] {
            Variant::Dictionary(d) => match d.get(key.to_string()) {
                Some(Variant::String(s)) => s.clone(),
                other => panic!("entry {i} field {key} should be a String, got {other:?}"),
            },
            other => panic!("entry {i} should be a Dictionary, got {other:?}"),
        }
    };

    assert_eq!(
        entries.len(),
        3,
        "the shader declares three uniforms; got {} entries",
        entries.len()
    );

    // Declaration order is preserved, as Godot reports it.
    assert_eq!(field(0, "name"), "tint");
    assert_eq!(field(1, "name"), "speed");
    assert_eq!(field(2, "name"), "noise_tex");

    // The type keyword, and the hint that distinguishes a colour from a plain
    // vec4 — a `vec4` with no hint is NOT a colour.
    assert_eq!(field(0, "type"), "vec4");
    assert_eq!(
        field(0, "hint"),
        "source_color",
        "the `: source_color` hint must survive"
    );
    assert_eq!(field(1, "type"), "float");
    assert_eq!(field(1, "hint"), "", "a uniform with no hint reports empty");
    assert_eq!(field(2, "type"), "sampler2D");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A shader whose file cannot be read reports no uniforms rather than pretending
/// the shader declares none — the two are different, and conflating them would
/// make a broken path look like a valid empty shader.
#[test]
fn an_unreadable_shader_has_no_code_and_no_uniforms() {
    let missing = "res://definitely/not/here.gdshader";
    assert_eq!(
        run_val(&format!("var s = load(\"{missing}\")\nreturn s.code\n")),
        Variant::Nil,
        "an unreadable shader must leave `code` unset, not empty-string"
    );
    match run_val(&format!(
        "var s = load(\"{missing}\")\nreturn s.get_shader_uniform_list()\n"
    )) {
        Variant::Array(a) => assert!(a.is_empty(), "no code means no uniforms"),
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// Reading a property a resource does not carry answers Nil, matching Godot's
/// `Object::get`, rather than raising.
#[test]
fn an_unknown_resource_property_is_nil() {
    let dir = temp_project("unknown");
    let path = dir.join("f.gdshader");
    std::fs::write(&path, SHADER).unwrap();
    let path_str = path.to_string_lossy().replace('\\', "/");

    assert_eq!(
        run_val(&format!(
            "var s = load(\"{path_str}\")\nreturn s.no_such_property\n"
        )),
        Variant::Nil
    );

    let _ = std::fs::remove_dir_all(&dir);
}
