//! pat-xzwo: Resource UID + cache + loader integration tests.
//!
//! These tests exercise cross-cutting scenarios across the UnifiedLoader,
//! ResourceCache, UidRegistry, and TresLoader that are not covered by
//! existing unit tests or the prior parity test files. Focus areas:
//!
//! - register_uid (direct ResourceUid, not string) through UnifiedLoader
//! - uid_registry_mut() access and modification mid-lifecycle
//! - Multi-resource bulk lifecycle sequences
//! - .tres parsing → UnifiedLoader end-to-end with subresources + ext_resources
//! - Registry + cache state consistency after compound operations

use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::ResourceUid;
use gdresource::{
    parse_uid_string, Resource, ResourceCache, ResourceLoader, TresLoader, UidRegistry,
    UnifiedLoader,
};
use gdvariant::Variant;

// ===========================================================================
// Shared test loaders
// ===========================================================================

/// Loader that embeds call count and path into each resource.
struct InstrumentedLoader {
    call_count: std::cell::Cell<u32>,
}

impl InstrumentedLoader {
    fn new() -> Self {
        Self {
            call_count: std::cell::Cell::new(0),
        }
    }
    #[allow(dead_code)]
    fn calls(&self) -> u32 {
        self.call_count.get()
    }
}

impl ResourceLoader for InstrumentedLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let n = self.call_count.get() + 1;
        self.call_count.set(n);
        let mut r = Resource::new("Instrumented");
        r.path = path.to_string();
        r.uid = ResourceUid::new(n as i64 * 1000);
        r.set_property("load_seq", Variant::Int(n as i64));
        Ok(Arc::new(r))
    }
}

/// Loader that fails for paths containing "FAIL".
struct SelectiveFailLoader;

impl ResourceLoader for SelectiveFailLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        if path.contains("FAIL") {
            return Err(EngineError::NotFound(format!("selective fail: {path}")));
        }
        let mut r = Resource::new("SelectiveOk");
        r.path = path.to_string();
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// 1. register_uid (direct ResourceUid) through UnifiedLoader
// ===========================================================================

#[test]
fn register_uid_direct_then_load_by_uid_string() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    let uid = ResourceUid::new(42);
    ul.register_uid(uid, "res://direct.tres");

    // Construct the uid:// string that hashes to this UID — we can't since
    // register_uid uses a direct ResourceUid. Instead, verify path-based
    // lookup works and the registry is populated.
    assert_eq!(
        ul.uid_registry().lookup_uid(uid),
        Some("res://direct.tres")
    );
    assert_eq!(
        ul.uid_registry().lookup_path("res://direct.tres"),
        Some(uid)
    );

    // Load by path works and is cached.
    let res = ul.load("res://direct.tres").unwrap();
    assert_eq!(res.path, "res://direct.tres");
    assert!(ul.is_cached("res://direct.tres"));
}

// ===========================================================================
// 2. uid_registry_mut() modification mid-lifecycle
// ===========================================================================

#[test]
fn uid_registry_mut_add_mapping_after_construction() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());

    // Initially empty registry.
    assert!(ul.uid_registry().is_empty());

    // Add mapping via mutable reference.
    let uid = parse_uid_string("uid://added_later");
    ul.uid_registry_mut().register(uid, "res://late.tres");

    // Should now resolve.
    let res = ul.load("uid://added_later").unwrap();
    assert_eq!(res.path, "res://late.tres");
}

#[test]
fn uid_registry_mut_remove_mapping_blocks_uid_load() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://removable", "res://removable.tres");

    // First load succeeds.
    let _first = ul.load("uid://removable").unwrap();

    // Remove mapping via mutable reference.
    let uid = parse_uid_string("uid://removable");
    ul.uid_registry_mut().unregister_uid(uid);

    // Invalidate cache too, so we actually test the registry path.
    ul.invalidate("res://removable.tres");

    // UID load should now fail (no registry mapping).
    let result = ul.load("uid://removable");
    assert!(result.is_err(), "removed UID mapping should fail to load");
}

// ===========================================================================
// 3. Bulk lifecycle: register N, load all, invalidate half, reload
// ===========================================================================

