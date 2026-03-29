//! pat-gr4m: Integrate resource UID and cache behavior into loader paths.
//!
//! End-to-end integration tests that verify res:// and uid:// loads resolve
//! consistently through the full UnifiedLoader pipeline. Unlike unit tests
//! in individual crates, these exercise the complete path:
//!
//!   .tres parse → UID extract → registry register → uid:// load → cache dedup
//!
//! Acceptance: res:// and UID loads resolve consistently with integration coverage.

use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::ResourceUid;
use gdresource::{
    parse_uid_string, Resource, ResourceLoader, TresLoader, UidRegistry, UnifiedLoader,
};
use gdvariant::Variant;

// ===========================================================================
// Test loaders
// ===========================================================================

/// A loader that returns resources with a known path and class, tracking call
/// count so we can verify cache hits vs misses.
struct TrackingLoader {
    call_count: std::cell::Cell<u32>,
}

impl TrackingLoader {
    fn new() -> Self {
        Self {
            call_count: std::cell::Cell::new(0),
        }
    }
    fn _calls(&self) -> u32 {
        self.call_count.get()
    }
}

impl ResourceLoader for TrackingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.call_count.set(self.call_count.get() + 1);
        let mut r = Resource::new("Tracked");
        r.path = path.to_string();
        r.set_property("source", Variant::String(path.to_string()));
        Ok(Arc::new(r))
    }
}

/// A loader that always fails — for error propagation tests.
struct FailingLoader;

impl ResourceLoader for FailingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        Err(EngineError::NotFound(format!("cannot load {path}")))
    }
}

// ===========================================================================
// 1. Parse .tres → extract UID → register → load by uid:// → same cache entry
// ===========================================================================

#[test]
fn parse_extract_register_load_by_uid_full_pipeline() {
    let source = r#"[gd_resource type="Theme" format=3 uid="uid://my_sword"]

[resource]
name = "Sword"
damage = 25
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://weapons/sword.tres").unwrap();

    // UID should have been extracted from the header.
    assert!(parsed.uid.is_valid());
    assert_eq!(parsed.path, "res://weapons/sword.tres");

    // Register extracted UID in a UnifiedLoader.
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://my_sword", "res://weapons/sword.tres");

    // Load by res:// path.
    let by_path = ul.load("res://weapons/sword.tres").unwrap();
    // Load by uid:// reference.
    let by_uid = ul.load("uid://my_sword").unwrap();

    // Both must resolve to the same cached Arc.
    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "res:// and uid:// for same resource must return same Arc"
    );
    assert_eq!(ul.cache_len(), 1, "only one entry in cache");
}

// ===========================================================================
// 2. Parsed UID matches string-derived UID (deterministic hashing)
// ===========================================================================

#[test]
fn parsed_uid_matches_string_derived_uid() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://player_stats"]

[resource]
hp = 100
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://stats/player.tres").unwrap();

    let from_string = parse_uid_string("uid://player_stats");

    assert_eq!(
        parsed.uid, from_string,
        "UID from .tres header must equal UID from parse_uid_string"
    );
}

// ===========================================================================
// 3. Multiple UIDs all resolve through cache without extra loader calls
// ===========================================================================

#[test]
fn multiple_uids_cache_efficiency() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://res_a", "res://a.tres");
    ul.register_uid_str("uid://res_b", "res://b.tres");
    ul.register_uid_str("uid://res_c", "res://c.tres");

    // First load each by UID.
    let a1 = ul.load("uid://res_a").unwrap();
    let b1 = ul.load("uid://res_b").unwrap();
    let c1 = ul.load("uid://res_c").unwrap();

    // Second load each by res:// path — should all hit cache.
    let a2 = ul.load("res://a.tres").unwrap();
    let b2 = ul.load("res://b.tres").unwrap();
    let c2 = ul.load("res://c.tres").unwrap();

    assert!(Arc::ptr_eq(&a1, &a2));
    assert!(Arc::ptr_eq(&b1, &b2));
    assert!(Arc::ptr_eq(&c1, &c2));
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 4. Path-first then uid-second resolves same Arc
// ===========================================================================

#[test]
fn path_first_uid_second_same_arc() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://shield", "res://items/shield.tres");

    // Load by path first.
    let by_path = ul.load("res://items/shield.tres").unwrap();
    // Then by UID.
    let by_uid = ul.load("uid://shield").unwrap();

    assert!(Arc::ptr_eq(&by_path, &by_uid));
}

// ===========================================================================
// 5. uid-first then path-second resolves same Arc
// ===========================================================================

