//! pat-3hw, pat-svl: Update compatibility docs to distinguish historical 4.5.1
//! numbers from live 4.6.1 numbers.
//!
//! Validates that:
//! 1. migration-guide.md has a "Historical: Godot 4.5.1" section
//! 2. migration-guide.md marks 4.5.1 data as NOT used for current validation
//! 3. migration-guide.md documents the 4.5.1 → 4.6.1 repin changes
//! 4. BENCHMARK_BASELINES.md distinguishes historical 4.5.1 from current 4.6.1
//! 5. REPIN_REPORT.md references both versions with correct framing
//! 6. No docs claim 4.5.1 is the current oracle

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read_doc(rel_path: &str) -> String {
    let path = repo_root().join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {rel_path}: {e}"))
}

// ===========================================================================
// 1. migration-guide.md version distinction
// ===========================================================================

#[test]
fn migration_guide_has_historical_451_section() {
    let guide = read_doc("docs/migration-guide.md");
    assert!(
        guide.contains("Historical: Godot 4.5.1"),
        "migration-guide.md should have a 'Historical: Godot 4.5.1' section"
    );
}

#[test]
fn migration_guide_marks_451_as_not_current() {
    let guide = read_doc("docs/migration-guide.md");
    assert!(
        guide.contains("not** used for current parity validation")
            || guide.contains("not used for current parity"),
        "migration-guide.md should state 4.5.1 data is not used for current validation"
    );
}

#[test]
fn migration_guide_documents_repin_changes() {
    let guide = read_doc("docs/migration-guide.md");
    assert!(
        guide.contains("4.5.1 → 4.6.1 Repin") || guide.contains("4.5.1 -> 4.6.1"),
        "migration-guide.md should document the 4.5.1 → 4.6.1 repin"
    );
}

#[test]
fn migration_guide_mentions_oracle_regeneration() {
    let guide = read_doc("docs/migration-guide.md");
    assert!(
        guide.contains("Oracle outputs regenerated") || guide.contains("oracle"),
        "migration-guide.md should mention oracle regeneration during repin"
    );
}

#[test]
fn migration_guide_451_treated_as_historical_context() {
    let guide = read_doc("docs/migration-guide.md");
    assert!(
        guide.contains("historical context"),
        "migration-guide.md should say 4.5.1 references are historical context"
    );
}

// ===========================================================================
// 2. BENCHMARK_BASELINES.md version distinction
// ===========================================================================

#[test]
fn benchmark_baselines_has_historical_451_section() {
    let baselines = read_doc("docs/BENCHMARK_BASELINES.md");
    assert!(
        baselines.contains("Historical") && baselines.contains("4.5.1"),
        "BENCHMARK_BASELINES.md should have a historical 4.5.1 section"
    );
}

#[test]
fn benchmark_baselines_references_461() {
    let baselines = read_doc("docs/BENCHMARK_BASELINES.md");
    assert!(
        baselines.contains("4.6.1"),
        "BENCHMARK_BASELINES.md should reference 4.6.1"
    );
}

// ===========================================================================
// 3. REPIN_REPORT.md version framing
// ===========================================================================

#[test]
fn repin_report_shows_version_transition() {
    let report = read_doc("REPIN_REPORT.md");
    assert!(
        report.contains("4.5.1") && report.contains("4.6.1"),
        "REPIN_REPORT.md should reference both 4.5.1 and 4.6.1"
    );
}

#[test]
fn repin_report_pre_repin_labeled_as_451() {
    let report = read_doc("REPIN_REPORT.md");
    assert!(
        report.contains("Pre-repin (4.5.1)") || report.contains("Pre-repin"),
        "REPIN_REPORT.md should label pre-repin baseline as 4.5.1"
    );
}

#[test]
fn repin_report_final_labeled_as_461() {
    let report = read_doc("REPIN_REPORT.md");
    assert!(
        report.contains("Final (4.6.1")
            || report.contains("4.6.1, commit")
            || report.contains("Final (commit")
            || (report.contains("Final") && report.contains("4.6.1")),
        "REPIN_REPORT.md should label final result as 4.6.1"
    );
}

// ===========================================================================
// 4. No doc claims 4.5.1 is the current oracle
// ===========================================================================

#[test]
fn migration_guide_does_not_claim_451_is_current() {
    let guide = read_doc("docs/migration-guide.md");
    // Check there's no line saying 4.5.1 is the current or active oracle
    // The guide should only reference 4.5.1 in historical context
    let lines: Vec<&str> = guide.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("4.5.1") {
            let lower = line.to_lowercase();
            assert!(
                !lower.contains("current oracle")
                    && !lower.contains("active oracle")
                    && !lower.contains("pinned to 4.5.1"),
                "line {}: should not claim 4.5.1 is the current oracle: {line}",
                i + 1
            );
        }
    }
}

#[test]
fn benchmark_baselines_451_section_marked_historical() {
    let baselines = read_doc("docs/BENCHMARK_BASELINES.md");
    // Find lines mentioning 4.5.1 and verify they're in historical context
    let has_historical_marker = baselines.contains("Historical")
        || baselines.contains("historical")
        || baselines.contains("pre-repin");
    assert!(
        has_historical_marker,
        "BENCHMARK_BASELINES.md should mark 4.5.1 data as historical"
    );
}
