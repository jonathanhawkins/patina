//! Golden staleness and orphan detection tests (pat-gkv).
//!
//! These tests ensure that:
//! 1. Every golden file in `fixtures/golden/` is referenced by at least one
//!    test or tool in the repository (no orphaned goldens).
//! 2. Scene and resource goldens can be regenerated from their source fixtures
//!    and produce identical output (no stale goldens).
//!
//! Physics and trace goldens are validated by their respective integration
//! tests (`physics_integration_test`, `trace_parity_test`), so this file
//! focuses on scene/resource goldens and cross-cutting orphan detection.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs parent")
        .to_path_buf()
}

fn golden_dir() -> PathBuf {
    repo_root().join("fixtures").join("golden")
}

/// Recursively collect all files under `dir`.
fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_files(&path));
        } else {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Search for `needle` (a filename stem or basename) in all `.rs` and `.py`
/// files under the given directories. Returns true if at least one file
/// contains the needle.
fn is_referenced(needle: &str, search_dirs: &[PathBuf]) -> bool {
    for dir in search_dirs {
        if !dir.is_dir() {
            continue;
        }
        for entry in walkdir(dir) {
            let ext = entry.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "py" | "md" | "sh" | "toml") {
                continue;
            }
            if let Ok(contents) = std::fs::read_to_string(&entry) {
                if contents.contains(needle) {
                    return true;
                }
            }
        }
    }
    false
}

