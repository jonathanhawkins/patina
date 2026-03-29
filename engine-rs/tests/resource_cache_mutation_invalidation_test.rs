//! pat-u81g: Resource cache invalidation after in-memory mutation.
//!
//! Validates that the cache correctly handles the clone-mutate-replace
//! workflow, matching Godot's resource mutation semantics:
//!
//! - After replacing a cached resource, new loads return the updated version
//! - Old Arc holders retain their reference to previous data
//! - UID-based lookups resolve to the replaced resource
//! - Multiple replace cycles produce correct results
//! - Invalidation after replacement falls back to the loader (disk reload)
//! - get_cached returns None for uncached paths, correct Arc for cached ones
//! - Sub-resource mutations propagate through replace

use std::cell::Cell;
use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::{Resource, ResourceCache, ResourceLoader, UnifiedLoader};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Test loaders
// ---------------------------------------------------------------------------

struct CountingLoader {
    call_count: Cell<u32>,
}

impl CountingLoader {
    fn new() -> Self {
        Self {
            call_count: Cell::new(0),
        }
    }
}

impl ResourceLoader for CountingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let n = self.call_count.get() + 1;
        self.call_count.set(n);
        let mut r = Resource::new("Counted");
        r.path = path.to_string();
        r.set_property("load_index", Variant::Int(n as i64));
        Ok(Arc::new(r))
    }
}

struct FakeLoader;

impl ResourceLoader for FakeLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("Fake");
        r.path = path.to_string();
        r.set_property("origin", Variant::String("disk".to_string()));
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// 1. Clone-mutate-replace workflow
// ===========================================================================

#[test]
fn clone_mutate_replace_basic_workflow() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let original = cache.load("res://player.tres").unwrap();
    assert_eq!(
        original.get_property("load_index"),
        Some(&Variant::Int(1))
    );

    // Clone the inner Resource (not Arc::clone), mutate, replace.
    let mut mutated = (*original).clone();
    mutated.set_property("hp", Variant::Int(100));
    mutated.set_property("name", Variant::String("Hero".to_string()));
    let mutated_arc = Arc::new(mutated);

    cache.insert("res://player.tres", Arc::clone(&mutated_arc));

    // New load returns mutated version.
    let fetched = cache.load("res://player.tres").unwrap();
    assert!(Arc::ptr_eq(&fetched, &mutated_arc));
    assert_eq!(fetched.get_property("hp"), Some(&Variant::Int(100)));
    assert_eq!(
        fetched.get_property("name"),
        Some(&Variant::String("Hero".to_string()))
    );

    // Original load_index is preserved in the mutated clone.
    assert_eq!(fetched.get_property("load_index"), Some(&Variant::Int(1)));

    // Original holder is unaffected — no "hp" property.
    assert!(original.get_property("hp").is_none());
}

// ===========================================================================
// 2. Old holders survive replacement
// ===========================================================================

#[test]
fn old_arc_holders_survive_replace() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let holder_a = cache.load("res://item.tres").unwrap();
    let holder_b = cache.load("res://item.tres").unwrap();
    assert!(Arc::ptr_eq(&holder_a, &holder_b));
    assert_eq!(Arc::strong_count(&holder_a), 3); // cache + a + b

    // Replace.
    let replacement = Arc::new(Resource::new("Replaced"));
    cache.insert("res://item.tres", replacement);

    // Old holders still alive, strong_count dropped by 1 (cache replaced its ref).
    assert_eq!(Arc::strong_count(&holder_a), 2); // a + b
    assert!(Arc::ptr_eq(&holder_a, &holder_b));

    // New load returns the replacement.
    let new = cache.load("res://item.tres").unwrap();
    assert!(!Arc::ptr_eq(&holder_a, &new));
}

// ===========================================================================
// 3. Multiple replace cycles
// ===========================================================================

