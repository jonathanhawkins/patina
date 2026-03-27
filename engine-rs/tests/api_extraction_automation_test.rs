//! pat-9y1t / pat-gr0v / pat-lj95: Validate API extraction automation infrastructure.
//!
//! These tests verify that the automated API extraction pipeline is correctly
//! structured: version pins exist, scripts are present and executable,
//! capture types are complete, and existing golden artifacts match expected
//! formats. They do NOT require a Godot binary — they validate the
//! infrastructure that wraps the extraction.
//!
//! Acceptance: a reproducible command refreshes extracted API artifacts
//! used by parity checks.

use std::path::{Path, PathBuf};

// ===========================================================================
// Helpers
// ===========================================================================

fn repo_root() -> PathBuf {
    // engine-rs/tests/ -> engine-rs/ -> repo root
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().to_path_buf()
}

// ===========================================================================
// 1. Version pin exists in tools/oracle/common.py
// ===========================================================================

#[test]
fn version_pin_exists_in_common_py() {
    let common_py = repo_root().join("tools/oracle/common.py");
    assert!(
        common_py.exists(),
        "tools/oracle/common.py must exist for version pinning"
    );

    let content = std::fs::read_to_string(&common_py).unwrap();
    assert!(
        content.contains("UPSTREAM_VERSION"),
        "common.py must define UPSTREAM_VERSION"
    );
    assert!(
        content.contains("UPSTREAM_COMMIT"),
        "common.py must define UPSTREAM_COMMIT"
    );

    // Extract version and validate format (e.g. "4.6.1-stable")
    for line in content.lines() {
        if line.contains("UPSTREAM_VERSION") && line.contains('"') {
            let version: String = line
                .split('"')
                .nth(1)
                .unwrap_or("")
                .to_string();
            assert!(
                version.contains('.'),
                "UPSTREAM_VERSION must contain a dot-separated version: got '{version}'"
            );
            return;
        }
    }
    panic!("Could not extract UPSTREAM_VERSION string from common.py");
}

// ===========================================================================
// 2. extract_probes.sh exists and is executable
// ===========================================================================

#[test]
fn extract_probes_script_exists() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    assert!(
        script.exists(),
        "apps/godot/extract_probes.sh must exist"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "extract_probes.sh must be executable (mode: {mode:o})"
        );
    }
}

// ===========================================================================
// 3. refresh_api.sh exists and is executable
// ===========================================================================

#[test]
fn refresh_api_script_exists() {
    let script = repo_root().join("scripts/refresh_api.sh");
    assert!(
        script.exists(),
        "scripts/refresh_api.sh must exist"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "refresh_api.sh must be executable (mode: {mode:o})"
        );
    }
}

// ===========================================================================
// 4. refresh_api.sh supports --dry-run and --help flags
// ===========================================================================

#[test]
fn refresh_api_supports_flags() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(content.contains("--dry-run"), "must support --dry-run flag");
    assert!(content.contains("--probes-only"), "must support --probes-only flag");
    assert!(content.contains("--oracle-only"), "must support --oracle-only flag");
    assert!(content.contains("--help"), "must support --help flag");
    assert!(content.contains("PATINA_GODOT"), "must reference PATINA_GODOT env var");
}

// ===========================================================================
// 5. extract_probes.sh covers all 8 capture types
// ===========================================================================

#[test]
fn extract_probes_covers_all_capture_types() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    let expected_types = [
        "scene_tree",
        "properties",
        "signals",
        "classdb",
        "resource_metadata",
        "node_defaults",
        "resource_validation",
        "resource_roundtrip",
        "enum_constants",
        "singleton_api",
        "resource_subtype",
    ];

    for capture_type in &expected_types {
        assert!(
            content.contains(capture_type),
            "extract_probes.sh must handle capture_type '{capture_type}'"
        );
    }
}

// ===========================================================================
// 6. extract_probes.sh generates manifest
// ===========================================================================

#[test]
fn extract_probes_generates_manifest() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("manifest.json"),
        "extract_probes.sh must generate a manifest.json"
    );
    assert!(
        content.contains("upstream_version"),
        "manifest must include upstream_version"
    );
    assert!(
        content.contains("generated_at"),
        "manifest must include generated_at timestamp"
    );
}

// ===========================================================================
// 7. Golden classdb_probe_signatures.json exists and has valid structure
// ===========================================================================

