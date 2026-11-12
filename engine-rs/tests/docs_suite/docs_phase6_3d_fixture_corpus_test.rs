//! pat-3gs3o: Acceptance gate for the Phase-6 3D fixture corpus.
//!
//! The corpus is a small, checked-in set of `.tscn` scenes plus matching
//! oracle goldens that drive the Phase-6 parity tests. This test guards the
//! corpus's *presence and shape* so the audit and parity tests upstream can
//! assume the fixtures are on disk. Class-coverage and audit-alignment are
//! validated by the sibling `phase6_3d_fixture_corpus_audit_test.rs`.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The audited Phase-6 3D fixture corpus, sourced from
/// `fixtures/patina_outputs/real_3d_demo_parity_report.json`.
const CORPUS: &[&str] = &[
    "minimal_3d",
    "hierarchy_3d",
    "indoor_3d",
    "multi_light_3d",
    "physics_3d_playground",
];

#[test]
fn corpus_tscn_fixtures_exist() {
    let root = repo_root();
    for name in CORPUS {
        let p = root.join("fixtures/scenes").join(format!("{name}.tscn"));
        assert!(p.exists(), "corpus tscn missing: {}", p.display());
    }
}

#[test]
fn corpus_golden_fixtures_exist() {
    let root = repo_root();
    for name in CORPUS {
        let p = root
            .join("fixtures/golden/scenes")
            .join(format!("{name}.json"));
        assert!(p.exists(), "corpus golden missing: {}", p.display());
    }
}

#[test]
fn corpus_aggregate_index_lists_each_fixture() {
    let root = repo_root();
    let path = root.join("fixtures/patina_outputs/real_3d_demo_parity_report.json");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("aggregate index missing at {}: {e}", path.display()));
    for name in CORPUS {
        assert!(
            body.contains(&format!("\"name\": \"{name}\"")),
            "aggregate index does not list corpus fixture: {name}"
        );
    }
}

#[test]
fn corpus_goldens_carry_3d_marker_classes() {
    let root = repo_root();
    let markers = [
        "Node3D",
        "Camera3D",
        "MeshInstance3D",
        "DirectionalLight3D",
        "OmniLight3D",
        "SpotLight3D",
        "StaticBody3D",
    ];
    for name in CORPUS {
        let p = root
            .join("fixtures/golden/scenes")
            .join(format!("{name}.json"));
        let body = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
        let has_marker = markers.iter().any(|m| body.contains(m));
        assert!(
            has_marker,
            "corpus golden '{name}' must carry at least one 3D marker class \
             from {markers:?}"
        );
    }
}

#[test]
fn corpus_size_meets_minimum() {
    assert!(
        CORPUS.len() >= 5,
        "Phase-6 corpus must have at least 5 fixtures (got {})",
        CORPUS.len()
    );
}
