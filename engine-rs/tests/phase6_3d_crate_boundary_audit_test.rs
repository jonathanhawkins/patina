//! pat-hx666: classify the supported 3D runtime slice and align crate boundaries
//! with the audited scope.
//!
//! This test validates that the 3D crate/module boundaries in the Patina engine
//! match the Phase 6 3D Parity Audit classification. The single source of truth
//! for the supported 3D slice is `prd/PHASE6_3D_PARITY_AUDIT.md`.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn crate_dir(name: &str) -> PathBuf {
    repo_root().join("engine-rs/crates").join(name)
}

fn module_exists(crate_name: &str, module: &str) -> bool {
    crate_dir(crate_name).join("src").join(module).exists()
}

// ─────────────────────────────────────────────────────────────────────
// 1. The audit document exists and is the source of truth
// ─────────────────────────────────────────────────────────────────────

#[test]
fn audit_document_exists() {
    let path = repo_root().join("prd/PHASE6_3D_PARITY_AUDIT.md");
    assert!(
        path.exists(),
        "Phase 6 3D Parity Audit must exist at prd/PHASE6_3D_PARITY_AUDIT.md"
    );
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("Phase 6 3D Parity Audit"),
        "audit doc must contain its title"
    );
    assert!(
        content.contains("Audit Rules"),
        "audit doc must contain audit rules section"
    );
    assert!(
        content.contains("Initial Phase 6 Classification"),
        "audit doc must contain the initial classification"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 2. Primary 3D crates exist with correct module structure
// ─────────────────────────────────────────────────────────────────────

#[test]
fn primary_3d_crates_exist() {
    // Per the audit: "Primary local crates"
    for crate_name in &["gdserver3d", "gdrender3d", "gdphysics3d"] {
        let dir = crate_dir(crate_name);
        assert!(
            dir.join("Cargo.toml").exists(),
            "3D crate {crate_name} must have Cargo.toml"
        );
        assert!(
            dir.join("src/lib.rs").exists(),
            "3D crate {crate_name} must have src/lib.rs"
        );
    }
    // gdscene hosts Node3D / scene-tree bindings
    assert!(
        crate_dir("gdscene").join("src/node3d.rs").exists(),
        "gdscene must have node3d module for 3D scene bindings"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 3. gdserver3d modules match the audit's measured + implemented slice
// ─────────────────────────────────────────────────────────────────────

#[test]
fn gdserver3d_has_measured_slice_modules() {
    // Measured families from the audit: mesh, light, material, environment,
    // projection (Camera3D), server trait, viewport, instance management
    let measured_modules = [
        "mesh.rs",
        "light.rs",
        "material.rs",
        "environment.rs",
        "projection.rs",
        "server.rs",
        "viewport.rs",
        "instance.rs",
        "primitive_mesh.rs",
        "shader.rs",
    ];
    for module in &measured_modules {
        assert!(
            module_exists("gdserver3d", module),
            "gdserver3d must have {module} (measured in Phase 6 audit)"
        );
    }
}

#[test]
fn gdserver3d_has_implemented_not_yet_measured_modules() {
    // Audit classifies these as "Implemented, not yet measured" or "partially measured"
    let implemented_modules = [
        ("reflection_probe.rs", "ReflectionProbe"),
        ("fog_volume.rs", "FogVolume"),
        ("multimesh.rs", "MultiMeshInstance3D"),
        ("sky.rs", "Sky resources"),
        ("csg.rs", "CSG families"),
        ("gi.rs", "GI systems"),
        ("navigation.rs", "NavigationRegion3D"),
        ("occluder.rs", "Occluder3D"),
        ("particles3d.rs", "GPUParticles3D"),
    ];
    for (module, family) in &implemented_modules {
        assert!(
            module_exists("gdserver3d", module),
            "gdserver3d must have {module} for {family} (implemented per Phase 6 audit)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 4. gdrender3d modules match the audit's render pipeline slice
// ─────────────────────────────────────────────────────────────────────

#[test]
fn gdrender3d_has_render_pipeline_modules() {
    // Audit: "Measured for bounded Phase 6 slice" via software renderer
    let required = [
        "renderer.rs",     // SoftwareRenderer3D
        "rasterizer.rs",   // triangle rasterization
        "depth_buffer.rs", // depth testing
        "shader.rs",       // vertex/fragment shaders
        "shadow_map.rs",   // shadow generation
        "compare.rs",      // framebuffer comparison for parity testing
    ];
    for module in &required {
        assert!(
            module_exists("gdrender3d", module),
            "gdrender3d must have {module} (Phase 6 render pipeline)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 5. gdphysics3d modules match the audit's physics slice
// ─────────────────────────────────────────────────────────────────────

#[test]
fn gdphysics3d_has_measured_physics_modules() {
    // Audit classifies RigidBody3D, StaticBody3D, CharacterBody3D as measured
    let measured = [
        ("body.rs", "RigidBody3D / StaticBody3D"),
        ("character.rs", "CharacterBody3D"),
        ("collision.rs", "collision detection"),
        ("shape.rs", "collision shapes"),
        ("world.rs", "PhysicsWorld3D stepping"),
        ("query.rs", "ray/shape query parameters"),
    ];
    for (module, family) in &measured {
        assert!(
            module_exists("gdphysics3d", module),
            "gdphysics3d must have {module} for {family} (measured in Phase 6 audit)"
        );
    }
}

#[test]
fn gdphysics3d_has_implemented_not_measured_modules() {
    // Audit: Area3D is "Implemented, not yet cleanly measured as runtime parity"
    // Audit: joints are "Implemented data types, deferred runtime parity"
    let implemented = [
        ("area3d.rs", "Area3D overlap and signals"),
        ("joint.rs", "physics joints (data model only)"),
    ];
    for (module, family) in &implemented {
        assert!(
            module_exists("gdphysics3d", module),
            "gdphysics3d must have {module} for {family} (implemented per Phase 6 audit)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 6. gdscene has 3D scene-tree bridge modules
// ─────────────────────────────────────────────────────────────────────

#[test]
fn gdscene_has_3d_bridge_modules() {
    // Audit: gdscene hosts Node3D scene-tree bindings plus server bridges
    let required = [
        ("node3d.rs", "Node3D transform helpers"),
        ("physics_server_3d.rs", "PhysicsServer3D bridge"),
        ("render_server_3d.rs", "RenderingServer3D bridge"),
        ("camera3d.rs", "Camera3D helpers"),
        ("skeleton3d.rs", "Skeleton3D"),
    ];
    for (module, desc) in &required {
        assert!(
            module_exists("gdscene", module),
            "gdscene must have {module} for {desc} (3D scene-tree bindings per Phase 6 audit)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 7. Deferred families are NOT expected as standalone crates
// ─────────────────────────────────────────────────────────────────────

#[test]
fn deferred_families_do_not_have_standalone_crates() {
    // Audit explicitly defers these — they should not have dedicated crates
    // that would imply Phase 6 parity
    let deferred_crate_names = [
        "gdvehicle3d",
        "gdsoftbody3d",
        "gdspringarm3d",
        "gdnavagent3d",
    ];
    for name in &deferred_crate_names {
        assert!(
            !crate_dir(name).exists(),
            "deferred family should not have standalone crate {name} \
             (per Phase 6 audit: outside bounded slice)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 8. Evidence test files cited in the audit exist
// ─────────────────────────────────────────────────────────────────────

#[test]
fn audit_evidence_test_files_exist() {
    let evidence_tests = [
        "tests/real_3d_demo_unified_parity_test.rs",
        "tests/render_3d_parity_test.rs",
        "tests/characterbody3d_move_and_slide_test.rs",
        "tests/physics_ray_shape_query3d_test.rs",
        "tests/rigidbody3d_forces_torques_contacts_test.rs",
        "tests/real_3d_demo_parity_report_artifact_test.rs",
    ];

    let engine_dir = repo_root().join("engine-rs");
    for test_file in &evidence_tests {
        let path = engine_dir.join(test_file);
        assert!(
            path.exists(),
            "audit evidence test file must exist: {test_file}"
        );
    }
}

#[test]
fn audit_evidence_docs_exist() {
    let evidence_docs = [
        "docs/3D_DEMO_PARITY_REPORT.md",
        "docs/migration-guide.md",
        "COMPAT_DASHBOARD.md",
        "COMPAT_MATRIX.md",
    ];

    for doc in &evidence_docs {
        let path = repo_root().join(doc);
        assert!(path.exists(), "audit evidence doc must exist: {doc}");
    }
}

// ─────────────────────────────────────────────────────────────────────
// 9. Cross-crate dependency chain is correct
// ─────────────────────────────────────────────────────────────────────

#[test]
fn render_crate_depends_on_server_crate() {
    // gdrender3d implements gdserver3d::RenderingServer3D
    let cargo = std::fs::read_to_string(crate_dir("gdrender3d").join("Cargo.toml")).unwrap();
    assert!(
        cargo.contains("gdserver3d"),
        "gdrender3d must depend on gdserver3d (implements RenderingServer3D trait)"
    );
}

#[test]
fn scene_crate_bridges_to_3d_crates() {
    // gdscene bridges scene tree to both physics and render 3D crates
    let cargo = std::fs::read_to_string(crate_dir("gdscene").join("Cargo.toml")).unwrap();
    assert!(
        cargo.contains("gdphysics3d"),
        "gdscene must depend on gdphysics3d (PhysicsServer3D bridge)"
    );
    assert!(
        cargo.contains("gdrender3d"),
        "gdscene must depend on gdrender3d (RenderingServer3D bridge)"
    );
    assert!(
        cargo.contains("gdserver3d"),
        "gdscene must depend on gdserver3d (server trait types)"
    );
}
