//! pat-0q2kx.225 (slice 1) — `[shader_globals]` parsing from `project.godot`.
//!
//! Godot's global uniforms are project-wide shader parameters: a shader
//! declaring `global uniform vec4 tint;` binds to whatever `[shader_globals]`
//! holds, `RenderingServer.global_shader_parameter_set` mutates it at runtime,
//! and every shader referencing it updates together.
//!
//! Patina parsed none of it — `grep shader_globals` across `gdresource` was
//! zero — so the very first link in that chain was missing: the project file's
//! declarations never reached the engine at all.
//!
//! ⚠️ **This is one slice of the bead, not the whole feature.** The remaining
//! parts (parsing `global uniform` in the shader front-end, the runtime store,
//! and the `RenderingServer` script surface) live in `gdserver3d/src/shader.rs`,
//! which is both mid-edit by another agent and in a crate that does not
//! currently compile. See the bead notes.

use gdresource::GodotProjectFile;

/// The layout Godot actually writes: a multi-line dict per global, values as
/// GDScript literals rather than JSON, and no trailing comma on the last field.
const PROJECT: &str = r#"
[application]

config/name="Demo"

[shader_globals]

tint={
"type": "color",
"value": Color(1, 0, 0, 1)
}
wind_strength={
"type": "float",
"value": 2.5
}
"#;

#[test]
fn shader_globals_are_parsed_from_project_godot() {
    let pf = GodotProjectFile::parse(PROJECT).expect("project parses");
    let globals = pf.shader_globals();

    assert_eq!(
        globals.len(),
        2,
        "both globals must be found, got {globals:?}"
    );

    // Sorted by name, so the order is stable rather than HashMap-arbitrary.
    assert_eq!(globals[0].name, "tint");
    assert_eq!(globals[0].type_name, "color");
    assert_eq!(
        globals[0].value_literal, "Color(1, 0, 0, 1)",
        "the value literal must survive whole — commas inside Color(..) must \
         not split the field",
    );

    assert_eq!(globals[1].name, "wind_strength");
    assert_eq!(globals[1].type_name, "float");
    assert_eq!(globals[1].value_literal, "2.5");
}

/// A project with no such section yields nothing rather than failing to load —
/// most projects declare no globals at all.
#[test]
fn absent_section_yields_no_globals() {
    let pf =
        GodotProjectFile::parse("[application]\n\nconfig/name=\"Bare\"\n").expect("project parses");
    assert!(pf.shader_globals().is_empty());
}

/// A malformed entry is skipped, not guessed at. A global with no declared type
/// cannot be bound to a uniform slot, and inventing one would surface as a
/// failure far from the `project.godot` line that caused it.
#[test]
fn entries_missing_a_field_are_skipped() {
    let pf = GodotProjectFile::parse(
        "[shader_globals]\n\nbroken={\n\"value\": 1.0\n}\ngood={\n\"type\": \"float\",\n\"value\": 3.0\n}\n",
    )
    .expect("project parses");

    let globals = pf.shader_globals();
    assert_eq!(globals.len(), 1, "only the well-formed entry survives");
    assert_eq!(globals[0].name, "good");
    assert_eq!(globals[0].value_literal, "3.0");
}

/// Vector literals keep their inner commas too — the bracket tracking has to
/// hold for every literal shape Godot writes, not just Color.
#[test]
fn vector_literals_survive_intact() {
    let pf = GodotProjectFile::parse(
        "[shader_globals]\n\nwind_dir={\n\"type\": \"vec3\",\n\"value\": Vector3(0, 1, 0)\n}\n",
    )
    .expect("project parses");

    let globals = pf.shader_globals();
    assert_eq!(globals.len(), 1);
    assert_eq!(globals[0].type_name, "vec3");
    assert_eq!(globals[0].value_literal, "Vector3(0, 1, 0)");
}
