//! pat-4m6k: Validate that compatibility docs correctly distinguish
//! historical 4.5.1 numbers from live 4.6.1 measurements.
//!
//! These tests ensure that:
//! 1. COMPAT_MATRIX.md references the live oracle pin (4.6.1)
//! 2. Historical 4.5.1 references are annotated as pre-repin
//! 3. The oracle pin in tools/oracle/common.py matches COMPAT_MATRIX.md
//! 4. No doc file presents 4.5.1 numbers as current without annotation
//!
//! Acceptance: historical version references are clearly distinguished
//! from live measurements across all compatibility documentation.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs inside repo root")
        .to_path_buf()
}

// ===========================================================================
// 1. COMPAT_MATRIX.md references 4.6.1 as the live oracle version
// ===========================================================================

#[test]
fn compat_matrix_references_live_version() {
    let path = repo_root().join("COMPAT_MATRIX.md");
    assert!(path.exists(), "COMPAT_MATRIX.md must exist");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        content.contains("4.6.1"),
        "COMPAT_MATRIX.md must reference Godot 4.6.1 (the live oracle pin)"
    );
    assert!(
        content.contains("measured against Godot 4.6.1")
            || content.contains("Measured against **Godot 4.6.1**"),
        "COMPAT_MATRIX.md must state measurements are against Godot 4.6.1"
    );
}

// ===========================================================================
// 2. Any 4.5.1 reference in COMPAT_MATRIX.md is annotated as historical
// ===========================================================================

#[test]
fn compat_matrix_451_annotated_as_historical() {
    let path = repo_root().join("COMPAT_MATRIX.md");
    let content = std::fs::read_to_string(&path).unwrap();

    for (i, line) in content.lines().enumerate() {
        if line.contains("4.5.1") {
            assert!(
                line.contains("historical")
                    || line.contains("Historical")
                    || line.contains("pre-repin")
                    || line.contains("no longer live"),
                "COMPAT_MATRIX.md line {}: references 4.5.1 without historical annotation: {line}",
                i + 1
            );
        }
    }
}

// ===========================================================================
// 3. tools/oracle/common.py pin matches COMPAT_MATRIX.md
// ===========================================================================

#[test]
fn oracle_pin_matches_compat_matrix() {
    let common_py = repo_root().join("tools/oracle/common.py");
    let compat = repo_root().join("COMPAT_MATRIX.md");

    let py_content = std::fs::read_to_string(&common_py).unwrap();
    let compat_content = std::fs::read_to_string(&compat).unwrap();

    // Extract version from common.py
    let mut pinned_version = String::new();
    for line in py_content.lines() {
        if line.contains("UPSTREAM_VERSION") && line.contains('"') {
            pinned_version = line.split('"').nth(1).unwrap_or("").to_string();
            break;
        }
    }
    assert!(
        !pinned_version.is_empty(),
        "Could not extract UPSTREAM_VERSION from common.py"
    );

    // The version (e.g., "4.6.1-stable") should appear in COMPAT_MATRIX.md
    let version_major_minor_patch = pinned_version.split('-').next().unwrap();
    assert!(
        compat_content.contains(version_major_minor_patch),
        "COMPAT_MATRIX.md must reference the pinned version {version_major_minor_patch} \
         from tools/oracle/common.py"
    );
}

// ===========================================================================
// 4. UPSTREAM_VERSION stamp matches common.py
// ===========================================================================

#[test]
fn upstream_version_stamp_matches_common_py() {
    let stamp = repo_root().join("fixtures/golden/UPSTREAM_VERSION");
    let common_py = repo_root().join("tools/oracle/common.py");

    let stamp_content = std::fs::read_to_string(&stamp).unwrap();
    let py_content = std::fs::read_to_string(&common_py).unwrap();

    let stamp_hash = stamp_content.trim();

    let mut pinned_commit = String::new();
    for line in py_content.lines() {
        if line.contains("UPSTREAM_COMMIT") && line.contains('"') {
            pinned_commit = line.split('"').nth(1).unwrap_or("").to_string();
            break;
        }
    }

    assert_eq!(
        stamp_hash, pinned_commit,
        "UPSTREAM_VERSION stamp ({stamp_hash}) must match \
         UPSTREAM_COMMIT in common.py ({pinned_commit})"
    );
}

// ===========================================================================
// 5. Historical 4.5.1 feature audit is clearly marked as frozen
// ===========================================================================

#[test]
fn feature_audit_451_marked_historical() {
    let path = repo_root().join("prd/GODOT_4_5_1_FEATURE_AUDIT.md");
    if !path.exists() {
        return; // File may have been removed — that's fine
    }
    let content = std::fs::read_to_string(&path).unwrap();

    // The file header must contain a historical/frozen notice
    let first_500_chars: String = content.chars().take(500).collect();
    assert!(
        first_500_chars.contains("HISTORICAL")
            || first_500_chars.contains("frozen")
            || first_500_chars.contains("pre-repin"),
        "GODOT_4_5_1_FEATURE_AUDIT.md must be clearly marked as historical/frozen \
         in its header (first 500 chars)"
    );
}

// ===========================================================================
// 6. PARITY_CLOSURE_BEADS.md annotates 4.5.1 references as superseded
// ===========================================================================

#[test]
fn parity_closure_beads_annotated() {
    let path = repo_root().join("prd/PARITY_CLOSURE_BEADS.md");
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap();

    // Must contain a notice about historical context
    let first_800_chars: String = content.chars().take(800).collect();
    assert!(
        first_800_chars.contains("Historical")
            || first_800_chars.contains("historical")
            || first_800_chars.contains("pre-repin")
            || first_800_chars.contains("superseded"),
        "PARITY_CLOSURE_BEADS.md must annotate its 4.5.1 context as historical"
    );
}

