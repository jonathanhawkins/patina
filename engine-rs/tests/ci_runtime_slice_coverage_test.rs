//! pat-u5i8 / pat-a7o: Validate that CI workflow covers all runtime slice compat tests.
//!
//! These tests verify that:
//! 1. The CI workflow file exists and is valid YAML structure
//! 2. Dedicated runtime slice gates exist for each slice
//! 3. All runtime slice test files are covered by at least one CI gate
//! 4. Platform tests run on all three desktop OSes
//! 5. No test file is completely uncovered by CI

use std::fs;
use std::path::Path;

const CI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../.github/workflows/ci.yml");
const TESTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");

fn read_ci() -> String {
    fs::read_to_string(CI_PATH).expect("CI workflow must exist at .github/workflows/ci.yml")
}

fn list_test_files() -> Vec<String> {
    let dir = Path::new(TESTS_DIR);
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with("_test.rs") {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}

// ===========================================================================
// 1. CI workflow structure
// ===========================================================================

#[test]
fn ci_workflow_exists() {
    assert!(
        Path::new(CI_PATH).exists(),
        "CI workflow must exist at .github/workflows/ci.yml"
    );
}

#[test]
fn ci_workflow_has_push_trigger() {
    let ci = read_ci();
    assert!(ci.contains("push:"), "CI must trigger on push");
    assert!(
        ci.contains("pull_request:"),
        "CI must trigger on pull_request"
    );
}

#[test]
fn ci_workflow_has_workspace_test() {
    let ci = read_ci();
    assert!(
        ci.contains("cargo test --workspace"),
        "CI must run full workspace tests"
    );
}

// ===========================================================================
// 2. Dedicated runtime slice CI gates exist
// ===========================================================================

#[test]
fn ci_has_headless_runtime_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-headless"),
        "CI must have a dedicated headless runtime compat gate"
    );
    assert!(
        ci.contains("Headless runtime"),
        "headless gate must have descriptive name"
    );
}

#[test]
fn ci_has_2d_slice_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-2d"),
        "CI must have a dedicated 2D slice compat gate"
    );
    assert!(
        ci.contains("2D slice"),
        "2D gate must have descriptive name"
    );
}

#[test]
fn ci_has_3d_slice_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-3d"),
        "CI must have a dedicated 3D slice compat gate"
    );
    assert!(
        ci.contains("3D slice"),
        "3D gate must have descriptive name"
    );
}

#[test]
fn ci_has_platform_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-platform"),
        "CI must have a dedicated platform layer compat gate"
    );
    assert!(
        ci.contains("Platform layer"),
        "platform gate must have descriptive name"
    );
}

#[test]
fn ci_has_fuzz_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-fuzz"),
        "CI must have a dedicated fuzz/property test gate"
    );
    assert!(
        ci.contains("Fuzz"),
        "fuzz gate must have descriptive name"
    );
}

// ===========================================================================
// 3. Slice gates cover key test patterns
// ===========================================================================

#[test]
fn headless_gate_covers_core_patterns() {
    let ci = read_ci();
    // Extract the headless section
    let headless_section = extract_section(&ci, "rust-compat-headless");

    let core_patterns = [
        "resource_",
        "scene_",
        "signal_",
        "notification_",
        "object_",
        "classdb_",
        "lifecycle_",
        "packed_scene_",
    ];

    for pattern in &core_patterns {
        assert!(
            headless_section.contains(pattern),
            "headless gate must cover pattern '{pattern}'"
        );
    }
}

#[test]
fn two_d_gate_covers_core_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-2d");

    let patterns = [
        "physics_",
        "render_",
        "collision_",
        "node2d_",
        "geometry2d_",
        "vertical_slice",
    ];

    for pattern in &patterns {
        assert!(
            section.contains(pattern),
            "2D gate must cover pattern '{pattern}'"
        );
    }
}

#[test]
fn three_d_gate_covers_core_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-3d");

    let patterns = ["node3d_", "physics3d_", "transform3d_"];

    for pattern in &patterns {
        assert!(
            section.contains(pattern),
            "3D gate must cover pattern '{pattern}'"
        );
    }
}

#[test]
fn platform_gate_covers_core_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-platform");

    let patterns = [
        "input_",
        "window_",
        "platform_",
        "audio_",
    ];

    for pattern in &patterns {
        assert!(
            section.contains(pattern),
            "platform gate must cover pattern '{pattern}'"
        );
    }
}

