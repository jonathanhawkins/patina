//! Cache deduplication regression tests (pat-2iu).
//!
//! Stress-tests the resource cache and UID registry to ensure correctness
//! under repeated loads, bulk operations, invalidation, and UID churn.

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdcore::ResourceUid;
use gdresource::{Resource, ResourceCache, ResourceLoader, UidRegistry, UnifiedLoader};
use gdvariant::Variant;

// ===========================================================================
// Test loaders
// ===========================================================================

struct FakeLoader;

impl ResourceLoader for FakeLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = Resource::new("Fake");
        r.path = path.to_string();
        Ok(Arc::new(r))
    }
}

/// Loader that embeds a unique counter in each resource.
struct SequenceLoader {
    counter: std::cell::Cell<u64>,
}

impl SequenceLoader {
    fn new() -> Self {
        Self {
            counter: std::cell::Cell::new(0),
        }
    }
}

impl ResourceLoader for SequenceLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let seq = self.counter.get();
        self.counter.set(seq + 1);
        let mut r = Resource::new("Seq");
        r.path = path.to_string();
        r.set_property("seq", Variant::Int(seq as i64));
        Ok(Arc::new(r))
    }
}

// ===========================================================================
// 1. Load same resource 100 times → same Arc, cache size stays 1
// ===========================================================================

#[test]
fn load_same_path_100_times_same_arc() {
    let mut cache = ResourceCache::new(SequenceLoader::new());
    let first = cache.load("res://stress.tres").unwrap();

    for _ in 1..100 {
        let again = cache.load("res://stress.tres").unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    }

    assert_eq!(cache.len(), 1);
}

#[test]
fn load_same_path_100_times_loader_called_once() {
    let mut cache = ResourceCache::new(SequenceLoader::new());

    let mut arcs = Vec::new();
    for _ in 0..100 {
        arcs.push(cache.load("res://single.tres").unwrap());
    }

    // All Arcs point to same allocation.
    for arc in &arcs {
        assert!(Arc::ptr_eq(&arcs[0], arc));
    }

    // The sequence counter in the resource should be 0 (loaded once).
    assert_eq!(
        arcs[0].get_property("seq"),
        Some(&Variant::Int(0)),
        "loader should have been called exactly once"
    );
}

// ===========================================================================
// 2. Load 50 different resources → cache size 50, all unique Arcs
// ===========================================================================

#[test]
fn load_50_different_resources_all_unique() {
    let mut cache = ResourceCache::new(FakeLoader);
    let mut arcs = Vec::new();

    for i in 0..50 {
        let res = cache.load(&format!("res://resource_{i}.tres")).unwrap();
        arcs.push(res);
    }

    assert_eq!(cache.len(), 50);

    // Every pair should be distinct.
    for i in 0..50 {
        for j in (i + 1)..50 {
            assert!(
                !Arc::ptr_eq(&arcs[i], &arcs[j]),
                "resources {i} and {j} should be distinct Arcs"
            );
        }
    }
}

#[test]
fn load_50_then_reload_all_cached() {
    let mut cache = ResourceCache::new(SequenceLoader::new());

    // First pass: load 50 resources.
    let first_pass: Vec<_> = (0..50)
        .map(|i| cache.load(&format!("res://item_{i}.tres")).unwrap())
        .collect();

    // Second pass: reload all 50.
    for i in 0..50 {
        let reloaded = cache.load(&format!("res://item_{i}.tres")).unwrap();
        assert!(Arc::ptr_eq(&first_pass[i], &reloaded));
    }

    assert_eq!(cache.len(), 50);
}

// ===========================================================================
// 3. Invalidate + reload → new Arc, old references still valid
// ===========================================================================

#[test]
fn invalidate_reload_produces_new_arc() {
    let mut cache = ResourceCache::new(SequenceLoader::new());

    let old = cache.load("res://mutable.tres").unwrap();
    assert_eq!(old.get_property("seq"), Some(&Variant::Int(0)));

    cache.invalidate("res://mutable.tres");
    let new = cache.load("res://mutable.tres").unwrap();
    assert_eq!(new.get_property("seq"), Some(&Variant::Int(1)));

    assert!(!Arc::ptr_eq(&old, &new));
}

