//! pat-2abh6: Acceptance gate for the Phase-6 render and physics
//! comparison tooling.
//!
//! Acceptance: comparison tooling can ingest Patina and oracle outputs for one
//! representative 3D fixture and a checked-in test or doc cites the command
//! path. The tooling ships as `gdcore::compare3d` + `gdcore::comparison_tooling`;
//! the representative fixture is the `minimal_3d` scene with its golden oracle
//! JSON; the command path is anchored in the sibling test docstrings. This
//! test guards each anchor so any regression — module renamed, fixture
//! removed, command-path citation stripped — fails under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn comparison_tooling_modules_exist() {
    let crate_src = repo_root().join("engine-rs/crates/gdcore/src");
    for module in ["compare3d.rs", "comparison_tooling.rs"] {
        let p = crate_src.join(module);
        assert!(
            p.exists(),
            "comparison tooling module must exist: {}",
            p.display()
        );
    }
}

#[test]
fn representative_3d_fixture_and_oracle_present() {
    let root = repo_root();
    let scene = root.join("fixtures/scenes/minimal_3d.tscn");
    let oracle = root.join("fixtures/golden/scenes/minimal_3d.json");
    assert!(
        scene.exists(),
        "representative 3D fixture scene must exist at {}",
        scene.display()
    );
    assert!(
        oracle.exists(),
        "matching oracle output must exist at {}",
        oracle.display()
    );
}

#[test]
fn sibling_test_cites_command_path() {
    let p = repo_root().join("engine-rs/tests/audited_3d_comparison_tooling_test.rs");
    let body = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "audited 3D comparison tooling test must exist at {}: {e}",
            p.display()
        )
    });
    assert!(
        body.contains("cargo nextest run") && body.contains("comparison_tooling"),
        "sibling test must cite the command path (e.g. `cargo nextest run … comparison_tooling…`)"
    );
}

#[test]
fn comparison_evidence_test_set_is_present() {
    for f in [
        "engine-rs/tests/comparison_tooling_3d_test.rs",
        "engine-rs/tests/render_physics_comparison_tooling_test.rs",
        "engine-rs/tests/audited_3d_comparison_tooling_test.rs",
        "engine-rs/tests/render_3d_parity_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "comparison evidence test must exist: {}",
            p.display()
        );
    }
}
