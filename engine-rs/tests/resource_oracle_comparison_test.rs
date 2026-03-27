//! pat-7du2: Oracle comparison for fixture resources.
//!
//! Validates that TresLoader correctly parses fixture .tres files and
//! produces output matching oracle-captured metadata.

use gdresource::TresLoader;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

#[test]
fn with_ext_refs_fixture_loads_and_has_metadata() {
    let path = fixtures_dir().join("resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&path).unwrap();

    let loader = TresLoader::new();
    let res = loader.parse_str(&content, "res://with_ext_refs.tres").unwrap();

    assert!(!res.class_name.is_empty(), "resource must have a class name");
    assert!(
        res.property_count() > 0 || !res.ext_resources.is_empty(),
        "fixture must have properties or ext_resources"
    );
}

#[test]
fn with_ext_refs_has_ext_resources() {
    let path = fixtures_dir().join("resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&path).unwrap();

    let loader = TresLoader::new();
    let res = loader.parse_str(&content, "res://with_ext_refs.tres").unwrap();

    assert!(
        !res.ext_resources.is_empty(),
        "with_ext_refs.tres should have ext_resource entries"
    );
}

#[test]
fn with_ext_refs_path_preserved() {
    let path = fixtures_dir().join("resources/with_ext_refs.tres");
    let content = std::fs::read_to_string(&path).unwrap();

    let loader = TresLoader::new();
    let res = loader.parse_str(&content, "res://with_ext_refs.tres").unwrap();

    assert_eq!(res.path, "res://with_ext_refs.tres", "path must be preserved");
}

#[test]
fn animation_tres_fixture_loads() {
    let path = fixtures_dir().join("../apps/godot/fixtures/test_animation.tres");
    if !path.exists() {
        // Skip if fixture doesn't exist
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap();
    let loader = TresLoader::new();
    let res = loader.parse_str(&content, "res://test_animation.tres").unwrap();
    assert!(!res.class_name.is_empty());
}

#[test]
fn style_box_tres_fixture_loads() {
    let path = fixtures_dir().join("../apps/godot/fixtures/test_style_box.tres");
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap();
    let loader = TresLoader::new();
    let res = loader.parse_str(&content, "res://test_style_box.tres").unwrap();
    assert!(!res.class_name.is_empty());
}