/// Simple recursive file walker (no external dependency).
fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and .git/
            let name = path.file_name().unwrap().to_str().unwrap_or("");
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            out.extend(walkdir(&path));
        } else {
            out.push(path);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Test: No orphaned golden files
// ---------------------------------------------------------------------------

#[test]
fn no_orphaned_golden_files() {
    let root = repo_root();
    let golden = golden_dir();
    let all_goldens = collect_files(&golden);

    // Directories to search for references.
    let search_dirs = vec![
        root.join("engine-rs"),
        root.join("tools"),
        root.join("fixtures"), // some goldens reference each other or are listed in manifests
    ];

    let mut orphans = Vec::new();

    for path in &all_goldens {
        let basename = path.file_name().unwrap().to_str().unwrap();
        let stem = path.file_stem().unwrap().to_str().unwrap();

        // A golden is "referenced" if its basename or stem appears in any
        // source file. We check both to handle cases like `foo.json` being
        // referenced as `"foo"` or `"foo.json"`.
        if !is_referenced(basename, &search_dirs) && !is_referenced(stem, &search_dirs) {
            orphans.push(
                path.strip_prefix(&root)
                    .unwrap_or(path)
                    .display()
                    .to_string(),
            );
        }
    }

    assert!(
        orphans.is_empty(),
        "Found {} orphaned golden file(s) with no test or tool reference:\n  - {}",
        orphans.len(),
        orphans.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Test: Scene goldens are not stale
// ---------------------------------------------------------------------------
//
// For scene goldens, we can regenerate them by loading the source .tscn file,
// parsing it through PackedScene, instancing into a SceneTree, and dumping to
// JSON. If the output differs from the stored golden, the golden is stale.

#[test]
fn scene_goldens_are_fresh() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
    use gdscene::scene_tree::SceneTree;
    use gdvariant::serialize::to_json;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    let root = repo_root();
    let fixtures = root.join("fixtures");
    let scene_golden_dir = golden_dir().join("scenes");

    if !scene_golden_dir.is_dir() {
        return;
    }

    // Map of golden filename -> source .tscn filename.
    // Only include goldens that are generated from .tscn source fixtures.
    let scene_map: Vec<(&str, &str)> = vec![
        ("minimal.json", "scenes/minimal.tscn"),
        ("hierarchy.json", "scenes/hierarchy.tscn"),
        ("with_properties.json", "scenes/with_properties.tscn"),
        ("platformer.json", "scenes/platformer.tscn"),
        ("ui_menu.json", "scenes/ui_menu.tscn"),
        ("physics_playground.json", "scenes/physics_playground.tscn"),
        ("signals_complex.json", "scenes/signals_complex.tscn"),
        (
            "unique_name_resolution.json",
            "scenes/unique_name_resolution.tscn",
        ),
        // character_body_test, space_shooter, test_scripts goldens are oracle-generated
        // (different property set) — validated by oracle_parity_test instead.
    ];

    fn dump_node(tree: &SceneTree, node_id: gdscene::node::NodeId) -> Value {
        let node = tree.get_node(node_id).unwrap();
        let path = tree.node_path(node_id).unwrap();
        let mut props = BTreeMap::new();
        for (key, value) in node.properties() {
            props.insert(key.clone(), to_json(value));
        }
        let children: Vec<Value> = node
            .children()
            .iter()
            .map(|&child_id| dump_node(tree, child_id))
            .collect();
        json!({
            "name": node.name(),
            "class": node.class_name(),
            "path": path,
            "children": children,
            "properties": props,
        })
    }

    let mut stale = Vec::new();

    for (golden_name, tscn_rel_path) in &scene_map {
        let tscn_path = fixtures.join(tscn_rel_path);
        let golden_path = scene_golden_dir.join(golden_name);

        if !tscn_path.exists() || !golden_path.exists() {
            // Source or golden missing — skip (other tests catch missing fixtures).
            continue;
        }

        let source = std::fs::read_to_string(&tscn_path).unwrap();
        let packed = match PackedScene::from_tscn(&source) {
            Ok(p) => p,
            Err(_) => continue, // Parse error — not a staleness issue.
        };

        let mut tree = SceneTree::new();
        let root_id = tree.root_id();
        if add_packed_scene_to_tree(&mut tree, root_id, &packed).is_err() {
            continue;
        }

        // Find scene root (first child of root).
        let root_node = tree.get_node(root_id).unwrap();
        let scene_root_id = match root_node.children().first() {
            Some(&id) => id,
            None => continue,
        };

        let actual = dump_node(&tree, scene_root_id);

        // Read stored golden and extract the comparable portion.
        let golden_str = std::fs::read_to_string(&golden_path).unwrap();
        let golden_val: Value = serde_json::from_str(&golden_str).unwrap();

        // Goldens have a "nodes" envelope — extract root node for comparison.
        let golden_root = golden_val
            .get("nodes")
            .and_then(|n| n.as_array())
            .and_then(|a| a.first())
            .cloned();

        let comparable_golden = match golden_root {
            Some(g) => g,
            None => {
                // No "nodes" envelope — compare directly.
                golden_val.clone()
            }
        };

        // Normalize both to canonical JSON for comparison.
        let actual_str = serde_json::to_string_pretty(&actual).unwrap();
        let golden_canonical = serde_json::to_string_pretty(&comparable_golden).unwrap();

        if actual_str != golden_canonical {
            stale.push(format!(
                "fixtures/golden/scenes/{golden_name} (from {tscn_rel_path})"
            ));
        }
    }

    assert!(
        stale.is_empty(),
        "Found {} stale scene golden(s) — regenerate with the golden test suite:\n  - {}",
        stale.len(),
        stale.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Test: Every fixture scene has a corresponding golden file
// ---------------------------------------------------------------------------
//
// Ensures that every .tscn file in fixtures/scenes/ has a matching .json golden
// in fixtures/golden/scenes/. A missing golden means test coverage has a gap.

#[test]
fn fixture_scenes_have_goldens() {
    let root = repo_root();
    let scenes_dir = root.join("fixtures").join("scenes");
    let golden_scenes_dir = golden_dir().join("scenes");

    if !scenes_dir.is_dir() {
        panic!("fixtures/scenes/ directory does not exist");
    }

    let mut missing = Vec::new();

    for entry in std::fs::read_dir(&scenes_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("tscn") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let golden_path = golden_scenes_dir.join(format!("{stem}.json"));
        if !golden_path.exists() {
            missing.push(stem.to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "Found {} fixture scene(s) with no golden file in fixtures/golden/scenes/:\n  - {}\n\
         Regenerate goldens with: cargo test --workspace -- golden",
        missing.len(),
        missing.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Test: Golden file inventory is complete
// ---------------------------------------------------------------------------
//
// Ensures we know about all golden subdirectories and that none are empty.

#[test]
fn golden_directories_are_populated() {
    let golden = golden_dir();
    let expected_subdirs = ["physics", "render", "resources", "scenes", "traces"];

    for subdir in &expected_subdirs {
        let dir = golden.join(subdir);
        assert!(
            dir.is_dir(),
            "Expected golden subdirectory missing: {subdir}"
        );
        let files = collect_files(&dir);
        assert!(
            !files.is_empty(),
            "Golden subdirectory {subdir}/ is empty — expected at least one golden file"
        );
    }
}

// ---------------------------------------------------------------------------
// Test: All golden JSON files are valid JSON
// ---------------------------------------------------------------------------

#[test]
fn all_golden_json_files_parse() {
    let golden = golden_dir();
    let all_files = collect_files(&golden);

    let mut failures = Vec::new();

    for path in &all_files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "json" {
            continue;
        }

        let contents = std::fs::read_to_string(path).unwrap();
        if serde_json::from_str::<serde_json::Value>(&contents).is_err() {
            let rel = path
                .strip_prefix(&repo_root())
                .unwrap_or(path)
                .display()
                .to_string();
            failures.push(rel);
        }
    }

    assert!(
        failures.is_empty(),
        "Found {} golden JSON file(s) that fail to parse:\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Test: Golden files match the pinned upstream version (pat-3h6a)
// ---------------------------------------------------------------------------
//
// When the upstream Godot submodule is repinned to a new commit, golden files
// may need regeneration. This test compares the commit recorded in
// `fixtures/golden/UPSTREAM_VERSION` against the actual submodule HEAD.
// If they diverge, the test fails with instructions to regenerate goldens.

#[test]
fn upstream_version_matches_golden_stamp() {
    let root = repo_root();
    let stamp_path = golden_dir().join("UPSTREAM_VERSION");
    let submodule_dir = root.join("upstream").join("godot");

    // Read the recorded version stamp.
    let stamp = match std::fs::read_to_string(&stamp_path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            panic!(
                "fixtures/golden/UPSTREAM_VERSION is missing.\n\
                 After regenerating goldens, write the upstream commit hash to this file:\n  \
                 git -C upstream/godot rev-parse HEAD > fixtures/golden/UPSTREAM_VERSION"
            );
        }
    };

    // Resolve the current submodule HEAD via .git file or directory.
    let current_commit = resolve_submodule_head(&submodule_dir);

    let current = match &current_commit {
        Some(c) => c.as_str(),
        None => {
            // Submodule not checked out (e.g., shallow clone in CI). Skip gracefully.
            eprintln!(
                "WARNING: upstream/godot submodule not available — \
                 skipping version staleness check"
            );
            return;
        }
    };

    assert_eq!(
        stamp, current,
        "Golden files were generated against upstream commit:\n  {stamp}\n\
         but the upstream/godot submodule is now at:\n  {current}\n\n\
         Goldens are likely stale. Regenerate them:\n  \
         1. cargo test --workspace  (regenerates physics/render/scene goldens)\n  \
         2. Run patina-runner --event-trace on each fixture scene (trace goldens)\n  \
         3. Update the stamp:  git -C upstream/godot rev-parse HEAD > fixtures/golden/UPSTREAM_VERSION"
    );
}

/// Resolve the HEAD commit of a git submodule directory.
///
/// Handles both regular `.git` directories and submodule `.git` files
/// that contain a `gitdir:` pointer.
fn resolve_submodule_head(submodule_dir: &Path) -> Option<String> {
    if !submodule_dir.exists() {
        return None;
    }

    // Try reading HEAD directly via git plumbing files.
    // First check if .git is a file (submodule) pointing to the real git dir.
    let dot_git = submodule_dir.join(".git");
    let git_dir = if dot_git.is_file() {
        // Parse "gitdir: <path>" and resolve relative to submodule_dir.
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let gitdir_line = content.trim();
        let rel = gitdir_line.strip_prefix("gitdir: ")?;
        submodule_dir.join(rel).canonicalize().ok()?
    } else if dot_git.is_dir() {
        dot_git
    } else {
        return None;
    };

    // Read HEAD — could be a ref or a detached commit hash.
    let head_content = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head_content.trim();

    if let Some(ref_path) = head.strip_prefix("ref: ") {
        // Symbolic ref — resolve it.
        let ref_file = git_dir.join(ref_path);
        std::fs::read_to_string(ref_file)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        // Detached HEAD — commit hash directly.
        Some(head.to_string())
    }
}
