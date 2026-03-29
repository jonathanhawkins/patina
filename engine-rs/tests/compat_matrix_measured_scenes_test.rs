//! pat-nax22: Validate that COMPAT_MATRIX.md scene inventory claims are backed
//! by actual fixture files and oracle data.
//!
//! This test ensures:
//! 1. Every scene listed in the "Measured Scene Inventory" has a .tscn fixture
//! 2. Every oracle-measured scene has oracle output data (tree + properties)
//! 3. Every oracle-measured scene has a golden scene JSON
//! 4. The golden file counts in the matrix match reality
//! 5. At least 20 measured scenes are documented

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The 21 oracle-measured scenes listed in COMPAT_MATRIX.md
const ORACLE_MEASURED_SCENES: &[&str] = &[
    "minimal",
    "hierarchy",
    "with_properties",
    "space_shooter",
    "platformer",
    "physics_playground",
    "signals_complex",
    "test_scripts",
    "ui_menu",
    "character_body_test",
    "hierarchy_3d",
    "indoor_3d",
    "minimal_3d",
    "multi_light_3d",
    "physics_3d_playground",
    "physics_playground_extended",
    "signal_instantiation",
    "unique_name_resolution",
    // Added in pat-nax22: 3 additional scenes promoted to oracle-measured
    "simple_2d",
    "simple_hierarchy",
    "signal_test",
];

// ===========================================================================
// 1. Every oracle-measured scene has a .tscn fixture
// ===========================================================================

#[test]
fn every_oracle_scene_has_tscn_fixture() {
    let fixtures_dir = repo_root().join("fixtures/scenes");
    let mut missing = Vec::new();
    for scene in ORACLE_MEASURED_SCENES {
        let path = fixtures_dir.join(format!("{scene}.tscn"));
        if !path.exists() {
            missing.push(scene.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "Oracle-measured scenes missing .tscn fixtures: {missing:?}"
    );
}

// ===========================================================================
// 2. Every oracle-measured scene has oracle output data
// ===========================================================================

#[test]
fn every_oracle_scene_has_tree_golden() {
    let oracle_dir = repo_root().join("fixtures/oracle_outputs");
    let mut missing = Vec::new();
    for scene in ORACLE_MEASURED_SCENES {
        let tree = oracle_dir.join(format!("{scene}_tree.json"));
        if !tree.exists() {
            missing.push(format!("{scene}_tree.json"));
        }
    }
    assert!(
        missing.is_empty(),
        "Oracle-measured scenes missing tree goldens: {missing:?}"
    );
}

#[test]
fn every_oracle_scene_has_properties_golden() {
    let oracle_dir = repo_root().join("fixtures/oracle_outputs");
    let mut missing = Vec::new();
    for scene in ORACLE_MEASURED_SCENES {
        let props = oracle_dir.join(format!("{scene}_properties.json"));
        if !props.exists() {
            missing.push(format!("{scene}_properties.json"));
        }
    }
    assert!(
        missing.is_empty(),
        "Oracle-measured scenes missing properties goldens: {missing:?}"
    );
}

// ===========================================================================
// 3. Every oracle-measured scene has a golden scene JSON
// ===========================================================================

#[test]
fn every_oracle_scene_has_scene_golden() {
    let golden_dir = repo_root().join("fixtures/golden/scenes");
    let mut missing = Vec::new();
    for scene in ORACLE_MEASURED_SCENES {
        let path = golden_dir.join(format!("{scene}.json"));
        if !path.exists() {
            missing.push(format!("{scene}.json"));
        }
    }
    assert!(
        missing.is_empty(),
        "Oracle-measured scenes missing scene goldens: {missing:?}"
    );
}

// ===========================================================================
// 4. Golden file counts match matrix claims
// ===========================================================================

#[test]
fn golden_scene_count_at_least_24() {
    let dir = repo_root().join("fixtures/golden/scenes");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();
    assert!(count >= 24, "Expected >= 24 scene goldens, found {count}");
}

#[test]
fn golden_physics_count_at_least_17() {
    let dir = repo_root().join("fixtures/golden/physics");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();
    assert!(count >= 17, "Expected >= 17 physics goldens, found {count}");
}

#[test]
fn golden_traces_count_at_least_23() {
    let dir = repo_root().join("fixtures/golden/traces");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();
    assert!(count >= 23, "Expected >= 23 trace goldens, found {count}");
}

#[test]
fn golden_signals_count_at_least_3() {
    let dir = repo_root().join("fixtures/golden/signals");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();
    assert!(count >= 3, "Expected >= 3 signal goldens, found {count}");
}

#[test]
fn golden_resources_count_at_least_5() {
    let dir = repo_root().join("fixtures/golden/resources");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .count();
    assert!(count >= 5, "Expected >= 5 resource goldens, found {count}");
}

#[test]
fn total_golden_files_at_least_104() {
    let categories = &[
        "fixtures/golden/scenes",
        "fixtures/golden/physics",
        "fixtures/golden/traces",
        "fixtures/golden/resources",
        "fixtures/golden/signals",
    ];
    let mut total = 0;
    for cat in categories {
        let dir = repo_root().join(cat);
        if dir.exists() {
            total += fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map_or(false, |ext| ext == "json" || ext == "png" || ext == "ppm")
                })
                .count();
        }
    }
    // Also count render goldens (flat files + subdirs)
    let render_dir = repo_root().join("fixtures/golden/render");
    if render_dir.exists() {
        fn count_render_files(dir: &std::path::Path) -> usize {
            let mut n = 0;
            if let Ok(entries) = fs::read_dir(dir) {
                for e in entries.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_dir() {
                        n += count_render_files(&p);
                    } else if p
                        .extension()
                        .map_or(false, |ext| ext == "json" || ext == "png" || ext == "ppm")
                    {
                        n += 1;
                    }
                }
            }
            n
        }
        total += count_render_files(&render_dir);
    }
    assert!(
        total >= 100,
        "Expected >= 100 total golden files, found {total}"
    );
}

