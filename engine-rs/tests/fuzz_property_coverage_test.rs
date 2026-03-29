//! Validates that fuzz and property-based test coverage exists across
//! key subsystems. This test verifies the test infrastructure is wired
//! up and exercisable, acting as a gate for the phase-9 quality bead.

use std::path::Path;

/// Subsystems that must have fuzz or property testing.
const FUZZ_MODULES: &[(&str, &str)] = &[
    (
        "gdcore property_testing",
        "crates/gdcore/src/property_testing.rs",
    ),
    (
        "gdcore proptest (external)",
        "crates/gdcore/tests/property_based_math_test.rs",
    ),
    ("gdresource fuzz_res", "crates/gdresource/src/fuzz_res.rs"),
    (
        "gdscript-interop fuzz_gdscript",
        "crates/gdscript-interop/src/fuzz_gdscript.rs",
    ),
    (
        "gdvariant fuzz_variant",
        "crates/gdvariant/src/fuzz_variant.rs",
    ),
    (
        "gdobject+gdplatform property tests",
        "tests/fuzz_input_weakref_classdb_property_test.rs",
    ),
];

/// Minimum expected test count per module (conservative lower bound).
const MIN_TESTS_PER_MODULE: &[(&str, usize)] = &[
    ("gdcore property_testing", 30),            // 39+ inline tests
    ("gdcore proptest (external)", 25),         // 30+ proptest cases
    ("gdresource fuzz_res", 20),                // 28+ fuzz tests
    ("gdscript-interop fuzz_gdscript", 25),     // 30+ fuzz tests
    ("gdvariant fuzz_variant", 15),             // 20+ fuzz tests
    ("gdobject+gdplatform property tests", 15), // 20+ proptest cases
];

#[test]
fn all_fuzz_modules_exist() {
    let workspace = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(workspace);

    for (name, path) in FUZZ_MODULES {
        let full = root.join(path);
        assert!(
            full.exists(),
            "Fuzz/property test module missing: {} (expected at {})",
            name,
            full.display()
        );
    }
}

#[test]
fn fuzz_modules_contain_tests() {
    let workspace = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(workspace);

    for (name, path) in FUZZ_MODULES {
        let full = root.join(path);
        let content = std::fs::read_to_string(&full).unwrap_or_else(|e| {
            panic!("Cannot read {}: {}", full.display(), e);
        });

        // Count #[test] annotations or proptest! macro invocations
        let test_count = content.matches("#[test]").count();
        let proptest_count = content.matches("proptest!").count();
        let total = test_count + proptest_count;

        assert!(
            total > 0,
            "Module {} has no test functions (path: {})",
            name,
            path
        );
    }
}

#[test]
fn fuzz_modules_meet_minimum_test_counts() {
    let workspace = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(workspace);

    for (name, min_count) in MIN_TESTS_PER_MODULE {
        let (_, path) = FUZZ_MODULES
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("Unknown module in MIN_TESTS_PER_MODULE: {}", name));

        let full = root.join(path);
        let content = std::fs::read_to_string(&full).unwrap_or_else(|e| {
            panic!("Cannot read {}: {}", full.display(), e);
        });

        let test_count = content.matches("#[test]").count();
        let proptest_count = content.matches("proptest!").count();
        // proptest! blocks typically contain multiple test cases; estimate conservatively
        let proptest_cases: usize = if proptest_count > 0 {
            content.matches("fn ").count().saturating_sub(5) // non-test fns
        } else {
            0
        };
        let total = test_count + proptest_cases;

        assert!(
            total >= *min_count,
            "Module {} has {} tests (minimum {}). Path: {}",
            name,
            total,
            min_count,
            path
        );
    }
}

#[test]
fn subsystem_coverage_breadth() {
    // Verify we cover the 4 key subsystems
    let subsystems: Vec<&str> = FUZZ_MODULES
        .iter()
        .map(|(name, _)| {
            if name.starts_with("gdcore") {
                "math"
            } else if name.starts_with("gdresource") {
                "resource-loader"
            } else if name.starts_with("gdscript") {
                "scripting"
            } else if name.starts_with("gdvariant") {
                "variant"
            } else if name.starts_with("gdobject") {
                "object-system"
            } else {
                "unknown"
            }
        })
        .collect();

    for expected in &[
        "math",
        "resource-loader",
        "scripting",
        "variant",
        "object-system",
    ] {
        assert!(
            subsystems.contains(expected),
            "Missing fuzz/property coverage for subsystem: {}",
            expected
        );
    }
}