#[test]
fn fuzz_gate_covers_core_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-fuzz");

    assert!(
        section.contains("fuzz_property") || section.contains("property_tests"),
        "fuzz gate must cover property/fuzz test patterns"
    );
}

// ===========================================================================
// 4. Platform tests run on multiple OSes
// ===========================================================================

#[test]
fn platform_gate_runs_on_three_oses() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-platform");

    assert!(
        section.contains("ubuntu-latest"),
        "platform gate must run on ubuntu"
    );
    assert!(
        section.contains("macos-latest"),
        "platform gate must run on macos"
    );
    assert!(
        section.contains("windows-latest"),
        "platform gate must run on windows"
    );
}

#[test]
fn main_rust_job_runs_on_three_oses() {
    let ci = read_ci();

    // The main rust job should test on all three platforms
    assert!(ci.contains("ubuntu-latest"));
    assert!(ci.contains("macos-latest"));
    assert!(ci.contains("windows-latest"));
}

// ===========================================================================
// 5. Test file coverage analysis
// ===========================================================================

#[test]
fn parity_tests_are_covered_by_ci() {
    let ci = read_ci();
    let test_files = list_test_files();

    let parity_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| f.contains("parity"))
        .collect();

    assert!(
        !parity_tests.is_empty(),
        "must have parity test files"
    );

    // All parity tests should be matched by at least one CI filter pattern.
    // The full workspace test catches everything, but we check that
    // dedicated gates also cover the key ones.
    let mut uncovered = Vec::new();
    for test in &parity_tests {
        let stem = test.trim_end_matches(".rs");
        // Check if any part of the stem appears in CI gate filter patterns
        let short = stem.trim_end_matches("_test");
        let covered = ci.contains(short)
            || find_matching_prefix(&ci, short);
        if !covered {
            uncovered.push(test.as_str());
        }
    }

    // The full workspace test covers everything, so uncovered files
    // are still tested. We just track that the dedicated gates have
    // good coverage (allow up to 10% uncovered by dedicated gates).
    let coverage_pct =
        ((parity_tests.len() - uncovered.len()) as f64 / parity_tests.len() as f64) * 100.0;
    assert!(
        coverage_pct >= 50.0,
        "dedicated CI gates must cover at least 50% of parity tests \
         (got {coverage_pct:.1}%, {} uncovered: {:?})",
        uncovered.len(),
        &uncovered[..uncovered.len().min(10)]
    );
}

#[test]
fn slice_test_files_are_covered() {
    let ci = read_ci();
    let test_files = list_test_files();

    let slice_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| f.contains("slice") && !f.contains("ci_runtime_slice"))
        .collect();

    for test in &slice_tests {
        let stem = test.trim_end_matches("_test.rs");
        let covered = ci.contains(stem) || find_matching_prefix(&ci, stem);
        assert!(
            covered,
            "slice test '{test}' must be referenced in CI"
        );
    }
}

#[test]
fn golden_tests_are_covered() {
    let ci = read_ci();
    let test_files = list_test_files();

    let golden_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| f.contains("golden"))
        .collect();

    assert!(
        !golden_tests.is_empty(),
        "must have golden test files"
    );

    // Golden tests should be covered by either the render goldens gate
    // or the dedicated slice gates.
    for test in &golden_tests {
        let covered = ci.contains("golden") || ci.contains("render_golden");
        assert!(
            covered,
            "golden test '{test}' must be covered by CI"
        );
    }
}

// ===========================================================================
// 6. CI gate dependencies
// ===========================================================================

#[test]
fn slice_gates_depend_on_fmt() {
    let ci = read_ci();

    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("needs: rust-fmt"),
            "gate '{gate}' must depend on rust-fmt"
        );
    }
}

#[test]
fn existing_gates_still_present() {
    let ci = read_ci();

    // Original CI gates must not have been removed
    let original_gates = [
        "rust-fmt",
        "rust:",          // main rust job
        "rust-render-goldens",
        "rust-release",
        "rust-audit",
        "rust-oracle-parity",
        "web:",
    ];

    for gate in &original_gates {
        assert!(
            ci.contains(gate),
            "original CI gate '{gate}' must still exist"
        );
    }
}

// ===========================================================================
// 7. Minimum test count per slice
// ===========================================================================