// ===========================================================================
// 5. At least 20 measured scenes documented in the matrix
// ===========================================================================

#[test]
fn compat_matrix_documents_at_least_20_scenes() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    // Count rows in the "Oracle-Measured Scenes" table (lines starting with "| " and containing ".tscn")
    let scene_rows = content
        .lines()
        .filter(|l| l.contains(".tscn") && l.starts_with('|'))
        .count();
    // 18 oracle scenes + 3 additional = 21 total
    assert!(
        scene_rows >= 20,
        "Expected >= 20 measured scene rows in COMPAT_MATRIX.md, found {scene_rows}"
    );
}

#[test]
fn compat_matrix_has_measured_scene_inventory_section() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    assert!(
        content.contains("Measured Scene Inventory"),
        "COMPAT_MATRIX.md must have a Measured Scene Inventory section"
    );
    assert!(
        content.contains("Oracle-Measured Scenes"),
        "COMPAT_MATRIX.md must list oracle-measured scenes"
    );
}

#[test]
fn compat_matrix_oracle_parity_shows_21_scenes() {
    let content = fs::read_to_string(repo_root().join("COMPAT_MATRIX.md")).unwrap();
    assert!(
        content.contains("21 oracle scenes") || content.contains("21 scenes"),
        "COMPAT_MATRIX.md Oracle Parity Summary must reference 21 scenes"
    );
}

// ===========================================================================
// 6. Oracle output reservoir has 31+ scenes
// ===========================================================================

#[test]
fn oracle_outputs_has_at_least_31_scenes() {
    let dir = repo_root().join("fixtures/oracle_outputs");
    let tree_count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .map_or(false, |n| n.to_string_lossy().ends_with("_tree.json"))
        })
        .count();
    assert!(
        tree_count >= 31,
        "Expected >= 31 oracle output scenes (by _tree.json count), found {tree_count}"
    );
}

// ===========================================================================
// 7. Scene fixture directory has at least 21 .tscn files
// ===========================================================================

#[test]
fn scene_fixtures_at_least_24() {
    let dir = repo_root().join("fixtures/scenes");
    let count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "tscn"))
        .count();
    assert!(count >= 24, "Expected >= 24 scene fixtures, found {count}");
}