// ===========================================================================
// 7. No doc file presents 4.5.1 parity numbers without context
// ===========================================================================

#[test]
fn no_unannotated_451_parity_numbers() {
    // Scan key docs for lines that contain both "4.5.1" and a percentage
    // without historical annotation.
    let docs_to_check = [
        "COMPAT_MATRIX.md",
        "docs/BENCHMARK_BASELINES.md",
    ];

    for doc_name in &docs_to_check {
        let path = repo_root().join(doc_name);
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path).unwrap();

        for (i, line) in content.lines().enumerate() {
            if line.contains("4.5.1") && line.contains('%') {
                assert!(
                    line.contains("historical")
                        || line.contains("Historical")
                        || line.contains("pre-repin")
                        || line.contains("PRE-REPIN")
                        || line.contains("no longer live")
                        || line.contains("baseline"),
                    "{doc_name} line {}: contains 4.5.1 percentage without annotation: {line}",
                    i + 1
                );
            }
        }
    }
}

// ===========================================================================
// 8. BENCHMARK_BASELINES.md has historical section properly labeled
// ===========================================================================

#[test]
fn benchmark_baselines_historical_section() {
    let path = repo_root().join("docs/BENCHMARK_BASELINES.md");
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap();

    if content.contains("4.5.1") {
        assert!(
            content.contains("Historical: Godot 4.5.1")
                || content.contains("Historical:")
                || content.contains("PRE-REPIN"),
            "BENCHMARK_BASELINES.md must label 4.5.1 section as Historical"
        );
    }

    // The live baseline must reference 4.6.1
    assert!(
        content.contains("Baseline: Godot 4.6.1")
            || content.contains("4.6.1-stable"),
        "BENCHMARK_BASELINES.md must have a live 4.6.1 baseline section"
    );
}

// ===========================================================================
// 9. GODOT_4_6_1_REPIN_DIFF.md exists and references both versions
// ===========================================================================

#[test]
fn repin_diff_exists_and_references_both_versions() {
    let path = repo_root().join("prd/GODOT_4_6_1_REPIN_DIFF.md");
    assert!(
        path.exists(),
        "prd/GODOT_4_6_1_REPIN_DIFF.md must exist (repin delta documentation)"
    );
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(content.contains("4.5.1"), "repin diff must reference old pin 4.5.1");
    assert!(content.contains("4.6.1"), "repin diff must reference new pin 4.6.1");
    assert!(
        content.contains("Previous pin") || content.contains("previous pin"),
        "repin diff must label 4.5.1 as previous pin"
    );
}

// ===========================================================================
// 10. Migration guide references live 4.6.1 oracle pin
// ===========================================================================

#[test]
fn migration_guide_references_live_oracle_pin() {
    let path = repo_root().join("docs/migration-guide.md");
    assert!(path.exists(), "docs/migration-guide.md must exist");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        content.contains("4.6.1-stable"),
        "migration-guide.md must reference the live oracle pin 4.6.1-stable"
    );
    assert!(
        content.contains("Live Oracle Pin"),
        "migration-guide.md must have a 'Live Oracle Pin' section"
    );
}

// ===========================================================================
// 11. Migration guide marks 4.5.1 as historical
// ===========================================================================

#[test]
fn migration_guide_marks_451_as_historical() {
    let path = repo_root().join("docs/migration-guide.md");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        content.contains("Historical: Godot 4.5.1"),
        "migration-guide.md must label 4.5.1 as historical"
    );
    assert!(
        !content.contains("Live Oracle Pin: Godot 4.5.1"),
        "migration-guide.md must not present 4.5.1 as the live oracle pin"
    );
}

// ===========================================================================
// 12. Migration guide documents repin changes
// ===========================================================================

#[test]
fn migration_guide_has_repin_changelog() {
    let path = repo_root().join("docs/migration-guide.md");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        content.contains("4.5.1 → 4.6.1") || content.contains("4.5.1 to 4.6.1"),
        "migration-guide.md must document what changed in the repin"
    );
}

// ===========================================================================
// 13. Migration guide version table is complete
// ===========================================================================

#[test]
fn migration_guide_version_table_complete() {
    let path = repo_root().join("docs/migration-guide.md");
    let content = std::fs::read_to_string(&path).unwrap();

    let required_items = ["Upstream oracle pin", "GDExtension lab", "Scene format", "Minimum Rust"];
    for item in &required_items {
        assert!(
            content.contains(item),
            "migration-guide.md version table must include '{item}'"
        );
    }
}

// ===========================================================================
// 14. COMPAT_MATRIX.md oracle parity line cites 4.6.1
// ===========================================================================

#[test]
fn compat_matrix_oracle_parity_cites_461() {
    let path = repo_root().join("COMPAT_MATRIX.md");
    let content = std::fs::read_to_string(&path).unwrap();

    // The Oracle Parity section must explicitly cite 4.6.1
    let oracle_section = content
        .find("Oracle Parity")
        .expect("COMPAT_MATRIX.md must have an Oracle Parity section");
    let section = &content[oracle_section..];
    let section_end = section.find("\n---").unwrap_or(section.len());
    let oracle_text = &section[..section_end];

    assert!(
        oracle_text.contains("4.6.1"),
        "Oracle Parity section must cite Godot 4.6.1"
    );
}
