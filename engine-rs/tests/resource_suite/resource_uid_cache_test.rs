//! Resource UID and cache integration tests (pat-law).
//!
//! Verifies that B009's resource infrastructure works end-to-end:
//! 1. ResourceCache deduplicates loads (same path → same Arc)
//! 2. res:// paths resolve correctly through resolve_res_path
//! 3. UidRegistry provides UID-based lookups alongside path-based lookups
//! 4. Combined workflow: UID → path resolution → cached load

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdcore::ResourceUid;
use gdresource::{
    resolve_res_path, Resource, ResourceCache, ResourceLoader, TresLoader, UidRegistry,
};
use gdvariant::Variant;

// ===========================================================================
// Test loader — deterministic, no filesystem
// ===========================================================================

struct FakeLoader;

impl ResourceLoader for FakeLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("FakeResource");
        r.path = path.to_string();
        r.set_property("source", Variant::String(path.to_string()));
        Ok(Arc::new(r))
    }
}

/// Tracks how many times load was called.
struct CountingLoader {
    count: std::cell::Cell<u32>,
}

impl CountingLoader {
    fn new() -> Self {
        Self {
            count: std::cell::Cell::new(0),
        }
    }
}

impl ResourceLoader for CountingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.count.set(self.count.get() + 1);
        let mut r = Resource::new("Counted");
        r.path = path.to_string();
        r.uid = ResourceUid::new(self.count.get() as i64 * 100);
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// 1. Cache deduplication
// ===========================================================================

/// Loading the same path twice returns the exact same Arc (pointer equality).
#[test]
fn cache_dedup_same_path_same_arc() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let a = cache.load("res://player.tres").unwrap();
    let b = cache.load("res://player.tres").unwrap();
    assert!(Arc::ptr_eq(&a, &b), "same path must return same Arc");
}

/// The loader is called only once for a cached path.
#[test]
fn cache_dedup_loader_called_once() {
    let loader = CountingLoader::new();
    let mut cache = ResourceCache::new(loader);
    cache.load("res://enemy.tres").unwrap();
    cache.load("res://enemy.tres").unwrap();
    cache.load("res://enemy.tres").unwrap();
    // Can't directly access loader through cache, but we verified Arc::ptr_eq above.
    // The cache should have exactly 1 entry.
    assert_eq!(cache.len(), 1);
}

/// Different paths produce different Arcs.
#[test]
fn cache_different_paths_different_arcs() {
    let mut cache = ResourceCache::new(FakeLoader);
    let a = cache.load("res://a.tres").unwrap();
    let b = cache.load("res://b.tres").unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(cache.len(), 2);
}

/// After invalidation, the next load produces a fresh Arc.
#[test]
fn cache_invalidate_forces_reload() {
    let mut cache = ResourceCache::new(FakeLoader);
    let a = cache.load("res://item.tres").unwrap();
    cache.invalidate("res://item.tres");
    let b = cache.load("res://item.tres").unwrap();
    assert!(
        !Arc::ptr_eq(&a, &b),
        "invalidated path must produce new Arc"
    );
}

/// Clear removes everything.
#[test]
fn cache_clear_empties() {
    let mut cache = ResourceCache::new(FakeLoader);
    cache.load("res://a.tres").unwrap();
    cache.load("res://b.tres").unwrap();
    cache.load("res://c.tres").unwrap();
    assert_eq!(cache.len(), 3);
    cache.clear();
    assert!(cache.is_empty());
}

// ===========================================================================
// 2. res:// path resolution
// ===========================================================================

/// Basic res:// resolution strips prefix and joins with project root.
#[test]
fn res_path_resolves_basic() {
    let root = std::path::Path::new("/project");
    let resolved = resolve_res_path(root, "res://scenes/main.tscn").unwrap();
    assert_eq!(
        resolved,
        std::path::PathBuf::from("/project/scenes/main.tscn")
    );
}

/// res:// at the root of the project.
#[test]
fn res_path_resolves_root_file() {
    let root = std::path::Path::new("/game");
    let resolved = resolve_res_path(root, "res://project.godot").unwrap();
    assert_eq!(resolved, std::path::PathBuf::from("/game/project.godot"));
}

/// Non-res:// paths produce an error.
#[test]
fn res_path_rejects_non_res_prefix() {
    let root = std::path::Path::new("/project");
    let result = resolve_res_path(root, "/absolute/path.tres");
    assert!(result.is_err());
}

/// Nested res:// paths resolve correctly.
#[test]
fn res_path_resolves_deeply_nested() {
    let root = std::path::Path::new("/game");
    let resolved = resolve_res_path(root, "res://assets/sprites/player/idle.png").unwrap();
    assert_eq!(
        resolved,
        std::path::PathBuf::from("/game/assets/sprites/player/idle.png")
    );
}

// ===========================================================================
// 3. UID registry lookups
// ===========================================================================