#[test]
fn uid_first_path_second_same_arc() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://potion", "res://items/potion.tres");

    let by_uid = ul.load("uid://potion").unwrap();
    let by_path = ul.load("res://items/potion.tres").unwrap();

    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

// ===========================================================================
// 6. Error propagation: uid:// to a path that fails to load
// ===========================================================================

#[test]
fn uid_to_failing_path_propagates_error() {
    let mut ul = UnifiedLoader::new(FailingLoader);
    ul.register_uid_str("uid://broken", "res://broken.tres");

    let result = ul.load("uid://broken");
    assert!(
        result.is_err(),
        "uid:// to a failing path must propagate the loader error"
    );
}

// ===========================================================================
// 7. Unregistered uid:// returns NotFound, not panic
// ===========================================================================

#[test]
fn unregistered_uid_returns_not_found() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    let result = ul.load("uid://never_registered");
    assert!(result.is_err());
}

// ===========================================================================
// 8. Invalidation via path after uid load forces fresh load
// ===========================================================================

#[test]
fn invalidate_path_after_uid_load_forces_reload() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://weapon", "res://weapon.tres");

    let first = ul.load("uid://weapon").unwrap();
    ul.invalidate("res://weapon.tres");
    let second = ul.load("uid://weapon").unwrap();

    assert!(
        !Arc::ptr_eq(&first, &second),
        "invalidation must force a new load even through uid:// path"
    );
}

// ===========================================================================
// 9. UID re-registration to different path resolves new resource
// ===========================================================================

#[test]
fn uid_reregistration_resolves_new_path() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://movable", "res://old_location.tres");

    let old = ul.load("uid://movable").unwrap();
    assert_eq!(old.path, "res://old_location.tres");

    // Re-register UID to new path (simulating a file rename).
    ul.register_uid_str("uid://movable", "res://new_location.tres");

    let new = ul.load("uid://movable").unwrap();
    assert_eq!(new.path, "res://new_location.tres");
    assert!(!Arc::ptr_eq(&old, &new));
}

// ===========================================================================
// 10. Two different UIDs pointing to same path return same Arc
// ===========================================================================

#[test]
fn two_uids_same_path_same_arc() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());

    // UidRegistry enforces one-to-one (second register overwrites first),
    // so we test that loading by the surviving UID and by path both hit cache.
    ul.register_uid_str("uid://alias_a", "res://shared.tres");
    ul.register_uid_str("uid://alias_b", "res://shared.tres");
    // alias_a is now evicted; alias_b owns the path.

    let by_uid = ul.load("uid://alias_b").unwrap();
    let by_path = ul.load("res://shared.tres").unwrap();

    assert!(
        Arc::ptr_eq(&by_uid, &by_path),
        "uid and path for same resource must return same Arc"
    );
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 11. Real fixture: theme.tres with uid="uid://theme_res"
// ===========================================================================

#[test]
fn fixture_theme_tres_uid_round_trips() {
    let fixture_path = format!(
        "{}/../fixtures/resources/theme.tres",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    // Verify the fixture has the expected UID.
    assert!(content.contains("uid=\"uid://theme_res\""));

    // Parse it.
    let tres = TresLoader::new();
    let parsed = tres
        .parse_str(&content, "res://resources/theme.tres")
        .unwrap();

    assert!(parsed.uid.is_valid());
    assert_eq!(parsed.class_name, "Theme");

    // The parsed UID should match the string-derived UID.
    let uid_from_str = parse_uid_string("uid://theme_res");
    assert_eq!(parsed.uid, uid_from_str);

    // Register and load through UnifiedLoader.
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://theme_res", "res://resources/theme.tres");

    let by_uid = ul.load("uid://theme_res").unwrap();
    let by_path = ul.load("res://resources/theme.tres").unwrap();
    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

// ===========================================================================
// 12. Resource without UID: parse succeeds, UID is INVALID
// ===========================================================================

#[test]
fn resource_without_uid_has_invalid_uid() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "NoUid"
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://no_uid.tres").unwrap();

    assert!(!parsed.uid.is_valid());
}

// ===========================================================================
// 13. INVALID UID cannot be registered (register_uid_str guards)
// ===========================================================================

#[test]
fn invalid_uid_string_not_registered() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    // "not_a_uid" has no uid:// prefix → parse_uid_string returns INVALID.
    ul.register_uid_str("not_a_uid", "res://orphan.tres");

    // UID should not have been registered.
    let result = ul.load("uid://not_a_uid");
    // This tests the load path — it should either fail or load via the
    // string hash, depending on whether parse_uid_string("uid://not_a_uid")
    // is valid. The key thing is no panic.
    let _ = result;

    // The registry should be empty since "not_a_uid" has no uid:// prefix.
    assert!(ul.uid_registry().is_empty());
}

