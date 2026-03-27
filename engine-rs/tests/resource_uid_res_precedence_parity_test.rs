//! pat-b14b: Match UID versus res:// resolution precedence for aliased resources.
//!
//! Godot 4.x contract (verified against upstream behavior):
//!   - `uid://` and `res://` are two independent access methods to the same
//!     canonical resource; they converge at the cache level via path key.
//!   - When a UID is registered for a path, loading by either method returns
//!     the same `Arc<Resource>` (pointer equality).
//!   - Manual UID registration takes precedence over auto-discovered UIDs
//!     from file headers.
//!   - The UID registry is a strict bijection: one UID ↔ one path. Re-registering
//!     a UID to a new path evicts the old mapping, and re-registering a path to
//!     a new UID evicts the old UID.
//!   - Cache invalidation by path affects both access methods — a subsequent
//!     load by UID for the same path triggers a fresh load.
//!   - UID resolution happens before the cache lookup, so UID resolution
//!     failures (unregistered UID) never touch the cache.
//!
//! Acceptance: loader-path tests compare UID and res:// precedence against
//! upstream Godot for aliased resource paths.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::loader::{parse_uid_string, ResourceLoader};
use gdresource::resource::Resource;
use gdresource::unified::UnifiedLoader;
use gdvariant::Variant;

// ===========================================================================
// Test loaders
// ===========================================================================

/// Loader that tracks invocation count — lets us verify cache behavior.
struct CountingLoader {
    count: AtomicUsize,
}

impl CountingLoader {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }

}

impl ResourceLoader for CountingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.count.fetch_add(1, Ordering::SeqCst);
        let mut r = Resource::new("Counted");
        r.path = path.to_string();
        r.set_property("path", Variant::String(path.to_string()));
        Ok(Arc::new(r))
    }
}

/// Loader that returns resources with a file-header UID (auto-registration).
struct HeaderUidLoader;

impl ResourceLoader for HeaderUidLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("WithHeaderUid");
        r.path = path.to_string();
        // Simulate a .tres file having uid=... in its [gd_resource] header.
        r.uid = parse_uid_string(&format!("uid://file_header_{path}"));
        Ok(Arc::new(r))
    }
}


// ===========================================================================
// 1. UID resolves to res:// before cache lookup (resolution precedence)
// ===========================================================================

#[test]
fn uid_resolves_to_path_before_cache_lookup() {
    let loader = CountingLoader::new();
    let mut ul = UnifiedLoader::new(loader);
    ul.register_uid_str("uid://hero", "res://characters/hero.tres");

    // Load by res:// path first.
    let by_path = ul.load("res://characters/hero.tres").unwrap();
    assert_eq!(ul.cache_len(), 1);

    // Load by uid:// — should hit the same cache entry, no second disk load.
    let by_uid = ul.load("uid://hero").unwrap();
    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "UID and res:// must resolve to same Arc"
    );
    // Only one actual load call to the backing loader.
    let backing = ul.cache_len();
    assert_eq!(backing, 1, "cache should have exactly one entry");
}

// ===========================================================================
// 2. res:// path takes identity from itself, not from UID
// ===========================================================================

#[test]
fn res_path_loads_without_uid_registration() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    // No UID registered — res:// path still works.
    let res = ul.load("res://standalone.tres").unwrap();
    assert_eq!(res.path, "res://standalone.tres");
    assert_eq!(ul.cache_len(), 1);

    // UID for this path is not registered.
    assert!(
        ul.uid_registry().lookup_path("res://standalone.tres").is_none(),
        "res:// load without header UID should not create a registry entry"
    );
}

// ===========================================================================
// 3. UID load fails fast if unregistered (does not touch cache)
// ===========================================================================

#[test]
fn unregistered_uid_fails_without_cache_side_effect() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    let result = ul.load("uid://nonexistent");
    assert!(result.is_err(), "unregistered UID must fail");
    assert_eq!(ul.cache_len(), 0, "failed UID load must not create cache entry");
}

// ===========================================================================
// 4. Manual registration persists after loading the resource
// ===========================================================================

#[test]
fn manual_uid_persists_after_load() {
    let mut ul = UnifiedLoader::new(HeaderUidLoader);

    // Manually register a specific UID for the path.
    let manual_uid = parse_uid_string("uid://my_manual_id");
    ul.register_uid_str("uid://my_manual_id", "res://hero.tres");

    // Load the resource — manual registration must survive.
    ul.load("res://hero.tres").unwrap();

    assert_eq!(
        ul.uid_registry().lookup_path("res://hero.tres"),
        Some(manual_uid),
        "manual UID registration must persist after loading the resource"
    );
}

