//! pat-6pyk: Validate contributor onboarding documentation structure.
//!
//! Coverage:
//!  1. Onboarding doc exists at expected path
//!  2. Required sections are present (Prerequisites, Repository Layout, Runtime, Oracle)
//!  3. Referenced crate names match actual crates in engine-rs/crates/
//!  4. Referenced Makefile targets match actual Makefile
//!  5. Referenced oracle scripts exist on disk
//!  6. Test tier table is internally consistent
//!  7. Fixture directory references are valid

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs should be inside repo root")
        .to_path_buf()
}

#[test]
fn onboarding_doc_exists() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    assert!(doc.exists(), "docs/contributor-onboarding.md must exist");
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(
        content.len() > 500,
        "onboarding doc should be substantive (got {} bytes)",
        content.len()
    );
}

#[test]
fn required_sections_present() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();

    let required_sections = [
        "Prerequisites",
        "Repository Layout",
        "Runtime Workflow",
        "Oracle Workflow",
        "Building the Engine",
        "Running Tests",
        "Writing Tests",
        "Engine Crate Structure",
        "Key Concepts",
        "Refreshing Oracle Artifacts",
        "Writing Parity Tests",
    ];

    for section in &required_sections {
        assert!(
            content.contains(section),
            "onboarding doc must contain section: {}",
            section
        );
    }
}

#[test]
fn referenced_crates_exist() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();
    let crates_dir = repo_root().join("engine-rs/crates");

    // Crates mentioned in the Engine Crate Structure table
    let expected_crates = [
        "gdcore",
        "gdvariant",
        "gdobject",
        "gdresource",
        "gdscene",
        "gdphysics2d",
        "gdrender2d",
        "gdserver2d",
        "gdplatform",
        "gdscript-interop",
        "gdaudio",
        "gdeditor",
    ];

    for crate_name in &expected_crates {
        assert!(
            content.contains(crate_name),
            "onboarding doc should reference crate: {}",
            crate_name
        );
        // patina-runner is a binary, not under crates/
        if *crate_name != "patina-runner" {
            assert!(
                crates_dir.join(crate_name).exists(),
                "referenced crate directory should exist: engine-rs/crates/{}",
                crate_name
            );
        }
    }
}

#[test]
fn makefile_targets_referenced() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();

    // The doc should mention the key Makefile targets
    let targets = ["test-fast", "test-golden", "make test"];
    for target in &targets {
        assert!(
            content.contains(target),
            "onboarding doc should reference Makefile target: {}",
            target
        );
    }
}

#[test]
fn oracle_scripts_referenced_and_exist() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();
    let root = repo_root();

    // Scripts referenced in the doc
    let scripts = [
        ("scripts/refresh_api.sh", "refresh_api"),
        ("apps/godot/extract_probes.sh", "extract_probes"),
    ];

    for (path, keyword) in &scripts {
        assert!(
            content.contains(keyword),
            "onboarding doc should reference {}",
            keyword
        );
        assert!(
            root.join(path).exists(),
            "referenced script should exist: {}",
            path
        );
    }
}

#[test]
fn fixture_directories_exist() {
    let root = repo_root();

    let dirs = [
        "fixtures/oracle_outputs",
        "fixtures/golden",
        "fixtures/scenes",
    ];

    for dir in &dirs {
        assert!(
            root.join(dir).exists(),
            "fixture directory should exist: {}",
            dir
        );
    }
}

#[test]
fn architecture_walkthrough_present() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();

    // The architecture walkthrough section must exist with key subsections
    let required = [
        "Architecture Walkthrough",
        "Dependency Graph",
        "Runtime Data Flow",
        "Scene Loading Pipeline",
        "Editor Architecture",
        "Key Design Patterns",
    ];
    for keyword in &required {
        assert!(
            content.contains(keyword),
            "onboarding doc must contain architecture subsection: {}",
            keyword
        );
    }
}

#[test]
fn test_tier_table_consistent() {
    let doc = repo_root().join("docs/contributor-onboarding.md");
    let content = std::fs::read_to_string(&doc).unwrap();

    // Verify the three tiers are documented
    assert!(content.contains("Tier 1") || content.contains("| 1 |"), "Tier 1 must be documented");
    assert!(content.contains("Tier 2") || content.contains("| 2 |"), "Tier 2 must be documented");
    assert!(content.contains("Tier 3") || content.contains("| 3 |"), "Tier 3 must be documented");

    // Verify Makefile has matching targets
    let makefile = std::fs::read_to_string(repo_root().join("engine-rs/Makefile")).unwrap();
    assert!(makefile.contains("test-fast"), "Makefile must have test-fast target");
    assert!(makefile.contains("test-golden"), "Makefile must have test-golden target");
}
