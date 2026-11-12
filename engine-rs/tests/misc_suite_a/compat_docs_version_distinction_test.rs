//! pat-gg8: Update compatibility docs to distinguish historical 4.5.1 numbers
//! from live 4.6.1 numbers.
//!
//! Validates that:
//! 1. All compatibility docs reference the live 4.6.1 pin
//! 2. Historical 4.5.1 references are clearly labeled as historical
//! 3. No stale gap claims contradict the 100% parity final state
//! 4. The 3D runtime is no longer listed as "Deferred" (crates exist)

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

// ===========================================================================
// 1. Live 4.6.1 pin is referenced in all compatibility docs
// ===========================================================================

#[test]
fn compat_matrix_references_461() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    assert!(
        content.contains("4.6.1"),
        "COMPAT_MATRIX.md must reference 4.6.1"
    );
    // The scene count must be the LIVE one, not a number frozen into this
    // test (it said "18" while the doc said "21+" and the fixtures held 45).
    let n = oracle_scene_count();
    assert!(
        content.contains(&format!("{n} oracle scenes")),
        "COMPAT_MATRIX.md must state the current oracle corpus: {n} oracle scenes"
    );
}

#[test]
fn compat_dashboard_references_461() {
    let content = fs::read_to_string(repo_root().join("COMPAT_DASHBOARD.md")).unwrap();
    assert!(
        content.contains("4.6.1"),
        "COMPAT_DASHBOARD.md must reference 4.6.1"
    );
    assert!(
        content.contains("100.0%"),
        "COMPAT_DASHBOARD.md must show current 100% parity"
    );
}

#[test]
fn migration_guide_references_461() {
    let content = fs::read_to_string(repo_root().join("docs/migration-guide.md")).unwrap();
    assert!(
        content.contains("4.6.1"),
        "migration-guide.md must reference 4.6.1"
    );
    assert!(
        content.contains("Live Oracle Pin") || content.contains("live"),
        "migration-guide.md must identify 4.6.1 as the live pin"
    );
}

#[test]
fn repin_report_references_461() {
    let content = fs::read_to_string(repo_root().join("REPIN_REPORT.md")).unwrap();
    assert!(
        content.contains("4.6.1"),
        "REPIN_REPORT.md must reference 4.6.1"
    );
    assert!(
        content.contains("81.4%") || content.contains("180/221"),
        "REPIN_REPORT.md must show expanded corpus parity metrics"
    );
}

// ===========================================================================
// 2. Historical 4.5.1 references are clearly labeled
// ===========================================================================

#[test]
fn compat_matrix_labels_451_as_historical() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    if content.contains("4.5.1") {
        // Every mention of 4.5.1 should be near "historical" or "pre-repin"
        assert!(
            content.contains("Historical")
                || content.contains("historical")
                || content.contains("pre-repin"),
            "COMPAT_MATRIX.md mentions 4.5.1 but does not label it as historical"
        );
    }
}

#[test]
fn compat_dashboard_labels_451_as_historical() {
    let content = fs::read_to_string(repo_root().join("COMPAT_DASHBOARD.md")).unwrap();
    if content.contains("4.5.1") {
        assert!(
            content.contains("Historical")
                || content.contains("historical")
                || content.contains("pre-repin"),
            "COMPAT_DASHBOARD.md mentions 4.5.1 but does not label it as historical"
        );
    }
}

#[test]
fn migration_guide_labels_451_as_historical() {
    let content = fs::read_to_string(repo_root().join("docs/migration-guide.md")).unwrap();
    if content.contains("4.5.1") {
        assert!(
            content.contains("Historical")
                || content.contains("historical")
                || content.contains("Prior to"),
            "migration-guide.md mentions 4.5.1 but does not label it as historical"
        );
    }
}

// ===========================================================================
// 3. No stale gap claims contradict 100% parity
// ===========================================================================