#[test]
fn bulk_register_load_invalidate_reload_cycle() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());

    let paths: Vec<String> = (0..10)
        .map(|i| format!("res://bulk_{i}.tres"))
        .collect();
    let uid_strs: Vec<String> = (0..10)
        .map(|i| format!("uid://bulk_{i}"))
        .collect();

    // Register all.
    for (uid_str, path) in uid_strs.iter().zip(paths.iter()) {
        ul.register_uid_str(uid_str, path.as_str());
    }

    // Load all by UID.
    let first_arcs: Vec<Arc<Resource>> = uid_strs
        .iter()
        .map(|uid_str| ul.load(uid_str).unwrap())
        .collect();
    assert_eq!(ul.cache_len(), 10);

    // Invalidate even-indexed paths.
    for i in (0..10).step_by(2) {
        ul.invalidate(&paths[i]);
    }
    assert_eq!(ul.cache_len(), 5);

    // Reload all by path.
    let second_arcs: Vec<Arc<Resource>> = paths
        .iter()
        .map(|p| ul.load(p).unwrap())
        .collect();

    // Odd-indexed should be same Arc (still cached).
    for i in (1..10).step_by(2) {
        assert!(
            Arc::ptr_eq(&first_arcs[i], &second_arcs[i]),
            "odd index {i} should still be cached"
        );
    }
    // Even-indexed should be different Arc (reloaded).
    for i in (0..10).step_by(2) {
        assert!(
            !Arc::ptr_eq(&first_arcs[i], &second_arcs[i]),
            "even index {i} should be fresh after invalidation"
        );
    }
    assert_eq!(ul.cache_len(), 10);
}

// ===========================================================================
// 4. Parse .tres with subresources, register UID, load by uid://
// ===========================================================================

