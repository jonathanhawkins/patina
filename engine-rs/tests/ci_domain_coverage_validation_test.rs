//! pat-xupn / pat-4no: CI domain coverage validation.
//!
//! Ensures every integration test file in `tests/` is matched by at least one
//! CI domain lane filter. If a new test file is added whose name doesn't match
//! any domain lane prefix, this test fails — forcing the developer to assign
//! the test to a domain lane in `.github/workflows/ci.yml`.
//!
//! This prevents parity regressions from being silently missed by the
//! fast-feedback domain lanes.

use std::collections::HashSet;

/// CI domain lane prefixes, mirroring `.github/workflows/ci.yml`.
///
/// Each entry is a substring that `cargo test --workspace -- <prefix>` would
/// use to filter tests in a dedicated CI job.
const HEADLESS_PREFIXES: &[&str] = &[
    "resource_",
    "scene_",
    "signal_",
    "notification_",
    "object_",
    "classdb_",
    "lifecycle_",
    "packed_scene_",
    "nodepath_",
    "unique_name_",
    "connect_",
    "deferred_",
    "instanc",
    "reparent_",
    "default_property_",
    "gdscript_",
    "script_",
    "process_priority",
    "frame_",
    "trace_",
    "mainloop_",
    "change_scene_",
    "node_",
    "cache_",
    "project_loading",
    "unified_loader",
    "container_layout_",
    "func_dispatch_",
    "onready_",
    "weakref_",
    "time_singleton",
    "os_singleton",
    "regex_singleton",
    "project_settings_singleton",
];

const DOMAIN_2D_PREFIXES: &[&str] = &[
    "physics_",
    "render_",
    "collision_",
    "node2d_",
    "geometry2d_",
    "vertical_slice",
    "measured_2d",
    "camera_viewport",
    "viewport_clear",
    "draw_ordering",
    "texture_",
    "area2d_",
    "character_",
    "fixed_step_",
    "gdphysics2d_",
    "deterministic_physics",
    "cpu_particles2d_",
    "pixel_diff_",
    "visibility_suppression",
    "wgpu_backend_",
    "viewport_2d_",
    "viewport_golden_",
];

const DOMAIN_3D_PREFIXES: &[&str] = &[
    "node3d_",
    "physics3d_",
    "transform3d_",
    "render_3d_",
    "camera3d_",
    "representative_3d_",
    "crate_boundary_3d_",
    "demo_3d_",
    "hierarchy_3d_",
    "crate_set_3d_",
    "animation_player_3d_",
    "characterbody3d_",
    "collision_shape_3d_",
    "cpu_particles3d_",
    "light3d_",
    "multimesh_instance3d_",
    "navigation_region3d_",
    "omnilight3d_",
    "rigidbody3d_",
    "shader_material_3d_",
    "spotlight3d_",
    "real_3d_demo_",
    "directional_light_",
    "reflection_probe_",
    "skeleton3d_",
    "sky_resource_",
    "csg_",
    "fog_volume_",
    "environment_resource_",
    "voxelgi_",
    "viewport_3d_",
];

const PLATFORM_PREFIXES: &[&str] = &[
    "input_",
    "window_",
    "platform_",
    "audio_",
    "keyboard_",
    "mouse_input_",
    "winit_",
    "clipboard_",
    "cursor_",
    "display_server_",
    "drag_drop_",
    "gamepad_",
    "vsync_mode_",
    "web_wasm_",
];

const FUZZ_PREFIXES: &[&str] = &[
    "fuzz_property",
    "fuzz_variant",
    "fuzz_collision",
    "fuzz_signal",
    "fuzz_resource",
    "fuzz_binary",
    "fuzz_tres",
    "property_tests",
    "robustness",
];

const ORACLE_PREFIXES: &[&str] = &["oracle_parity", "golden_staleness", "oracle_regression"];

const CI_META_PREFIXES: &[&str] = &[
    "ci_",
    "version_consistency",
    "startup_packaging",
    "release_",
    "migration_",
    "contributor_",
    "crash_triage",
    "core_runtime",
    "benchmark_",
    "headless_",
    "demo_",
    "space_shooter",
    "platformer",
    "godot_compat",
    "probe_output",
    "api_extraction",
    "editor",
    "golden_refresh",
    "upstream_",
    "gdext_",
    "repin_diff_report",
    "compat",
    "v1_acceptance",
    "shader_tokenizer",
    "broader_integration",
    "animation_",
    "command_palette_",
    "community_",
    "debugger_",
    "dependency_",
    "example_",
    "filesystem_",
    "import_",
    "memory_profiler_",
    "perf_",
    "rustdoc_",
    "unsafe_code_",
    "vcs_",
    "asset_",
    "container_",
    "tilemap_",
    "undo_redo_",
    "curve_",
    "profiler_",
    "inspector_",
    "script_editor_",
    "dock_",
    "tooling_",
];

/// Returns true if `name` matches any prefix in the given set.
fn matches_any(name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| name.contains(p))
}

