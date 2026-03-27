//! pat-2z6a: Validate that the repin-validation CI workflow covers all
//! required gates for a complete Godot version repin.
//!
//! These tests verify that:
//! 1. The repin-validation workflow exists and has correct triggers
//! 2. All required validation jobs are defined
//! 3. The parity-summary job depends on all gates
//! 4. The fail condition checks all required gates
//! 5. Auto-detect of Godot version works for non-dispatch triggers

use std::fs;
use std::path::Path;

const REPIN_CI_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../.github/workflows/repin-validation.yml");

fn read_repin_ci() -> String {
    fs::read_to_string(REPIN_CI_PATH)
        .expect("repin-validation.yml must exist at .github/workflows/repin-validation.yml")
}

// ===========================================================================
// 1. Workflow exists and has correct triggers
// ===========================================================================

#[test]
fn repin_workflow_exists() {
    assert!(
        Path::new(REPIN_CI_PATH).exists(),
        "repin-validation.yml must exist"
    );
}

#[test]
fn repin_workflow_has_dispatch_trigger() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("workflow_dispatch:"),
        "repin workflow must support manual dispatch"
    );
    assert!(
        ci.contains("godot_version"),
        "dispatch must accept godot_version input"
    );
}

#[test]
fn repin_workflow_has_push_trigger() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("push:"),
        "repin workflow must trigger on push"
    );
    assert!(
        ci.contains("upstream/godot"),
        "push trigger must watch upstream/godot submodule"
    );
}

#[test]
fn repin_workflow_has_pr_trigger() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("pull_request:"),
        "repin workflow must trigger on pull_request"
    );
}

#[test]
fn repin_workflow_watches_oracle_and_golden_paths() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("fixtures/oracle_outputs/**"),
        "must watch oracle output changes"
    );
    assert!(
        ci.contains("fixtures/golden/**"),
        "must watch golden fixture changes"
    );
}

// ===========================================================================
// 2. Required validation jobs exist
// ===========================================================================

#[test]
fn repin_has_detect_version_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("detect-version:"),
        "must have detect-version job"
    );
}

#[test]
fn repin_has_oracle_parity_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("oracle-parity:"),
        "must have oracle-parity job"
    );
}

#[test]
fn repin_has_render_goldens_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("render-goldens:"),
        "must have render-goldens job"
    );
}

#[test]
fn repin_has_physics_trace_goldens_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("physics-trace-goldens:"),
        "must have physics-trace-goldens job"
    );
}

#[test]
fn repin_has_runtime_compat_slices_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("runtime-compat-slices:"),
        "must have runtime-compat-slices job"
    );
}

#[test]
fn repin_has_pin_verification_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("pin-verification:"),
        "must have pin-verification job"
    );
}

#[test]
fn repin_has_parity_summary_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("parity-summary:"),
        "must have parity-summary job"
    );
}

#[test]
fn repin_has_gdextension_lab_job() {
    let ci = read_repin_ci();
    assert!(
        ci.contains("gdextension-lab:"),
        "must have gdextension-lab job"
    );
}

// ===========================================================================
// 3. Parity summary depends on all gates
// ===========================================================================

#[test]
fn parity_summary_depends_on_all_gates() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "parity-summary");

    let required_deps = [
        "detect-version",
        "oracle-parity",
        "render-goldens",
        "physics-trace-goldens",
        "runtime-compat-slices",
        "pin-verification",
    ];

    for dep in &required_deps {
        assert!(
            section.contains(dep),
            "parity-summary must depend on '{dep}'"
        );
    }
}

// ===========================================================================
// 4. Fail condition checks all required gates
// ===========================================================================

#[test]
fn fail_condition_checks_all_gates() {
    let ci = read_repin_ci();

    let required_checks = [
        "oracle-parity",
        "render-goldens",
        "physics-trace-goldens",
        "runtime-compat-slices",
        "pin-verification",
    ];

    // The fail condition should reference each gate's result
    for gate in &required_checks {
        let pattern = format!("needs.{gate}.result");
        assert!(
            ci.contains(&pattern),
            "fail condition must check '{gate}' result"
        );
    }
}

