//! pat-0q2kx.225 (slice 5b) — `[shader_globals]` reaches the runtime store at boot.
//!
//! This is the link that closes the feature. Parsing existed
//! (`GodotProjectFile::shader_globals`) and the store existed
//! (`gdvariant::shader_globals`), but **nothing called one with the other**, so
//! a project declaring `shader_globals/tint` booted with an empty store and
//! every `global uniform vec4 tint;` rendered as if undeclared.
//!
//! Exactly the shape of the `input_map` gap this file's neighbour fixed: boot
//! had always *parsed* the action map, and nothing ever handed it to the loop.
//!
//! Binary named `globals_boot` rather than something descriptive: `patina-runner`
//! leaves only 16 characters before the verifier's 58-char command truncation,
//! past which the target silently resolves to nothing and runs zero tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_shader_globals_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a minimal bootable project whose `project.godot` carries `body`.
fn project_with(tag: &str, body: &str) -> PathBuf {
    let dir = unique_project_dir(tag);
    std::fs::write(
        dir.join("project.godot"),
        format!("config_version=5\n\n[application]\n\nconfig/name=\"ShaderGlobals\"\n\n{body}\n"),
    )
    .unwrap();
    dir
}

/// **The boot link.** A declared global is in the store once the project boots.
#[test]
fn shader_globals_are_seeded_at_boot() {
    gdvariant::shader_globals::clear();

    let dir = project_with(
        "seed",
        "[shader_globals]\n\ntint={\n\"type\": \"color\",\n\"value\": Color(1, 0, 0, 1)\n}\nwind={\n\"type\": \"float\",\n\"value\": 2.5\n}",
    );
    boot_project(&dir).expect("project boots");

    assert_eq!(
        gdvariant::shader_globals::get_list(),
        vec!["tint".to_string(), "wind".to_string()],
        "both declared globals must reach the store",
    );
    assert_eq!(
        gdvariant::shader_globals::get_type("tint").as_deref(),
        Some("color"),
        "the declared type must survive so it can bind to a uniform slot",
    );
    assert_eq!(
        gdvariant::shader_globals::get("wind"),
        Variant::Float(2.5),
        "the value must be parsed from its GDScript literal, not left as text",
    );
}

/// A `Color(...)` literal parses to a real Color rather than a string — the
/// inner commas must not split the value, and a shader reading it needs the
/// typed variant.
#[test]
fn color_literals_are_parsed_to_variants() {
    gdvariant::shader_globals::clear();

    let dir = project_with(
        "color",
        "[shader_globals]\n\ntint={\n\"type\": \"color\",\n\"value\": Color(0.25, 0.5, 0.75, 1)\n}",
    );
    boot_project(&dir).expect("project boots");

    match gdvariant::shader_globals::get("tint") {
        Variant::Color(c) => {
            assert!((c.r - 0.25).abs() < 1e-6, "r = {}", c.r);
            assert!((c.g - 0.5).abs() < 1e-6, "g = {}", c.g);
            assert!((c.b - 0.75).abs() < 1e-6, "b = {}", c.b);
        }
        other => panic!("expected a Color variant, got {other:?}"),
    }
}

/// A project with no `[shader_globals]` boots normally and leaves the store
/// empty — most projects declare none.
#[test]
fn absent_section_boots_with_an_empty_store() {
    gdvariant::shader_globals::clear();

    let dir = project_with(
        "none",
        "[rendering]\n\nrenderer/rendering_method=\"gl_compatibility\"",
    );
    boot_project(&dir).expect("project boots");

    assert!(gdvariant::shader_globals::get_list().is_empty());
}

/// An unparseable value is skipped, not defaulted — and crucially the boot
/// still succeeds, so one bad line cannot stop the game from starting.
#[test]
fn unparseable_value_is_skipped_without_failing_boot() {
    gdvariant::shader_globals::clear();

    let dir = project_with(
        "bad",
        "[shader_globals]\n\nbroken={\n\"type\": \"float\",\n\"value\": ????\n}\ngood={\n\"type\": \"float\",\n\"value\": 1.5\n}",
    );
    boot_project(&dir).expect("boot must survive a malformed shader global");

    assert_eq!(
        gdvariant::shader_globals::get("good"),
        Variant::Float(1.5),
        "the well-formed entry still lands",
    );
    assert!(
        !gdvariant::shader_globals::contains("broken")
            || gdvariant::shader_globals::get("broken") == Variant::Nil,
        "a broken entry must not be seeded with a guessed value",
    );
}
