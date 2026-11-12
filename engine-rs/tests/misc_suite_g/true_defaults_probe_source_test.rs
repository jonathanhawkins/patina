//! pat-ghfv0.42: true_defaults must come from the ClassDB probe artifact, not a
//! hand-curated Python table.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn true_defaults_json_exists_and_has_classes() {
    let path = repo_root().join("fixtures/editor_oracle/true_defaults.json");
    assert!(path.is_file(), "missing {}", path.display());
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).expect("json");
    let classes = v
        .get("classes")
        .and_then(|c| c.as_object())
        .expect("classes object");
    assert!(
        classes.contains_key("Camera3D"),
        "probe artifact must include Camera3D.current default"
    );
    assert!(
        classes
            .get("Camera3D")
            .and_then(|c| c.get("current"))
            .is_some(),
        "Camera3D.current true default required"
    );
    assert!(
        v.get("probe")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .contains("editor_true_defaults_probe"),
        "artifact must cite the ClassDB probe script"
    );
}

#[test]
fn gen_inspector_schema_has_no_hand_curated_known_true_defaults_table() {
    let path = repo_root().join("tools/oracle/gen_inspector_schema.py");
    let src = fs::read_to_string(&path).expect("read generator");
    assert!(
        !src.contains("KNOWN_TRUE_DEFAULTS = {"),
        "generator must not define a hand-curated KNOWN_TRUE_DEFAULTS table"
    );
    assert!(
        src.contains("true_defaults.json"),
        "generator must load true_defaults.json"
    );
    assert!(
        src.contains("load_true_defaults_table"),
        "generator must use load_true_defaults_table()"
    );
}

#[test]
fn true_defaults_probe_script_exists() {
    let path = repo_root().join("tools/oracle/editor_true_defaults_probe.gd");
    assert!(path.is_file(), "missing probe script {}", path.display());
    let src = fs::read_to_string(&path).unwrap();
    assert!(
        src.contains("class_get_property_default_value"),
        "probe must call ClassDB.class_get_property_default_value"
    );
}