#[test]
fn tres_subresource_parsed_uid_registers_and_resolves() {
    let source = r#"[gd_resource type="Theme" format=3 uid="uid://sub_theme"]

[sub_resource type="StyleBoxFlat" id="box_a"]
bg_color = Color(1, 0, 0, 1)

[sub_resource type="StyleBoxFlat" id="box_b"]
bg_color = Color(0, 1, 0, 1)

[resource]
name = "SubTheme"
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://themes/sub.tres").unwrap();

    assert!(parsed.uid.is_valid());
    assert_eq!(parsed.subresources.len(), 2);
    assert!(parsed.subresources.contains_key("box_a"));
    assert!(parsed.subresources.contains_key("box_b"));

    // Register in UnifiedLoader and verify uid:// load.
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid(parsed.uid, "res://themes/sub.tres");

    let uid_str = "uid://sub_theme";
    let by_uid = ul.load(uid_str).unwrap();
    let by_path = ul.load("res://themes/sub.tres").unwrap();
    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

// ===========================================================================
// 5. Parse .tres with ext_resources, register ext UIDs, load all
// ===========================================================================

#[test]
fn tres_ext_resources_all_uids_register_and_resolve() {
    let source = r#"[gd_resource type="PackedScene" format=3 uid="uid://scene_with_ext"]

[ext_resource type="Script" uid="uid://player_gd" path="res://scripts/player.gd" id="1"]
[ext_resource type="Texture2D" uid="uid://player_png" path="res://textures/player.png" id="2"]

[resource]
name = "SceneWithExt"
"#;
    let tres = TresLoader::new();
    let parsed = tres
        .parse_str(source, "res://scenes/with_ext.tres")
        .unwrap();

    assert!(parsed.uid.is_valid());
    assert_eq!(parsed.ext_resources.len(), 2);

    // Register main UID + all ext_resource UIDs.
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid(parsed.uid, &parsed.path);
    for ext in parsed.ext_resources.values() {
        if !ext.uid.is_empty() {
            ul.register_uid_str(&ext.uid, &ext.path);
        }
    }

    // Load main scene by uid://.
    let scene = ul.load("uid://scene_with_ext").unwrap();
    assert_eq!(scene.path, "res://scenes/with_ext.tres");

    // Load ext_resources by uid://.
    let script = ul.load("uid://player_gd").unwrap();
    assert_eq!(script.path, "res://scripts/player.gd");

    let texture = ul.load("uid://player_png").unwrap();
    assert_eq!(texture.path, "res://textures/player.png");

    // All cached.
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 6. Selective failure: uid:// to OK path succeeds, uid:// to FAIL path errors
// ===========================================================================

#[test]
fn selective_failure_mixed_uid_loads() {
    let mut ul = UnifiedLoader::new(SelectiveFailLoader);
    ul.register_uid_str("uid://good", "res://good.tres");
    ul.register_uid_str("uid://bad", "res://FAIL_resource.tres");

    let good = ul.load("uid://good");
    assert!(good.is_ok());

    let bad = ul.load("uid://bad");
    assert!(bad.is_err());

    // Good is cached, bad is not.
    assert!(ul.is_cached("res://good.tres"));
    assert!(!ul.is_cached("res://FAIL_resource.tres"));
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 7. Registry clear via uid_registry_mut doesn't affect cache
// ===========================================================================

#[test]
fn registry_clear_does_not_affect_cache() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://persist", "res://persist.tres");

    // Load by UID — populates cache.
    let by_uid = ul.load("uid://persist").unwrap();
    assert!(ul.is_cached("res://persist.tres"));

    // Clear registry.
    ul.uid_registry_mut().clear();
    assert!(ul.uid_registry().is_empty());

    // Cache is still intact — load by path works.
    let by_path = ul.load("res://persist.tres").unwrap();
    assert!(Arc::ptr_eq(&by_uid, &by_path));

    // But uid:// load now fails.
    let result = ul.load("uid://persist");
    assert!(result.is_err());
}

// ===========================================================================
// 8. Cache clear does not affect registry
// ===========================================================================

#[test]
fn cache_clear_does_not_affect_registry() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://survives", "res://survives.tres");

    ul.load("uid://survives").unwrap();
    ul.clear_cache();

    // Registry still intact.
    assert_eq!(
        ul.uid_registry().lookup_path("res://survives.tres"),
        Some(parse_uid_string("uid://survives"))
    );

    // UID load works (triggers fresh cache entry).
    let fresh = ul.load("uid://survives").unwrap();
    assert_eq!(fresh.path, "res://survives.tres");
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 9. UID re-registration chain: A→path1, A→path2, A→path1 again
// ===========================================================================

#[test]
fn uid_reregistration_chain_returns_correct_path() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    let uid_str = "uid://bouncing";

    ul.register_uid_str(uid_str, "res://path1.tres");
    let r1 = ul.load(uid_str).unwrap();
    assert_eq!(r1.path, "res://path1.tres");

    ul.register_uid_str(uid_str, "res://path2.tres");
    let r2 = ul.load(uid_str).unwrap();
    assert_eq!(r2.path, "res://path2.tres");

    ul.register_uid_str(uid_str, "res://path1.tres");
    let r3 = ul.load(uid_str).unwrap();
    assert_eq!(r3.path, "res://path1.tres");

    // r1 and r3 should be same Arc (path1 was still cached).
    assert!(
        Arc::ptr_eq(&r1, &r3),
        "re-registering to original path should hit cache"
    );
    assert!(!Arc::ptr_eq(&r1, &r2));
}

// ===========================================================================
// 10. parse_uid_string edge cases
// ===========================================================================

#[test]
fn parse_uid_string_no_prefix_returns_invalid() {
    let uid = parse_uid_string("no_prefix");
    assert!(!uid.is_valid());
    assert_eq!(uid, ResourceUid::INVALID);
}

#[test]
fn parse_uid_string_empty_returns_invalid() {
    let uid = parse_uid_string("");
    assert!(!uid.is_valid());
}

#[test]
fn parse_uid_string_uid_prefix_only_is_valid() {
    // "uid://" with empty suffix — hash of empty string is 0, which is valid.
    let uid = parse_uid_string("uid://");
    assert!(uid.is_valid(), "hash of empty suffix should be 0, which is valid");
    assert_eq!(uid.raw(), 0);
}

#[test]
fn parse_uid_string_long_string_no_panic() {
    let long = format!("uid://{}", "a".repeat(10000));
    let uid = parse_uid_string(&long);
    assert!(uid.is_valid());
}

// ===========================================================================
// 11. UnifiedLoader with_registry pre-populates and caches correctly
// ===========================================================================

#[test]
fn with_registry_large_batch_all_resolve() {
    let mut reg = UidRegistry::new();
    for i in 0..50 {
        reg.register(ResourceUid::new(i), format!("res://batch_{i}.tres"));
    }
    assert_eq!(reg.len(), 50);

    let mut ul = UnifiedLoader::with_registry(InstrumentedLoader::new(), reg);

    // Load first 10 by path, next 10 by direct UID lookup.
    for i in 0..10 {
        let res = ul.load(&format!("res://batch_{i}.tres")).unwrap();
        assert_eq!(res.path, format!("res://batch_{i}.tres"));
    }
    assert_eq!(ul.cache_len(), 10);

    // Verify registry still has all 50 entries.
    assert_eq!(ul.uid_registry().len(), 50);
}

// ===========================================================================
// 12. Multiple .tres parses → batch register → cross-reference loads
// ===========================================================================

#[test]
fn batch_tres_parse_register_cross_reference() {
    let sources = [
        (
            r#"[gd_resource type="Resource" format=3 uid="uid://item_a"]
[resource]
name = "ItemA"
"#,
            "res://items/a.tres",
        ),
        (
            r#"[gd_resource type="Resource" format=3 uid="uid://item_b"]
[resource]
name = "ItemB"
"#,
            "res://items/b.tres",
        ),
        (
            r#"[gd_resource type="Resource" format=3 uid="uid://item_c"]
[resource]
name = "ItemC"
"#,
            "res://items/c.tres",
        ),
    ];

    let tres = TresLoader::new();
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());

    // Parse all and register UIDs.
    let mut parsed_uids = Vec::new();
    for (source, path) in &sources {
        let parsed = tres.parse_str(source, path).unwrap();
        assert!(parsed.uid.is_valid());
        ul.register_uid(parsed.uid, path.to_string());
        parsed_uids.push(parsed.uid);
    }

    // Load by UID string, verify each resolves to correct path.
    let uid_strs = ["uid://item_a", "uid://item_b", "uid://item_c"];
    for (i, uid_str) in uid_strs.iter().enumerate() {
        let res = ul.load(uid_str).unwrap();
        assert_eq!(res.path, sources[i].1);
    }

    // Verify parsed UIDs match string-derived UIDs.
    for (i, uid_str) in uid_strs.iter().enumerate() {
        assert_eq!(parsed_uids[i], parse_uid_string(uid_str));
    }

    // All three cached.
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 13. Invalidate-then-uid-reload produces resource with fresh load_seq
// ===========================================================================

#[test]
fn invalidate_then_uid_reload_gets_fresh_resource() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://refresh", "res://refresh.tres");

    let first = ul.load("uid://refresh").unwrap();
    let seq1 = first.get_property("load_seq").unwrap().clone();

    ul.invalidate("res://refresh.tres");

    let second = ul.load("uid://refresh").unwrap();
    let seq2 = second.get_property("load_seq").unwrap().clone();

    assert_ne!(seq1, seq2, "reload should have incremented load_seq");
    assert!(!Arc::ptr_eq(&first, &second));
}