// ===========================================================================
// 14. Cache survives across interleaved uid:// and res:// loads
// ===========================================================================

#[test]
fn interleaved_uid_and_path_loads_all_cached() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://x", "res://x.tres");
    ul.register_uid_str("uid://y", "res://y.tres");

    // Interleave loads: uid, path, uid, path, uid, path.
    let x1 = ul.load("uid://x").unwrap();
    let y1 = ul.load("res://y.tres").unwrap();
    let x2 = ul.load("res://x.tres").unwrap();
    let y2 = ul.load("uid://y").unwrap();
    let x3 = ul.load("uid://x").unwrap();
    let y3 = ul.load("res://y.tres").unwrap();

    assert!(Arc::ptr_eq(&x1, &x2));
    assert!(Arc::ptr_eq(&x2, &x3));
    assert!(Arc::ptr_eq(&y1, &y2));
    assert!(Arc::ptr_eq(&y2, &y3));
    assert!(!Arc::ptr_eq(&x1, &y1));
    assert_eq!(ul.cache_len(), 2);
}

// ===========================================================================
// 15. clear_cache + reload via uid:// produces fresh Arcs
// ===========================================================================

#[test]
fn clear_cache_then_uid_reload_produces_fresh_arcs() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://item", "res://item.tres");

    let first = ul.load("uid://item").unwrap();
    ul.clear_cache();
    assert_eq!(ul.cache_len(), 0);

    let second = ul.load("uid://item").unwrap();
    assert!(
        !Arc::ptr_eq(&first, &second),
        "clear_cache must force fresh allocation"
    );
}

// ===========================================================================
// 16. is_cached reflects state after uid:// load
// ===========================================================================

#[test]
fn is_cached_after_uid_load() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://check", "res://check.tres");

    assert!(!ul.is_cached("res://check.tres"));
    ul.load("uid://check").unwrap();
    assert!(
        ul.is_cached("res://check.tres"),
        "uid:// load must populate the path-based cache"
    );
}

// ===========================================================================
// 17. Registry with_registry constructor pre-populates UID mappings
// ===========================================================================

#[test]
fn with_registry_constructor_resolves_preloaded_uids() {
    let mut reg = UidRegistry::new();
    reg.register(parse_uid_string("uid://pre_a"), "res://a.tres");
    reg.register(parse_uid_string("uid://pre_b"), "res://b.tres");

    let mut ul = UnifiedLoader::with_registry(TrackingLoader::new(), reg);

    let a = ul.load("uid://pre_a").unwrap();
    let b = ul.load("uid://pre_b").unwrap();
    assert_eq!(a.path, "res://a.tres");
    assert_eq!(b.path, "res://b.tres");
    assert!(!Arc::ptr_eq(&a, &b));
}

// ===========================================================================
// 18. Batch register from parsed .tres files, then load all by uid://
// ===========================================================================