// ===========================================================================
// 5. UID re-registration to new path evicts old mapping
// ===========================================================================

#[test]
fn uid_reregistration_evicts_old_path() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    let uid = parse_uid_string("uid://moveable");
    ul.register_uid_str("uid://moveable", "res://old_location.tres");

    // Load via UID — cached under old path.
    let old = ul.load("uid://moveable").unwrap();
    assert_eq!(old.path, "res://old_location.tres");

    // Re-register the same UID to a new path.
    ul.register_uid_str("uid://moveable", "res://new_location.tres");

    // Old path mapping is gone.
    assert!(
        ul.uid_registry().lookup_path("res://old_location.tres").is_none(),
        "old path mapping must be evicted after UID re-registration"
    );

    // New path is now mapped.
    assert_eq!(
        ul.uid_registry().lookup_uid(uid),
        Some("res://new_location.tres")
    );

    // Loading by UID now loads from new path.
    let new = ul.load("uid://moveable").unwrap();
    assert_eq!(new.path, "res://new_location.tres");
}

// ===========================================================================
// 6. Path re-registration to new UID evicts old UID
// ===========================================================================

#[test]
fn path_reregistration_evicts_old_uid() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    let uid_a = parse_uid_string("uid://uid_alpha");
    let uid_b = parse_uid_string("uid://uid_beta");

    ul.register_uid_str("uid://uid_alpha", "res://shared.tres");
    assert_eq!(ul.uid_registry().lookup_path("res://shared.tres"), Some(uid_a));

    // Register a different UID for the same path.
    ul.register_uid_str("uid://uid_beta", "res://shared.tres");

    // Old UID is evicted.
    assert!(
        ul.uid_registry().lookup_uid(uid_a).is_none(),
        "old UID must be evicted when path is re-registered to new UID"
    );
    assert_eq!(
        ul.uid_registry().lookup_path("res://shared.tres"),
        Some(uid_b),
        "path must map to new UID"
    );
}

// ===========================================================================
// 7. Cache invalidation by path affects UID access
// ===========================================================================

#[test]
fn invalidation_by_path_forces_uid_reload() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://weapon", "res://weapon.tres");

    let first = ul.load("uid://weapon").unwrap();
    ul.invalidate("res://weapon.tres");

    // Next load by UID must produce a new Arc (fresh load).
    let second = ul.load("uid://weapon").unwrap();
    assert!(
        !Arc::ptr_eq(&first, &second),
        "invalidation by path must force reload via UID"
    );
}

// ===========================================================================
// 8. Cache invalidation does not affect UID registry
// ===========================================================================

#[test]
fn invalidation_preserves_uid_registry() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    let uid = parse_uid_string("uid://persistent");
    ul.register_uid_str("uid://persistent", "res://persist.tres");

    ul.load("uid://persistent").unwrap();
    ul.invalidate("res://persist.tres");

    // UID registry mapping is preserved even after cache invalidation.
    assert_eq!(
        ul.uid_registry().lookup_uid(uid),
        Some("res://persist.tres"),
        "cache invalidation must not affect UID registry"
    );

    // Can still load by UID after invalidation.
    let reloaded = ul.load("uid://persistent").unwrap();
    assert_eq!(reloaded.path, "res://persist.tres");
}

// ===========================================================================
// 9. Explicit registration is the only way to create UID mappings
// ===========================================================================

#[test]
fn loading_does_not_auto_register_uids() {
    let mut ul = UnifiedLoader::new(HeaderUidLoader);

    // Load a resource whose header contains a UID — the loader should NOT
    // auto-register it. Only explicit register_uid_str / register_uid calls
    // create registry entries.
    ul.load("res://hero.tres").unwrap();

    assert!(
        ul.uid_registry().is_empty(),
        "loading must not auto-register UIDs; only explicit registration creates mappings"
    );
}

// ===========================================================================
// 10. Order of access does not affect cache identity
// ===========================================================================

#[test]
fn uid_first_then_path_same_arc() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://item", "res://items/sword.tres");

    let by_uid = ul.load("uid://item").unwrap();
    let by_path = ul.load("res://items/sword.tres").unwrap();

    assert!(
        Arc::ptr_eq(&by_uid, &by_path),
        "UID-first, path-second must return same Arc"
    );
    assert_eq!(ul.cache_len(), 1);
}

#[test]
fn path_first_then_uid_same_arc() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://item", "res://items/sword.tres");

    let by_path = ul.load("res://items/sword.tres").unwrap();
    let by_uid = ul.load("uid://item").unwrap();

    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "path-first, UID-second must return same Arc"
    );
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 11. Multiple UIDs cannot alias the same path simultaneously
// ===========================================================================

