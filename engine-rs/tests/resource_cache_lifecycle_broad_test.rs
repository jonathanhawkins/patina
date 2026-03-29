//! pat-zg81: Broaden resource cache lifecycle coverage.
//!
//! Covers ResourceCache and UidRegistry operations that had zero or minimal
//! integration test coverage:
//! - ResourceCache::replace() mutation workflow
//! - ResourceCache::clear() with existing entries
//! - ResourceCache::is_empty()
//! - UidRegistry::unregister_uid() / unregister_path()
//! - UidRegistry::clear() / is_empty()
//! - UnifiedLoader::clear_cache() and reload cycle

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::cache::ResourceCache;
use gdresource::loader::ResourceLoader;
use gdresource::resource::Resource;
use gdcore::ResourceUid;
use gdresource::uid::UidRegistry;
use gdresource::UnifiedLoader;
use gdvariant::Variant;

// ===========================================================================
// Stub loader for tests
// ===========================================================================

struct StubLoader;

impl ResourceLoader for StubLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("StubResource");
        r.path = path.to_string();
        r.set_property("loaded", Variant::Bool(true));
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn make_resource(type_name: &str) -> Arc<Resource> {
    Arc::new(Resource::new(type_name))
}

fn make_resource_with_prop(type_name: &str, key: &str, val: Variant) -> Arc<Resource> {
    let mut r = Resource::new(type_name);
    r.set_property(key, val);
    Arc::new(r)
}

// ===========================================================================
// 1. ResourceCache::replace()
// ===========================================================================

#[test]
fn cache_replace_overwrites_existing() {
    let mut cache = ResourceCache::new(StubLoader);
    let _ = cache.load("res://icon.tres").unwrap();

    let r2 = make_resource_with_prop("Texture2D", "width", Variant::Int(128));
    cache.insert("res://icon.tres", r2.clone());

    let cached = cache.get("res://icon.tres").unwrap();
    assert_eq!(
        cached.get_property("width"),
        Some(&Variant::Int(128)),
        "replace should overwrite"
    );
}

#[test]
fn cache_replace_inserts_if_missing() {
    let mut cache = ResourceCache::new(StubLoader);
    assert!(cache.get("res://new.tres").is_none());

    let r = make_resource("StyleBoxFlat");
    cache.insert("res://new.tres", r);

    assert!(cache.get("res://new.tres").is_some());
}

// ===========================================================================
// 2. ResourceCache::clear() and is_empty()
// ===========================================================================

#[test]
fn cache_clear_empties_all_entries() {
    let mut cache = ResourceCache::new(StubLoader);
    let _ = cache.load("res://a.tres").unwrap();
    let _ = cache.load("res://b.tres").unwrap();
    assert!(!cache.is_empty());

    cache.clear();

    assert!(cache.is_empty());
    assert!(cache.get("res://a.tres").is_none());
    assert!(cache.get("res://b.tres").is_none());
}

#[test]
fn cache_is_empty_initially() {
    let cache = ResourceCache::new(StubLoader);
    assert!(cache.is_empty());
}

// ===========================================================================
// 3. ResourceCache::invalidate()
// ===========================================================================

#[test]
fn cache_invalidate_removes_single_entry() {
    let mut cache = ResourceCache::new(StubLoader);
    let _ = cache.load("res://a.tres").unwrap();
    let _ = cache.load("res://b.tres").unwrap();

    cache.invalidate("res://a.tres");

    assert!(cache.get("res://a.tres").is_none());
    assert!(cache.get("res://b.tres").is_some());
}

#[test]
fn cache_invalidate_nonexistent_is_noop() {
    let mut cache = ResourceCache::new(StubLoader);
    cache.invalidate("res://nope.tres"); // should not panic
}

// ===========================================================================
// 4. UidRegistry lifecycle
// ===========================================================================

#[test]
fn uid_registry_unregister_uid_removes_both_directions() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(12345);
    reg.register(uid, "res://icon.tres");

    assert_eq!(reg.lookup_uid(uid), Some("res://icon.tres"));
    assert_eq!(reg.lookup_path("res://icon.tres"), Some(uid));

    reg.unregister_uid(uid);

    assert!(reg.lookup_uid(uid).is_none());
    assert!(reg.lookup_path("res://icon.tres").is_none());
}