#[test]
fn multiple_replace_cycles_produce_correct_state() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    cache.load("res://counter.tres").unwrap();

    let mut arcs = Vec::new();
    for i in 1..=5 {
        let current = cache.load("res://counter.tres").unwrap();
        let mut mutated = (*current).clone();
        mutated.set_property("version", Variant::Int(i));
        let new_arc = Arc::new(mutated);
        arcs.push(Arc::clone(&new_arc));
        cache.insert("res://counter.tres", new_arc);
    }

    // Final load returns the last replacement.
    let final_res = cache.load("res://counter.tres").unwrap();
    assert_eq!(
        final_res.get_property("version"),
        Some(&Variant::Int(5))
    );

    // All intermediate Arcs are distinct.
    for i in 0..arcs.len() {
        for j in (i + 1)..arcs.len() {
            assert!(
                !Arc::ptr_eq(&arcs[i], &arcs[j]),
                "replace cycle {i} and {j} must produce different Arcs"
            );
        }
    }
}

// ===========================================================================
// 4. Invalidation after replacement reloads from disk
// ===========================================================================

#[test]
fn invalidate_after_replace_reloads_from_loader() {
    let mut cache = ResourceCache::new(FakeLoader);
    let original = cache.load("res://scene.tres").unwrap();
    assert_eq!(
        original.get_property("origin"),
        Some(&Variant::String("disk".to_string()))
    );

    // Replace with in-memory mutation.
    let mut mutated = (*original).clone();
    mutated.set_property("origin", Variant::String("memory".to_string()));
    cache.insert("res://scene.tres", Arc::new(mutated));

    let after_replace = cache.load("res://scene.tres").unwrap();
    assert_eq!(
        after_replace.get_property("origin"),
        Some(&Variant::String("memory".to_string()))
    );

    // Invalidate — next load goes back to disk (the loader).
    cache.invalidate("res://scene.tres");
    let after_invalidate = cache.load("res://scene.tres").unwrap();
    assert_eq!(
        after_invalidate.get_property("origin"),
        Some(&Variant::String("disk".to_string()))
    );
    assert!(!Arc::ptr_eq(&after_replace, &after_invalidate));
}

// ===========================================================================
// 5. get_cached semantics
// ===========================================================================

#[test]
fn get_cached_returns_none_for_uncached() {
    let cache = ResourceCache::new(CountingLoader::new());
    assert!(cache.get("res://missing.tres").is_none());
}

#[test]
fn get_cached_returns_correct_arc_after_load() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let loaded = cache.load("res://item.tres").unwrap();
    let cached = cache.get("res://item.tres").unwrap();
    assert!(Arc::ptr_eq(&loaded, &cached));
}

#[test]
fn get_cached_returns_replaced_arc() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    cache.load("res://item.tres").unwrap();

    let mut mutated = Resource::new("Mutated");
    mutated.set_property("replaced", Variant::Bool(true));
    let mutated_arc = Arc::new(mutated);
    cache.insert("res://item.tres", Arc::clone(&mutated_arc));

    let cached = cache.get("res://item.tres").unwrap();
    assert!(Arc::ptr_eq(&cached, &mutated_arc));
}

#[test]
fn get_cached_returns_none_after_invalidate() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    cache.load("res://item.tres").unwrap();
    cache.invalidate("res://item.tres");
    assert!(cache.get("res://item.tres").is_none());
}

// ===========================================================================
// 6. UnifiedLoader replace + UID resolution
// ===========================================================================

#[test]
fn unified_replace_then_uid_load() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://weapon", "res://weapon.tres");
    let _original = ul.load("uid://weapon").unwrap();

    // Replace the cached version.
    let mut mutated = Resource::new("EnchantedWeapon");
    mutated.path = "res://weapon.tres".to_string();
    mutated.set_property("enchanted", Variant::Bool(true));
    ul.replace_cached("res://weapon.tres", Arc::new(mutated));

    // UID load resolves to the replaced resource.
    let via_uid = ul.load("uid://weapon").unwrap();
    assert_eq!(
        via_uid.get_property("enchanted"),
        Some(&Variant::Bool(true))
    );
}

#[test]
fn unified_get_cached_via_uid() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://armor", "res://armor.tres");
    let loaded = ul.load("uid://armor").unwrap();

    let cached = ul.get_cached("uid://armor").unwrap();
    assert!(Arc::ptr_eq(&loaded, &cached));

    // Also works via path.
    let cached_path = ul.get_cached("res://armor.tres").unwrap();
    assert!(Arc::ptr_eq(&loaded, &cached_path));
}

