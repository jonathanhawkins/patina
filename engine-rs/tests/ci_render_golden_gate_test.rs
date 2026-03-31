//! pat-f831: Validate CI execution path for render golden tests.
//!
//! These tests ensure the CI render golden gate is correctly configured:
//!
//! 1. Makefile test-render-ci target covers all render_* test files
//! 2. Golden artifact directories exist and are non-empty
//! 3. Golden staleness infrastructure is in place (UPSTREAM_VERSION stamp)
//! 4. CI workflow references the correct make target
//! 5. No render test file is orphaned from the CI gate
//!
//! Acceptance: the render golden suite runs reproducibly in CI with clear
//! stale-artifact handling.

use std::path::{Path, PathBuf};

// ===========================================================================
// Helpers
// ===========================================================================

fn repo_root() -> PathBuf {
    // engine-rs/tests/ -> engine-rs/ -> repo root
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().to_path_buf()
}

fn engine_rs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ===========================================================================
// 1. Makefile test-render-ci target exists and has all three phases
// ===========================================================================

#[test]
fn makefile_has_test_render_ci_target() {
    let makefile = engine_rs_dir().join("Makefile");
    assert!(makefile.exists(), "engine-rs/Makefile must exist");

    let content = std::fs::read_to_string(&makefile).unwrap();
    assert!(
        content.contains("test-render-ci:"),
        "Makefile must define test-render-ci target"
    );
    assert!(
        content.contains("golden_staleness_test"),
        "test-render-ci must include golden_staleness_test (Phase 1)"
    );
    assert!(
        content.contains("render_golden_test"),
        "test-render-ci must include render_golden_test (Phase 2)"
    );
    assert!(
        content.contains("ci_render_golden_gate_test"),
        "test-render-ci must include ci_render_golden_gate_test (Phase 3)"
    );
}

// ===========================================================================
// 2. All render_* test files are covered by test-render or test-render-ci
// ===========================================================================

#[test]
fn makefile_covers_all_render_test_files() {
    let makefile = engine_rs_dir().join("Makefile");
    let content = std::fs::read_to_string(&makefile).unwrap();

    let tests_dir = engine_rs_dir().join("tests");
    let mut uncovered = Vec::new();

    for entry in std::fs::read_dir(&tests_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_render_test = (name.starts_with("render_")
            || name.starts_with("texture_draw_sprite"))
            && name.ends_with("_test.rs");
        if is_render_test {
            let test_name = name.trim_end_matches(".rs");
            if !content.contains(test_name) {
                uncovered.push(test_name.to_string());
            }
        }
    }

    assert!(
        uncovered.is_empty(),
        "Makefile test-render target is missing {} render test file(s):\n  - {}\n\
         Add them to the test-render target in engine-rs/Makefile.",
        uncovered.len(),
        uncovered.join("\n  - ")
    );
}

// ===========================================================================
// 3. CI workflow references test-render-ci
// ===========================================================================

#[test]
fn ci_workflow_uses_render_ci_target() {
    let ci_yml = repo_root().join(".github/workflows/ci.yml");
    assert!(ci_yml.exists(), ".github/workflows/ci.yml must exist");

    let content = std::fs::read_to_string(&ci_yml).unwrap();
    assert!(
        content.contains("test-render-ci"),
        "CI workflow must use 'make test-render-ci' for the render golden gate"
    );
}

// ===========================================================================
// 4. CI workflow uploads artifacts on failure
// ===========================================================================

#[test]
fn ci_workflow_uploads_artifacts_on_failure() {
    let ci_yml = repo_root().join(".github/workflows/ci.yml");
    let content = std::fs::read_to_string(&ci_yml).unwrap();

    assert!(
        content.contains("upload-artifact"),
        "CI workflow must upload golden artifacts on failure for debugging"
    );
    assert!(
        content.contains("if: failure()"),
        "Artifact upload must be conditional on failure"
    );
    assert!(
        content.contains("golden-render"),
        "Artifact name must reference golden-render"
    );
}

// ===========================================================================
// 5. Golden render directory exists and has PNG files
// ===========================================================================

#[test]
fn golden_render_dir_has_png_files() {
    let golden_render = repo_root().join("fixtures/golden/render");
    assert!(golden_render.is_dir(), "fixtures/golden/render/ must exist");

    let pngs: Vec<_> = std::fs::read_dir(&golden_render)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map_or(false, |ext| ext == "png" || ext == "ppm")
        })
        .collect();

    assert!(
        !pngs.is_empty(),
        "fixtures/golden/render/ must contain at least one golden image"
    );
}

// ===========================================================================
// 6. Golden render subdirectories exist for organized test output
// ===========================================================================

#[test]
fn golden_render_subdirs_exist() {
    let golden_render = repo_root().join("fixtures/golden/render");

    let expected_subdirs = ["camera_viewport", "draw_ordering"];
    for subdir in &expected_subdirs {
        let dir = golden_render.join(subdir);
        assert!(dir.is_dir(), "fixtures/golden/render/{subdir}/ must exist");

        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        assert!(
            !files.is_empty(),
            "fixtures/golden/render/{subdir}/ must not be empty"
        );
    }
}

// ===========================================================================
// 7. UPSTREAM_VERSION stamp exists for staleness detection
// ===========================================================================