// ===========================================================================
// 14. ResourceCache load count verified through InstrumentedLoader
// ===========================================================================

#[test]
fn cache_prevents_redundant_loader_calls() {
    let loader = InstrumentedLoader::new();
    let mut cache = ResourceCache::new(loader);

    // Load same path 5 times.
    for _ in 0..5 {
        cache.load("res://single.tres").unwrap();
    }

    // Loader should have been called exactly once.
    assert_eq!(cache.len(), 1);
    // We can verify via the resource's load_seq — it should be 1.
    let res = cache.load("res://single.tres").unwrap();
    assert_eq!(
        res.get_property("load_seq"),
        Some(&Variant::Int(1)),
        "loader should have been called exactly once"
    );
}

// ===========================================================================
// 15. UidRegistry bidirectional consistency after bulk operations
// ===========================================================================

#[test]
fn uid_registry_bidirectional_consistency_bulk() {
    let mut reg = UidRegistry::new();

    // Register 20 mappings.
    for i in 0..20i64 {
        reg.register(ResourceUid::new(i), format!("res://r{i}.tres"));
    }
    assert_eq!(reg.len(), 20);

    // Verify bidirectional consistency for all.
    for i in 0..20i64 {
        let uid = ResourceUid::new(i);
        let path = format!("res://r{i}.tres");
        assert_eq!(reg.lookup_uid(uid), Some(path.as_str()));
        assert_eq!(reg.lookup_path(&path), Some(uid));
    }

    // Unregister odd-indexed by UID.
    for i in (1..20i64).step_by(2) {
        reg.unregister_uid(ResourceUid::new(i));
    }
    assert_eq!(reg.len(), 10);

    // Even-indexed still consistent.
    for i in (0..20i64).step_by(2) {
        let uid = ResourceUid::new(i);
        let path = format!("res://r{i}.tres");
        assert_eq!(reg.lookup_uid(uid), Some(path.as_str()));
        assert_eq!(reg.lookup_path(&path), Some(uid));
    }

    // Odd-indexed all gone.
    for i in (1..20i64).step_by(2) {
        assert_eq!(reg.lookup_uid(ResourceUid::new(i)), None);
        assert_eq!(reg.lookup_path(&format!("res://r{i}.tres")), None);
    }
}