/// Collects all domain lanes that match a given test file name.
fn matched_domains(name: &str) -> Vec<&'static str> {
    let mut domains = Vec::new();
    if matches_any(name, HEADLESS_PREFIXES) {
        domains.push("headless");
    }
    if matches_any(name, DOMAIN_2D_PREFIXES) {
        domains.push("2d");
    }
    if matches_any(name, DOMAIN_3D_PREFIXES) {
        domains.push("3d");
    }
    if matches_any(name, PLATFORM_PREFIXES) {
        domains.push("platform");
    }
    if matches_any(name, FUZZ_PREFIXES) {
        domains.push("fuzz");
    }
    if matches_any(name, ORACLE_PREFIXES) {
        domains.push("oracle");
    }
    if matches_any(name, CI_META_PREFIXES) {
        domains.push("ci-meta");
    }
    domains
}

#[test]
fn every_test_file_has_a_ci_domain_lane() {
    let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut uncovered = Vec::new();

    // Files that are test utilities, not test binaries — skip them.
    let skip: HashSet<&str> = [
        "oracle_fixture.rs",
        "trace_compare.rs",
        "bench_render_baselines.rs",
        "bench_runtime_baselines.rs",
    ]
    .into_iter()
    .collect();

    for entry in std::fs::read_dir(&tests_dir).expect("tests/ directory must exist") {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();

        if !name.ends_with(".rs") {
            continue;
        }
        if skip.contains(name.as_str()) {
            continue;
        }

        let stem = name.trim_end_matches(".rs");
        let domains = matched_domains(stem);
        if domains.is_empty() {
            uncovered.push(stem.to_string());
        }
    }

    uncovered.sort();

    assert!(
        uncovered.is_empty(),
        "The following test files are not covered by any CI domain lane.\n\
         Add a matching prefix to a CI lane in .github/workflows/ci.yml \n\
         and update the prefix constants in this test:\n  - {}",
        uncovered.join("\n  - ")
    );
}

#[test]
fn ci_domain_lanes_have_no_empty_prefixes() {
    // Sanity check: no prefix list should be empty.
    assert!(!HEADLESS_PREFIXES.is_empty(), "headless prefixes empty");
    assert!(!DOMAIN_2D_PREFIXES.is_empty(), "2d prefixes empty");
    assert!(!DOMAIN_3D_PREFIXES.is_empty(), "3d prefixes empty");
    assert!(!PLATFORM_PREFIXES.is_empty(), "platform prefixes empty");
    assert!(!FUZZ_PREFIXES.is_empty(), "fuzz prefixes empty");
    assert!(!ORACLE_PREFIXES.is_empty(), "oracle prefixes empty");
    assert!(!CI_META_PREFIXES.is_empty(), "ci-meta prefixes empty");
}

#[test]
fn no_duplicate_prefixes_within_domain() {
    let all_domains: &[(&str, &[&str])] = &[
        ("headless", HEADLESS_PREFIXES),
        ("2d", DOMAIN_2D_PREFIXES),
        ("3d", DOMAIN_3D_PREFIXES),
        ("platform", PLATFORM_PREFIXES),
        ("fuzz", FUZZ_PREFIXES),
        ("oracle", ORACLE_PREFIXES),
        ("ci-meta", CI_META_PREFIXES),
    ];

    for (domain, prefixes) in all_domains {
        let mut seen = HashSet::new();
        for p in *prefixes {
            assert!(
                seen.insert(*p),
                "duplicate prefix '{p}' in domain '{domain}'"
            );
        }
    }
}

// ===========================================================================
// pat-4no: Verify domain prefix constants stay in sync with CI yaml
// ===========================================================================

/// Reads the actual CI workflow file and verifies that every prefix listed in
/// the domain constants above actually appears somewhere in the CI yaml.
/// This catches drift where a prefix is added to the test constants but never
/// wired into the real CI workflow.
#[test]
fn domain_prefixes_present_in_ci_yaml() {
    let ci_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/ci.yml");
    let ci = std::fs::read_to_string(&ci_path).expect("CI workflow must exist");

    let all_prefixes: &[(&str, &[&str])] = &[
        ("headless", HEADLESS_PREFIXES),
        ("2d", DOMAIN_2D_PREFIXES),
        ("3d", DOMAIN_3D_PREFIXES),
        ("platform", PLATFORM_PREFIXES),
        ("fuzz", FUZZ_PREFIXES),
        ("oracle", ORACLE_PREFIXES),
        ("ci-meta", CI_META_PREFIXES),
    ];

    let mut missing = Vec::new();

    for (domain, prefixes) in all_prefixes {
        for prefix in *prefixes {
            // The prefix should appear in the CI yaml as a cargo test filter.
            // In YAML it appears either on its own line or in a multi-line run block.
            if !ci.contains(prefix) {
                missing.push(format!(
                    "[{domain}] prefix '{prefix}' in test constant but NOT in CI yaml"
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Domain prefix constants have drifted from CI yaml:\n  {}",
        missing.join("\n  ")
    );
}
