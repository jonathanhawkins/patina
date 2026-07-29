//! pat-0q2kx.167 — a `@tool` script can actually reach `EditorPaths`.
//!
//! Godot exposes the editor's data/config/cache directories through
//! `EditorInterface.get_editor_paths()`, and a plugin uses them to decide where
//! to keep its own state (`upstream/godot/editor/editor_paths.cpp`,
//! `doc/classes/EditorPaths.xml`).
//!
//! ⚠️ **ClassDB registration does not make a class callable.** An earlier pass
//! of this bead registered `EditorPaths` in ClassDB with its documented methods
//! and signals, and a script naming `EditorPaths` still failed with
//! `UndefinedFunction` — registration describes a surface, but nothing routed a
//! call to it. These tests cover the routing, which is precisely the half
//! registration cannot give you.
//!
//! ## Why this is an integration test and not a `#[cfg(test)]` module
//!
//! `cargo nextest run -p gdscript-interop --lib` cannot pass: `fuzz_gdscript`'s
//! `fuzz_deeply_nested_parens` overflows the stack and cancels the whole run,
//! so any unit test living beside the interpreter is unverifiable. Several
//! sibling files in this directory carry the same note.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Every `EditorPaths` directory getter resolves and answers with a string.
#[test]
fn editor_paths_singleton_is_script_reachable() {
    for method in [
        "get_data_dir",
        "get_config_dir",
        "get_cache_dir",
        "get_project_settings_dir",
    ] {
        match run_val(&format!("return EditorPaths.{method}()\n")) {
            Variant::String(_) => {}
            other => panic!("EditorPaths.{method}() must return a String, got {other:?}"),
        }
    }
    assert_eq!(
        run_val("return EditorPaths.is_self_contained()\n"),
        Variant::Bool(false),
        "Patina has no self-contained editor mode; Godot answers false rather than erroring"
    );
}

/// Config and cache must name the same directories `OS` does — both answer from
/// `gdplatform::os`, and a plugin that writes through one and reads through the
/// other would otherwise silently lose its state.
///
/// ⚠️ `get_data_dir` is deliberately EXCLUDED, because it is the one that
/// legitimately differs: `OS.get_data_dir()` is the *user* data dir (the
/// per-project `user://` location) while `EditorPaths.get_data_dir()` is the
/// *editor's* own data dir. Godot draws the same distinction, so asserting
/// these two match would pin a bug rather than a contract.
#[test]
fn editor_paths_agree_with_the_os_singleton_on_shared_dirs() {
    for method in ["get_cache_dir", "get_config_dir"] {
        assert_eq!(
            run_val(&format!("return EditorPaths.{method}()\n")),
            run_val(&format!("return OS.{method}()\n")),
            "EditorPaths.{method}() and OS.{method}() must name the same directory"
        );
    }
}

/// An unknown method must still raise rather than quietly answering — a
/// catch-all that swallowed typos would make a misspelled call look like an
/// empty directory.
#[test]
fn editor_paths_rejects_an_unknown_method() {
    let mut interp = Interpreter::new();
    let err = interp
        .run("return EditorPaths.get_nonexistent_dir()\n")
        .expect_err("an unknown EditorPaths method must raise");
    assert!(
        format!("{err:?}").contains("EditorPaths"),
        "the error must name the class so the typo is findable, got {err:?}"
    );
}
