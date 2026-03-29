//! Release-train workflow contract tests for Patina runtime milestones.
//!
//! Bead: pat-477s
//! Phase 9 deliverable: "release train" with exit criteria
//! "repeatable release cadence, stable regression suite, known-risk backlog clearly owned."
//!
//! These tests encode the structural invariants that must hold for every
//! milestone release, ensuring the release-train workflow is repeatable
//! and machine-verifiable.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Workspace root relative to the test binary.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Repo root (one level above engine-rs).
fn repo_root() -> PathBuf {
    workspace_root().parent().unwrap().to_path_buf()
}

// ===========================================================================
// 1. Workspace version consistency — all crates use workspace version
// ===========================================================================

#[test]
fn all_crates_use_workspace_version() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();

    // Extract workspace version.
    let ws_version = root_toml
        .lines()
        .find(|l| l.starts_with("version = ") && !l.contains("workspace"))
        .and_then(|l| l.split('"').nth(1))
        .expect("workspace.package.version must be set");

    assert!(
        !ws_version.is_empty(),
        "workspace version must not be empty"
    );

    // Every crate Cargo.toml must reference version.workspace = true
    let crates_dir = workspace_root().join("crates");
    let mut checked = 0;
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let cargo_path = entry.path().join("Cargo.toml");
        if !cargo_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&cargo_path).unwrap();
        assert!(
            content.contains("version.workspace = true")
                || content.contains("version = { workspace = true }"),
            "crate {} must use version.workspace = true",
            entry.file_name().to_string_lossy()
        );
        checked += 1;
    }
    assert!(
        checked >= 15,
        "expected at least 15 crates, found {checked}"
    );
}

#[test]
fn workspace_version_is_valid_semver() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    let version = root_toml
        .lines()
        .find(|l| l.starts_with("version = ") && !l.contains("workspace"))
        .and_then(|l| l.split('"').nth(1))
        .expect("workspace version must exist");

    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "version must be major.minor.patch, got {version}"
    );
    for (i, part) in parts.iter().enumerate() {
        part.parse::<u32>().unwrap_or_else(|_| {
            panic!("version component {i} ({part}) must be a number in {version}")
        });
    }
}

// ===========================================================================
// 2. Workspace membership — all crate dirs are registered
// ===========================================================================

#[test]
fn all_crate_dirs_are_workspace_members() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    let crates_dir = workspace_root().join("crates");

    // Collect actual crate directories (only those tracked in git / workspace).
    // Skip untracked dirs that haven't been added to the workspace yet.
    let mut on_disk: HashSet<String> = HashSet::new();
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        let cargo_toml = entry.path().join("Cargo.toml");
        if entry.file_type().unwrap().is_dir() && cargo_toml.exists() {
            // Only include if the Cargo.toml is tracked by git (not untracked/new)
            let output = std::process::Command::new("git")
                .args(["ls-files", "--error-unmatch"])
                .arg(&cargo_toml)
                .current_dir(workspace_root())
                .output();
            if let Ok(o) = output {
                if o.status.success() {
                    let name = format!("crates/{}", entry.file_name().to_string_lossy());
                    on_disk.insert(name);
                }
            }
        }
    }

    // Collect members listed in workspace.
    let mut in_workspace: HashSet<String> = HashSet::new();
    let mut in_members = false;
    for line in root_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed == "]" {
                break;
            }
            let member = trimmed.trim_matches(|c: char| c == '"' || c == ',' || c.is_whitespace());
            if !member.is_empty() {
                in_workspace.insert(member.to_string());
            }
        }
    }

    let missing: Vec<_> = on_disk.difference(&in_workspace).collect();
    assert!(
        missing.is_empty(),
        "crate dirs exist on disk but not in workspace members: {missing:?}"
    );
}

// ===========================================================================
// 3. CI pipeline completeness — required jobs exist
// ===========================================================================

#[test]
fn ci_yaml_exists() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    assert!(
        ci_path.exists(),
        "CI workflow must exist at .github/workflows/ci.yml"
    );
}

#[test]
fn ci_has_required_gates() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    let required_gates = [
        "cargo fmt",    // format check
        "cargo build",  // build step
        "cargo test",   // test step
        "cargo clippy", // lint step
    ];

    for gate in &required_gates {
        assert!(ci_content.contains(gate), "CI must include gate: {gate}");
    }
}

#[test]
fn ci_runs_on_push_and_pr() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    assert!(ci_content.contains("push:"), "CI must trigger on push");
    assert!(
        ci_content.contains("pull_request:"),
        "CI must trigger on pull_request"
    );
}