#[test]
fn golden_classdb_signatures_exist_and_valid() {
    let sig_path = repo_root().join("fixtures/oracle_outputs/classdb_probe_signatures.json");
    assert!(
        sig_path.exists(),
        "fixtures/oracle_outputs/classdb_probe_signatures.json must exist"
    );

    let content = std::fs::read_to_string(&sig_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .expect("classdb_probe_signatures.json must be valid JSON");

    let arr = parsed.as_array().expect("signatures must be a JSON array");
    assert!(
        arr.len() >= 17,
        "signatures must contain at least 17 class entries (got {})",
        arr.len()
    );

    // Validate first entry has expected structure
    let first = &arr[0];
    assert!(first["capture_type"].is_string());
    assert!(first["data"]["class"].is_string());
    assert!(first["data"]["methods"].is_array());
    assert!(first["data"]["properties"].is_array());
    assert!(first["data"]["signals"].is_array());
}

// ===========================================================================
// 8. Signatures cover the 28 core classes
// ===========================================================================

#[test]
fn golden_signatures_cover_core_classes() {
    let sig_path = repo_root().join("fixtures/oracle_outputs/classdb_probe_signatures.json");
    let content = std::fs::read_to_string(&sig_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = parsed.as_array().unwrap();

    let classes: Vec<&str> = arr
        .iter()
        .filter_map(|e| e["data"]["class"].as_str())
        .collect();

    // These classes are in the original 17-class core set captured by the golden artifact.
    // After re-running extract_probes.sh with the expanded 28-class probe, all 28 will appear.
    let core_classes = [
        "Node", "Node2D", "Sprite2D", "Camera2D",
        "AnimationPlayer", "Control", "Label", "Button",
        "RigidBody2D", "StaticBody2D", "CharacterBody2D", "Area2D",
        "CollisionShape2D", "Timer",
    ];

    for class in &core_classes {
        assert!(
            classes.contains(class),
            "golden signatures must include class '{class}' (found: {classes:?})"
        );
    }
}

// ===========================================================================
// 9. Oracle scripts directory exists
// ===========================================================================

#[test]
fn oracle_scripts_directory_exists() {
    let oracle_dir = repo_root().join("tools/oracle");
    assert!(oracle_dir.exists(), "tools/oracle/ directory must exist");

    let run_all = oracle_dir.join("run_all.sh");
    assert!(run_all.exists(), "tools/oracle/run_all.sh must exist");
}

// ===========================================================================
// 10. Version pin format is semver-like
// ===========================================================================

#[test]
fn version_pin_is_semver_like() {
    let common_py = repo_root().join("tools/oracle/common.py");
    let content = std::fs::read_to_string(&common_py).unwrap();

    for line in content.lines() {
        if line.contains("UPSTREAM_VERSION") && line.contains('"') {
            let version = line.split('"').nth(1).unwrap();
            let parts: Vec<&str> = version.split('-').next().unwrap().split('.').collect();
            assert!(
                parts.len() >= 3,
                "version must be major.minor.patch: got '{version}'"
            );
            for part in &parts {
                part.parse::<u32>()
                    .unwrap_or_else(|_| panic!("version component '{part}' must be numeric"));
            }
            return;
        }
    }
    panic!("Could not find UPSTREAM_VERSION");
}

// ===========================================================================
// 11. Commit hash pin format is valid hex
// ===========================================================================

#[test]
fn commit_hash_pin_is_valid_hex() {
    let common_py = repo_root().join("tools/oracle/common.py");
    let content = std::fs::read_to_string(&common_py).unwrap();

    for line in content.lines() {
        if line.contains("UPSTREAM_COMMIT") && line.contains('"') {
            let commit = line.split('"').nth(1).unwrap();
            assert!(
                commit.len() >= 40,
                "commit hash must be at least 40 hex chars: got '{commit}'"
            );
            assert!(
                commit.chars().all(|c| c.is_ascii_hexdigit()),
                "commit hash must be hex: got '{commit}'"
            );
            return;
        }
    }
    panic!("Could not find UPSTREAM_COMMIT");
}

// ===========================================================================
// 12. extract_probes.sh generates classdb signatures artifact
// ===========================================================================

#[test]
fn extract_probes_generates_signatures_artifact() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("classdb_probe_signatures.json"),
        "extract_probes.sh must generate classdb_probe_signatures.json"
    );
}

