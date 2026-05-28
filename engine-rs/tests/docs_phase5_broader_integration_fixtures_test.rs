//! pat-1fbpy: Acceptance gate for broader integration fixtures.
//!
//! Acceptance: the deliverable is broken into measurable evidence with tests,
//! docs, or oracle-backed artifacts. The broader-integration deliverable is
//! the set of new tscn fixtures (timer_animation, particles_multi,
//! csg_composition, nested_ui, multi_layer_2d) plus the integration test
//! that exercises them against golden JSON. This test asserts the fixtures
//! and goldens are checked in and the integration test that drives them
//! survives.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

const FIXTURES: &[&str] = &[
    "timer_animation",
    "particles_multi",
    "csg_composition",
    "nested_ui",
    "multi_layer_2d",
];

#[test]
fn broader_integration_fixture_tscns_exist() {
    let root = repo_root();
    for name in FIXTURES {
        let p = root.join("fixtures/scenes").join(format!("{name}.tscn"));
        assert!(
            p.exists(),
            "broader-integration tscn fixture missing: {}",
            p.display()
        );
    }
}

#[test]
fn broader_integration_fixture_goldens_exist() {
    let root = repo_root();
    for name in FIXTURES {
        let p = root
            .join("fixtures/golden/scenes")
            .join(format!("{name}.json"));
        assert!(
            p.exists(),
            "broader-integration golden missing: {}",
            p.display()
        );
    }
}

#[test]
fn broader_integration_test_anchor_exists() {
    let p = repo_root().join("engine-rs/tests/broader_integration_fixtures_test.rs");
    assert!(
        p.exists(),
        "broader integration test must exist at {}",
        p.display()
    );
    let body = std::fs::read_to_string(&p).unwrap();
    for name in FIXTURES {
        assert!(
            body.contains(name),
            "integration test must reference fixture '{name}'"
        );
    }
}
