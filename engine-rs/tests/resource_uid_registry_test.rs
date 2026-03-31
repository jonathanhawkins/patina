//! pat-6mdk: Resource UID registry for uid:// references.
//!
//! Validates UidRegistry bidirectional mapping, UnifiedLoader uid:// resolution,
//! and .tres UID parsing.

use gdcore::ResourceUid;
use gdresource::{TresLoader, UidRegistry, UnifiedLoader};

#[test]
fn registry_register_and_lookup() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(1);
    reg.register(uid, "res://a.tres");
    assert_eq!(reg.lookup_uid(uid), Some("res://a.tres"));
    assert_eq!(reg.lookup_path("res://a.tres"), Some(uid));
}

#[test]
fn registry_overwrite_replaces_mapping() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(1);
    reg.register(uid, "res://old.tres");
    reg.register(uid, "res://new.tres");
    assert_eq!(reg.lookup_uid(uid), Some("res://new.tres"));
    assert_eq!(reg.lookup_path("res://old.tres"), None);
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_unregister() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(1);
    reg.register(uid, "res://a.tres");
    reg.unregister_uid(uid);
    assert_eq!(reg.lookup_uid(uid), None);
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_multiple_entries() {
    let mut reg = UidRegistry::new();
    reg.register(ResourceUid::new(1), "res://a.tres");
    reg.register(ResourceUid::new(2), "res://b.tres");
    reg.register(ResourceUid::new(3), "res://c.tres");
    assert_eq!(reg.len(), 3);
    assert_eq!(reg.lookup_uid(ResourceUid::new(2)), Some("res://b.tres"));
}

#[test]
fn tres_with_uid_header_carries_uid() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://test123"]

[resource]
name = "Test"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    assert!(res.uid.is_valid(), "resource must carry a valid UID");
}

#[test]
fn tres_without_uid_has_invalid_uid() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "NoUid"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    assert!(
        !res.uid.is_valid(),
        "resource without uid= should be invalid"
    );
}

#[test]
fn unified_loader_resolves_uid_to_path() {
    let mut unified = UnifiedLoader::new(TresLoader::new());
    unified.register_uid_str("uid://abc", "res://item.tres");
    let resolved = unified.resolve_to_path("uid://abc").unwrap();
    assert_eq!(resolved, "res://item.tres");
}

#[test]
fn unified_loader_passthrough_res_path() {
    let unified = UnifiedLoader::new(TresLoader::new());
    let resolved = unified.resolve_to_path("res://direct.tres").unwrap();
    assert_eq!(resolved, "res://direct.tres");
}

#[test]
fn unified_loader_unresolved_uid_errors() {
    let unified = UnifiedLoader::new(TresLoader::new());
    let result = unified.resolve_to_path("uid://nonexistent");
    assert!(result.is_err());
}
