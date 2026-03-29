//! pat-57aw6: Phase 6 parity report artifact aligned with the audit.
//!
//! Verifies that the Phase 6 parity report artifact stays aligned with the
//! committed 3D scene and physics golden corpus, and that its classification
//! matches the audited support claims in prd/PHASE6_3D_PARITY_AUDIT.md.

mod oracle_fixture;

use std::path::PathBuf;

use oracle_fixture::load_json_fixture;
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn report_path() -> PathBuf {
    repo_root().join("fixtures/patina_outputs/real_3d_demo_parity_report.json")
}

fn count_scene_classes(nodes: &[Value], classes: &[&str]) -> usize {
    nodes.iter()
        .filter(|node| {
            node.get("class")
                .and_then(Value::as_str)
                .is_some_and(|class_name| classes.contains(&class_name))
        })
        .count()
}

#[test]
fn report_artifact_exists_and_has_phase6_metadata() {
    let path = report_path();
    assert!(path.exists(), "missing report artifact: {}", path.display());

    let report = load_json_fixture(&path);
    assert_eq!(report["report_id"], "real_3d_demo_parity_report");
    assert_eq!(report["bead_id"], "pat-57aw6");
    assert_eq!(report["phase"], "Phase 6: 3D Runtime Slice");
    assert_eq!(report["status"], "implemented");
    assert_eq!(report["upstream_version"], "4.6.1-stable");
    assert_eq!(
        report["upstream_commit"],
        "14d19694e0c88a3f9e82d899a0400f27a24c176e"
    );
}

#[test]
fn report_cites_phase6_audit_and_classifies_3d_families() {
    let report = load_json_fixture(&report_path());

    // Must cite the audit source
    assert_eq!(
        report["audit_source"].as_str().unwrap(),
        "prd/PHASE6_3D_PARITY_AUDIT.md",
        "report must cite the Phase 6 parity audit document"
    );
    let audit_path = repo_root().join(report["audit_source"].as_str().unwrap());
    assert!(audit_path.exists(), "audit source file must exist: {}", audit_path.display());

    // Must have classification with all three tiers
    let classification = &report["classification"];
    let measured = classification["measured"]
        .as_array()
        .expect("classification.measured should be an array");
    let implemented = classification["implemented_not_yet_measured"]
        .as_array()
        .expect("classification.implemented_not_yet_measured should be an array");
    let deferred = classification["deferred"]
        .as_array()
        .expect("classification.deferred should be an array");

    assert!(!measured.is_empty(), "measured families should not be empty");
    assert!(!implemented.is_empty(), "implemented-not-yet-measured families should not be empty");
    assert!(!deferred.is_empty(), "deferred families should not be empty");

    // Each measured family must have evidence
    for family in measured {
        let name = family["family"].as_str().expect("measured family must have a name");
        let evidence = family["evidence"].as_array().expect("measured family must have evidence");
        assert!(!evidence.is_empty(), "measured family '{name}' must cite at least one evidence item");
    }

    // Each implemented-not-yet-measured family must have a reason
    for family in implemented {
        let name = family["family"].as_str().expect("implemented family must have a name");
        let reason = family["reason"].as_str().expect("implemented family must have a reason");
        assert!(!reason.is_empty(), "implemented family '{name}' must explain why not yet measured");
    }

    // Each deferred family must have a reason
    for family in deferred {
        let name = family["family"].as_str().expect("deferred family must have a name");
        let reason = family["reason"].as_str().expect("deferred family must have a reason");
        assert!(!reason.is_empty(), "deferred family '{name}' must explain why deferred");
    }

    // Key families from the audit must be classified somewhere
    let all_families: Vec<&str> = measured.iter()
        .chain(implemented.iter())
        .chain(deferred.iter())
        .filter_map(|f| f["family"].as_str())
        .collect();

    // Key families from the Phase 6 audit must all be classified
    for required in &[
        "Node3D", "Camera3D", "MeshInstance3D", "DirectionalLight3D",
        "OmniLight3D", "SpotLight3D", "CollisionShape3D",
        "RigidBody3D", "StaticBody3D", "CharacterBody3D",
        "Area3D", "ReflectionProbe", "FogVolume", "Decal",
        "VehicleBody3D", "SoftBody3D", "NavigationRegion3D",
    ] {
        assert!(
            all_families.contains(required),
            "audit-required family '{required}' must appear in the classification"
        );
    }
}