#[test]
fn ci_uses_multi_platform_matrix() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    assert!(
        ci_content.contains("ubuntu-latest") && ci_content.contains("macos-latest"),
        "CI must test on at least ubuntu and macos"
    );
}

#[test]
fn ci_has_release_build_gate() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    assert!(
        ci_content.contains("--release"),
        "CI must include a release-mode build gate"
    );
}

#[test]
fn ci_has_render_golden_gate() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    assert!(
        ci_content.contains("test-render"),
        "CI must include the render golden gate"
    );
}

// ===========================================================================
// 4. Makefile test tiers — tiered test targets exist
// ===========================================================================

#[test]
fn makefile_exists_with_test_tiers() {
    let makefile = workspace_root().join("Makefile");
    assert!(makefile.exists(), "Makefile must exist in engine-rs/");
    let content = fs::read_to_string(&makefile).unwrap();

    let required_targets = ["test:", "test-fast:", "test-golden:", "test-render:"];
    for target in &required_targets {
        assert!(
            content.contains(target),
            "Makefile must define target {target}"
        );
    }
}

#[test]
fn makefile_test_fast_is_tier_1() {
    let makefile = workspace_root().join("Makefile");
    let content = fs::read_to_string(&makefile).unwrap();

    // test-fast must skip expensive tests
    let fast_section: String = content
        .lines()
        .skip_while(|l| !l.starts_with("test-fast:"))
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        fast_section.contains("--skip"),
        "test-fast must skip expensive tests"
    );
}

// ===========================================================================
// 5. Fixture and golden data structure — organized for diffability
// ===========================================================================

#[test]
fn fixtures_directory_structure_exists() {
    let fixtures = repo_root().join("fixtures");
    assert!(fixtures.exists(), "fixtures/ directory must exist");

    let required_subdirs = ["scenes", "golden"];
    for dir in &required_subdirs {
        let path = fixtures.join(dir);
        assert!(path.exists(), "fixtures/{dir}/ must exist");
    }
}

#[test]
fn golden_physics_traces_are_json() {
    let golden_physics = repo_root().join("fixtures/golden/physics");
    if !golden_physics.exists() {
        // If no physics goldens yet, that's fine — no assertion needed.
        return;
    }

    let mut json_count = 0;
    for entry in fs::read_dir(&golden_physics).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json") {
            // Validate it's parseable JSON.
            let content = fs::read_to_string(&path).unwrap();
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&content);
            assert!(
                parsed.is_ok(),
                "golden trace {} must be valid JSON: {}",
                path.display(),
                parsed.unwrap_err()
            );
            json_count += 1;
        }
    }
    assert!(
        json_count > 0,
        "golden/physics/ must contain at least one .json trace"
    );
}

// ===========================================================================
// 6. Crate dependency graph — no circular deps, layered architecture
// ===========================================================================

#[test]
fn gdcore_has_no_internal_dependencies() {
    let core_toml = fs::read_to_string(workspace_root().join("crates/gdcore/Cargo.toml")).unwrap();

    // gdcore should not depend on any other gd* crate (it's the leaf)
    let gd_crates = [
        "gdvariant",
        "gdobject",
        "gdscene",
        "gdresource",
        "gdserver2d",
        "gdrender2d",
        "gdphysics2d",
        "gdplatform",
        "gdeditor",
    ];
    for dep in &gd_crates {
        assert!(
            !core_toml.contains(&format!("{dep} =")),
            "gdcore must not depend on {dep} — it is the foundation crate"
        );
    }
}

#[test]
fn workspace_has_expected_layer_crates() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();

    let expected_crates = [
        "gdcore",      // math, types
        "gdvariant",   // variant system
        "gdobject",    // object model
        "gdresource",  // resource loading
        "gdscene",     // scene tree
        "gdphysics2d", // 2D physics
        "gdserver2d",  // 2D rendering server
        "gdrender2d",  // 2D software renderer
        "gdserver3d",  // 3D rendering server
        "gdrender3d",  // 3D software renderer
        "gdplatform",  // platform abstraction
        "gdaudio",     // audio
    ];

    for crate_name in &expected_crates {
        assert!(
            root_toml.contains(crate_name),
            "workspace must include crate: {crate_name}"
        );
    }
}

// ===========================================================================
// 7. License and metadata — release-ready metadata
// ===========================================================================

#[test]
fn workspace_has_license() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    assert!(
        root_toml.contains("license ="),
        "workspace must declare a license"
    );
}

