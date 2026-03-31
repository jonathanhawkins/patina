//! pat-hx666: Validate that 3D crate boundaries match the Phase 6 audit.
//!
//! Source of truth: `prd/PHASE6_3D_PARITY_AUDIT.md`
//!
//! This test guards the crate boundary classification from the audit doc.
//! It ensures:
//! 1. The four primary 3D crates exist on disk
//! 2. Each crate exposes the expected module surface
//! 3. The Phase 6 audit doc exists and references pat-hx666
//! 4. Supporting crates used by the 3D slice exist

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn crate_src(name: &str) -> PathBuf {
    repo_root().join(format!("engine-rs/crates/{name}/src"))
}

fn has_module(crate_name: &str, module: &str) -> bool {
    let dir = crate_src(crate_name);
    dir.join(format!("{module}.rs")).exists() || dir.join(module).join("mod.rs").exists()
}

fn audit_path() -> PathBuf {
    repo_root().join("prd/PHASE6_3D_PARITY_AUDIT.md")
}

fn read_audit() -> String {
    std::fs::read_to_string(audit_path()).expect("prd/PHASE6_3D_PARITY_AUDIT.md must exist")
}

// ── Audit doc validation ────────────────────────────────────────────

#[test]
fn phase6_audit_doc_exists_and_cites_crate_boundary_bead() {
    let audit = read_audit();
    assert!(
        audit.contains("Phase 6 3D Parity Audit"),
        "audit doc must have its title"
    );
    assert!(
        audit.contains("pat-hx666"),
        "audit doc must reference the crate boundary bead"
    );
    assert!(
        audit.contains("Crate Boundary Classification"),
        "audit doc must have the crate boundary section"
    );
}

// ── Primary 3D crates exist ─────────────────────────────────────────

#[test]
fn primary_3d_crates_exist() {
    let crates = ["gdscene", "gdserver3d", "gdrender3d", "gdphysics3d"];
    for name in &crates {
        let src = crate_src(name);
        assert!(
            src.exists(),
            "primary 3D crate '{name}' must exist at {}",
            src.display()
        );
        assert!(
            src.join("lib.rs").exists(),
            "crate '{name}' must have lib.rs"
        );
    }
}

// ── gdscene: 3D scene-tree modules ─────────────────────────────────

#[test]
fn gdscene_has_3d_modules() {
    let expected = [
        "node3d",
        "camera3d",
        "skeleton3d",
        "particle3d",
        "decal",
        "physics_server_3d",
        "render_server_3d",
        "collision",
    ];
    for module in &expected {
        assert!(
            has_module("gdscene", module),
            "gdscene must have module '{module}'"
        );
    }
}

// ── gdserver3d: abstract 3D rendering server ────────────────────────

#[test]
fn gdserver3d_has_expected_modules() {
    let expected = [
        "server",
        "mesh",
        "material",
        "light",
        "shader",
        "sky",
        "environment",
        "fog_volume",
        "reflection_probe",
        "csg",
        "gi",
        "navigation",
        "particles3d",
        "multimesh",
        "primitive_mesh",
        "projection",
        "viewport",
        "instance",
    ];
    for module in &expected {
        assert!(
            has_module("gdserver3d", module),
            "gdserver3d must have module '{module}'"
        );
    }
}

// ── gdrender3d: software 3D renderer ────────────────────────────────

#[test]
fn gdrender3d_has_expected_modules() {
    let expected = [
        "renderer",
        "rasterizer",
        "shader",
        "shadow_map",
        "depth_buffer",
        "compare",
        "test_adapter",
    ];
    for module in &expected {
        assert!(
            has_module("gdrender3d", module),
            "gdrender3d must have module '{module}'"
        );
    }
}

// ── gdphysics3d: 3D physics simulation ─────────────────────────────

#[test]
fn gdphysics3d_has_expected_modules() {
    let expected = [
        "body",
        "character",
        "shape",
        "collision",
        "world",
        "area3d",
        "query",
        "joint",
    ];
    for module in &expected {
        assert!(
            has_module("gdphysics3d", module),
            "gdphysics3d must have module '{module}'"
        );
    }
}

// ── Supporting crates exist ─────────────────────────────────────────

#[test]
fn supporting_crates_for_3d_slice_exist() {
    let crates = ["gdcore", "gdobject", "gdresource", "gdvariant"];
    for name in &crates {
        let src = crate_src(name);
        assert!(
            src.exists(),
            "supporting crate '{name}' must exist at {}",
            src.display()
        );
    }
}

#[test]
fn gdcore_has_3d_math_modules() {
    let expected = ["math3d", "compare3d"];
    for module in &expected {
        assert!(
            has_module("gdcore", module),
            "gdcore must have 3D math module '{module}'"
        );
    }
}

// ── Boundary rule: no 3D code outside the four primary crates ───────

#[test]
fn no_3d_scene_nodes_outside_primary_crates() {
    // The four primary 3D crates are the only ones that should define
    // 3D node behavior (node3d, camera3d, etc.).  Supporting crates
    // provide math, serialization, and class registration — not nodes.
    let non_3d_crates = [
        "gdaudio",
        "gdeditor",
        "gdplatform",
        "gdrender2d",
        "gdserver2d",
        "gdphysics2d",
        "gdscript-interop",
    ];
    for name in &non_3d_crates {
        // These crates should not have a node3d.rs module
        assert!(
            !has_module(name, "node3d"),
            "crate '{name}' should not define node3d — 3D nodes belong in the primary 3D crates"
        );
    }
}

// ── Audit doc content guards ────────────────────────────────────────

#[test]
fn audit_documents_all_four_primary_crates() {
    let audit = read_audit();
    for crate_name in &["gdscene", "gdserver3d", "gdrender3d", "gdphysics3d"] {
        assert!(
            audit.contains(&format!("`{crate_name}`")),
            "audit doc must reference primary crate '{crate_name}'"
        );
    }
}

#[test]
fn audit_documents_supporting_crates() {
    let audit = read_audit();
    for crate_name in &["gdcore", "gdobject", "gdresource", "gdvariant"] {
        assert!(
            audit.contains(&format!("`{crate_name}`")),
            "audit doc must reference supporting crate '{crate_name}'"
        );
    }
}

#[test]
fn audit_has_measured_and_deferred_classifications() {
    let audit = read_audit();
    assert!(
        audit.contains("Measured"),
        "audit must classify items as Measured"
    );
    assert!(
        audit.contains("Deferred"),
        "audit must classify items as Deferred"
    );
    assert!(
        audit.contains("Implemented, not yet"),
        "audit must classify items as Implemented, not yet measured"
    );
}