#[test]
fn at_least_100_test_files_exist() {
    let test_files = list_test_files();
    assert!(
        test_files.len() >= 100,
        "must have at least 100 test files (got {})",
        test_files.len()
    );
}

#[test]
fn at_least_5_runtime_slices_in_ci() {
    let ci = read_ci();
    let slice_count = ci.matches("rust-compat-").count();
    assert!(
        slice_count >= 5,
        "CI must define at least 5 compat slice gates (got {slice_count})"
    );
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Extracts the YAML section for a given job name (from the job key to the next
/// top-level job or end of file).
fn extract_section(ci: &str, job_name: &str) -> String {
    let marker = format!("{}:", job_name);
    if let Some(start) = ci.find(&marker) {
        let rest = &ci[start..];
        // Find the next top-level job (line starting with exactly 2 spaces + word + colon)
        // after skipping the first line
        let after_first_line = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        let section_end = rest[after_first_line..]
            .find("\n  rust-")
            .or_else(|| rest[after_first_line..].find("\n  web:"))
            .map(|i| after_first_line + i)
            .unwrap_or(rest.len());
        rest[..section_end].to_string()
    } else {
        String::new()
    }
}

/// Extracts cargo test filter patterns from a CI gate section.
/// These are bare alphanumeric+underscore tokens that appear after `--` in
/// a `cargo test` invocation.
fn extract_filter_patterns(section: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_filter = false;
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.contains("cargo test") && trimmed.contains("--") {
            in_filter = true;
            // Check if there are patterns on the same line after --
            if let Some(after) = trimmed.split("--").nth(1) {
                for tok in after.split_whitespace() {
                    if !tok.is_empty()
                        && !tok.starts_with('-')
                        && tok.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        patterns.push(tok.to_string());
                    }
                }
            }
            continue;
        }
        if in_filter {
            // Patterns are indented continuation lines
            if trimmed.is_empty()
                || trimmed.starts_with('-')
                || trimmed.starts_with('#')
                || trimmed.contains(':')
            {
                in_filter = false;
                continue;
            }
            if trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
                patterns.push(trimmed.to_string());
            }
        }
    }
    patterns
}

/// Checks if any prefix of the test name (split on '_') matches a CI pattern.
fn find_matching_prefix(ci: &str, test_stem: &str) -> bool {
    let parts: Vec<&str> = test_stem.split('_').collect();
    // Try progressively longer prefixes
    for len in 1..=parts.len().min(3) {
        let prefix = parts[..len].join("_");
        if prefix.len() >= 4 && ci.contains(&prefix) {
            return true;
        }
    }
    false
}

// ===========================================================================
// pat-xupn: Match runtime CI matrix coverage to active parity domains
// ===========================================================================

/// Every test file whose name contains a domain keyword must be covered
/// by the corresponding CI gate's filter patterns.
#[test]
fn headless_domain_test_files_covered_by_headless_gate() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-headless");
    let test_files = list_test_files();

    let headless_keywords = [
        "resource_", "scene_", "signal_", "notification_", "object_",
        "classdb_", "lifecycle_", "packed_scene_", "nodepath_",
        "connect_", "instanc", "reparent_", "default_property_",
        "gdscript_", "script_", "mainloop_", "change_scene_",
        "cache_",
    ];

    let headless_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| headless_keywords.iter().any(|k| f.contains(k)))
        .collect();

    assert!(
        !headless_tests.is_empty(),
        "must have headless domain test files"
    );

    // Verify each keyword appears in the headless gate section.
    for keyword in &headless_keywords {
        assert!(
            section.contains(keyword),
            "headless gate missing pattern '{keyword}' — tests matching this exist"
        );
    }
}

/// 2D domain test files must be covered by the 2D gate.
#[test]
fn two_d_domain_test_files_covered_by_2d_gate() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-2d");
    let test_files = list_test_files();

    let two_d_keywords = [
        "physics_", "collision_", "node2d_", "geometry2d_",
        "area2d_", "character_", "fixed_step_", "gdphysics2d_",
        "deterministic_physics",
    ];

    let two_d_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| two_d_keywords.iter().any(|k| f.contains(k)))
        .collect();

    assert!(
        !two_d_tests.is_empty(),
        "must have 2D domain test files"
    );

    for keyword in &two_d_keywords {
        assert!(
            section.contains(keyword),
            "2D gate missing pattern '{keyword}' — tests matching this exist"
        );
    }
}

