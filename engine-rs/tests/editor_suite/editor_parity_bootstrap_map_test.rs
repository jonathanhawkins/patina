//! Bootstrap tests for the editor-parity phase.
//!
//! These tests verify that the per-lane execution maps named in
//! `prd/EDITOR_PARITY_BOOTSTRAP_MAP.md` exist and meet the structural
//! contract every execution map must follow:
//!
//! 1. The file exists in `prd/`.
//! 2. It has at least one `## Now` priority section.
//! 3. It declares at least the minimum number of numbered beads listed in
//!    the bootstrap map.
//! 4. Each bead has an `Acceptance:` line that names a test
//!    `(test: \`<name>\`)` the planner can wire into its analysis.
//!
//! Each test is named to match the `(test: \`...\`)` marker in
//! `prd/EDITOR_PARITY_BOOTSTRAP_EXIT.md`. The planner runs the named test;
//! when it passes, the corresponding checkbox auto-ticks and the meta-bead
//! is considered done.
//!
//! Tests are #[ignore] until the corresponding map file is authored — that
//! way `cargo nextest run` on a fresh checkout doesn't fail loudly. The
//! planner runs them via explicit nextest filter regardless of #[ignore].

use std::fs;
use std::path::{Path, PathBuf};

const PRD_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../prd");

fn prd_path(name: &str) -> PathBuf {
    Path::new(PRD_DIR).join(name)
}

/// Count lines matching ``  N. `key` …`` (numbered execution-map beads).
fn count_numbered_beads(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            // Match `<digits>. \``
            let mut chars = trimmed.chars().peekable();
            let mut saw_digit = false;
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    saw_digit = true;
                    chars.next();
                } else {
                    break;
                }
            }
            if !saw_digit {
                return false;
            }
            // After digits, expect `. `
            if chars.next() != Some('.') {
                return false;
            }
            if chars.next() != Some(' ') {
                return false;
            }
            // Then a backtick.
            chars.next() == Some('`')
        })
        .count()
}

/// Count lines whose `Acceptance:` body contains `(test: \`name\`)`.
fn count_acceptance_with_test_marker(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            let trimmed = lower.trim_start();
            if !trimmed.starts_with("acceptance:") {
                return false;
            }
            // Find `(test:` then a backtick later in the same line.
            if let Some(start) = lower.find("(test:") {
                let rest = &lower[start..];
                rest.contains('`')
            } else {
                false
            }
        })
        .count()
}

/// Assert that `path` exists, has at least one `## Now` section, contains at
/// least `min_items` numbered bead entries, and each numbered entry has an
/// `Acceptance:` line with a `(test: \`...\`)` marker.
fn assert_execution_map_valid(path: &Path, min_items: usize) {
    let body = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", path.display()));

    assert!(
        body.contains("## Now"),
        "{} must have a `## Now` priority section",
        path.display()
    );

    let bead_count = count_numbered_beads(&body);
    let acceptance_count = count_acceptance_with_test_marker(&body);

    assert!(
        bead_count >= min_items,
        "{} has {} numbered beads but bootstrap requires at least {}",
        path.display(),
        bead_count,
        min_items
    );
    assert!(
        acceptance_count >= min_items,
        "{} has {} Acceptance lines with test markers, expected at least {} (one per bead)",
        path.display(),
        acceptance_count,
        min_items
    );
}

#[test]
#[ignore]
fn bootstrap_scene_tree_ops_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_SCENE_TREE_OPS_MAP.md"), 8);
}

#[test]
#[ignore]
fn bootstrap_scene_tree_indicators_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_SCENE_TREE_INDICATORS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_inspector_toolbar_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_INSPECTOR_TOOLBAR_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_inspector_properties_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_INSPECTOR_PROPERTIES_MAP.md"), 8);
}

#[test]
#[ignore]
fn bootstrap_inspector_advanced_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_INSPECTOR_ADVANCED_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_viewport_selection_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_VIEWPORT_SELECTION_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_viewport_gizmos_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_VIEWPORT_GIZMOS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_viewport_overlays_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_VIEWPORT_OVERLAYS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_top_bar_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_TOP_BAR_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_menus_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_MENUS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_create_node_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_CREATE_NODE_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_bottom_panels_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_BOTTOM_PANELS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_script_editor_core_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_SCRIPT_EDITOR_CORE_MAP.md"), 8);
}

#[test]
#[ignore]
fn bootstrap_script_editor_nav_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_SCRIPT_EDITOR_NAV_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_filesystem_dock_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_FILESYSTEM_DOCK_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_signals_dock_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_SIGNALS_DOCK_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_animation_editor_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_ANIMATION_EDITOR_MAP.md"), 8);
}

#[test]
#[ignore]
fn bootstrap_editor_systems_map_authored() {
    assert_execution_map_valid(&prd_path("EDITOR_PARITY_EDITOR_SYSTEMS_MAP.md"), 6);
}

#[test]
#[ignore]
fn bootstrap_aggregate_editor_parity_map_assembled() {
    let exec_map = prd_path("EDITOR_PARITY_EXECUTION_MAP.md");
    let exit = prd_path("EDITOR_PARITY_EXIT.md");

    // The aggregate must itself be a valid execution map with at least
    // one bead carried over from each lane (18 lanes → 18+ items minimum).
    assert_execution_map_valid(&exec_map, 18);

    let exit_body = fs::read_to_string(&exit)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", exit.display()));
    // Count both `- [ ]` and `- [x]` checkbox lines that carry a `(test:` marker.
    let total = exit_body
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            (t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]"))
                && t.contains("(test:")
                && t.contains('`')
        })
        .count();
    assert!(
        total >= 18,
        "{} has {} checkbox lines with test markers; expected at least 18",
        exit.display(),
        total
    );
}