#[test]
fn unified_replace_does_not_affect_other_paths() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    let a = ul.load("res://a.tres").unwrap();
    let b = ul.load("res://b.tres").unwrap();

    ul.replace_cached("res://a.tres", Arc::new(Resource::new("ReplacedA")));

    // B untouched.
    let b2 = ul.load("res://b.tres").unwrap();
    assert!(Arc::ptr_eq(&b, &b2));

    // A replaced.
    let a2 = ul.load("res://a.tres").unwrap();
    assert!(!Arc::ptr_eq(&a, &a2));
    assert_eq!(a2.class_name, "ReplacedA");
}

// ===========================================================================
// 7. Sub-resource mutation through replace
// ===========================================================================

#[test]
fn replace_with_modified_subresources() {
    let mut cache = ResourceCache::new(FakeLoader);
    let original = cache.load("res://theme.tres").unwrap();

    // Clone and add a sub-resource.
    let mut mutated = (*original).clone();
    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("bg_color", Variant::String("red".to_string()));
    mutated
        .subresources
        .insert("style_1".to_string(), Arc::new(sub));
    cache.insert("res://theme.tres", Arc::new(mutated));

    // Reload and verify sub-resource is present.
    let fetched = cache.load("res://theme.tres").unwrap();
    assert!(fetched.subresources.contains_key("style_1"));
    let sub_ref = &fetched.subresources["style_1"];
    assert_eq!(
        sub_ref.get_property("bg_color"),
        Some(&Variant::String("red".to_string()))
    );

    // Original has no sub-resources (FakeLoader doesn't add them).
    assert!(original.subresources.is_empty());
}

// ===========================================================================
// 8. Replace with property removal
// ===========================================================================

#[test]
fn replace_can_remove_properties() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let original = cache.load("res://item.tres").unwrap();
    assert!(original.get_property("load_index").is_some());

    // Clone and remove the property.
    let mut mutated = (*original).clone();
    mutated.remove_property("load_index");
    mutated.set_property("custom", Variant::Bool(true));
    cache.insert("res://item.tres", Arc::new(mutated));

    let fetched = cache.load("res://item.tres").unwrap();
    assert!(fetched.get_property("load_index").is_none());
    assert_eq!(
        fetched.get_property("custom"),
        Some(&Variant::Bool(true))
    );
}

// ===========================================================================
// 9. Arc strong count consistency through replace
// ===========================================================================

#[test]
fn strong_count_consistent_through_replace() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let r1 = cache.load("res://counted.tres").unwrap();
    let _r2 = cache.load("res://counted.tres").unwrap();
    assert_eq!(Arc::strong_count(&r1), 3); // cache + r1 + r2

    let replacement = Arc::new(Resource::new("New"));
    cache.insert("res://counted.tres", Arc::clone(&replacement));

    // Old Arcs lost the cache ref.
    assert_eq!(Arc::strong_count(&r1), 2); // r1 + r2
    // Replacement has: cache + our local clone.
    assert_eq!(Arc::strong_count(&replacement), 2);

    let r3 = cache.load("res://counted.tres").unwrap();
    assert_eq!(Arc::strong_count(&replacement), 3); // cache + replacement + r3
    assert!(Arc::ptr_eq(&r3, &replacement));
}

// ===========================================================================
// 10. Cache len stays consistent through replace
// ===========================================================================

#[test]
fn cache_len_consistent_through_replace() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    cache.load("res://a.tres").unwrap();
    cache.load("res://b.tres").unwrap();
    assert_eq!(cache.len(), 2);

    // Replace A — len stays 2.
    cache.insert("res://a.tres", Arc::new(Resource::new("NewA")));
    assert_eq!(cache.len(), 2);

    // Replace a new path — len becomes 3.
    cache.insert("res://c.tres", Arc::new(Resource::new("C")));
    assert_eq!(cache.len(), 3);

    // Invalidate B — len becomes 2.
    cache.invalidate("res://b.tres");
    assert_eq!(cache.len(), 2);
}