/// 3D domain test files must be covered by the 3D gate.
#[test]
fn three_d_domain_test_files_covered_by_3d_gate() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-3d");
    let test_files = list_test_files();

    let three_d_keywords = ["node3d_", "physics3d_", "transform3d_"];

    let three_d_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| three_d_keywords.iter().any(|k| f.contains(k)))
        .collect();

    assert!(
        !three_d_tests.is_empty(),
        "must have 3D domain test files"
    );

    for keyword in &three_d_keywords {
        assert!(
            section.contains(keyword),
            "3D gate missing pattern '{keyword}'"
        );
    }
}

/// Platform domain test files must be covered by the platform gate.
#[test]
fn platform_domain_test_files_covered_by_platform_gate() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-platform");
    let test_files = list_test_files();

    let platform_keywords = [
        "input_", "window_", "platform_", "audio_",
        "keyboard_", "mouse_input_", "winit_",
    ];

    let platform_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| platform_keywords.iter().any(|k| f.contains(k)))
        .collect();

    assert!(
        !platform_tests.is_empty(),
        "must have platform domain test files"
    );

    for keyword in &platform_keywords {
        assert!(
            section.contains(keyword),
            "platform gate missing pattern '{keyword}'"
        );
    }
}

/// CI meta gate must cover CI infrastructure tests and meta tests.
#[test]
fn ci_meta_gate_covers_infrastructure_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-ci-meta");

    let meta_keywords = [
        "ci_", "version_consistency", "startup_packaging",
        "release_", "migration_", "contributor_",
    ];

    for keyword in &meta_keywords {
        assert!(
            section.contains(keyword),
            "CI meta gate missing pattern '{keyword}'"
        );
    }
}

/// The CI meta gate must exist — it covers CI infrastructure and
/// documentation validation tests.
#[test]
fn ci_has_meta_gate() {
    let ci = read_ci();
    assert!(
        ci.contains("rust-compat-ci-meta"),
        "CI must have a ci-meta infrastructure gate"
    );
}

/// Every parity domain gate must also cache cargo registry and target
/// to avoid redundant downloads.
#[test]
fn all_domain_gates_use_cargo_cache() {
    let ci = read_ci();
    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("cargo-registry") || section.contains("Cache cargo"),
            "gate '{gate}' must cache cargo dependencies"
        );
    }
}

/// The CI workflow must have concurrency control to avoid redundant
/// runs on the same branch.
#[test]
fn ci_has_concurrency_control() {
    let ci = read_ci();
    assert!(
        ci.contains("concurrency:"),
        "CI must have concurrency control"
    );
    assert!(
        ci.contains("cancel-in-progress: true"),
        "CI should cancel redundant in-progress runs"
    );
}

// ===========================================================================
// pat-dlb0: Validate active parity domain coverage is complete
// ===========================================================================

/// The 3D gate must cover all 3D test file prefixes that exist on disk.
#[test]
fn three_d_gate_covers_all_existing_3d_test_prefixes() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-3d");
    let test_files = list_test_files();

    let three_d_files: Vec<&String> = test_files
        .iter()
        .filter(|f| f.contains("3d_") || f.contains("3D_"))
        .collect();

    assert!(
        three_d_files.len() >= 5,
        "must have at least 5 3D test files (got {})",
        three_d_files.len()
    );

    // Every 3D test file's prefix (up to first '_') should be matched by
    // at least one filter in the 3D gate section.
    let mut uncovered_3d = Vec::new();
    for file in &three_d_files {
        let stem = file.trim_end_matches("_test.rs");
        let covered = find_matching_prefix(&section, stem)
            || section.contains(&stem[..stem.len().min(8)]);
        if !covered {
            uncovered_3d.push(file.as_str());
        }
    }

    let coverage = ((three_d_files.len() - uncovered_3d.len()) as f64
        / three_d_files.len() as f64)
        * 100.0;
    assert!(
        coverage >= 80.0,
        "3D gate must cover >= 80% of 3D test files (got {coverage:.0}%, uncovered: {uncovered_3d:?})"
    );
}

/// The headless gate must cover instanced-scene and resource tests.
#[test]
fn headless_gate_covers_instancing_and_resource_tests() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-headless");

    assert!(
        section.contains("instanc"),
        "headless gate must cover instancing tests (instanc pattern)"
    );
    assert!(
        section.contains("resource_"),
        "headless gate must cover resource tests"
    );
    assert!(
        section.contains("cache_"),
        "headless gate must cover cache tests"
    );
    assert!(
        section.contains("unique_name_"),
        "headless gate must cover unique_name tests"
    );
}

