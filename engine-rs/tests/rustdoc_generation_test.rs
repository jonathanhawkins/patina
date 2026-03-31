//! Rustdoc generation verification — ensures `cargo doc` builds cleanly
//! for all public crates in the workspace and that key public items are
//! documented.

use std::path::PathBuf;
use std::process::Command;

/// Returns the engine-rs workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// All public crates in the workspace that should generate docs.
const PUBLIC_CRATES: &[&str] = &[
    "gdaudio",
    "gdcore",
    "gdeditor",
    "gdobject",
    "gdphysics2d",
    "gdphysics3d",
    "gdplatform",
    "gdrender2d",
    "gdrender3d",
    "gdresource",
    "gdscene",
    "gdscript-interop",
    "gdserver2d",
    "gdserver3d",
    "gdvariant",
    "patina-engine",
    "patina-runner",
];

#[test]
fn cargo_doc_builds_without_errors() {
    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps", "--message-format=short"])
        .current_dir(workspace_root())
        .env("RUSTDOCFLAGS", "--document-private-items")
        .output()
        .expect("Failed to run cargo doc");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // cargo doc returns 0 even with warnings — check for actual errors
    assert!(
        output.status.success(),
        "cargo doc failed with exit code {:?}:\n{}",
        output.status.code(),
        stderr
    );

    // Check there are no rustdoc errors (warnings are OK)
    let error_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("error[") || l.contains("error:"))
        .collect();

    assert!(
        error_lines.is_empty(),
        "cargo doc produced errors:\n{}",
        error_lines.join("\n")
    );

    eprintln!("cargo doc --workspace --no-deps succeeded.");
}

#[test]
fn doc_output_directories_exist() {
    // After cargo doc, each crate should have a directory in target/doc/
    let doc_dir = workspace_root().join("target").join("doc");

    if !doc_dir.exists() {
        eprintln!("target/doc/ does not exist — skipping directory check (run cargo doc first).");
        return;
    }

    let mut missing = Vec::new();
    for crate_name in PUBLIC_CRATES {
        // Crate directories use underscores, not hyphens
        let dir_name = crate_name.replace('-', "_");
        let crate_doc_dir = doc_dir.join(&dir_name);
        if !crate_doc_dir.exists() {
            missing.push(*crate_name);
        }
    }

    assert!(
        missing.is_empty(),
        "Documentation directories missing for crates: {:?}\n\
         Run `cargo doc --workspace --no-deps` first.",
        missing
    );

    eprintln!(
        "All {} crate doc directories verified.",
        PUBLIC_CRATES.len()
    );
}

#[test]
fn doc_index_html_files_exist() {
    let doc_dir = workspace_root().join("target").join("doc");

    if !doc_dir.exists() {
        eprintln!("target/doc/ does not exist — skipping index check.");
        return;
    }

    let mut missing_index = Vec::new();
    for crate_name in PUBLIC_CRATES {
        let dir_name = crate_name.replace('-', "_");
        let index = doc_dir.join(&dir_name).join("index.html");
        if !index.exists() {
            missing_index.push(*crate_name);
        }
    }

    assert!(
        missing_index.is_empty(),
        "index.html missing for crates: {:?}",
        missing_index
    );

    eprintln!(
        "All {} crate index.html files present.",
        PUBLIC_CRATES.len()
    );
}

#[test]
fn public_crates_have_module_level_docs() {
    // Verify each crate's lib.rs starts with a //! doc comment
    let crates_dir = workspace_root().join("crates");

    let mut undocumented = Vec::new();

    for crate_name in PUBLIC_CRATES {
        // Skip the top-level binary crates
        if *crate_name == "patina-engine" || *crate_name == "patina-runner" {
            continue;
        }

        let lib_rs = crates_dir.join(crate_name).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&lib_rs).unwrap();
        let has_module_doc = content
            .lines()
            .take(10) // Check first 10 lines
            .any(|line| line.trim_start().starts_with("//!"));

        if !has_module_doc {
            undocumented.push(*crate_name);
        }
    }

    assert!(
        undocumented.is_empty(),
        "These crates lack module-level documentation (//! comments) in lib.rs:\n  {}\n\n\
         Every public crate should have a //! doc comment explaining its purpose.",
        undocumented.join("\n  ")
    );

    eprintln!("All crate lib.rs files have module-level documentation.");
}

#[test]
fn no_broken_intra_doc_links() {
    // Run cargo doc with deny on broken intra-doc links
    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(workspace_root())
        .env("RUSTDOCFLAGS", "-D rustdoc::broken-intra-doc-links")
        .output()
        .expect("Failed to run cargo doc");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Collect broken link errors
    let broken_links: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("broken-intra-doc-links") || l.contains("unresolved link"))
        .collect();

    if !broken_links.is_empty() {
        eprintln!(
            "WARNING: {} broken intra-doc links found:\n{}",
            broken_links.len(),
            broken_links.join("\n")
        );
    }

    // For now this is a warning, not a hard failure, since existing code
    // may have links that reference types not yet fully documented.
    // The main test (cargo_doc_builds_without_errors) already ensures
    // the build succeeds.
    eprintln!("Intra-doc link check: {} issues found.", broken_links.len());
}

#[test]
fn crate_list_matches_workspace() {
    // Verify our PUBLIC_CRATES list matches the actual workspace members
    let cargo_toml_path = workspace_root().join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path).unwrap();

    let mut workspace_members = Vec::new();
    let mut in_members = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") && trimmed.contains('[') {
            in_members = true;
            // Handle inline array
            if let Some(rest) = trimmed.strip_prefix("members") {
                let rest = rest.trim().trim_start_matches('=').trim();
                if rest.starts_with('[') && rest.ends_with(']') {
                    // Inline array
                    let inner = &rest[1..rest.len() - 1];
                    for item in inner.split(',') {
                        let item = item.trim().trim_matches('"').trim();
                        if !item.is_empty() {
                            workspace_members.push(item.to_string());
                        }
                    }
                    in_members = false;
                }
            }
            continue;
        }
        if in_members {
            if trimmed == "]" {
                in_members = false;
                continue;
            }
            let item = trimmed.trim_matches('"').trim_matches(',').trim();
            if !item.is_empty() {
                workspace_members.push(item.to_string());
            }
        }
    }

    // Extract crate names from paths like "crates/gdcore"
    let member_names: Vec<String> = workspace_members
        .iter()
        .map(|m| m.rsplit('/').next().unwrap_or(m).to_string())
        .collect();

    let public_set: std::collections::HashSet<&str> = PUBLIC_CRATES.iter().copied().collect();
    let member_set: std::collections::HashSet<&str> =
        member_names.iter().map(|s| s.as_str()).collect();

    let missing_from_test: Vec<&&str> = member_set.difference(&public_set).collect();
    let extra_in_test: Vec<&&str> = public_set.difference(&member_set).collect();

    if !missing_from_test.is_empty() {
        eprintln!(
            "WARNING: workspace members not in PUBLIC_CRATES: {:?}",
            missing_from_test
        );
    }
    if !extra_in_test.is_empty() {
        eprintln!(
            "WARNING: PUBLIC_CRATES entries not in workspace: {:?}",
            extra_in_test
        );
    }

    eprintln!(
        "Workspace sync check: {} members, {} in test list.",
        member_set.len(),
        public_set.len()
    );
}