#[test]
fn batch_register_from_parsed_tres_then_load_by_uid() {
    let sources = [
        (
            "uid://weapon_1",
            r#"[gd_resource type="Resource" format=3 uid="uid://weapon_1"]
[resource]
name = "Sword"
"#,
            "res://weapons/sword.tres",
        ),
        (
            "uid://weapon_2",
            r#"[gd_resource type="Resource" format=3 uid="uid://weapon_2"]
[resource]
name = "Axe"
"#,
            "res://weapons/axe.tres",
        ),
        (
            "uid://armor_1",
            r#"[gd_resource type="Resource" format=3 uid="uid://armor_1"]
[resource]
name = "Shield"
"#,
            "res://armor/shield.tres",
        ),
    ];

    let tres = TresLoader::new();
    let mut ul = UnifiedLoader::new(TrackingLoader::new());

    // Parse each, verify UID, register.
    for (uid_str, source, path) in &sources {
        let parsed = tres.parse_str(source, path).unwrap();
        assert!(parsed.uid.is_valid());
        assert_eq!(parsed.uid, parse_uid_string(uid_str));
        ul.register_uid_str(uid_str, *path);
    }

    // Load all by uid:// — each should resolve correctly.
    let sword = ul.load("uid://weapon_1").unwrap();
    let axe = ul.load("uid://weapon_2").unwrap();
    let shield = ul.load("uid://armor_1").unwrap();

    assert_eq!(sword.path, "res://weapons/sword.tres");
    assert_eq!(axe.path, "res://weapons/axe.tres");
    assert_eq!(shield.path, "res://armor/shield.tres");

    // Verify all distinct.
    assert!(!Arc::ptr_eq(&sword, &axe));
    assert!(!Arc::ptr_eq(&sword, &shield));
    assert!(!Arc::ptr_eq(&axe, &shield));

    // Load again by path — all cached.
    let sword2 = ul.load("res://weapons/sword.tres").unwrap();
    assert!(Arc::ptr_eq(&sword, &sword2));
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 19. UID hashing: same uid string always produces same ResourceUid
// ===========================================================================

#[test]
fn uid_hashing_deterministic_across_calls() {
    let uid_strs = [
        "uid://test_123",
        "uid://player",
        "uid://a",
        "uid://very_long_identifier_that_should_still_hash_deterministically",
    ];

    for uid_str in &uid_strs {
        let a = parse_uid_string(uid_str);
        let b = parse_uid_string(uid_str);
        assert_eq!(a, b, "parse_uid_string must be deterministic for {uid_str}");
        assert!(a.is_valid());
    }
}

// ===========================================================================
// 20. Different uid strings produce different UIDs
// ===========================================================================

#[test]
fn different_uid_strings_produce_different_resource_uids() {
    let uid_a = parse_uid_string("uid://alpha");
    let uid_b = parse_uid_string("uid://beta");
    let uid_c = parse_uid_string("uid://gamma");

    assert_ne!(uid_a, uid_b);
    assert_ne!(uid_b, uid_c);
    assert_ne!(uid_a, uid_c);
}

// ===========================================================================
// 21. Registry lookup_path returns correct UID after registration
// ===========================================================================

#[test]
fn registry_reverse_lookup_after_unified_registration() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://reverse_test", "res://reverse.tres");

    let uid = ul
        .uid_registry()
        .lookup_path("res://reverse.tres")
        .expect("path should have a registered UID");

    assert_eq!(uid, parse_uid_string("uid://reverse_test"));
}

// ===========================================================================
// 22. Empty uid:// suffix handled gracefully
// ===========================================================================

#[test]
fn empty_uid_suffix_no_panic() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    // uid:// with no suffix — should not panic.
    let result = ul.load("uid://");
    // May succeed or fail depending on hash of empty string; key is no panic.
    let _ = result;
}

// ===========================================================================
// 23. Non-res non-uid path passes through to loader directly
// ===========================================================================

#[test]
fn bare_path_passes_through_to_loader() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    let res = ul.load("/absolute/path/to/resource.tres").unwrap();
    assert_eq!(res.path, "/absolute/path/to/resource.tres");
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 24. Invalidation of one path doesn't affect uid:// load for a different path
// ===========================================================================

#[test]
fn invalidation_scoped_to_specific_path() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://keep", "res://keep.tres");
    ul.register_uid_str("uid://drop", "res://drop.tres");

    let keep1 = ul.load("uid://keep").unwrap();
    let _drop1 = ul.load("uid://drop").unwrap();

    // Invalidate only drop.
    ul.invalidate("res://drop.tres");

    // keep should still be cached.
    let keep2 = ul.load("uid://keep").unwrap();
    assert!(Arc::ptr_eq(&keep1, &keep2), "keep must remain cached");
    assert!(ul.is_cached("res://keep.tres"));
    assert!(!ul.is_cached("res://drop.tres"));
}

// ===========================================================================
// 25. Fixture scene with uid: parse header, register, load by uid
// ===========================================================================

#[test]
fn fixture_scene_uid_parse_and_resolve() {
    let fixture_path = format!(
        "{}/../fixtures/scenes/test_scripts.tscn",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&fixture_path).unwrap();

    // Verify the fixture has the expected UID.
    assert!(content.contains("uid=\"uid://test_scripts\""));

    // Derive the UID from the string.
    let uid = parse_uid_string("uid://test_scripts");
    assert!(uid.is_valid());

    // Register and load.
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://test_scripts", "res://scenes/test_scripts.tscn");

    let res = ul.load("uid://test_scripts").unwrap();
    assert_eq!(res.path, "res://scenes/test_scripts.tscn");

    // Verify registry reverse lookup.
    let looked_up_uid = ul
        .uid_registry()
        .lookup_path("res://scenes/test_scripts.tscn")
        .unwrap();
    assert_eq!(looked_up_uid, uid);
}

// ===========================================================================
// 26. ResourceUid::INVALID cannot look up anything
// ===========================================================================