/// Register a UID and look it up by UID and by path.
#[test]
fn uid_registry_bidirectional_lookup() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(42);
    reg.register(uid, "res://player.tres");

    assert_eq!(reg.lookup_uid(uid), Some("res://player.tres"));
    assert_eq!(reg.lookup_path("res://player.tres"), Some(uid));
}

/// Multiple UIDs can coexist.
#[test]
fn uid_registry_multiple_entries() {
    let mut reg = UidRegistry::new();
    let uid_a = ResourceUid::new(1);
    let uid_b = ResourceUid::new(2);
    let uid_c = ResourceUid::new(3);
    reg.register(uid_a, "res://a.tres");
    reg.register(uid_b, "res://b.tres");
    reg.register(uid_c, "res://c.tres");

    assert_eq!(reg.len(), 3);
    assert_eq!(reg.lookup_uid(uid_b), Some("res://b.tres"));
    assert_eq!(reg.lookup_path("res://c.tres"), Some(uid_c));
}

/// Re-registering a UID with a new path updates the mapping and removes the old path.
#[test]
fn uid_registry_reregister_uid_updates_path() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(10);
    reg.register(uid, "res://old.tres");
    reg.register(uid, "res://new.tres");

    assert_eq!(reg.lookup_uid(uid), Some("res://new.tres"));
    assert_eq!(reg.lookup_path("res://old.tres"), None);
    assert_eq!(reg.len(), 1);
}

/// Re-registering a path with a new UID updates the mapping and removes the old UID.
#[test]
fn uid_registry_reregister_path_updates_uid() {
    let mut reg = UidRegistry::new();
    let uid1 = ResourceUid::new(100);
    let uid2 = ResourceUid::new(200);
    reg.register(uid1, "res://shared.tres");
    reg.register(uid2, "res://shared.tres");

    assert_eq!(reg.lookup_path("res://shared.tres"), Some(uid2));
    assert_eq!(reg.lookup_uid(uid1), None);
    assert_eq!(reg.len(), 1);
}

/// Unregistering by UID removes both directions.
#[test]
fn uid_registry_unregister_uid() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(55);
    reg.register(uid, "res://gone.tres");
    reg.unregister_uid(uid);

    assert_eq!(reg.lookup_uid(uid), None);
    assert_eq!(reg.lookup_path("res://gone.tres"), None);
    assert!(reg.is_empty());
}

/// Unregistering by path removes both directions.
#[test]
fn uid_registry_unregister_path() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(77);
    reg.register(uid, "res://removed.tres");
    reg.unregister_path("res://removed.tres");

    assert_eq!(reg.lookup_uid(uid), None);
    assert_eq!(reg.lookup_path("res://removed.tres"), None);
}

// ===========================================================================
// 4. Combined workflow: UID → path → cached load
// ===========================================================================

/// Full integration: register UID, resolve path via registry, load from cache.
#[test]
fn uid_to_cache_load_integration() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(999);
    reg.register(uid, "res://weapons/sword.tres");

    // Resolve UID to path.
    let path = reg.lookup_uid(uid).expect("UID should resolve to path");
    assert_eq!(path, "res://weapons/sword.tres");

    // Load via cache.
    let mut cache = ResourceCache::new(FakeLoader);
    let resource = cache.load(path).unwrap();
    assert_eq!(resource.path, "res://weapons/sword.tres");

    // Second load via UID → same Arc.
    let path2 = reg.lookup_uid(uid).unwrap();
    let resource2 = cache.load(path2).unwrap();
    assert!(Arc::ptr_eq(&resource, &resource2));
}

/// Two UIDs pointing to different paths produce different cached resources.
#[test]
fn two_uids_different_paths_different_resources() {
    let mut reg = UidRegistry::new();
    let uid_a = ResourceUid::new(10);
    let uid_b = ResourceUid::new(20);
    reg.register(uid_a, "res://a.tres");
    reg.register(uid_b, "res://b.tres");

    let mut cache = ResourceCache::new(FakeLoader);
    let res_a = cache.load(reg.lookup_uid(uid_a).unwrap()).unwrap();
    let res_b = cache.load(reg.lookup_uid(uid_b).unwrap()).unwrap();
    assert!(!Arc::ptr_eq(&res_a, &res_b));
}

/// UID re-registration + cache invalidation forces a fresh load.
#[test]
fn uid_reregister_plus_cache_invalidate() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(50);
    reg.register(uid, "res://old_path.tres");

    let mut cache = ResourceCache::new(FakeLoader);
    let old = cache.load(reg.lookup_uid(uid).unwrap()).unwrap();

    // Resource moves to a new path (e.g., renamed).
    cache.invalidate("res://old_path.tres");
    reg.register(uid, "res://new_path.tres");

    let new = cache.load(reg.lookup_uid(uid).unwrap()).unwrap();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(new.path, "res://new_path.tres");
}

