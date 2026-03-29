//! pat-zaafu: define the 3D fixture corpus from the audited Phase 6 class families.
//!
//! This test validates that the checked-in 3D golden fixtures cover all
//! "Measured" families from `prd/PHASE6_3D_PARITY_AUDIT.md`. It also guards
//! the fixture-to-family mapping so the corpus stays aligned with the audit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn golden_scenes_dir() -> PathBuf {
    repo_root().join("fixtures/golden/scenes")
}

/// Load a golden scene JSON and extract all class names from its nodes.
fn extract_classes(path: &std::path::Path) -> BTreeSet<String> {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let val: serde_json::Value =
        serde_json::from_str(&data).unwrap_or_else(|e| panic!("bad JSON {}: {e}", path.display()));

    let mut classes = BTreeSet::new();
    // Try both data.nodes (newer format) and nodes (older format)
    let nodes = val
        .get("data")
        .and_then(|d| d.get("nodes"))
        .or_else(|| val.get("nodes"))
        .and_then(|n| n.as_array());

    if let Some(nodes) = nodes {
        for node in nodes {
            if let Some(class) = node.get("class").and_then(|c| c.as_str()) {
                if !class.is_empty() {
                    classes.insert(class.to_string());
                }
            }
        }
    }
    classes
}

/// Marker classes that indicate a fixture is a 3D scene.
const MARKER_3D_CLASSES: &[&str] = &[
    "Node3D",
    "Camera3D",
    "MeshInstance3D",
    "DirectionalLight3D",
    "OmniLight3D",
    "SpotLight3D",
    "RigidBody3D",
    "StaticBody3D",
    "CharacterBody3D",
    "CollisionShape3D",
    "WorldEnvironment",
    "FogVolume",
    "Skeleton3D",
    "Area3D",
];

/// Build a map of fixture_name -> set of classes, for all 3D golden scenes.
fn build_3d_fixture_map() -> BTreeMap<String, BTreeSet<String>> {
    let dir = golden_scenes_dir();
    let mut map = BTreeMap::new();

    for entry in std::fs::read_dir(&dir).expect("cannot read golden scenes dir") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }
        let classes = extract_classes(&path);
        let is_3d = classes.iter().any(|c| MARKER_3D_CLASSES.contains(&c.as_str()));
        if is_3d {
            let name = path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            map.insert(name, classes);
        }
    }
    map
}

// ─────────────────────────────────────────────────────────────────────
// 1. The audit doc exists and has the fixture corpus section
// ─────────────────────────────────────────────────────────────────────

#[test]
fn audit_doc_has_fixture_corpus_section() {
    let path = repo_root().join("prd/PHASE6_3D_PARITY_AUDIT.md");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("## 3D Fixture Corpus"),
        "audit doc must have '## 3D Fixture Corpus' section"
    );
    assert!(
        content.contains("Corpus Guard"),
        "audit doc must reference the corpus guard test"
    );
}

// ─────────────────────────────────────────────────────────────────────
// 2. Known 3D fixtures exist as golden JSON
// ─────────────────────────────────────────────────────────────────────

/// The expected 3D fixtures from the audit's fixture coverage tables.
const EXPECTED_3D_FIXTURES: &[&str] = &[
    "minimal_3d",
    "hierarchy_3d",
    "indoor_3d",
    "outdoor_3d",
    "multi_light_3d",
    "spotlight_gallery_3d",
    "animated_scene_3d",
    "foggy_terrain_3d",
    "physics_3d_playground",
    "vehicle_3d",
];

#[test]
fn expected_3d_golden_fixtures_exist() {
    let dir = golden_scenes_dir();
    for name in EXPECTED_3D_FIXTURES {
        let path = dir.join(format!("{name}.json"));
        assert!(
            path.exists(),
            "expected 3D golden fixture must exist: {name}.json"
        );
    }
}

