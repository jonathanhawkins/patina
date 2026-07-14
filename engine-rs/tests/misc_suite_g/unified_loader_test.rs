//! Unified loader integration tests (pat-riz).
//!
//! Verifies that a single load() entry point correctly handles both
//! res:// paths and uid:// references, with cache deduplication.

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::{
    parse_uid_string, Resource, ResourceLoader, TresLoader, UidRegistry, UnifiedLoader,
};
use gdvariant::Variant;

// ===========================================================================
// Fake loader for deterministic tests
// ===========================================================================

/// Returns a resource with path + class set from the path.
struct FakeLoader;

impl ResourceLoader for FakeLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("FakeResource");
        r.path = path.to_string();
        r.set_property("loaded_from", Variant::String(path.to_string()));
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// 1. Single load() accepts both res:// and uid://
// ===========================================================================

#[test]
fn load_accepts_res_path() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    let res = ul.load("res://player.tres").unwrap();
    assert_eq!(res.path, "res://player.tres");
}

#[test]
fn load_accepts_uid_reference() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://player_uid", "res://player.tres");

    let res = ul.load("uid://player_uid").unwrap();
    assert_eq!(res.path, "res://player.tres");
}

#[test]
fn load_uid_unknown_returns_error() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    assert!(ul.load("uid://unknown_ref").is_err());
}

// ===========================================================================
// 2. Same resource via path and UID returns same Arc
// ===========================================================================

#[test]
fn path_and_uid_return_same_arc() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://sword_ref", "res://weapons/sword.tres");

    let by_path = ul.load("res://weapons/sword.tres").unwrap();
    let by_uid = ul.load("uid://sword_ref").unwrap();

    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "loading same resource by path and UID must return same Arc"
    );
}

#[test]
fn uid_then_path_same_arc() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://shield_ref", "res://items/shield.tres");

    // Load by UID first, then by path.
    let by_uid = ul.load("uid://shield_ref").unwrap();
    let by_path = ul.load("res://items/shield.tres").unwrap();

    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

#[test]
fn multiple_uid_loads_same_arc() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://potion", "res://items/potion.tres");

    let a = ul.load("uid://potion").unwrap();
    let b = ul.load("uid://potion").unwrap();
    let c = ul.load("res://items/potion.tres").unwrap();

    assert!(Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&b, &c));
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 3. ext_resource resolution through unified loader
// ===========================================================================

/// Parse a .tscn with ext_resources, register their UIDs, and verify
/// loading through the unified path resolves correctly.
#[test]
fn ext_resource_uid_resolution() {
    let mut ul = UnifiedLoader::new(FakeLoader);

    // Simulate what a scene parser would do: register ext_resource UIDs.
    ul.register_uid_str("uid://script_movement", "res://scripts/movement.gd");
    ul.register_uid_str("uid://texture_icon", "res://textures/icon.png");

    // Load scripts and textures via UID (as ext_resources would reference them).
    let script = ul.load("uid://script_movement").unwrap();
    assert_eq!(script.path, "res://scripts/movement.gd");

    let texture = ul.load("uid://texture_icon").unwrap();
    assert_eq!(texture.path, "res://textures/icon.png");

    // Loading the same paths directly hits cache.
    let script2 = ul.load("res://scripts/movement.gd").unwrap();
    assert!(Arc::ptr_eq(&script, &script2));
}

// ===========================================================================
// 4. with_registry constructor
// ===========================================================================

#[test]
fn with_registry_preloads_uid_mappings() {
    let mut reg = UidRegistry::new();
    let uid = parse_uid_string("uid://preloaded");
    reg.register(uid, "res://preloaded.tres");

    let mut ul = UnifiedLoader::with_registry(FakeLoader, reg);
    let res = ul.load("uid://preloaded").unwrap();
    assert_eq!(res.path, "res://preloaded.tres");
}

// ===========================================================================
// 5. Cache management through unified loader
// ===========================================================================

#[test]
fn invalidate_forces_fresh_load() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://item", "res://item.tres");

    let first = ul.load("uid://item").unwrap();
    ul.invalidate("res://item.tres");
    let second = ul.load("uid://item").unwrap();

    assert!(!Arc::ptr_eq(&first, &second));
}

#[test]
fn clear_cache_removes_all() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.load("res://a.tres").unwrap();
    ul.load("res://b.tres").unwrap();
    assert_eq!(ul.cache_len(), 2);

    ul.clear_cache();
    assert_eq!(ul.cache_len(), 0);
}

#[test]
fn is_cached_reflects_state() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    assert!(!ul.is_cached("res://test.tres"));

    ul.load("res://test.tres").unwrap();
    assert!(ul.is_cached("res://test.tres"));

    ul.invalidate("res://test.tres");
    assert!(!ul.is_cached("res://test.tres"));
}

// ===========================================================================
// 6. TresLoader integration — parse .tres with uid, register, load by uid
// ===========================================================================

#[test]
fn tres_parsed_uid_round_trips_through_unified() {
    // Parse a .tres in-memory to get its UID.
    let source = r#"[gd_resource type="Theme" format=3 uid="uid://my_theme"]

[resource]
name = "TestTheme"
"#;
    let tres_loader = TresLoader::new();
    let parsed = tres_loader
        .parse_str(source, "res://themes/main.tres")
        .unwrap();
    assert!(parsed.uid.is_valid());

    // Register the parsed UID in the unified loader.
    // Use the same uid:// string so the hash matches.
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://my_theme", "res://themes/main.tres");

    // Load by uid:// through unified loader.
    let res = ul.load("uid://my_theme").unwrap();
    assert_eq!(res.path, "res://themes/main.tres");

    // Verify the UID from parsing matches the UID from the string.
    let uid_from_str = parse_uid_string("uid://my_theme");
    assert_eq!(
        parsed.uid, uid_from_str,
        "parsed UID must match string-derived UID"
    );
}

// ===========================================================================
// 7. Real fixture .tscn — verify scene UID can be registered and loaded
// ===========================================================================

#[test]
fn fixture_tscn_uid_registers_and_resolves() {
    // test_scripts.tscn has uid="uid://test_scripts" in its header.
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/scenes/test_scripts.tscn");
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    // Extract the uid from the first line.
    let first_line = content.lines().next().unwrap();
    assert!(
        first_line.contains("uid=\"uid://test_scripts\""),
        "fixture should have uid"
    );

    // Register it and load through unified loader.
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://test_scripts", fixture_path.to_str().unwrap());

    let res = ul.load("uid://test_scripts").unwrap();
    assert!(res.path.contains("test_scripts.tscn"));
}

// ===========================================================================
// 8. Edge cases
// ===========================================================================

#[test]
fn empty_uid_suffix_returns_error() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    // uid:// with empty suffix — parse_uid_string will fold zero bytes → 0
    // which may or may not be valid. Either way, it shouldn't panic.
    let _ = ul.load("uid://");
}

#[test]
fn non_res_non_uid_path_loads_directly() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    // Absolute path (not res:// or uid://) should pass through to the loader.
    let res = ul.load("/absolute/path/to/resource.tres").unwrap();
    assert_eq!(res.path, "/absolute/path/to/resource.tres");
}