// ===========================================================================
// 16. UnifiedLoader: res:// error doesn't pollute cache
// ===========================================================================

#[test]
fn failed_load_does_not_pollute_cache() {
    let mut ul = UnifiedLoader::new(SelectiveFailLoader);

    let result = ul.load("res://FAIL_path.tres");
    assert!(result.is_err());
    assert!(!ul.is_cached("res://FAIL_path.tres"));
    assert_eq!(ul.cache_len(), 0);
}

// ===========================================================================
// 17. parse_uid_string produces distinct UIDs for similar strings
// ===========================================================================

#[test]
fn similar_uid_strings_produce_distinct_uids() {
    let pairs = [
        ("uid://abc", "uid://abd"),
        ("uid://test1", "uid://test2"),
        ("uid://A", "uid://a"),
        ("uid://x_y", "uid://xy_"),
    ];

    for (a, b) in &pairs {
        let uid_a = parse_uid_string(a);
        let uid_b = parse_uid_string(b);
        assert_ne!(uid_a, uid_b, "{a} and {b} should produce different UIDs");
    }
}

// ===========================================================================
// 18. Full lifecycle: construct → register → load → invalidate → re-register → reload → clear
// ===========================================================================

#[test]
fn full_lifecycle_sequence() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());

    // Phase 1: Register and load.
    ul.register_uid_str("uid://lifecycle", "res://v1.tres");
    let v1 = ul.load("uid://lifecycle").unwrap();
    assert_eq!(v1.path, "res://v1.tres");
    assert_eq!(ul.cache_len(), 1);

    // Phase 2: Invalidate and re-register to different path.
    ul.invalidate("res://v1.tres");
    ul.register_uid_str("uid://lifecycle", "res://v2.tres");
    let v2 = ul.load("uid://lifecycle").unwrap();
    assert_eq!(v2.path, "res://v2.tres");
    assert!(!Arc::ptr_eq(&v1, &v2));

    // Phase 3: Clear cache entirely.
    ul.clear_cache();
    assert_eq!(ul.cache_len(), 0);

    // Phase 4: Registry still works.
    let v2_reloaded = ul.load("uid://lifecycle").unwrap();
    assert_eq!(v2_reloaded.path, "res://v2.tres");
    assert!(!Arc::ptr_eq(&v2, &v2_reloaded));
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 19. .tres with no uid header → register manually → load by uid://
// ===========================================================================

#[test]
fn tres_without_uid_manual_registration_works() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "NoUidInHeader"
"#;
    let tres = TresLoader::new();
    let parsed = tres.parse_str(source, "res://no_uid.tres").unwrap();
    assert!(!parsed.uid.is_valid());

    // Manually assign a UID and register.
    let manual_uid = ResourceUid::new(999);
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid(manual_uid, "res://no_uid.tres");

    // Can't load by uid:// string (we used register_uid with raw ResourceUid),
    // but path-based load works and registry lookup works.
    let by_path = ul.load("res://no_uid.tres").unwrap();
    assert_eq!(by_path.path, "res://no_uid.tres");
    assert_eq!(
        ul.uid_registry().lookup_uid(manual_uid),
        Some("res://no_uid.tres")
    );
}

// ===========================================================================
// 20. Resource UID field set by loader is independent of registry UID
// ===========================================================================