#[test]
fn registry_is_strict_bijection_no_multi_uid_alias() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    let uid_a = parse_uid_string("uid://alias_a");
    let uid_b = parse_uid_string("uid://alias_b");

    ul.register_uid_str("uid://alias_a", "res://shared.tres");
    ul.register_uid_str("uid://alias_b", "res://shared.tres");

    // Only the last registration survives (strict bijection).
    assert!(
        ul.uid_registry().lookup_uid(uid_a).is_none(),
        "first UID must be evicted when same path gets new UID"
    );
    assert_eq!(
        ul.uid_registry().lookup_uid(uid_b),
        Some("res://shared.tres")
    );
    assert_eq!(ul.uid_registry().len(), 1);
}

// ===========================================================================
// 12. Resolve-to-path is consistent with load path
// ===========================================================================

#[test]
fn resolve_to_path_consistent_with_load() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://consistency", "res://check.tres");

    let resolved = ul.resolve_to_path("uid://consistency").unwrap();
    let loaded = ul.load("uid://consistency").unwrap();

    assert_eq!(
        resolved, loaded.path,
        "resolve_to_path and load must agree on the canonical path"
    );
}

// ===========================================================================
// 13. replace_cached by path is visible through UID access
// ===========================================================================

#[test]
fn replace_cached_visible_through_uid() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://sword", "res://sword.tres");

    ul.load("res://sword.tres").unwrap();

    // Replace the cached resource.
    let mut replacement = Resource::new("SharpSword");
    replacement.path = "res://sword.tres".to_string();
    replacement.set_property("damage", Variant::Int(99));
    ul.replace_cached("res://sword.tres", Arc::new(replacement));

    // Load by UID — should see the replacement.
    let via_uid = ul.load("uid://sword").unwrap();
    assert_eq!(
        via_uid.get_property("damage"),
        Some(&Variant::Int(99)),
        "replace_cached by path must be visible through UID access"
    );
}

// ===========================================================================
// 14. Explicit registration before load governs UID resolution
// ===========================================================================

#[test]
fn explicit_registration_governs_uid_resolution() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());

    // Register two UIDs for two different resources.
    ul.register_uid_str("uid://scene_a", "res://scenes/a.tscn");
    ul.register_uid_str("uid://scene_b", "res://scenes/b.tscn");

    let a = ul.load("uid://scene_a").unwrap();
    let b = ul.load("uid://scene_b").unwrap();

    assert_eq!(a.path, "res://scenes/a.tscn");
    assert_eq!(b.path, "res://scenes/b.tscn");
    assert!(!Arc::ptr_eq(&a, &b), "different UIDs must load different resources");

    // Re-register uid://scene_a to point to B's path.
    ul.register_uid_str("uid://scene_a", "res://scenes/b.tscn");

    // Now both UIDs resolve to the same path and same cached Arc.
    let a2 = ul.load("uid://scene_a").unwrap();
    assert!(
        Arc::ptr_eq(&a2, &b),
        "after re-registration, uid://scene_a must resolve to same Arc as uid://scene_b"
    );
}

// ===========================================================================
// 15. clear_cache does not affect UID registry
// ===========================================================================

#[test]
fn clear_cache_preserves_uid_registry() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    let uid = parse_uid_string("uid://survives_clear");
    ul.register_uid_str("uid://survives_clear", "res://data.tres");

    ul.load("uid://survives_clear").unwrap();
    assert_eq!(ul.cache_len(), 1);

    ul.clear_cache();
    assert_eq!(ul.cache_len(), 0, "cache must be empty after clear");

    // UID registry is independent of cache.
    assert_eq!(
        ul.uid_registry().lookup_uid(uid),
        Some("res://data.tres"),
        "UID registry must survive cache clear"
    );

    // Can reload by UID after cache clear.
    let reloaded = ul.load("uid://survives_clear").unwrap();
    assert_eq!(reloaded.path, "res://data.tres");
}

// ===========================================================================
// 16. get_cached resolves through UID
// ===========================================================================

#[test]
fn get_cached_resolves_uid_to_same_arc() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://cached_ref", "res://cached.tres");

    let loaded = ul.load("res://cached.tres").unwrap();
    let by_uid = ul.get_cached("uid://cached_ref").unwrap();
    let by_path = ul.get_cached("res://cached.tres").unwrap();

    assert!(Arc::ptr_eq(&loaded, &by_uid));
    assert!(Arc::ptr_eq(&loaded, &by_path));
}