#[test]
fn upstream_version_stamp_exists() {
    let stamp = repo_root().join("fixtures/golden/UPSTREAM_VERSION");
    assert!(
        stamp.exists(),
        "fixtures/golden/UPSTREAM_VERSION must exist for staleness detection.\n\
         Generate it with: git -C upstream/godot rev-parse HEAD > fixtures/golden/UPSTREAM_VERSION"
    );

    let content = std::fs::read_to_string(&stamp).unwrap();
    let trimmed = content.trim();
    assert!(
        !trimmed.is_empty(),
        "UPSTREAM_VERSION stamp must not be empty"
    );
    assert!(
        trimmed.len() >= 40,
        "UPSTREAM_VERSION must contain a full commit hash (got {} chars)",
        trimmed.len()
    );
    assert!(
        trimmed.chars().all(|c| c.is_ascii_hexdigit()),
        "UPSTREAM_VERSION must be a hex commit hash, got: '{trimmed}'"
    );
}

// ===========================================================================
// 8. CI workflow checks out submodules (needed for staleness check)
// ===========================================================================

#[test]
fn ci_render_job_checks_out_submodules() {
    let ci_yml = repo_root().join(".github/workflows/ci.yml");
    let content = std::fs::read_to_string(&ci_yml).unwrap();

    // Find the render goldens job section and verify it has submodules: true
    // We check that somewhere after "rust-render-goldens:" and before the next
    // top-level job, "submodules: true" appears.
    let render_section_start = content.find("rust-render-goldens:");
    assert!(
        render_section_start.is_some(),
        "CI must have rust-render-goldens job"
    );

    let section = &content[render_section_start.unwrap()..];
    // Find the next top-level job (indentation level 2) or end of file
    let section_end = section[1..]
        .find("\n  rust-")
        .or_else(|| section[1..].find("\n  web:"))
        .unwrap_or(section.len() - 1);
    let render_section = &section[..section_end + 1];

    assert!(
        render_section.contains("submodules: true") || render_section.contains("submodules: recursive"),
        "rust-render-goldens job must checkout with submodules for UPSTREAM_VERSION staleness check"
    );
}

// ===========================================================================
// 9. Makefile test-render target is subset of test-render-ci
// ===========================================================================

#[test]
fn test_render_is_subset_of_test_render_ci() {
    let makefile = engine_rs_dir().join("Makefile");
    let content = std::fs::read_to_string(&makefile).unwrap();

    // Extract test names from test-render target
    let render_start = content.find("test-render:").expect("test-render target");
    let render_section_end = content[render_start..]
        .find("\n\n")
        .unwrap_or(content.len() - render_start);
    let render_section = &content[render_start..render_start + render_section_end];

    // Every --test in test-render must also appear in the full content
    // (which includes test-render-ci)
    for line in render_section.lines() {
        if let Some(test_name) = line.trim().strip_prefix("--test ") {
            let test_name = test_name.trim().trim_end_matches('\\');
            let test_name = test_name.trim();
            if !test_name.is_empty() {
                // Count occurrences — should appear at least twice (once in test-render, once in test-render-ci)
                let count = content.matches(test_name).count();
                assert!(
                    count >= 2,
                    "Test '{test_name}' from test-render must also appear in test-render-ci (found {count} occurrence(s))"
                );
            }
        }
    }
}

// ===========================================================================
// 10. PATINA_CI env var is set in CI workflow
// ===========================================================================

#[test]
fn ci_workflow_sets_patina_ci_env() {
    let ci_yml = repo_root().join(".github/workflows/ci.yml");
    let content = std::fs::read_to_string(&ci_yml).unwrap();

    assert!(
        content.contains("PATINA_CI"),
        "CI workflow must set PATINA_CI env var for render golden gate"
    );
}

// ===========================================================================
// 11. Golden staleness test file exists
// ===========================================================================

#[test]
fn golden_staleness_test_exists() {
    let test_file = engine_rs_dir().join("tests/golden_staleness_test.rs");
    assert!(
        test_file.exists(),
        "engine-rs/tests/golden_staleness_test.rs must exist for staleness detection"
    );

    let content = std::fs::read_to_string(&test_file).unwrap();
    assert!(
        content.contains("no_orphaned_golden_files"),
        "golden_staleness_test must check for orphaned goldens"
    );
    assert!(
        content.contains("UPSTREAM_VERSION"),
        "golden_staleness_test must validate against UPSTREAM_VERSION"
    );
}

// ===========================================================================
// 12. Render test determinism — same input produces same output
// ===========================================================================

#[test]
fn software_renderer_is_deterministic() {
    use gdcore::math::{Color, Rect2, Transform2D, Vector2};
    use gdrender2d::renderer::SoftwareRenderer;
    use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
    use gdserver2d::server::RenderingServer2D;
    use gdserver2d::viewport::Viewport;

    let mut viewport = Viewport::new(8, 8, Color::BLACK);
    let item = CanvasItem {
        id: CanvasItemId(1),
        transform: Transform2D::IDENTITY,
        z_index: 0,
        visible: true,
        commands: vec![DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(2.0, 2.0), Vector2::new(4.0, 4.0)),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            filled: true,
        }],
        children: vec![],
        parent: None,
        layer_id: None,
    };
    viewport.add_canvas_item(item.clone());

    let mut renderer = SoftwareRenderer::new();
    let fb1 = renderer.render_frame(&viewport);
    let fb2 = renderer.render_frame(&viewport);

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "SoftwareRenderer must produce identical output for identical input (deterministic)"
    );
}