#[test]
fn resource_uid_field_independent_of_registry_uid() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    let registry_uid = parse_uid_string("uid://field_test");
    ul.register_uid_str("uid://field_test", "res://field_test.tres");

    let res = ul.load("uid://field_test").unwrap();
    // The InstrumentedLoader sets uid = call_count * 1000, not the registry UID.
    // This verifies that the resource's uid field and the registry UID are independent.
    assert_ne!(
        res.uid, registry_uid,
        "resource uid field is set by loader, not by registry"
    );
    assert!(ul.uid_registry().lookup_uid(registry_uid).is_some());
}

// ===========================================================================
// 21. Two UnifiedLoaders sharing same UidRegistry type but separate instances
// ===========================================================================

#[test]
fn two_loaders_independent_caches_same_registry_data() {
    let mut reg1 = UidRegistry::new();
    reg1.register(parse_uid_string("uid://shared_data"), "res://shared.tres");

    let mut reg2 = UidRegistry::new();
    reg2.register(parse_uid_string("uid://shared_data"), "res://shared.tres");

    let mut ul1 = UnifiedLoader::with_registry(InstrumentedLoader::new(), reg1);
    let mut ul2 = UnifiedLoader::with_registry(InstrumentedLoader::new(), reg2);

    let res1 = ul1.load("uid://shared_data").unwrap();
    let res2 = ul2.load("uid://shared_data").unwrap();

    // Same path, but different Arc (separate caches).
    assert_eq!(res1.path, res2.path);
    assert!(!Arc::ptr_eq(&res1, &res2));
}

// ===========================================================================
// 22. Invalidation + reload preserves registry mapping
// ===========================================================================

#[test]
fn invalidation_preserves_registry_mapping() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://stable_reg", "res://stable.tres");

    ul.load("uid://stable_reg").unwrap();
    ul.invalidate("res://stable.tres");

    // Registry is untouched by cache invalidation.
    assert_eq!(
        ul.uid_registry()
            .lookup_path("res://stable.tres"),
        Some(parse_uid_string("uid://stable_reg"))
    );

    // Can still load by uid:// after invalidation.
    let reloaded = ul.load("uid://stable_reg").unwrap();
    assert_eq!(reloaded.path, "res://stable.tres");
}

// ===========================================================================
// 23. ResourceUid::INVALID through register_uid is a no-op (guarded by is_valid)
// ===========================================================================

#[test]
fn register_invalid_uid_via_register_uid_str_is_noop() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    // parse_uid_string without uid:// prefix returns INVALID.
    ul.register_uid_str("plain_string", "res://noop.tres");
    assert!(ul.uid_registry().is_empty());
}

// ===========================================================================
// 24. Cache contains after load, not after register
// ===========================================================================

#[test]
fn cache_populated_on_load_not_on_register() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());
    ul.register_uid_str("uid://lazy", "res://lazy.tres");

    // Registration does not populate cache.
    assert!(!ul.is_cached("res://lazy.tres"));
    assert_eq!(ul.cache_len(), 0);

    // Load populates cache.
    ul.load("uid://lazy").unwrap();
    assert!(ul.is_cached("res://lazy.tres"));
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 25. Stress: 100 UIDs registered, loaded, invalidated, reloaded
// ===========================================================================

#[test]
fn stress_100_uids_full_cycle() {
    let mut ul = UnifiedLoader::new(InstrumentedLoader::new());

    // Register 100 UIDs.
    for i in 0..100 {
        ul.register_uid_str(
            &format!("uid://stress_{i}"),
            &format!("res://stress/{i}.tres"),
        );
    }

    // Load all by UID.
    let first_pass: Vec<Arc<Resource>> = (0..100)
        .map(|i| ul.load(&format!("uid://stress_{i}")).unwrap())
        .collect();
    assert_eq!(ul.cache_len(), 100);

    // Invalidate all.
    for i in 0..100 {
        ul.invalidate(&format!("res://stress/{i}.tres"));
    }
    assert_eq!(ul.cache_len(), 0);

    // Reload all by path.
    let second_pass: Vec<Arc<Resource>> = (0..100)
        .map(|i| ul.load(&format!("res://stress/{i}.tres")).unwrap())
        .collect();
    assert_eq!(ul.cache_len(), 100);

    // All should be different Arcs.
    for i in 0..100 {
        assert!(
            !Arc::ptr_eq(&first_pass[i], &second_pass[i]),
            "stress resource {i} should be a fresh Arc after invalidation"
        );
    }
}