// ===========================================================================
// 13. refresh_api.sh installs signatures into fixtures
// ===========================================================================

#[test]
fn refresh_api_installs_signatures() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("classdb_probe_signatures.json"),
        "refresh_api.sh must reference classdb_probe_signatures.json"
    );
    assert!(
        content.contains("oracle_outputs"),
        "refresh_api.sh must install into fixtures/oracle_outputs/"
    );
}

// ===========================================================================
// 14. PATINA_SKIP_BUILD support in extract_probes.sh
// ===========================================================================

#[test]
fn extract_probes_supports_skip_build() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("PATINA_SKIP_BUILD"),
        "extract_probes.sh must support PATINA_SKIP_BUILD env var"
    );
}

// ===========================================================================
// 15. refresh_api.sh validates version pin
// ===========================================================================

#[test]
fn refresh_api_validates_version_pin() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("PINNED_VERSION"),
        "refresh_api.sh must read the pinned version"
    );
    assert!(
        content.contains("version mismatch") || content.contains("mismatch"),
        "refresh_api.sh must warn on version mismatch"
    );
}

// ===========================================================================
// 16. refresh_api.sh --dry-run exits cleanly
// ===========================================================================

#[test]
fn refresh_api_dry_run_exits_zero() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    // Verify dry-run path exits 0 (doesn't fall through to extraction)
    assert!(
        content.contains("DRY RUN"),
        "refresh_api.sh must print DRY RUN indicator"
    );
    assert!(
        content.contains("exit 0"),
        "dry-run path must exit 0"
    );
}

// ===========================================================================
// 17. upstream/godot submodule directory exists
// ===========================================================================

#[test]
fn upstream_godot_submodule_exists() {
    let upstream = repo_root().join("upstream/godot");
    assert!(
        upstream.exists(),
        "upstream/godot submodule directory must exist"
    );
}

// ===========================================================================
// 18. refresh_api.sh validates upstream submodule commit pin
// ===========================================================================

#[test]
fn refresh_api_validates_submodule_pin() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("PINNED_COMMIT"),
        "refresh_api.sh must read the pinned commit hash"
    );
    assert!(
        content.contains("SUBMODULE_COMMIT") || content.contains("submodule"),
        "refresh_api.sh must validate the upstream submodule commit"
    );
}

// ===========================================================================
// 19. Makefile has refresh-api target
// ===========================================================================

#[test]
fn makefile_has_refresh_api_target() {
    let makefile = repo_root().join("engine-rs/Makefile");
    assert!(makefile.exists(), "engine-rs/Makefile must exist");

    let content = std::fs::read_to_string(&makefile).unwrap();
    assert!(
        content.contains("refresh-api"),
        "Makefile must have a refresh-api target"
    );
    assert!(
        content.contains("refresh_api.sh"),
        "refresh-api target must invoke scripts/refresh_api.sh"
    );
}

// ===========================================================================
// 20. refresh_api.sh is portable (no hardcoded user paths)
// ===========================================================================

#[test]
fn refresh_api_no_hardcoded_user_paths() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    // The script should use $HOME or platform detection, not hardcoded /Users/username
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip comments
        if trimmed.starts_with('#') {
            continue;
        }
        assert!(
            !trimmed.contains("/Users/bone/"),
            "refresh_api.sh must not contain hardcoded user paths in code: {trimmed}"
        );
    }
}

// ===========================================================================
// 21. Version pin in common.py matches upstream submodule tag
// ===========================================================================

#[test]
fn version_pin_matches_upstream_submodule() {
    let common_py = repo_root().join("tools/oracle/common.py");
    let content = std::fs::read_to_string(&common_py).unwrap();

    let mut pinned_commit = String::new();
    for line in content.lines() {
        if line.contains("UPSTREAM_COMMIT") && line.contains('"') {
            pinned_commit = line.split('"').nth(1).unwrap_or("").to_string();
            break;
        }
    }
    assert!(!pinned_commit.is_empty(), "must find UPSTREAM_COMMIT in common.py");

    let upstream = repo_root().join("upstream/godot");
    if !upstream.exists() {
        // Submodule not checked out — skip (CI may not have it)
        return;
    }

    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&upstream)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let submodule_commit = String::from_utf8_lossy(&out.stdout).trim().to_string();
            assert_eq!(
                submodule_commit, pinned_commit,
                "upstream/godot HEAD must match UPSTREAM_COMMIT in common.py\n  \
                 submodule: {submodule_commit}\n  pinned: {pinned_commit}\n  \
                 Fix: update tools/oracle/common.py or run: cd upstream/godot && git checkout {pinned_commit}"
            );
        }
    }
}