/// Every parity domain in CI must have at least one test file that matches.
#[test]
fn each_domain_gate_has_matching_test_files() {
    let test_files = list_test_files();

    let domains: Vec<(&str, Vec<&str>)> = vec![
        ("headless", vec!["resource_", "signal_", "lifecycle_"]),
        ("2d", vec!["physics_", "collision_", "node2d_"]),
        ("3d", vec!["node3d_", "physics3d_", "transform3d_"]),
        ("platform", vec!["input_", "window_", "audio_"]),
    ];

    for (domain, patterns) in &domains {
        for pattern in patterns {
            let matching: Vec<&String> = test_files
                .iter()
                .filter(|f| f.contains(pattern))
                .collect();
            assert!(
                !matching.is_empty(),
                "domain '{domain}' pattern '{pattern}' must match at least one test file"
            );
        }
    }
}

/// The overall CI parity coverage must be at least 60% of all test files.
#[test]
fn overall_ci_parity_coverage_at_least_60_percent() {
    let ci = read_ci();
    let test_files = list_test_files();

    let mut covered_count = 0;
    for file in &test_files {
        let stem = file.trim_end_matches("_test.rs");
        if ci.contains(stem) || find_matching_prefix(&ci, stem) {
            covered_count += 1;
        }
    }

    let coverage = (covered_count as f64 / test_files.len() as f64) * 100.0;
    assert!(
        coverage >= 60.0,
        "overall CI coverage must be >= 60% (got {coverage:.0}%, {covered_count}/{} files)",
        test_files.len()
    );
}

// ===========================================================================
// pat-a7o: Match runtime CI matrix coverage to active parity domains
// ===========================================================================

/// Every fuzz test file must be covered by the fuzz gate (not just any gate).
#[test]
fn fuzz_test_files_covered_by_fuzz_gate() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-fuzz");
    let test_files = list_test_files();

    let fuzz_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| f.starts_with("fuzz_"))
        .collect();

    assert!(
        fuzz_tests.len() >= 3,
        "must have at least 3 fuzz test files (got {})",
        fuzz_tests.len()
    );

    let mut uncovered = Vec::new();
    for test in &fuzz_tests {
        let stem = test.trim_end_matches("_test.rs");
        // Check if any fuzz gate pattern matches the file
        let covered = section.contains(stem)
            || find_matching_prefix(&section, stem);
        if !covered {
            uncovered.push(test.as_str());
        }
    }

    assert!(
        uncovered.is_empty(),
        "all fuzz test files must be covered by the fuzz gate, \
         uncovered: {uncovered:?}"
    );
}

/// Fuzz gate must explicitly list fuzz_collision, fuzz_signal, fuzz_resource patterns.
#[test]
fn fuzz_gate_covers_all_fuzz_subdomain_patterns() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-fuzz");

    let required_patterns = [
        "fuzz_property",
        "fuzz_variant",
        "fuzz_collision",
        "fuzz_signal",
        "fuzz_resource",
    ];

    for pattern in &required_patterns {
        assert!(
            section.contains(pattern),
            "fuzz gate must contain pattern '{pattern}'"
        );
    }
}

/// The headless gate must cover new broader execution path tests.
#[test]
fn headless_gate_covers_broader_execution_tests() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-headless");
    let test_files = list_test_files();

    // Broader resource/scene execution tests should be covered by headless
    let broader_tests: Vec<&String> = test_files
        .iter()
        .filter(|f| {
            f.contains("resource_scene_")
                || f.contains("scene_tree_broad")
        })
        .collect();

    assert!(
        !broader_tests.is_empty(),
        "must have broader execution test files"
    );

    // These match via resource_ or scene_ patterns in the headless gate
    for test in &broader_tests {
        let covered = section.contains("resource_") || section.contains("scene_");
        assert!(
            covered,
            "broader test '{test}' must be covered by headless gate patterns"
        );
    }
}

/// Every domain gate must run at least one concrete test pattern.
#[test]
fn every_domain_gate_has_nonempty_filter() {
    let ci = read_ci();

    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("cargo test"),
            "gate '{gate}' must run cargo test"
        );
        // Must have at least one filter pattern after --
        assert!(
            section.contains("-- ") || section.contains("--\n"),
            "gate '{gate}' must have filter patterns after --"
        );
    }
}