#[test]
fn invalid_uid_lookup_returns_none() {
    let mut reg = UidRegistry::new();
    reg.register(ResourceUid::new(42), "res://valid.tres");

    assert_eq!(reg.lookup_uid(ResourceUid::INVALID), None);
}

// ===========================================================================
// 27. Ext-resource UID references resolve through unified loader
// ===========================================================================

#[test]
fn ext_resource_uid_resolves_through_unified_loader() {
    // Simulate a scene with ext_resources that reference other resources by UID.
    let mut ul = UnifiedLoader::new(TrackingLoader::new());

    // Register ext_resource UIDs (as a scene parser would do).
    ul.register_uid_str("uid://player_script", "res://scripts/player.gd");
    ul.register_uid_str("uid://player_texture", "res://textures/player.png");
    ul.register_uid_str("uid://enemy_script", "res://scripts/enemy.gd");

    // Load all by UID (as ext_resource resolution would).
    let player_script = ul.load("uid://player_script").unwrap();
    let player_texture = ul.load("uid://player_texture").unwrap();
    let enemy_script = ul.load("uid://enemy_script").unwrap();

    assert_eq!(player_script.path, "res://scripts/player.gd");
    assert_eq!(player_texture.path, "res://textures/player.png");
    assert_eq!(enemy_script.path, "res://scripts/enemy.gd");

    // Verify cache: loading same resources by path hits cache.
    let ps2 = ul.load("res://scripts/player.gd").unwrap();
    assert!(Arc::ptr_eq(&player_script, &ps2));
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 28. UID registry overwrite: old UID removed when path gets new UID
// ===========================================================================

#[test]
fn registry_overwrite_old_uid_removed() {
    let mut reg = UidRegistry::new();
    let uid1 = ResourceUid::new(100);
    let uid2 = ResourceUid::new(200);

    reg.register(uid1, "res://shared.tres");
    assert_eq!(reg.lookup_uid(uid1), Some("res://shared.tres"));

    // Same path, different UID — old UID should be evicted.
    reg.register(uid2, "res://shared.tres");
    assert_eq!(reg.lookup_uid(uid1), None, "old UID must be evicted");
    assert_eq!(reg.lookup_uid(uid2), Some("res://shared.tres"));
    assert_eq!(reg.lookup_path("res://shared.tres"), Some(uid2));
    assert_eq!(reg.len(), 1);
}

// ===========================================================================
// 29. Strong count verifies cache ownership semantics
// ===========================================================================

#[test]
fn strong_count_verifies_cache_ownership() {
    let mut ul = UnifiedLoader::new(TrackingLoader::new());
    ul.register_uid_str("uid://counted", "res://counted.tres");

    let a = ul.load("uid://counted").unwrap();
    // Cache holds one ref, we hold one → strong_count = 2.
    assert_eq!(Arc::strong_count(&a), 2);

    let b = ul.load("res://counted.tres").unwrap();
    // Cache holds one ref, a holds one, b holds one → strong_count = 3.
    assert_eq!(Arc::strong_count(&a), 3);
    assert!(Arc::ptr_eq(&a, &b));

    // After invalidation, cache drops its ref.
    ul.invalidate("res://counted.tres");
    assert_eq!(
        Arc::strong_count(&a),
        2,
        "cache ref dropped after invalidation"
    );
}

// ===========================================================================
// 30. Full round-trip: parse .tres with subresources, register UID, load
// ===========================================================================

#[test]
fn full_round_trip_with_subresources() {
    let source = r#"[gd_resource type="Theme" format=3 uid="uid://full_theme"]

[sub_resource type="StyleBoxFlat" id="panel"]
bg_color = Color(0.1, 0.2, 0.3, 1.0)

[resource]
name = "FullTheme"
style = SubResource("panel")
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://themes/full.tres").unwrap();

    // Verify parse results.
    assert!(parsed.uid.is_valid());
    assert_eq!(parsed.uid, parse_uid_string("uid://full_theme"));
    assert_eq!(parsed.class_name, "Theme");
    assert!(!parsed.subresources.is_empty());
    assert!(parsed.subresources.contains_key("panel"));

    // Verify subresource properties.
    let panel = &parsed.subresources["panel"];
    assert_eq!(panel.class_name, "StyleBoxFlat");

    // Register and verify the UID round-trips through the registry.
    let mut reg = UidRegistry::new();
    reg.register(parsed.uid, &parsed.path);
    assert_eq!(reg.lookup_uid(parsed.uid), Some("res://themes/full.tres"));
    assert_eq!(reg.lookup_path("res://themes/full.tres"), Some(parsed.uid));
}