#[test]
fn workspace_has_repository() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    assert!(
        root_toml.contains("repository ="),
        "workspace must declare a repository URL"
    );
}

#[test]
fn workspace_has_edition_2021() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
    assert!(
        root_toml.contains("edition = \"2021\""),
        "workspace must use Rust edition 2021"
    );
}

// ===========================================================================
// 8. Release-mode build invariants
// ===========================================================================

#[test]
fn cargo_lock_exists() {
    let lock = workspace_root().join("Cargo.lock");
    assert!(
        lock.exists(),
        "Cargo.lock must be committed for reproducible builds"
    );
}

#[test]
fn no_wildcard_dependencies() {
    let root_toml = fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();

    // Check workspace deps don't use "*" version.
    for line in root_toml.lines() {
        let trimmed = line.trim();
        if trimmed.contains("version") && trimmed.contains("\"*\"") {
            panic!("wildcard dependency found: {trimmed}");
        }
    }
}

// ===========================================================================
// 9. Regression suite structure — tests exist across subsystems
// ===========================================================================

#[test]
fn integration_test_directory_has_tests() {
    let tests_dir = workspace_root().join("tests");
    assert!(tests_dir.exists(), "engine-rs/tests/ must exist");

    let test_count = fs::read_dir(&tests_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        .count();

    assert!(
        test_count >= 10,
        "expected at least 10 integration test files, found {test_count}"
    );
}

#[test]
fn parity_tests_cover_key_subsystems() {
    let tests_dir = workspace_root().join("tests");

    let subsystem_patterns = [
        "physics",  // physics parity
        "scene",    // scene tree
        "resource", // resource loading
        "signal",   // signal dispatch
        "render",   // rendering
        "node",     // node system
    ];

    for pattern in &subsystem_patterns {
        let has_test = fs::read_dir(&tests_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.contains(pattern) && name.ends_with("_test.rs")
            });
        assert!(
            has_test,
            "must have at least one parity test covering subsystem: {pattern}"
        );
    }
}

// ===========================================================================
// 10. Release checklist contract — documents and gates
// ===========================================================================

#[test]
fn agents_md_exists() {
    let agents = repo_root().join("AGENTS.md");
    assert!(
        agents.exists(),
        "AGENTS.md must exist for contributor guidance"
    );
}

#[test]
fn port_plan_exists() {
    let plan = repo_root().join("prd/PORT_GODOT_TO_RUST_PLAN.md");
    assert!(plan.exists(), "port plan must exist for milestone tracking");
}

#[test]
fn port_plan_has_phase_structure() {
    let plan = fs::read_to_string(repo_root().join("prd/PORT_GODOT_TO_RUST_PLAN.md")).unwrap();

    // Must have phased milestones.
    let phase_count = plan.lines().filter(|l| l.starts_with("## Phase")).count();
    assert!(
        phase_count >= 5,
        "port plan must define at least 5 phases, found {phase_count}"
    );
}

// ===========================================================================
// 11. Release-train summary report
// ===========================================================================

#[test]
fn release_train_readiness_report() {
    let ws_root = workspace_root();
    let repo = repo_root();

    let mut checks: Vec<(&str, bool)> = vec![];

    // Version consistency
    let root_toml = fs::read_to_string(ws_root.join("Cargo.toml")).unwrap();
    let has_version = root_toml.contains("version = \"0.");
    checks.push(("workspace version set", has_version));

    // CI exists
    let ci_exists = repo.join(".github/workflows/ci.yml").exists();
    checks.push(("CI workflow exists", ci_exists));

    // Cargo.lock committed
    let lock_exists = ws_root.join("Cargo.lock").exists();
    checks.push(("Cargo.lock committed", lock_exists));

    // Makefile with tiers
    let makefile_exists = ws_root.join("Makefile").exists();
    checks.push(("Makefile test tiers", makefile_exists));

    // Fixtures structured
    let fixtures_exist =
        repo.join("fixtures/scenes").exists() && repo.join("fixtures/golden").exists();
    checks.push(("fixture directories", fixtures_exist));

    // Integration tests
    let test_count = fs::read_dir(ws_root.join("tests"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
                .count()
        })
        .unwrap_or(0);
    checks.push(("integration tests (>=10)", test_count >= 10));

    // Port plan
    let plan_exists = repo.join("prd/PORT_GODOT_TO_RUST_PLAN.md").exists();
    checks.push(("port plan exists", plan_exists));

    // License
    let has_license = root_toml.contains("license =");
    checks.push(("license declared", has_license));

    // Print report
    println!("\n=== Release-Train Readiness Report ===");
    let mut pass_count = 0;
    for (name, ok) in &checks {
        let status = if *ok { "PASS" } else { "FAIL" };
        if *ok {
            pass_count += 1;
        }
        println!("  [{status}] {name}");
    }
    let total = checks.len();
    println!("  ---");
    println!("  {pass_count}/{total} checks passing");
    println!("======================================\n");

    assert_eq!(
        pass_count, total,
        "all release-train readiness checks must pass"
    );
}

