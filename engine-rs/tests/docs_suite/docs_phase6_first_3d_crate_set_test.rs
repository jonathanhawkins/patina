//! pat-0pskw: Acceptance gate for the first Phase-6 3D crate set.
//!
//! Acceptance:
//!   * crate boundaries are documented,
//!   * Cargo manifests exist for the selected 3D slice,
//!   * the new crates compile under `cargo test --workspace --no-run`.
//!
//! Compilation is enforced transitively: this test lives in `engine-rs/tests/`
//! and links against `gdserver3d` / `gdrender3d` / `gdphysics3d`, so the test
//! binary itself only builds when the workspace builds. The other two pillars
//! are checked structurally below.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

const FIRST_3D_CRATE_SET: &[&str] = &["gdserver3d", "gdrender3d", "gdphysics3d"];

#[test]
fn cargo_manifests_exist_for_3d_slice() {
    let crates_dir = repo_root().join("engine-rs/crates");
    for name in FIRST_3D_CRATE_SET {
        let manifest = crates_dir.join(name).join("Cargo.toml");
        assert!(
            manifest.exists(),
            "Cargo manifest missing for first-3D-slice crate '{}': {}",
            name,
            manifest.display()
        );
    }
}

#[test]
fn crate_boundaries_are_documented() {
    let spec = repo_root().join("docs/3D_ARCHITECTURE_SPEC.md");
    let body = std::fs::read_to_string(&spec).unwrap_or_else(|e| {
        panic!(
            "3D architecture spec must exist at {}: {e}",
            spec.display()
        )
    });
    for name in FIRST_3D_CRATE_SET {
        assert!(
            body.contains(name),
            "docs/3D_ARCHITECTURE_SPEC.md must document boundary for crate '{name}'"
        );
    }
}

#[test]
fn first_3d_crate_set_links_into_test_binary() {
    // Linking these symbols forces the workspace to compile the 3D crate set
    // when this test binary is built — that is the operational meaning of
    // the bead's compile-under-workspace acceptance pillar.
    use gdcore::math::Color;

    let _mesh = gdserver3d::Mesh3D::cube(1.0);
    let _world = gdphysics3d::world::PhysicsWorld3D::new();
    let _frame = gdrender3d::FrameBuffer3D::new(8, 8, Color::new(0.0, 0.0, 0.0, 1.0));
}