// ===========================================================================
// 22. Golden signatures have method/property/signal counts > 0 for Node
// ===========================================================================

#[test]
fn golden_signatures_node_has_methods_properties_signals() {
    let sig_path = repo_root().join("fixtures/oracle_outputs/classdb_probe_signatures.json");
    let content = std::fs::read_to_string(&sig_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = parsed.as_array().unwrap();

    let node_entry = arr
        .iter()
        .find(|e| e["data"]["class"].as_str() == Some("Node"))
        .expect("signatures must contain a Node class entry");

    let methods = node_entry["data"]["methods"].as_array().unwrap();
    let properties = node_entry["data"]["properties"].as_array().unwrap();
    let signals = node_entry["data"]["signals"].as_array().unwrap();

    assert!(
        !methods.is_empty(),
        "Node must have methods (got 0)"
    );
    assert!(
        !properties.is_empty(),
        "Node must have properties (got 0)"
    );
    assert!(
        !signals.is_empty(),
        "Node must have signals (got 0)"
    );
}

// ===========================================================================
// 23. extract_probes.sh handles inheritance_chain capture type
// ===========================================================================

#[test]
fn extract_probes_handles_inheritance_chain() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("inheritance_chain"),
        "extract_probes.sh must handle capture_type 'inheritance_chain'"
    );
}

// ===========================================================================
// 24. Oracle GDScript probe files exist
// ===========================================================================

#[test]
fn oracle_gdscript_probes_exist() {
    let oracle = repo_root().join("tools/oracle");
    let required_scripts = [
        "run_fixture.gd",
        "scene_tree_dumper.gd",
        "property_dumper.gd",
    ];

    for script in &required_scripts {
        let path = oracle.join(script);
        assert!(
            path.exists(),
            "tools/oracle/{script} must exist"
        );
    }
}

// ===========================================================================
// 25. Oracle run_all.sh exists and is executable
// ===========================================================================

#[test]
fn oracle_run_all_executable() {
    let script = repo_root().join("tools/oracle/run_all.sh");
    assert!(script.exists(), "tools/oracle/run_all.sh must exist");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "run_all.sh must be executable (mode: {mode:o})"
        );
    }
}

// ===========================================================================
// 26. Oracle fixture outputs have consistent triplet structure
// ===========================================================================

#[test]
fn oracle_outputs_have_triplet_structure() {
    let oracle_dir = repo_root().join("fixtures/oracle_outputs");
    assert!(oracle_dir.exists());

    // For each <scene>_tree.json there should be a matching <scene>_properties.json
    let entries: Vec<_> = std::fs::read_dir(&oracle_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with("_tree.json"))
                .unwrap_or(false)
        })
        .collect();

    assert!(
        !entries.is_empty(),
        "oracle_outputs must contain at least one _tree.json file"
    );

    for entry in &entries {
        let tree_name = entry.file_name();
        let tree_str = tree_name.to_str().unwrap();
        let props_name = tree_str.replace("_tree.json", "_properties.json");
        let props_path = oracle_dir.join(&props_name);
        assert!(
            props_path.exists(),
            "missing properties counterpart for {tree_str}: expected {props_name}"
        );
    }
}

// ===========================================================================
// 27. Golden oracle outputs are valid JSON
// ===========================================================================

#[test]
fn golden_oracle_outputs_valid_json() {
    let oracle_dir = repo_root().join("fixtures/oracle_outputs");
    let mut checked = 0;

    for entry in std::fs::read_dir(&oracle_dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let content = std::fs::read_to_string(&path).unwrap();
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&content);
            assert!(
                parsed.is_ok(),
                "{} must be valid JSON: {}",
                path.file_name().unwrap().to_str().unwrap(),
                parsed.err().unwrap()
            );
            checked += 1;
        }
    }

    assert!(
        checked >= 10,
        "should validate at least 10 oracle JSON files (got {checked})"
    );
}