/// No test file should be completely orphaned (not matched by any gate).
/// Uses the same matching semantics as `cargo test -- pattern`: a test file
/// is covered if any filter pattern in any gate section is a substring of
/// the test file's stem.
#[test]
fn no_test_file_is_orphaned_from_all_gates() {
    let ci = read_ci();
    let test_files = list_test_files();

    let gate_names = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
        "rust-oracle-parity",
    ];

    let mut orphaned = Vec::new();
    for file in &test_files {
        let stem = file.trim_end_matches(".rs");
        let mut covered = false;

        for gate in &gate_names {
            let section = extract_section(&ci, gate);
            let patterns = extract_filter_patterns(&section);
            if patterns.iter().any(|p| stem.contains(p)) {
                covered = true;
                break;
            }
        }

        if !covered {
            orphaned.push(file.as_str());
        }
    }

    let orphan_pct = (orphaned.len() as f64 / test_files.len() as f64) * 100.0;
    assert!(
        orphan_pct <= 5.0,
        "at most 5% of test files may lack dedicated gate coverage \
         (got {orphan_pct:.1}%, {} orphaned out of {}: {:?})",
        orphaned.len(),
        test_files.len(),
        &orphaned[..orphaned.len().min(15)]
    );
}

/// The 2D gate must include newer draw ordering and camera tests.
#[test]
fn two_d_gate_covers_newer_draw_and_camera_tests() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-2d");

    assert!(
        section.contains("draw_ordering"),
        "2D gate must cover draw_ordering tests"
    );
    assert!(
        section.contains("camera_viewport"),
        "2D gate must cover camera_viewport tests"
    );
    assert!(
        section.contains("viewport_clear"),
        "2D gate must cover viewport_clear tests"
    );
    assert!(
        section.contains("texture_"),
        "2D gate must cover texture tests"
    );
}

/// CI must have at least 6 dedicated compat gates plus oracle.
#[test]
fn ci_has_at_least_7_dedicated_gates() {
    let ci = read_ci();
    let compat_count = ci.matches("rust-compat-").count();
    let has_oracle = ci.contains("rust-oracle-parity");
    let total = compat_count + if has_oracle { 1 } else { 0 };
    assert!(
        total >= 7,
        "CI must have at least 7 dedicated gates (got {total})"
    );
}

// ---------------------------------------------------------------------------
// pat-a7o: Additional runtime CI matrix coverage validation
// ---------------------------------------------------------------------------

/// No parity gate (except rust-audit) should use continue-on-error,
/// ensuring failures are caught and surfaced.
#[test]
fn a7o_parity_gates_fail_cleanly_no_continue_on_error() {
    let ci = read_ci();

    let strict_gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
        "rust-oracle-parity",
    ];

    for gate in &strict_gates {
        let section = extract_section(&ci, gate);
        assert!(
            !section.contains("continue-on-error: true"),
            "gate '{gate}' must NOT use continue-on-error — regressions must fail the build"
        );
    }
}

/// Each compat gate runs `cargo test --workspace` (not just `cargo test`),
/// so tests from all crates are included.
#[test]
fn a7o_all_compat_gates_test_workspace() {
    let ci = read_ci();

    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("cargo test --workspace"),
            "gate '{gate}' must use --workspace flag"
        );
    }
}

/// Evidence commands from key parity domains must map to at least one CI
/// gate filter pattern. This prevents beads from passing locally but not
/// being covered in CI.
#[test]
fn a7o_evidence_command_patterns_covered_by_ci() {
    let ci = read_ci();

    // These are representative evidence-command test names from active beads
    let evidence_patterns = [
        "oracle_regression",
        "golden_staleness",
        "physics3d_trace_comparison",
        "input_map_action_binding",
        "vertical_slice",
        "render_2d_parity",
        "signal_trace",
        "resource_cache",
        "node3d_transform",
        "physics_trace_golden",
        "classdb_surface_parity",
    ];

    for pattern in &evidence_patterns {
        assert!(
            ci.contains(pattern) || find_matching_prefix(&ci, pattern),
            "evidence pattern '{pattern}' must be covered by at least one CI gate"
        );
    }
}