// ===========================================================================
// 5. Auto-detect version job works
// ===========================================================================

#[test]
fn detect_version_has_fallback_logic() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "detect-version");

    // Must handle both dispatch (inputs.godot_version) and auto-detect
    assert!(
        section.contains("inputs.godot_version") || section.contains("godot_version"),
        "detect-version must reference godot_version input"
    );
    assert!(
        section.contains("outputs"),
        "detect-version must define outputs"
    );
}

// ===========================================================================
// 6. Physics trace job covers key test patterns
// ===========================================================================

#[test]
fn physics_trace_job_covers_expected_tests() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "physics-trace-goldens");

    let patterns = [
        "physics_trace",
        "deterministic_physics",
        "physics_playground",
        "physics_stepping",
        "mainloop_physics",
    ];

    for pattern in &patterns {
        assert!(
            section.contains(pattern),
            "physics-trace-goldens must cover '{pattern}'"
        );
    }
}

// ===========================================================================
// 7. Runtime compat job covers headless and 2D slices
// ===========================================================================

#[test]
fn runtime_compat_job_covers_headless_and_2d() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "runtime-compat-slices");

    // Must run headless compat tests
    assert!(
        section.contains("resource_") || section.contains("headless"),
        "runtime-compat must cover headless slice"
    );

    // Must run 2D compat tests
    assert!(
        section.contains("render_") || section.contains("2d") || section.contains("2D"),
        "runtime-compat must cover 2D slice"
    );
}

// ===========================================================================
// 8. Pin verification checks submodule and fixtures
// ===========================================================================

#[test]
fn pin_verification_checks_submodule() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "pin-verification");

    assert!(
        section.contains("upstream/godot") || section.contains("submodule"),
        "pin-verification must check upstream/godot submodule"
    );
}

#[test]
fn pin_verification_checks_fixtures() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "pin-verification");

    assert!(
        section.contains("oracle_outputs") || section.contains("fixtures"),
        "pin-verification must check oracle output fixtures"
    );
}

// ===========================================================================
// 9. Summary reports all gate results
// ===========================================================================

#[test]
fn summary_reports_physics_and_runtime_results() {
    let ci = read_repin_ci();
    let section = extract_repin_section(&ci, "parity-summary");

    assert!(
        section.contains("PHYSICS_RESULT") || section.contains("physics_res"),
        "summary must report physics trace result"
    );
    assert!(
        section.contains("RUNTIME_RESULT") || section.contains("runtime_res"),
        "summary must report runtime compat result"
    );
    assert!(
        section.contains("PIN_RESULT") || section.contains("pin_res"),
        "summary must report pin verification result"
    );
}

// ===========================================================================
// 10. Minimum job count
// ===========================================================================

#[test]
fn repin_workflow_has_at_least_7_jobs() {
    let ci = read_repin_ci();
    // Count top-level job definitions (indented with exactly 2 spaces under `jobs:`)
    let job_markers = [
        "detect-version:",
        "oracle-parity:",
        "render-goldens:",
        "gdextension-lab:",
        "physics-trace-goldens:",
        "runtime-compat-slices:",
        "pin-verification:",
        "parity-summary:",
    ];

    let mut count = 0;
    for marker in &job_markers {
        if ci.contains(marker) {
            count += 1;
        }
    }

    assert!(
        count >= 7,
        "repin workflow must have at least 7 jobs (got {count})"
    );
}

// ===========================================================================
// 11. ci.yml has all 6 compat domain jobs — pat-k7d
// ===========================================================================

#[test]
fn ci_has_all_compat_domain_jobs() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).expect("ci.yml must exist");

    let required_jobs = [
        "rust-compat-headless:",
        "rust-compat-2d:",
        "rust-compat-3d:",
        "rust-compat-platform:",
        "rust-compat-fuzz:",
        "rust-compat-ci-meta:",
    ];

    for job in &required_jobs {
        assert!(
            ci.contains(job),
            "ci.yml must have compat domain job '{job}'"
        );
    }
}