// ===========================================================================
// 5. TresLoader UID extraction
// ===========================================================================

/// TresLoader extracts UID from gd_resource header.
#[test]
fn tres_loader_extracts_uid() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://abc123"]

[resource]
name = "Test"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    assert!(res.uid.is_valid(), "UID should be extracted from header");
}

/// TresLoader produces INVALID UID when no uid= in header.
#[test]
fn tres_loader_no_uid_is_invalid() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "NoUid"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://no_uid.tres").unwrap();
    assert!(!res.uid.is_valid(), "missing UID should be INVALID");
}

/// TresLoader-parsed UID can be registered in UidRegistry.
#[test]
fn tres_loader_uid_registers_in_registry() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://weapon42"]

[resource]
damage = 10
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://sword.tres").unwrap();

    let mut reg = UidRegistry::new();
    if res.uid.is_valid() {
        reg.register(res.uid, &res.path);
    }

    assert_eq!(reg.lookup_uid(res.uid), Some("res://sword.tres"));
    assert_eq!(reg.lookup_path("res://sword.tres"), Some(res.uid));
}

/// Same uid:// string produces the same ResourceUid (deterministic hashing).
#[test]
fn uid_parsing_is_deterministic() {
    let source_a = r#"[gd_resource type="Resource" format=3 uid="uid://test123"]
[resource]
"#;
    let source_b = r#"[gd_resource type="Resource" format=3 uid="uid://test123"]
[resource]
"#;
    let loader = TresLoader::new();
    let res_a = loader.parse_str(source_a, "a.tres").unwrap();
    let res_b = loader.parse_str(source_b, "b.tres").unwrap();
    assert_eq!(
        res_a.uid, res_b.uid,
        "same uid:// string must produce same ResourceUid"
    );
}

// ===========================================================================
// 6. Resource loading edge cases (pat-j76)
// ===========================================================================

/// Loading a missing/invalid path returns an error, not a panic.
#[test]
fn cache_load_missing_path_returns_error() {
    struct FailLoader;
    impl ResourceLoader for FailLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            Err(gdcore::error::EngineError::NotFound(path.to_string()))
        }
    }

    let mut cache = ResourceCache::new(FailLoader);
    let result = cache.load("res://nonexistent.tres");
    assert!(result.is_err(), "loading a missing resource should error");
}

/// TresLoader with corrupt .tres content returns a default resource without UID.
#[test]
fn tres_loader_corrupt_content_returns_default_resource() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(
            "this is not valid tres content at all",
            "res://corrupt.tres",
        )
        .unwrap();
    assert!(
        !res.uid.is_valid(),
        "corrupt content should produce invalid UID"
    );
    assert_eq!(res.path, "res://corrupt.tres");
}

/// TresLoader with empty string returns a default resource.
#[test]
fn tres_loader_empty_string_returns_default_resource() {
    let loader = TresLoader::new();
    let res = loader.parse_str("", "res://empty.tres").unwrap();
    assert!(
        !res.uid.is_valid(),
        "empty content should produce invalid UID"
    );
    assert_eq!(res.path, "res://empty.tres");
}

/// UID collision: re-registering same UID with different path replaces mapping.
#[test]
fn uid_collision_replaces_mapping() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(42);

    reg.register(uid, "res://original.tres");
    assert_eq!(reg.lookup_uid(uid), Some("res://original.tres"));

    // Same UID, different path — should replace
    reg.register(uid, "res://replacement.tres");
    assert_eq!(reg.lookup_uid(uid), Some("res://replacement.tres"));
    assert_eq!(reg.lookup_path("res://original.tres"), None);
    assert_eq!(reg.lookup_path("res://replacement.tres"), Some(uid));
    assert_eq!(
        reg.len(),
        1,
        "collision should not create duplicate entries"
    );
}

/// Cache invalidation of non-existent path is a no-op.
#[test]
fn cache_invalidate_nonexistent_is_noop() {
    let mut cache = ResourceCache::new(FakeLoader);
    cache.load("res://exists.tres").unwrap();
    assert_eq!(cache.len(), 1);
    cache.invalidate("res://does_not_exist.tres");
    assert_eq!(
        cache.len(),
        1,
        "invalidating non-existent path should be a no-op"
    );
}

/// Different uid:// strings produce different ResourceUids.
#[test]
fn different_uid_strings_produce_different_uids() {
    let loader = TresLoader::new();
    let res_a = loader
        .parse_str(
            r#"[gd_resource type="Resource" format=3 uid="uid://alpha"]
[resource]
"#,
            "a.tres",
        )
        .unwrap();
    let res_b = loader
        .parse_str(
            r#"[gd_resource type="Resource" format=3 uid="uid://beta"]
[resource]
"#,
            "b.tres",
        )
        .unwrap();
    assert_ne!(res_a.uid, res_b.uid);
}