#[test]
fn report_scene_inventory_matches_scene_goldens() {
    let report = load_json_fixture(&report_path());
    let scene_fixtures = report["scene_fixtures"]
        .as_array()
        .expect("scene_fixtures should be an array");
    assert_eq!(scene_fixtures.len(), 5, "expected 5 representative 3D scenes");

    let mut total_nodes = 0usize;
    let mut total_cameras = 0usize;
    let mut total_lights = 0usize;
    let mut total_bodies = 0usize;

    for fixture in scene_fixtures {
        let scene_path = repo_root().join(
            fixture["scene_path"]
                .as_str()
                .expect("scene_path should be present"),
        );
        let golden_path = repo_root().join(
            fixture["golden_path"]
                .as_str()
                .expect("golden_path should be present"),
        );

        assert!(scene_path.exists(), "missing scene fixture: {}", scene_path.display());
        assert!(
            golden_path.exists(),
            "missing golden scene fixture: {}",
            golden_path.display()
        );

        let golden = load_json_fixture(&golden_path);
        assert_eq!(golden["capture_type"], "scene_tree");
        assert_eq!(golden["upstream_version"], report["upstream_version"]);
        assert_eq!(golden["upstream_commit"], report["upstream_commit"]);

        let nodes = golden["data"]["nodes"]
            .as_array()
            .expect("golden scene should have data.nodes");
        let node_count = nodes.len();
        let camera_count = count_scene_classes(nodes, &["Camera3D"]);
        let light_count = count_scene_classes(
            nodes,
            &["DirectionalLight3D", "OmniLight3D", "SpotLight3D"],
        );
        let physics_body_count = count_scene_classes(
            nodes,
            &["StaticBody3D", "RigidBody3D", "CharacterBody3D", "Area3D"],
        );

        assert_eq!(
            fixture["expected_node_count"].as_u64().unwrap() as usize,
            node_count,
            "node count mismatch for {}",
            fixture["name"].as_str().unwrap()
        );
        assert_eq!(
            fixture["camera_count"].as_u64().unwrap() as usize,
            camera_count,
            "camera count mismatch for {}",
            fixture["name"].as_str().unwrap()
        );
        assert_eq!(
            fixture["light_count"].as_u64().unwrap() as usize,
            light_count,
            "light count mismatch for {}",
            fixture["name"].as_str().unwrap()
        );
        assert_eq!(
            fixture["physics_body_count"].as_u64().unwrap() as usize,
            physics_body_count,
            "physics body count mismatch for {}",
            fixture["name"].as_str().unwrap()
        );

        total_nodes += node_count;
        total_cameras += camera_count;
        total_lights += light_count;
        total_bodies += physics_body_count;
    }

    assert_eq!(report["summary"]["scene_fixture_count"], 5);
    assert_eq!(report["summary"]["total_scene_nodes"], total_nodes as u64);
    assert_eq!(report["summary"]["total_cameras"], total_cameras as u64);
    assert_eq!(report["summary"]["total_lights"], total_lights as u64);
    assert_eq!(report["summary"]["total_physics_bodies"], total_bodies as u64);
}

#[test]
fn report_physics_inventory_and_evidence_are_rerunnable() {
    let report = load_json_fixture(&report_path());
    let physics_goldens = report["physics_goldens"]
        .as_array()
        .expect("physics_goldens should be an array");
    assert_eq!(physics_goldens.len(), 3, "expected 3 physics golden traces");

    let mut total_frames = 0usize;
    for golden in physics_goldens {
        let path = repo_root().join(
            golden["golden_path"]
                .as_str()
                .expect("golden_path should be present"),
        );
        assert!(path.exists(), "missing physics golden: {}", path.display());
        let entries = load_json_fixture(&path)
            .as_array()
            .cloned()
            .expect("physics golden should be a JSON array");
        let frame_count = entries.len();
        assert_eq!(
            golden["frame_count"].as_u64().unwrap() as usize,
            frame_count,
            "frame count mismatch for {}",
            golden["name"].as_str().unwrap()
        );
        total_frames += frame_count;
    }

    assert_eq!(report["summary"]["physics_trace_count"], 3);
    assert_eq!(report["summary"]["physics_trace_frames"], total_frames as u64);

    let test_files = report["evidence"]["test_files"]
        .as_array()
        .expect("evidence.test_files should be an array");
    for test_file in test_files {
        let path = repo_root().join(test_file.as_str().expect("test file path should be a string"));
        assert!(path.exists(), "missing evidence test file: {}", path.display());
    }

    let tests = report["evidence"]["tests"]
        .as_array()
        .expect("evidence.tests should be an array");
    for test_cmd in tests {
        let cmd = test_cmd.as_str().expect("test command should be a string");
        assert!(
            cmd.starts_with("cargo test -p patina-engine --test "),
            "test command should be concrete and rerunnable: {cmd}"
        );
    }
}