// ===========================================================================
// 12. ci.yml rust job has multi-platform matrix — pat-k7d
// ===========================================================================

#[test]
fn ci_rust_job_has_multi_platform_matrix() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(ci.contains("ubuntu-latest"), "must test on ubuntu");
    assert!(ci.contains("macos-latest"), "must test on macos");
    assert!(ci.contains("windows-latest"), "must test on windows");
    assert!(ci.contains("fail-fast: false"), "matrix must not fail-fast");
}

// ===========================================================================
// 13. ci.yml rust-audit has continue-on-error — pat-k7d
// ===========================================================================

#[test]
fn ci_rust_audit_has_continue_on_error() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(ci.contains("rust-audit:"), "must have rust-audit job");
    assert!(
        ci.contains("continue-on-error: true"),
        "rust-audit must have continue-on-error: true (advisory DB may be stale)"
    );
}

// ===========================================================================
// 14. ci.yml job dependency chain — pat-k7d
// ===========================================================================

#[test]
fn ci_job_dependency_chain() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    // rust job needs rust-fmt
    assert!(
        ci.contains("needs: rust-fmt"),
        "rust job must depend on rust-fmt"
    );
    // rust-release needs rust
    assert!(
        ci.contains("needs: rust"),
        "rust-release must depend on rust"
    );
}

// ===========================================================================
// 15. ci.yml oracle-parity checks out submodules — pat-k7d
// ===========================================================================

#[test]
fn ci_oracle_parity_uses_submodules() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(ci.contains("rust-oracle-parity:"), "must have rust-oracle-parity job");
    // The oracle parity job and render goldens job both checkout with submodules
    assert!(
        ci.contains("submodules: true"),
        "oracle parity and render golden jobs must checkout submodules"
    );
}

// ===========================================================================
// 16. ci.yml render-goldens uploads artifacts on failure — pat-k7d
// ===========================================================================

#[test]
fn ci_render_goldens_uploads_artifacts_on_failure() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(
        ci.contains("upload-artifact@v4"),
        "render-goldens must upload artifacts"
    );
    assert!(
        ci.contains("if: failure()"),
        "artifact upload must be conditional on failure"
    );
    assert!(
        ci.contains("golden-render-"),
        "artifact name must include golden-render prefix"
    );
}

// ===========================================================================
// 17. ci.yml has web job with pnpm — pat-k7d
// ===========================================================================

#[test]
fn ci_has_web_job_with_pnpm() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(ci.contains("web:"), "ci.yml must have web job");
    assert!(ci.contains("pnpm"), "web job must use pnpm");
    assert!(ci.contains("pnpm build"), "web job must run pnpm build");
    assert!(ci.contains("pnpm lint"), "web job must run pnpm lint");
}

// ===========================================================================
// 18. ci.yml PATINA_CI env var set for render lane — pat-k7d
// ===========================================================================

#[test]
fn ci_render_lane_sets_patina_ci_env() {
    let ci_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci = fs::read_to_string(ci_path).unwrap();

    assert!(
        ci.contains("PATINA_CI"),
        "render golden lane must set PATINA_CI env var"
    );
}

// ===========================================================================
// Helpers
// ===========================================================================

fn extract_repin_section(ci: &str, job_name: &str) -> String {
    let marker = format!("  {}:", job_name);
    if let Some(start) = ci.find(&marker) {
        let rest = &ci[start..];
        let after_first_line = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        // Find next job at same indentation level
        let section_end = rest[after_first_line..]
            .find("\n  ")
            .and_then(|pos| {
                // Check if this is a new job (word followed by colon at indent 2)
                let candidate = &rest[after_first_line + pos + 3..];
                if candidate.starts_with(char::is_alphabetic)
                    && candidate.contains(':')
                    && candidate.find(':') < candidate.find('\n')
                    && !candidate.starts_with(' ')
                {
                    Some(after_first_line + pos)
                } else {
                    None
                }
            });

        // Fallback: grab a generous chunk
        let end = section_end.unwrap_or(rest.len().min(3000));
        rest[..end].to_string()
    } else {
        String::new()
    }
}
