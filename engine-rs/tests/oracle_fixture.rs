use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

pub fn load_json_fixture(path: &Path) -> Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to load fixture {}: {e}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", path.display()))
}

pub fn load_generated_scene_fixture(file_name: &str) -> Value {
    let path = fixtures_dir().join("golden/scenes").join(file_name);
    let envelope = load_json_fixture(&path);

    let fixture_id = envelope
        .get("fixture_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("generated fixture {} is missing fixture_id", path.display()));
    let capture_type = envelope
        .get("capture_type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "generated fixture {} is missing capture_type",
                path.display()
            )
        });
    let upstream_version = envelope
        .get("upstream_version")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "generated fixture {} is missing upstream_version",
                path.display()
            )
        });
    let upstream_commit = envelope
        .get("upstream_commit")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "generated fixture {} is missing upstream_commit",
                path.display()
            )
        });
    let generated_at = envelope
        .get("generated_at")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "generated fixture {} is missing generated_at",
                path.display()
            )
        });

    assert_eq!(
        capture_type,
        "scene_tree",
        "generated fixture {} must be a scene_tree capture, got {}",
        path.display(),
        capture_type
    );
    assert_ne!(
        upstream_version,
        "pending",
        "generated fixture {} must not use placeholder upstream_version",
        path.display()
    );
    assert!(
        !upstream_commit.is_empty(),
        "generated fixture {} must not have an empty upstream_commit",
        path.display()
    );
    assert!(
        !generated_at.is_empty(),
        "generated fixture {} must not have an empty generated_at",
        path.display()
    );

    let data = envelope
        .get("data")
        .unwrap_or_else(|| panic!("generated fixture {} is missing data", path.display()))
        .clone();
    assert!(
        data.get("nodes").and_then(Value::as_array).is_some(),
        "generated fixture {} must contain data.nodes for fixture_id {}",
        path.display(),
        fixture_id
    );
    data
}