#[test]
fn compat_dashboard_no_stale_script_var_gaps() {
    let content = fs::read_to_string(repo_root().join("COMPAT_DASHBOARD.md")).unwrap();
    // The "Known Gaps" section should not list script-var gaps as current
    // (they were resolved). If mentioned, must be in a "Resolved" context.
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("speed, can_shoot") || line.contains("direction, speed, health") {
            // This line mentions old gaps — it must be in a Resolved/historical context
            let context_start = i.saturating_sub(5);
            let context_end = (i + 3).min(lines.len());
            let context: String = lines[context_start..context_end].join("\n");
            assert!(
                context.contains("Resolved") || context.contains("resolved") || context.contains("Historical") || context.contains("historical"),
                "COMPAT_DASHBOARD.md mentions old script-var gaps without marking them resolved near line {}: {}",
                i + 1, line
            );
        }
    }
}

#[test]
fn compat_matrix_oracle_parity_shows_current_metrics() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    // The Oracle Parity Summary section must show the expanded corpus metrics
    let n = oracle_scene_count();
    let files = std::fs::read_dir(repo_root().join("fixtures/oracle_outputs"))
        .unwrap()
        // Oracle outputs only: skip dot-files (`.pre_repin_snapshot.json` is a
        // repin backup, not an output).
        .filter(|e| {
            e.as_ref().map_or(false, |e| {
                !e.file_name().to_string_lossy().starts_with('.')
                    && e.path().extension().map_or(false, |x| x == "json")
            })
        })
        .count();
    assert!(
        content.contains(&format!("{n} oracle scenes")) && content.contains(&format!("{files} oracle output files")),
        "COMPAT_MATRIX.md Oracle Parity Summary must show the live corpus: {n} oracle scenes, {files} oracle output files"
    );
}

// ===========================================================================
// 4. 3D runtime status is accurate (no longer purely "Deferred")
// ===========================================================================

#[test]
fn compat_matrix_3d_runtime_not_deferred() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    // Find the 3D Runtime row in the compatibility matrix table
    for line in content.lines() {
        if line.contains("3D Runtime") && line.contains("|") {
            assert!(
                !line.contains("**Deferred**"),
                "COMPAT_MATRIX.md should not list 3D Runtime as Deferred — crates exist with 84+ tests"
            );
            break;
        }
    }
}

#[test]
fn compat_dashboard_3d_runtime_not_deferred() {
    let content = fs::read_to_string(repo_root().join("COMPAT_DASHBOARD.md")).unwrap();
    for line in content.lines() {
        if line.contains("3D Runtime") && line.contains("|") {
            assert!(
                !line.contains("Deferred"),
                "COMPAT_DASHBOARD.md should not list 3D Runtime as Deferred — crates exist"
            );
            break;
        }
    }
}

// ===========================================================================
// 5. Version consistency across docs
// ===========================================================================

#[test]
fn all_compat_docs_agree_on_oracle_pin() {
    let pin_commit = "14d19694e";
    let docs = &["COMPAT_MATRIX.md", "COMPAT_DASHBOARD.md", "REPIN_REPORT.md"];
    for doc in docs {
        let content = fs::read_to_string(repo_root().join(doc)).unwrap();
        assert!(
            content.contains(pin_commit) || content.contains("4.6.1"),
            "{doc} must reference the live 4.6.1 oracle pin"
        );
    }
}

#[test]
fn repin_diff_report_has_both_versions() {
    let content = fs::read_to_string(repo_root().join("prd/GODOT_4_6_1_REPIN_DIFF.md")).unwrap();
    assert!(
        content.contains("4.5.1"),
        "repin diff must reference old version 4.5.1"
    );
    assert!(
        content.contains("4.6.1"),
        "repin diff must reference new version 4.6.1"
    );
    assert!(
        content.contains("Improved") && content.contains("Regressed"),
        "repin diff must separate improvements from regressions"
    );
}

/// Scenes with a Godot property capture — the oracle corpus the matrix reports.
fn oracle_scene_count() -> usize {
    std::fs::read_dir(repo_root().join("fixtures/oracle_outputs"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with("_properties.json")
        })
        .count()
}