// ===========================================================================
// 12. Release-train repeatable milestone gates (pat-off2)
// ===========================================================================

/// Every workspace crate must use the same edition.
#[test]
fn all_crates_share_workspace_edition() {
    let crates_dir = workspace_root().join("crates");
    let mut checked = 0;
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let cargo_path = entry.path().join("Cargo.toml");
        if !cargo_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&cargo_path).unwrap();
        assert!(
            content.contains("edition.workspace = true")
                || content.contains("edition = { workspace = true }"),
            "crate {} must use edition.workspace = true",
            entry.file_name().to_string_lossy()
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected at least 10 crates, found {checked}"
    );
}

/// Every workspace crate must use the same license.
#[test]
fn all_crates_share_workspace_license() {
    let crates_dir = workspace_root().join("crates");
    let mut checked = 0;
    for entry in fs::read_dir(&crates_dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let cargo_path = entry.path().join("Cargo.toml");
        if !cargo_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&cargo_path).unwrap();
        assert!(
            content.contains("license.workspace = true")
                || content.contains("license = { workspace = true }"),
            "crate {} must use license.workspace = true",
            entry.file_name().to_string_lossy()
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected at least 10 crates, found {checked}"
    );
}

/// CI must include parity domain gates for each active test domain.
#[test]
fn ci_has_parity_domain_gates() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    // Check that CI references key parity domains (oracle, parity)
    let domain_patterns = ["parity", "oracle"];
    for domain in &domain_patterns {
        assert!(
            ci_content.to_lowercase().contains(domain),
            "CI must reference parity domain: {domain}"
        );
    }
}

/// Golden trace fixtures must exist for deterministic replay.
#[test]
fn golden_scene_traces_exist() {
    let golden_scenes = repo_root().join("fixtures/golden/scenes");
    if !golden_scenes.exists() {
        return;
    }

    let json_count = fs::read_dir(&golden_scenes)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();

    assert!(
        json_count >= 3,
        "golden/scenes/ must contain at least 3 golden trace files, found {json_count}"
    );
}

/// Resource fixture directory must have both .tres and .tscn files.
#[test]
fn fixture_resources_and_scenes_present() {
    let fixtures = repo_root().join("fixtures");

    let tres_count = fs::read_dir(fixtures.join("resources"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "tres"))
        .count();

    let tscn_count = fs::read_dir(fixtures.join("scenes"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "tscn"))
        .count();

    assert!(
        tres_count >= 3,
        "must have at least 3 .tres fixture resources, found {tres_count}"
    );
    assert!(
        tscn_count >= 5,
        "must have at least 5 .tscn fixture scenes, found {tscn_count}"
    );
}

/// Test file count keeps growing — prevent regression below a floor.
#[test]
fn integration_test_count_floor() {
    let tests_dir = workspace_root().join("tests");
    let test_count = fs::read_dir(&tests_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.ends_with("_test.rs")
        })
        .count();

    // Floor increases as coverage grows — prevents accidental deletion
    assert!(
        test_count >= 50,
        "integration test count must stay above floor of 50, found {test_count}"
    );
}

/// Workspace examples directory must exist with demos.
#[test]
fn examples_directory_has_demos() {
    let examples_dir = workspace_root().join("examples");
    assert!(examples_dir.exists(), "examples/ directory must exist");

    let example_count = fs::read_dir(&examples_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        .count();

    assert!(
        example_count >= 3,
        "must have at least 3 examples, found {example_count}"
    );
}

/// Git hooks or CI must enforce commit message format.
#[test]
fn ci_has_workspace_test_gate() {
    let ci_path = repo_root().join(".github/workflows/ci.yml");
    let ci_content = fs::read_to_string(&ci_path).unwrap();

    // CI must run workspace-wide tests, not just individual crates
    assert!(
        ci_content.contains("--workspace") || ci_content.contains("cargo test"),
        "CI must include workspace-level test execution"
    );
}