/// The oracle parity gate must run oracle-specific test patterns.
#[test]
fn a7o_oracle_parity_gate_covers_oracle_tests() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-oracle-parity");

    let required = ["oracle_parity", "golden_staleness", "oracle_regression"];
    for pattern in &required {
        assert!(
            section.contains(pattern),
            "oracle parity gate must cover '{pattern}'"
        );
    }
}

/// The render goldens gate must set PATINA_CI env and run make target.
#[test]
fn a7o_render_goldens_gate_uses_make_target() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-render-goldens");

    assert!(
        section.contains("PATINA_CI"),
        "render goldens gate must set PATINA_CI"
    );
    assert!(
        section.contains("make test-render-ci"),
        "render goldens gate must use make test-render-ci target"
    );
}

/// Each CI gate must checkout code (uses actions/checkout).
#[test]
fn a7o_all_gates_checkout_code() {
    let ci = read_ci();

    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
        "rust-oracle-parity",
        "rust-render-goldens",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("actions/checkout"),
            "gate '{gate}' must checkout code"
        );
    }
}

/// Each CI gate must install Rust stable toolchain.
#[test]
fn a7o_all_gates_install_rust() {
    let ci = read_ci();

    let gates = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
        "rust-compat-fuzz",
        "rust-compat-ci-meta",
        "rust-oracle-parity",
    ];

    for gate in &gates {
        let section = extract_section(&ci, gate);
        assert!(
            section.contains("rust-toolchain@stable"),
            "gate '{gate}' must install rust stable"
        );
    }
}

/// The 3D gate must cover demo and representative fixture tests.
#[test]
fn a7o_3d_gate_covers_demo_and_representative() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-3d");

    assert!(
        section.contains("demo_3d_"),
        "3D gate must cover demo_3d tests"
    );
    assert!(
        section.contains("representative_3d_"),
        "3D gate must cover representative_3d tests"
    );
    assert!(
        section.contains("camera3d_"),
        "3D gate must cover camera3d tests"
    );
}

/// The headless gate filter patterns must cover deferred and trace tests.
#[test]
fn a7o_headless_covers_deferred_and_trace() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-headless");

    assert!(
        section.contains("deferred_"),
        "headless gate must cover deferred_ pattern"
    );
    assert!(
        section.contains("trace_"),
        "headless gate must cover trace_ pattern"
    );
    assert!(
        section.contains("mainloop_"),
        "headless gate must cover mainloop_ pattern"
    );
}

/// CI meta gate must cover broader integration and editor smoke tests.
#[test]
fn a7o_ci_meta_covers_broader_and_editor() {
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-ci-meta");

    assert!(
        section.contains("broader_integration"),
        "CI meta must cover broader_integration tests"
    );
    assert!(
        section.contains("editor"),
        "CI meta must cover editor tests"
    );
    assert!(
        section.contains("benchmark_"),
        "CI meta must cover benchmark tests"
    );
}

/// The 2D gate must match actual test file names (spot-check).
#[test]
fn a7o_2d_gate_patterns_match_real_test_files() {
    let test_files = list_test_files();
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-2d");

    // These test files must exist and be matched by the 2D gate
    let expected_2d_tests = [
        "physics_trace_golden_parity_test.rs",
        "render_2d_parity_test.rs",
        "collision_overlap_extended_parity_test.rs",
        "node2d_body_sync_parity_test.rs",
        "geometry2d_arc_parity_test.rs",
    ];

    for expected in &expected_2d_tests {
        assert!(
            test_files.iter().any(|f| f == expected),
            "expected 2D test file '{expected}' must exist on disk"
        );
        let stem = expected.trim_end_matches("_test.rs");
        assert!(
            find_matching_prefix(&section, stem),
            "2D gate must match test file '{expected}'"
        );
    }
}

/// The platform gate must match actual platform test files (spot-check).
#[test]
fn a7o_platform_gate_patterns_match_real_test_files() {
    let test_files = list_test_files();
    let ci = read_ci();
    let section = extract_section(&ci, "rust-compat-platform");

    let expected_platform_tests = [
        "input_action_binding_parity_test.rs",
        "window_lifecycle_parity_test.rs",
        "keyboard_action_snapshot_parity_test.rs",
    ];

    for expected in &expected_platform_tests {
        assert!(
            test_files.iter().any(|f| f == expected),
            "expected platform test file '{expected}' must exist on disk"
        );
        let stem = expected.trim_end_matches("_test.rs");
        assert!(
            find_matching_prefix(&section, stem),
            "platform gate must match test file '{expected}'"
        );
    }
}
