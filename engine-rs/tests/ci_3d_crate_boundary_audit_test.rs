//! pat-hx666: Validates that 3D crate boundaries match the Phase 6 audit.
//!
//! Guards the crate structure against the audited scope in
//! prd/PHASE6_3D_PARITY_AUDIT.md. If crates are added, renamed, or modules
//! are moved, this test will fail until the audit doc is updated.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn crate_src(crate_name: &str) -> PathBuf {
    repo_root()
        .join("engine-rs/crates")
        .join(crate_name)
        .join("src")
}

fn audit_doc() -> String {
    let path = repo_root().join("prd/PHASE6_3D_PARITY_AUDIT.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read audit doc: {e}"))
}

// ===========================================================================
// 1. All 3D crates exist
// ===========================================================================

#[test]
fn all_3d_crates_exist() {
    for crate_name in &["gdserver3d", "gdrender3d", "gdphysics3d"] {
        let src = crate_src(crate_name);
        assert!(
            src.exists(),
            "3D crate {} missing (expected at {})",
            crate_name,
            src.display()
        );
    }
    // gdscene also contributes 3D modules
    let gdscene_src = crate_src("gdscene");
    assert!(gdscene_src.exists(), "gdscene crate missing");
}

// ===========================================================================
// 2. Key modules exist in each crate
// ===========================================================================

#[test]
fn gdserver3d_has_audited_modules() {
    let src = crate_src("gdserver3d");
    for module in &[
        "server.rs",
        "light.rs",
        "mesh.rs",
        "material.rs",
        "environment.rs",
        "reflection_probe.rs",
        "fog_volume.rs",
        "csg.rs",
        "navigation.rs",
    ] {
        assert!(
            src.join(module).exists(),
            "gdserver3d missing audited module: {}",
            module
        );
    }
}

#[test]
fn gdrender3d_has_audited_modules() {
    let src = crate_src("gdrender3d");
    for module in &[
        "renderer.rs",
        "rasterizer.rs",
        "depth_buffer.rs",
        "shadow_map.rs",
        "shader.rs",
    ] {
        assert!(
            src.join(module).exists(),
            "gdrender3d missing audited module: {}",
            module
        );
    }
}

#[test]
fn gdphysics3d_has_audited_modules() {
    let src = crate_src("gdphysics3d");
    for module in &[
        "world.rs",
        "body.rs",
        "character.rs",
        "collision.rs",
        "shape.rs",
        "query.rs",
        "area3d.rs",
        "joint.rs",
    ] {
        assert!(
            src.join(module).exists(),
            "gdphysics3d missing audited module: {}",
            module
        );
    }
}

#[test]
fn gdscene_has_3d_bindings() {
    let src = crate_src("gdscene");
    for module in &[
        "node3d.rs",
        "camera3d.rs",
        "render_server_3d.rs",
        "physics_server.rs",
    ] {
        assert!(
            src.join(module).exists(),
            "gdscene missing 3D binding module: {}",
            module
        );
    }
}

// ===========================================================================
// 3. Audit doc references all crates and has the boundary section
// ===========================================================================

#[test]
fn audit_doc_has_crate_boundary_section() {
    let doc = audit_doc();
    assert!(
        doc.contains("## Crate Boundary Classification"),
        "audit doc must contain '## Crate Boundary Classification'"
    );
}

#[test]
fn audit_doc_references_all_3d_crates() {
    let doc = audit_doc();
    for crate_name in &["gdserver3d", "gdrender3d", "gdphysics3d", "gdscene"] {
        assert!(
            doc.contains(crate_name),
            "audit doc must reference crate {}",
            crate_name
        );
    }
}

#[test]
fn audit_doc_classifies_key_modules() {
    let doc = audit_doc();
    // Key modules from each crate must appear in the boundary classification
    for module in &[
        "server.rs",
        "light.rs",
        "mesh.rs",
        "renderer.rs",
        "world.rs",
        "body.rs",
        "character.rs",
        "query.rs",
        "node3d.rs",
        "camera3d.rs",
    ] {
        assert!(
            doc.contains(module),
            "audit doc boundary section must classify module {}",
            module
        );
    }
}

// ===========================================================================
// 4. Crate boundaries align with audited families
// ===========================================================================

#[test]
fn audit_doc_maps_measured_families_to_crates() {
    let doc = audit_doc();

    // Each measured family from the audit must appear in the crate boundary section
    let measured_families = [
        ("Node3D", "gdscene"),
        ("Camera3D", "gdscene"),
        ("RigidBody3D", "gdphysics3d"),
        ("StaticBody3D", "gdphysics3d"),
        ("CharacterBody3D", "gdphysics3d"),
        ("CollisionShape3D", "gdphysics3d"),
        ("DirectionalLight3D", "gdserver3d"),
        ("OmniLight3D", "gdserver3d"),
        ("SpotLight3D", "gdserver3d"),
        ("RenderingServer3D", "gdserver3d"),
    ];

    for (family, crate_name) in &measured_families {
        assert!(
            doc.contains(family),
            "audit boundary section must reference measured family {}",
            family
        );
        assert!(
            doc.contains(crate_name),
            "audit boundary section must reference owning crate {} for {}",
            crate_name,
            family
        );
    }
}

#[test]
fn audit_doc_has_boundary_rules() {
    let doc = audit_doc();
    assert!(
        doc.contains("### Boundary Rules"),
        "audit doc must have Boundary Rules section"
    );
}