#[test]
fn expected_3d_tscn_fixtures_exist() {
    let dir = repo_root().join("fixtures/scenes");
    for name in EXPECTED_3D_FIXTURES {
        let path = dir.join(format!("{name}.tscn"));
        assert!(
            path.exists(),
            "expected 3D scene fixture must exist: {name}.tscn"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 3. Measured families have fixture coverage
// ─────────────────────────────────────────────────────────────────────

/// Families classified as "Measured" in the Phase 6 audit.
/// Each must appear in at least one checked-in 3D golden fixture.
const MEASURED_FAMILIES: &[&str] = &[
    "Node3D",
    "Camera3D",
    "MeshInstance3D",
    "DirectionalLight3D",
    "OmniLight3D",
    "SpotLight3D",
    "StaticBody3D",
    "RigidBody3D",
    "CollisionShape3D",
];

#[test]
fn measured_families_have_fixture_coverage() {
    let fixture_map = build_3d_fixture_map();
    assert!(
        !fixture_map.is_empty(),
        "must have at least one 3D golden fixture"
    );

    for family in MEASURED_FAMILIES {
        let covering_fixtures: Vec<&str> = fixture_map
            .iter()
            .filter(|(_, classes)| classes.contains(*family))
            .map(|(name, _)| name.as_str())
            .collect();

        assert!(
            !covering_fixtures.is_empty(),
            "measured family '{family}' must have at least one golden fixture, \
             but found none in {} 3D fixtures",
            fixture_map.len()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 4. Implemented-not-yet-measured families that DO have fixture presence
// ─────────────────────────────────────────────────────────────────────

#[test]
fn implemented_families_with_fixtures() {
    let fixture_map = build_3d_fixture_map();

    // These are "Implemented, not yet measured" but DO appear in at least one fixture
    let expected_present = [
        ("WorldEnvironment", "foggy_terrain_3d"),
        ("FogVolume", "foggy_terrain_3d"),
        ("Skeleton3D", "animated_scene_3d"),
    ];

    for (family, expected_fixture) in &expected_present {
        let classes = fixture_map.get(*expected_fixture).unwrap_or_else(|| {
            panic!("expected fixture '{expected_fixture}' not found in 3D fixture map")
        });
        assert!(
            classes.contains(*family),
            "'{family}' should appear in '{expected_fixture}' per the audit's fixture mapping"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 5. Fixture count guard — detect new or removed 3D fixtures
// ─────────────────────────────────────────────────────────────────────

#[test]
fn fixture_count_guard() {
    let fixture_map = build_3d_fixture_map();
    // We expect at least the 10 known 3D fixtures. If new ones are added,
    // this test passes (it's a floor, not a ceiling).
    assert!(
        fixture_map.len() >= EXPECTED_3D_FIXTURES.len(),
        "expected at least {} 3D golden fixtures, found {}. \
         If fixtures were removed, update the audit doc and this test.",
        EXPECTED_3D_FIXTURES.len(),
        fixture_map.len()
    );
}

// ─────────────────────────────────────────────────────────────────────
// 6. Class coverage summary — informational, catches regression
// ─────────────────────────────────────────────────────────────────────

#[test]
fn class_coverage_summary() {
    let fixture_map = build_3d_fixture_map();

    // Collect all classes seen across all 3D fixtures
    let all_classes: BTreeSet<String> = fixture_map
        .values()
        .flat_map(|classes| classes.iter().cloned())
        .collect();

    // Must see at least the measured + some implemented families
    assert!(
        all_classes.len() >= 9,
        "expected at least 9 distinct 3D class families across fixtures, found {}",
        all_classes.len()
    );

    // Core 3D rendering classes must be present
    for required in &["Node3D", "Camera3D", "MeshInstance3D"] {
        assert!(
            all_classes.contains(*required),
            "'{required}' must appear somewhere in the 3D fixture corpus"
        );
    }

    // Core 3D physics classes must be present
    for required in &["RigidBody3D", "StaticBody3D", "CollisionShape3D"] {
        assert!(
            all_classes.contains(*required),
            "'{required}' must appear somewhere in the 3D fixture corpus"
        );
    }

    // All three light types must be present
    for required in &["DirectionalLight3D", "OmniLight3D", "SpotLight3D"] {
        assert!(
            all_classes.contains(*required),
            "'{required}' must appear somewhere in the 3D fixture corpus"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// 7. Evidence test files cited in the audit for non-fixture families exist
// ─────────────────────────────────────────────────────────────────────

#[test]
fn non_fixture_measured_families_have_test_evidence() {
    // CharacterBody3D and query objects are measured via tests, not scene goldens
    let test_evidence = [
        (
            "CharacterBody3D",
            "tests/characterbody3d_move_and_slide_test.rs",
        ),
        (
            "PhysicsRayQuery3D / PhysicsShapeQuery3D",
            "tests/physics_ray_shape_query3d_test.rs",
        ),
    ];

    let engine_dir = repo_root().join("engine-rs");
    for (family, test_path) in &test_evidence {
        let path = engine_dir.join(test_path);
        assert!(
            path.exists(),
            "non-fixture measured family '{family}' requires test evidence at {test_path}"
        );
    }
}