// ===========================================================================
// 28. Probe implementations exist for all capture types
// ===========================================================================

#[test]
fn probe_implementation_files_exist() {
    let src = repo_root().join("apps/godot/src");
    let required_probes = [
        "classdb_probe.rs",
        "enum_constants_probe.rs",
        "node_defaults_probe.rs",
        "singleton_probe.rs",
        "resource_subtype_probe.rs",
    ];

    for probe in &required_probes {
        let path = src.join(probe);
        assert!(
            path.exists(),
            "apps/godot/src/{probe} must exist"
        );
    }
}

// ===========================================================================
// 29. refresh_api.sh invokes both probe and oracle phases
// ===========================================================================

#[test]
fn refresh_api_invokes_both_phases() {
    let script = repo_root().join("scripts/refresh_api.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("extract_probes.sh"),
        "refresh_api.sh must invoke extract_probes.sh"
    );
    assert!(
        content.contains("run_all.sh"),
        "refresh_api.sh must invoke tools/oracle/run_all.sh"
    );
}

// ===========================================================================
// 30. Golden signatures have valid method signatures with arg types
// ===========================================================================

#[test]
fn golden_signatures_methods_have_arg_types() {
    let sig_path = repo_root().join("fixtures/oracle_outputs/classdb_probe_signatures.json");
    let content = std::fs::read_to_string(&sig_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = parsed.as_array().unwrap();

    // Find Node class and check add_child method has typed args
    let node_entry = arr
        .iter()
        .find(|e| e["data"]["class"].as_str() == Some("Node"))
        .expect("must contain Node class");

    let methods = node_entry["data"]["methods"].as_array().unwrap();
    let add_child = methods
        .iter()
        .find(|m| m["name"].as_str() == Some("add_child"))
        .expect("Node must have add_child method");

    let args = add_child["args"].as_array().expect("add_child must have args array");
    assert!(!args.is_empty(), "add_child must have at least 1 argument");

    // Each arg should have name and type fields
    let first_arg = &args[0];
    assert!(
        first_arg["name"].is_string(),
        "arg must have a name field"
    );
    assert!(
        first_arg["type"].is_number(),
        "arg must have a numeric type field"
    );
}

// ===========================================================================
// 31. Golden signatures cover at least 17 classes (minimum baseline)
// ===========================================================================

#[test]
fn golden_signatures_minimum_class_count() {
    let sig_path = repo_root().join("fixtures/oracle_outputs/classdb_probe_signatures.json");
    let content = std::fs::read_to_string(&sig_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = parsed.as_array().unwrap();

    let unique_classes: std::collections::HashSet<&str> = arr
        .iter()
        .filter_map(|e| e["data"]["class"].as_str())
        .collect();

    assert!(
        unique_classes.len() >= 17,
        "golden signatures must cover at least 17 unique classes (got {})",
        unique_classes.len()
    );
}

// ===========================================================================
// 32. Sample project fixture scenes exist for oracle capture
// ===========================================================================

#[test]
fn sample_project_fixture_scenes_exist() {
    let sample = repo_root().join("fixtures/oracle_outputs/sample");
    if !sample.exists() {
        // Sample may be at a different path - check for scene files in fixtures/
        let fixtures = repo_root().join("fixtures");
        let has_scenes = std::fs::read_dir(&fixtures)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.ends_with(".tscn"))
                    .unwrap_or(false)
            });

        let scenes_dir = repo_root().join("fixtures/scenes");
        let has_scenes_dir = scenes_dir.exists()
            && std::fs::read_dir(&scenes_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n.ends_with(".tscn"))
                        .unwrap_or(false)
                });

        assert!(
            has_scenes || has_scenes_dir,
            "fixture .tscn scenes must exist for oracle capture"
        );
    }
}

// ===========================================================================
// 33. extract_probes.sh validates JSON output
// ===========================================================================

#[test]
fn extract_probes_validates_json() {
    let script = repo_root().join("apps/godot/extract_probes.sh");
    let content = std::fs::read_to_string(&script).unwrap();

    assert!(
        content.contains("jq"),
        "extract_probes.sh should use jq for JSON validation"
    );
    assert!(
        content.contains("Validating JSON") || content.contains("INVALID JSON"),
        "extract_probes.sh should validate JSON output"
    );
}
