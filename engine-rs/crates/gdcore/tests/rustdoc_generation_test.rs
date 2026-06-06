//! Verifies that `cargo doc` succeeds for all workspace crates.
//!
//! This test ensures API documentation can be generated without errors.
//! It runs `cargo doc --workspace --no-deps` and checks the exit code.
//! Broken intra-doc links produce warnings (not errors) so generation
//! still succeeds — a stricter lint pass can be added later.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    // gdcore/tests/ -> gdcore/ -> crates/ -> engine-rs/
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn cargo_doc_workspace_succeeds() {
    let root = workspace_root();

    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(&root)
        .output()
        .expect("Failed to run cargo doc");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "cargo doc --workspace --no-deps failed!\nstderr:\n{}",
        stderr
    );

    // Verify some expected doc output files exist
    let doc_dir = root.join("target/doc");
    assert!(
        doc_dir.exists(),
        "Doc output directory not found at {:?}",
        doc_dir
    );

    // Check that key crate docs were generated
    let expected_crates = [
        "gdcore",
        "gdscene",
        "gdplatform",
        "gdrender2d",
        "gdresource",
        "gdvariant",
        "gdobject",
        "gdphysics2d",
    ];

    let mut missing = Vec::new();
    for crate_name in &expected_crates {
        let index = doc_dir.join(crate_name).join("index.html");
        if !index.exists() {
            missing.push(*crate_name);
        }
    }

    assert!(
        missing.is_empty(),
        "Documentation missing for crates: {:?}\nExpected index.html files in {:?}",
        missing,
        doc_dir
    );

    eprintln!(
        "rustdoc generation PASSED: all {} crate docs generated at {:?}",
        expected_crates.len(),
        doc_dir
    );
}

#[test]
fn all_public_crates_have_crate_level_docs() {
    // Verify each public crate's lib.rs starts with a doc comment (//!)
    let crates_dir = workspace_root().join("crates");

    let expected_crates = [
        "gdcore",
        "gdscene",
        "gdplatform",
        "gdrender2d",
        "gdresource",
        "gdvariant",
        "gdobject",
        "gdphysics2d",
        "gdaudio",
        "gdscript-interop",
        "gdserver2d",
    ];

    let mut missing_docs = Vec::new();

    for crate_name in &expected_crates {
        let lib_rs = crates_dir.join(crate_name).join("src/lib.rs");
        if !lib_rs.exists() {
            missing_docs.push(format!("{}: lib.rs not found", crate_name));
            continue;
        }

        let content = std::fs::read_to_string(&lib_rs).unwrap();
        let has_crate_doc = content
            .lines()
            .any(|line| line.trim_start().starts_with("//!"));

        if !has_crate_doc {
            missing_docs.push(format!("{}: no //! crate-level doc comment", crate_name));
        }
    }

    if !missing_docs.is_empty() {
        panic!(
            "\n\nCrate-level documentation check FAILED:\n{}\n\n\
             Every public crate should have a //! doc comment at the top of lib.rs\n\
             explaining what the crate provides.\n",
            missing_docs.join("\n")
        );
    }

    eprintln!(
        "Crate docs check PASSED: all {} crates have crate-level documentation.",
        expected_crates.len()
    );
}

#[test]
fn doc_warnings_count_is_bounded() {
    // Advisory: count how many doc warnings exist. This doesn't fail but
    // reports the count so CI can track improvement over time.
    let root = workspace_root();

    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(&root)
        .output()
        .expect("Failed to run cargo doc");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let warning_count = stderr.lines().filter(|l| l.contains("warning:")).count();

    eprintln!(
        "\n=== Rustdoc Warning Report ===\nTotal warnings: {}\n\
         (Track this number — it should decrease over time)\n",
        warning_count
    );

    // Set a generous upper bound to catch regressions that add many new warnings
    assert!(
        warning_count < 200,
        "Too many rustdoc warnings ({warning_count}). This likely means new \
         broken doc links were introduced. Please fix them."
    );
}
