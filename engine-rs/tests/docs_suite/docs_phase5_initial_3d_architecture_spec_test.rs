//! pat-1ttrk: Acceptance gate for the initial 3D architecture spec.
//!
//! Acceptance: the deliverable is broken into measurable evidence with tests,
//! docs, or oracle-backed artifacts. The deliverable is the spec doc at
//! `docs/3D_ARCHITECTURE_SPEC.md`. This test guards the spec's presence and
//! load-bearing structure (status anchor, math/server/render/physics
//! subsystem coverage, named 3D crates) so a regression to a stub doc fails
//! under the bead's own marker.

use std::path::PathBuf;

fn spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("3D_ARCHITECTURE_SPEC.md")
}

fn spec_body() -> String {
    let p = spec_path();
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("3D architecture spec must exist at {}: {e}", p.display()))
}

#[test]
fn spec_file_exists_and_is_substantive() {
    let body = spec_body();
    assert!(
        body.len() > 500,
        "3D_ARCHITECTURE_SPEC.md should be substantive, got {} bytes",
        body.len()
    );
}

#[test]
fn spec_names_first_3d_crate_set() {
    let body = spec_body();
    for crate_name in ["gdserver3d", "gdrender3d", "gdphysics3d"] {
        assert!(
            body.contains(crate_name),
            "spec must name first-3D-slice crate '{crate_name}'"
        );
    }
}

#[test]
fn spec_covers_core_3d_subsystems() {
    let body = spec_body();
    for surface in ["math3d", "Vector3", "Quaternion", "Transform3D"] {
        assert!(
            body.contains(surface),
            "spec must cover core 3D math surface '{surface}'"
        );
    }
}

#[test]
fn spec_has_top_level_status_anchor() {
    let body = spec_body();
    assert!(
        body.contains("## Status") || body.contains("# Status"),
        "spec must have a top-level Status anchor describing the phase/baseline"
    );
}
