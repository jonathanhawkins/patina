//! pat-yzdv7: keep gdplatform stable-layer claims aligned with the audited Phase 7 slice.
//!
//! Source of truth: `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`
//!
//! Validates:
//! 1. The stable layer doc exists and cites the Phase 7 audit
//! 2. The stable layer doc documents windowing, input, timing, and OS subsystems
//! 3. The audit doc identifies gdplatform as a primary local crate
//! 4. Integration tests exist for each stable-layer subsystem
//! 5. The stable layer doc scope note distinguishes headless from native parity

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_stable_layer_doc() -> String {
    let path = repo_root().join("docs/PLATFORM_STABLE_LAYER.md");
    std::fs::read_to_string(&path).expect("docs/PLATFORM_STABLE_LAYER.md must exist")
}

fn read_audit_doc() -> String {
    let path = repo_root().join("prd/PHASE7_PLATFORM_PARITY_AUDIT.md");
    std::fs::read_to_string(&path).expect("prd/PHASE7_PLATFORM_PARITY_AUDIT.md must exist")
}

// ─────────────────────────────────────────────────────────────────────
// 1. Stable layer doc exists and cites the audit
// ─────────────────────────────────────────────────────────────────────

#[test]
fn stable_layer_doc_exists() {
    let doc = read_stable_layer_doc();
    assert!(
        doc.contains("gdplatform"),
        "stable layer doc must reference gdplatform"
    );
}

#[test]
fn stable_layer_doc_cites_phase7_audit() {
    let doc = read_stable_layer_doc();
    assert!(
        doc.contains("PHASE7_PLATFORM_PARITY_AUDIT"),
        "stable layer doc must cite Phase 7 audit as source of truth"
    );
}

#[test]
fn stable_layer_doc_distinguishes_headless_from_native() {
    let doc = read_stable_layer_doc();
    assert!(
        doc.contains("headless"),
        "stable layer doc must mention headless mode"
    );
    assert!(
        doc.contains("not") || doc.contains("not yet"),
        "stable layer doc must note what is not yet measured"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 2. Stable layer documents all four subsystem areas
// ─────────────────────────────────────────────────────────────────────

#[test]
fn stable_layer_documents_windowing() {
    let doc = read_stable_layer_doc();
    assert!(
        doc.contains("Windowing"),
        "must document windowing subsystem"
    );
    assert!(
        doc.contains("WindowConfig"),
        "must reference WindowConfig type"
    );
    assert!(
        doc.contains("WindowManager"),
        "must reference WindowManager type"
    );
    assert!(
        doc.contains("HeadlessPlatform") || doc.contains("HeadlessWindow"),
        "must reference headless windowing"
    );
}

#[test]
fn stable_layer_documents_input() {
    let doc = read_stable_layer_doc();
    assert!(doc.contains("Input"), "must document input subsystem");
    assert!(doc.contains("InputState"), "must reference InputState type");
    assert!(doc.contains("InputEvent"), "must reference InputEvent type");
    assert!(doc.contains("InputMap"), "must reference InputMap type");
}

#[test]
fn stable_layer_documents_timing() {
    let doc = read_stable_layer_doc();
    assert!(doc.contains("Timing"), "must document timing subsystem");
    assert!(doc.contains("Timer"), "must reference Timer type");
    assert!(
        doc.contains("get_ticks_msec") || doc.contains("get_ticks_usec"),
        "must reference tick functions"
    );
}

#[test]
fn stable_layer_documents_os_integration() {
    let doc = read_stable_layer_doc();
    assert!(
        doc.contains("OS Integration") || doc.contains("platform_targets"),
        "must document OS integration"
    );
    assert!(
        doc.contains("DesktopTarget"),
        "must reference DesktopTarget type"
    );
    assert!(
        doc.contains("current_platform") || doc.contains("Platform"),
        "must reference platform detection"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 3. Audit doc identifies gdplatform as primary crate
// ─────────────────────────────────────────────────────────────────────

#[test]
fn audit_identifies_gdplatform_crate() {
    let audit = read_audit_doc();
    assert!(
        audit.contains("gdplatform"),
        "Phase 7 audit must reference gdplatform crate"
    );
}

#[test]
fn audit_references_stable_layer_evidence() {
    let audit = read_audit_doc();
    assert!(
        audit.contains("platform_first_stable_layer_test"),
        "audit must cite the stable layer integration test"
    );
    assert!(
        audit.contains("PLATFORM_STABLE_LAYER"),
        "audit must cite the stable layer doc"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 4. Integration test files exist for each stable-layer subsystem
// ─────────────────────────────────────────────────────────────────────

#[test]
fn stable_layer_integration_test_exists() {
    let path = repo_root().join("engine-rs/tests/platform_first_stable_layer_test.rs");
    assert!(
        path.exists(),
        "platform_first_stable_layer_test.rs must exist"
    );
}

#[test]
fn windowing_tests_exist() {
    let tests_dir = repo_root().join("engine-rs/tests");
    let window_tests = [
        "window_lifecycle_test.rs",
        "window_lifecycle_parity_test.rs",
        "window_creation_abstraction_test.rs",
    ];
    for test in &window_tests {
        assert!(
            tests_dir.join(test).exists(),
            "windowing test must exist: {test}"
        );
    }
}

#[test]
fn platform_target_tests_exist() {
    let tests_dir = repo_root().join("engine-rs/tests");
    assert!(
        tests_dir
            .join("platform_targets_validation_test.rs")
            .exists(),
        "platform targets validation test must exist"
    );
    assert!(
        tests_dir.join("ci_build_matrix_platform_test.rs").exists(),
        "CI build matrix test must exist"
    );
}

#[test]
fn per_os_platform_tests_exist() {
    let tests_dir = repo_root().join("engine-rs/tests");
    let os_tests = [
        "linux_x11_wayland_platform_test.rs",
        "windows_platform_win32_backend_test.rs",
    ];
    for test in &os_tests {
        assert!(
            tests_dir.join(test).exists(),
            "per-OS platform test must exist: {test}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 5. gdplatform crate modules match stable-layer responsibilities
// ─────────────────────────────────────────────────────────────────────

fn gdplatform_module_exists(module: &str) -> bool {
    repo_root()
        .join("engine-rs/crates/gdplatform/src")
        .join(module)
        .exists()
}

#[test]
fn gdplatform_has_windowing_modules() {
    assert!(
        gdplatform_module_exists("window.rs"),
        "must have window module"
    );
    assert!(
        gdplatform_module_exists("display.rs"),
        "must have display module"
    );
    assert!(
        gdplatform_module_exists("backend.rs"),
        "must have backend module"
    );
}

#[test]
fn gdplatform_has_input_module() {
    assert!(
        gdplatform_module_exists("input.rs"),
        "must have input module"
    );
}

#[test]
fn gdplatform_has_timing_modules() {
    assert!(gdplatform_module_exists("time.rs"), "must have time module");
    assert!(gdplatform_module_exists("os.rs"), "must have os module");
}

#[test]
fn gdplatform_has_platform_targets_module() {
    assert!(
        gdplatform_module_exists("platform_targets.rs"),
        "must have platform_targets module"
    );
}

#[test]
fn gdplatform_has_export_module() {
    assert!(
        gdplatform_module_exists("export.rs"),
        "must have export module"
    );
}