#[test]
fn old_arc_still_valid_after_invalidate() {
    let mut cache = ResourceCache::new(SequenceLoader::new());

    let old = cache.load("res://kept.tres").unwrap();
    let old_clone = Arc::clone(&old);

    cache.invalidate("res://kept.tres");
    let _new = cache.load("res://kept.tres").unwrap();

    // Old references are still valid and usable.
    assert_eq!(old.path, "res://kept.tres");
    assert_eq!(old.get_property("seq"), Some(&Variant::Int(0)));
    assert!(Arc::ptr_eq(&old, &old_clone));
    assert_eq!(Arc::strong_count(&old), 2); // old + old_clone
}

#[test]
fn invalidate_reload_cycle_10_times() {
    let mut cache = ResourceCache::new(SequenceLoader::new());
    let mut previous_arcs = Vec::new();

    for i in 0..10 {
        let res = cache.load("res://cycled.tres").unwrap();
        assert_eq!(res.get_property("seq"), Some(&Variant::Int(i)));

        // Verify this is different from all previous loads.
        for prev in &previous_arcs {
            assert!(!Arc::ptr_eq(&res, prev));
        }

        previous_arcs.push(res);
        cache.invalidate("res://cycled.tres");
    }

    // All 10 previous Arcs are still alive.
    assert_eq!(previous_arcs.len(), 10);
    for (i, arc) in previous_arcs.iter().enumerate() {
        assert_eq!(arc.get_property("seq"), Some(&Variant::Int(i as i64)));
    }
}

// ===========================================================================
// 4. Clear cache while resources are held → Arcs survive
// ===========================================================================

#[test]
fn clear_cache_arcs_survive() {
    let mut cache = ResourceCache::new(FakeLoader);

    let a = cache.load("res://a.tres").unwrap();
    let b = cache.load("res://b.tres").unwrap();
    let c = cache.load("res://c.tres").unwrap();

    cache.clear();
    assert!(cache.is_empty());

    // All Arcs are still valid — Arc refcount keeps them alive.
    assert_eq!(a.path, "res://a.tres");
    assert_eq!(b.path, "res://b.tres");
    assert_eq!(c.path, "res://c.tres");

    // Reload produces new Arcs.
    let a2 = cache.load("res://a.tres").unwrap();
    assert!(!Arc::ptr_eq(&a, &a2));
}

#[test]
fn clear_cache_with_50_held_references() {
    let mut cache = ResourceCache::new(SequenceLoader::new());

    let held: Vec<_> = (0..50)
        .map(|i| cache.load(&format!("res://held_{i}.tres")).unwrap())
        .collect();

    cache.clear();
    assert!(cache.is_empty());

    // All 50 references still valid.
    for (i, arc) in held.iter().enumerate() {
        assert_eq!(arc.path, format!("res://held_{i}.tres"));
        assert_eq!(arc.get_property("seq"), Some(&Variant::Int(i as i64)));
    }
}

// ===========================================================================
// 5. Rapid register/unregister UID cycles don't corrupt state
// ===========================================================================

#[test]
fn uid_register_unregister_100_cycles() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(42);

    for i in 0..100 {
        let path = format!("res://cycle_{i}.tres");
        reg.register(uid, &path);
        assert_eq!(reg.lookup_uid(uid), Some(path.as_str()));
        assert_eq!(reg.lookup_path(&path), Some(uid));

        reg.unregister_uid(uid);
        assert_eq!(reg.lookup_uid(uid), None);
        assert_eq!(reg.lookup_path(&path), None);
        assert!(reg.is_empty());
    }
}

#[test]
fn uid_register_50_then_unregister_all() {
    let mut reg = UidRegistry::new();

    for i in 0..50 {
        reg.register(ResourceUid::new(i), format!("res://r_{i}.tres"));
    }
    assert_eq!(reg.len(), 50);

    for i in 0..50 {
        reg.unregister_uid(ResourceUid::new(i));
    }
    assert!(reg.is_empty());
}