#[test]
fn uid_registry_unregister_path_removes_both_directions() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(99999);
    reg.register(uid, "res://theme.tres");

    reg.unregister_path("res://theme.tres");

    assert!(reg.lookup_uid(uid).is_none());
    assert!(reg.lookup_path("res://theme.tres").is_none());
}

#[test]
fn uid_registry_clear_removes_all() {
    let mut reg = UidRegistry::new();
    reg.register(ResourceUid::new(1), "res://a.tres");
    reg.register(ResourceUid::new(2), "res://b.tres");
    assert!(!reg.is_empty());

    reg.clear();

    assert!(reg.is_empty());
    assert!(reg.lookup_uid(ResourceUid::new(1)).is_none());
}

#[test]
fn uid_registry_is_empty_initially() {
    let reg = UidRegistry::new();
    assert!(reg.is_empty());
}

#[test]
fn uid_registry_unregister_nonexistent_is_noop() {
    let mut reg = UidRegistry::new();
    reg.unregister_uid(ResourceUid::new(42)); // no panic
    reg.unregister_path("res://nope.tres"); // no panic
}

// ===========================================================================
// 5. UnifiedLoader cache clear and reload cycle
// ===========================================================================

#[test]
fn unified_loader_clear_cache_then_reload() {
    let mut loader = UnifiedLoader::new(StubLoader);

    // Load a resource (goes through cache)
    let _ = loader.load("res://mat.tres").unwrap();
    assert!(loader.is_cached("res://mat.tres"));

    loader.clear_cache();
    assert!(!loader.is_cached("res://mat.tres"));

    // Reload — should work fine after cache clear
    let _ = loader.load("res://mat.tres").unwrap();
    assert!(loader.is_cached("res://mat.tres"));
}

// ===========================================================================
// 6. Compound: register, cache, mutate, invalidate, re-register
// ===========================================================================

#[test]
fn full_resource_lifecycle_register_cache_mutate_invalidate() {
    let mut loader = UnifiedLoader::new(StubLoader);

    // Step 1: Register UID
    let uid = ResourceUid::new(777);
    loader.register_uid(uid, "res://player.tres");

    // Step 2: Load via the loader (populates cache)
    let _ = loader.load("res://player.tres").unwrap();
    assert!(loader.is_cached("res://player.tres"));

    // Step 3: Replace with mutated version
    let r2 = make_resource_with_prop("CharacterBody2D", "speed", Variant::Float(200.0));
    loader.replace_cached("res://player.tres", r2);

    let cached = loader.get_cached("res://player.tres").unwrap();
    assert_eq!(cached.get_property("speed"), Some(&Variant::Float(200.0)));

    // Step 4: Invalidate cache entry
    loader.invalidate("res://player.tres");
    assert!(!loader.is_cached("res://player.tres"));

    // Step 5: UID mapping should still be intact
    assert_eq!(
        loader.uid_registry().lookup_uid(uid),
        Some("res://player.tres")
    );
}

// ===========================================================================
// 7. UID-based load after registration
// ===========================================================================

#[test]
fn unified_loader_load_by_uid_str_after_registration() {
    let mut loader = UnifiedLoader::new(StubLoader);
    loader.register_uid_str("uid://abc123", "res://enemy.tres");

    // Load using uid:// reference
    let r = loader.load("uid://abc123").unwrap();
    assert_eq!(r.get_property("loaded"), Some(&Variant::Bool(true)));
}

#[test]
fn unified_loader_replace_cached_then_get() {
    let mut loader = UnifiedLoader::new(StubLoader);

    // Pre-populate
    let _ = loader.load("res://weapon.tres").unwrap();

    // Replace
    let new_r = make_resource_with_prop("Weapon", "damage", Variant::Int(50));
    loader.replace_cached("res://weapon.tres", new_r);

    let cached = loader.get_cached("res://weapon.tres").unwrap();
    assert_eq!(cached.get_property("damage"), Some(&Variant::Int(50)));
}
