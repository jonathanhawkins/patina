//! pat-2y541: Acceptance gate for fuzz/property coverage on high-risk
//! runtime surfaces.
//!
//! Acceptance:
//!   * at least one high-risk surface gains fuzz or property coverage,
//!   * the suite is wired into a documented local or CI command.
//!
//! High-risk surfaces (sourced from `prd/PHASE9_HARDENING_AUDIT.md` and the
//! sibling coverage gate `fuzz_property_coverage_test.rs`): variant
//! serialization, resource loading, GDScript parsing, math property testing.
//! This test asserts at least one of these surfaces has a checked-in fuzz or
//! property module *and* that the documented `cargo test $WS -- fuzz_property`
//! CI command is published in `.github/workflows/ci.yml`.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn at_least_one_high_risk_surface_has_fuzz_or_property_coverage() {
    let candidates = [
        ("variant serialization", "engine-rs/crates/gdvariant/src/fuzz_variant.rs"),
        ("resource loading", "engine-rs/crates/gdresource/src/fuzz_res.rs"),
        ("GDScript parsing", "engine-rs/crates/gdscript-interop/src/fuzz_gdscript.rs"),
        ("math property testing", "engine-rs/crates/gdcore/src/property_testing.rs"),
    ];
    let covered: Vec<&str> = candidates
        .iter()
        .filter(|(_, p)| repo_root().join(p).exists())
        .map(|(name, _)| *name)
        .collect();
    assert!(
        !covered.is_empty(),
        "at least one high-risk surface must have a fuzz/property module \
         (checked: {candidates:?})"
    );
}

#[test]
fn fuzz_property_suite_wired_to_ci_command() {
    let ci = repo_root().join(".github/workflows/ci.yml");
    let body = std::fs::read_to_string(&ci)
        .unwrap_or_else(|e| panic!(".github/workflows/ci.yml must exist at {}: {e}", ci.display()));
    assert!(
        body.contains("Fuzz / property tests"),
        "CI must declare a 'Fuzz / property tests' lane"
    );
    assert!(
        body.contains("cargo test $WS -- fuzz_property"),
        "CI lane must invoke the documented `cargo test $WS -- fuzz_property` command"
    );
}

#[test]
fn fuzz_property_coverage_gate_anchor_exists() {
    // The sibling gate that this bead depends on for a "suite" (it iterates
    // every high-risk surface). Guard its presence so a regression that
    // strips the gate file fails under pat-2y541's marker.
    let p = repo_root().join("engine-rs/tests/fuzz_property_coverage_test.rs");
    assert!(
        p.exists(),
        "fuzz/property coverage gate must exist at {}",
        p.display()
    );
    let body = std::fs::read_to_string(&p).unwrap();
    assert!(
        body.contains("FUZZ_MODULES"),
        "coverage gate must enumerate FUZZ_MODULES — the canonical surface list"
    );
}