#[test]
fn uid_rapid_reregister_same_uid_different_paths() {
    let mut reg = UidRegistry::new();
    let uid = ResourceUid::new(999);

    for i in 0..100 {
        reg.register(uid, format!("res://path_{i}.tres"));
    }

    // Only the last registration should survive.
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.lookup_uid(uid), Some("res://path_99.tres"));

    // No stale path mappings.
    for i in 0..99 {
        assert_eq!(
            reg.lookup_path(&format!("res://path_{i}.tres")),
            None,
            "stale path {i} should be gone"
        );
    }
}

#[test]
fn uid_rapid_reregister_same_path_different_uids() {
    let mut reg = UidRegistry::new();

    for i in 0..100 {
        reg.register(ResourceUid::new(i), "res://shared.tres");
    }

    // Only the last UID should survive.
    assert_eq!(reg.len(), 1);
    assert_eq!(
        reg.lookup_path("res://shared.tres"),
        Some(ResourceUid::new(99))
    );

    // No stale UID mappings.
    for i in 0..99 {
        assert_eq!(
            reg.lookup_uid(ResourceUid::new(i)),
            None,
            "stale UID {i} should be gone"
        );
    }
}

// ===========================================================================
// 6. UnifiedLoader alternating path/UID loads → always same Arc
// ===========================================================================

#[test]
fn unified_alternating_path_uid_50_times() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://alternator", "res://alt.tres");

    let first = ul.load("res://alt.tres").unwrap();

    for i in 0..50 {
        let res = if i % 2 == 0 {
            ul.load("uid://alternator").unwrap()
        } else {
            ul.load("res://alt.tres").unwrap()
        };
        assert!(
            Arc::ptr_eq(&first, &res),
            "iteration {i}: alternating path/UID should return same Arc"
        );
    }

    assert_eq!(ul.cache_len(), 1);
}

#[test]
fn unified_multiple_uids_alternating() {
    let mut ul = UnifiedLoader::new(FakeLoader);
    ul.register_uid_str("uid://alpha", "res://a.tres");
    ul.register_uid_str("uid://beta", "res://b.tres");

    let a_ref = ul.load("res://a.tres").unwrap();
    let b_ref = ul.load("res://b.tres").unwrap();

    for i in 0..25 {
        let a = if i % 2 == 0 {
            ul.load("uid://alpha").unwrap()
        } else {
            ul.load("res://a.tres").unwrap()
        };
        let b = if i % 2 == 0 {
            ul.load("res://b.tres").unwrap()
        } else {
            ul.load("uid://beta").unwrap()
        };

        assert!(Arc::ptr_eq(&a_ref, &a));
        assert!(Arc::ptr_eq(&b_ref, &b));
        assert!(!Arc::ptr_eq(&a, &b));
    }
}

#[test]
fn unified_invalidate_mid_alternation() {
    let mut ul = UnifiedLoader::new(SequenceLoader::new());
    ul.register_uid_str("uid://volatile", "res://volatile.tres");

    let v1 = ul.load("uid://volatile").unwrap();
    assert_eq!(v1.get_property("seq"), Some(&Variant::Int(0)));

    // Alternating loads, all same Arc.
    for _ in 0..10 {
        let r = ul.load("res://volatile.tres").unwrap();
        assert!(Arc::ptr_eq(&v1, &r));
    }

    // Invalidate and reload.
    ul.invalidate("res://volatile.tres");
    let v2 = ul.load("uid://volatile").unwrap();
    assert_eq!(v2.get_property("seq"), Some(&Variant::Int(1)));
    assert!(!Arc::ptr_eq(&v1, &v2));

    // Post-invalidation alternating loads, all same new Arc.
    for _ in 0..10 {
        let r = ul.load("res://volatile.tres").unwrap();
        assert!(Arc::ptr_eq(&v2, &r));
    }
}
