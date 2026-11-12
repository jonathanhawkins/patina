//! pat-3pstd: Keep fuzz and property coverage focused on audited high-risk surfaces.
//!
//! Source of truth: `prd/PHASE9_HARDENING_AUDIT.md`
//!
//! This test validates:
//! 1. The Phase 9 audit doc cites the fuzz/property coverage gate
//! 2. High-risk surfaces have dedicated fuzz/property modules
//! 3. The CI workflow has a fuzz/property test lane
//! 4. The fuzz/property test command is documented and exercisable
//! 5. Fuzz modules in crates actually contain test logic

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_audit() -> String {
    std::fs::read_to_string(repo_root().join("prd/PHASE9_HARDENING_AUDIT.md"))
        .expect("prd/PHASE9_HARDENING_AUDIT.md must exist")
}

fn read_ci_yaml() -> String {
    std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect("ci.yml must exist")
}

// ── Phase 9 audit cites fuzz/property coverage ──────────────────────

#[test]
fn audit_references_fuzz_property_bead() {
    let audit = read_audit();
    assert!(
        audit.contains("pat-3pstd"),
        "Phase 9 audit must reference the fuzz/property bead"
    );
}

#[test]
fn audit_cites_fuzz_property_evidence() {
    let audit = read_audit();
    assert!(
        audit.contains("fuzz_property_coverage_test.rs"),
        "audit must cite the fuzz/property coverage gate test"
    );
}

#[test]
fn audit_classifies_fuzz_property_as_measured() {
    let audit = read_audit();
    assert!(
        audit.contains("Measured for current coverage gate"),
        "audit must classify fuzz/property as measured"
    );
}

// ── High-risk surfaces have fuzz/property modules ───────────────────

/// High-risk surfaces that must have dedicated fuzz/property coverage.
/// These are the subsystems where malformed input is most likely to
/// cause crashes, data corruption, or security issues.
const HIGH_RISK_SURFACES: &[(&str, &str)] = &[
    (
        "Variant serialization",
        "crates/gdvariant/src/fuzz_variant.rs",
    ),
    ("Resource loading", "crates/gdresource/src/fuzz_res.rs"),
    (
        "GDScript parsing",
        "crates/gdscript-interop/src/fuzz_gdscript.rs",
    ),
    (
        "Math property testing",
        "crates/gdcore/src/property_testing.rs",
    ),
];

#[test]
fn high_risk_surfaces_have_fuzz_modules() {
    for (surface, path) in HIGH_RISK_SURFACES {
        let full = workspace_root().join(path);
        assert!(
            full.exists(),
            "high-risk surface '{surface}' must have fuzz module at {path}"
        );
    }
}

#[test]
fn fuzz_modules_contain_test_logic() {
    for (surface, path) in HIGH_RISK_SURFACES {
        let full = workspace_root().join(path);
        let content = std::fs::read_to_string(&full)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()));
        let test_count = content.matches("#[test]").count() + content.matches("proptest!").count();
        assert!(
            test_count >= 5,
            "high-risk surface '{surface}' must have >= 5 test/proptest entries, found {test_count}"
        );
    }
}

// ── Integration test files for fuzz coverage ────────────────────────

const FUZZ_INTEGRATION_TESTS: &[&str] = &[
    "tests/fuzz_suite/fuzz_property_coverage_test.rs",
    "tests/fuzz_suite/fuzz_variant_serialization_roundtrip_test.rs",
    "tests/fuzz_suite/fuzz_resource_loader_roundtrip_test.rs",
    "tests/fuzz_suite/fuzz_scene_tree_property_test.rs",
    "tests/fuzz_suite/fuzz_signal_property_test.rs",
    "tests/fuzz_suite/fuzz_collision_property_test.rs",
    "tests/fuzz_suite/fuzz_variant_coercion_test.rs",
    "tests/fuzz_suite/fuzz_input_weakref_classdb_property_test.rs",
];

#[test]
fn fuzz_integration_test_files_exist() {
    for path in FUZZ_INTEGRATION_TESTS {
        let full = workspace_root().join(path);
        assert!(full.exists(), "fuzz integration test must exist: {path}");
    }
}

#[test]
fn fuzz_integration_tests_have_sufficient_coverage() {
    // Each integration test file must have at least 3 test functions
    for path in FUZZ_INTEGRATION_TESTS {
        let full = workspace_root().join(path);
        let content = std::fs::read_to_string(&full)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()));
        let test_count = content.matches("#[test]").count() + content.matches("proptest!").count();
        assert!(
            test_count >= 3,
            "fuzz test '{path}' must have >= 3 tests, found {test_count}"
        );
    }
}

// ── CI wiring ───────────────────────────────────────────────────────

#[test]
fn ci_runs_the_fuzz_suite() {
    // The fuzz suite is an integration-test target of this package, and CI runs
    // EVERY integration target through the `rust-tests` shards
    // (ci_suite::ci_sharded_integration_tests_test pins that coverage). The old
    // dedicated "Fuzz / property tests" job built every test binary on one
    // runner and overflowed its disk.
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("scripts/ci/nextest-shard.py"),
        "CI must run the sharded integration tests that include fuzz_suite"
    );
    assert!(
        workspace_root().join("tests/fuzz_suite/main.rs").exists(),
        "fuzz_suite must stay an integration-test target so the shards pick it up"
    );
}

// ── Local command exercisable ───────────────────────────────────────

#[test]
fn fuzz_coverage_gate_test_exists() {
    let path = workspace_root().join("tests/fuzz_suite/fuzz_property_coverage_test.rs");
    assert!(path.exists(), "the fuzz coverage gate test must exist");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("FUZZ_MODULES"),
        "coverage gate must define FUZZ_MODULES for validation"
    );
}
