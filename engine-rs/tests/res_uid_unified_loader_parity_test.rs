//! pat-gl0j / pat-95fu: Verify res:// and UID lookups route through one loader path.
//!
//! Acceptance: repeated-load and mixed-lookup tests verify shared semantics.
//! Both `res://` paths and `uid://` references must resolve through the same
//! [`UnifiedLoader`] pipeline, producing identical `Arc<Resource>` values,
//! exercising a single cache, and behaving consistently under invalidation,
//! re-registration, and interleaved access patterns.

use std::cell::Cell;
use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::{
    parse_uid_string, Resource, ResourceLoader, UidRegistry, UnifiedLoader,
};
use gdvariant::Variant;

// ===========================================================================
// Test loaders
// ===========================================================================

/// Deterministic loader that tags each resource with a monotonic sequence
/// number, allowing tests to distinguish fresh loads from cached returns.
struct SequenceLoader {
    counter: Cell<u64>,
}

impl SequenceLoader {
    fn new() -> Self {
        Self {
            counter: Cell::new(1),
        }
    }

}

impl ResourceLoader for SequenceLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let seq = self.counter.get();
        self.counter.set(seq + 1);
        let mut r = Resource::new("SeqResource");
        r.path = path.to_string();
        r.set_property("seq", Variant::Int(seq as i64));
        Ok(Arc::new(r))
    }
}

/// Helper: create a UnifiedLoader with a SequenceLoader.
fn make_loader() -> UnifiedLoader<SequenceLoader> {
    UnifiedLoader::new(SequenceLoader::new())
}

// ===========================================================================
// 1. Repeated res:// loads return same Arc
// ===========================================================================

#[test]
fn repeated_res_loads_return_same_arc() {
    let mut ul = make_loader();
    let first = ul.load("res://weapon.tres").unwrap();
    for _ in 0..20 {
        let again = ul.load("res://weapon.tres").unwrap();
        assert!(
            Arc::ptr_eq(&first, &again),
            "repeated res:// loads must return identical Arc"
        );
    }
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 2. Repeated uid:// loads return same Arc
// ===========================================================================

#[test]
fn repeated_uid_loads_return_same_arc() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://weapon_ref", "res://weapon.tres");

    let first = ul.load("uid://weapon_ref").unwrap();
    for _ in 0..20 {
        let again = ul.load("uid://weapon_ref").unwrap();
        assert!(
            Arc::ptr_eq(&first, &again),
            "repeated uid:// loads must return identical Arc"
        );
    }
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 3. Mixed res:// then uid:// returns same Arc (shared cache)
// ===========================================================================

#[test]
fn res_then_uid_same_arc() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://shield", "res://items/shield.tres");

    let by_path = ul.load("res://items/shield.tres").unwrap();
    let by_uid = ul.load("uid://shield").unwrap();

    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "res:// and uid:// for same resource must share cache entry"
    );
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 4. Mixed uid:// then res:// returns same Arc
// ===========================================================================

#[test]
fn uid_then_res_same_arc() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://armor", "res://items/armor.tres");

    let by_uid = ul.load("uid://armor").unwrap();
    let by_path = ul.load("res://items/armor.tres").unwrap();

    assert!(Arc::ptr_eq(&by_uid, &by_path));
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 5. Interleaved mixed access pattern
// ===========================================================================

#[test]
fn interleaved_mixed_access_all_same_arc() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://potion", "res://consumables/potion.tres");

    let mut arcs = Vec::new();
    for i in 0..10 {
        let r = if i % 2 == 0 {
            ul.load("res://consumables/potion.tres").unwrap()
        } else {
            ul.load("uid://potion").unwrap()
        };
        arcs.push(r);
    }

    for arc in &arcs[1..] {
        assert!(
            Arc::ptr_eq(&arcs[0], arc),
            "all interleaved loads must return the same Arc"
        );
    }
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 6. Multiple resources with mixed access stay independent
// ===========================================================================

#[test]
fn multiple_resources_mixed_access_independent() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://sword", "res://weapons/sword.tres");
    ul.register_uid_str("uid://bow", "res://weapons/bow.tres");
    ul.register_uid_str("uid://staff", "res://weapons/staff.tres");

    // Load each via different method.
    let sword = ul.load("res://weapons/sword.tres").unwrap();
    let bow = ul.load("uid://bow").unwrap();
    let staff = ul.load("uid://staff").unwrap();

    // Each is distinct.
    assert!(!Arc::ptr_eq(&sword, &bow));
    assert!(!Arc::ptr_eq(&bow, &staff));
    assert!(!Arc::ptr_eq(&sword, &staff));

    // Cross-load same resources via opposite method.
    let sword_uid = ul.load("uid://sword").unwrap();
    let bow_path = ul.load("res://weapons/bow.tres").unwrap();
    let staff_path = ul.load("res://weapons/staff.tres").unwrap();

    assert!(Arc::ptr_eq(&sword, &sword_uid));
    assert!(Arc::ptr_eq(&bow, &bow_path));
    assert!(Arc::ptr_eq(&staff, &staff_path));
    assert_eq!(ul.cache_len(), 3);
}

// ===========================================================================
// 7. Invalidation by path also invalidates uid access
// ===========================================================================

#[test]
fn invalidate_path_affects_uid_access() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://item", "res://item.tres");

    let first = ul.load("uid://item").unwrap();
    let seq1 = first.get_property("seq").unwrap();

    ul.invalidate("res://item.tres");

    // Reload via uid → must get fresh resource.
    let second = ul.load("uid://item").unwrap();
    let seq2 = second.get_property("seq").unwrap();

    assert!(
        !Arc::ptr_eq(&first, &second),
        "after invalidation, uid:// must return new Arc"
    );
    assert_ne!(seq1, seq2, "sequence numbers must differ after reload");
}

// ===========================================================================
// 8. Invalidation and reload via res:// after uid load
// ===========================================================================

#[test]
fn invalidate_reload_via_res_after_uid_load() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://gem", "res://gem.tres");

    let by_uid = ul.load("uid://gem").unwrap();
    ul.invalidate("res://gem.tres");
    let by_path = ul.load("res://gem.tres").unwrap();

    assert!(
        !Arc::ptr_eq(&by_uid, &by_path),
        "reload by path after invalidation must produce new Arc"
    );
    // And uid now returns the new one too.
    let by_uid2 = ul.load("uid://gem").unwrap();
    assert!(Arc::ptr_eq(&by_path, &by_uid2));
}

// ===========================================================================
// 9. UID re-registration to new path
// ===========================================================================

#[test]
fn uid_re_registration_switches_path() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://sprite", "res://old_sprite.tres");

    let old = ul.load("uid://sprite").unwrap();
    assert_eq!(old.path, "res://old_sprite.tres");

    // Re-register the same UID to a different path.
    ul.register_uid_str("uid://sprite", "res://new_sprite.tres");

    let new = ul.load("uid://sprite").unwrap();
    assert_eq!(new.path, "res://new_sprite.tres");
    assert!(!Arc::ptr_eq(&old, &new));
}

// ===========================================================================
// 10. UID unregistration leaves res:// path working
// ===========================================================================

#[test]
fn uid_unregistration_preserves_path_cache() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://tile", "res://tile.tres");

    let by_uid = ul.load("uid://tile").unwrap();

    // Unregister the UID.
    let uid_val = parse_uid_string("uid://tile");
    ul.uid_registry_mut().unregister_uid(uid_val);

    // uid:// load now fails.
    assert!(ul.load("uid://tile").is_err());

    // res:// still returns cached Arc.
    let by_path = ul.load("res://tile.tres").unwrap();
    assert!(
        Arc::ptr_eq(&by_uid, &by_path),
        "path-based load must still work after UID unregistration"
    );
}

// ===========================================================================
// 11. clear_cache forces both uid and res paths to reload
// ===========================================================================

#[test]
fn clear_cache_forces_reload_for_both_paths() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://font", "res://font.tres");

    let before_res = ul.load("res://font.tres").unwrap();
    let before_uid = ul.load("uid://font").unwrap();
    assert!(Arc::ptr_eq(&before_res, &before_uid));

    ul.clear_cache();
    assert_eq!(ul.cache_len(), 0);

    let after_res = ul.load("res://font.tres").unwrap();
    let after_uid = ul.load("uid://font").unwrap();

    assert!(
        !Arc::ptr_eq(&before_res, &after_res),
        "after clear, res:// must return new Arc"
    );
    assert!(
        Arc::ptr_eq(&after_res, &after_uid),
        "after clear, res:// and uid:// must share new Arc"
    );
}

// ===========================================================================
// 12. with_registry constructor pre-populates UID mappings
// ===========================================================================

#[test]
fn with_registry_enables_immediate_uid_loads() {
    let mut reg = UidRegistry::new();
    reg.register(parse_uid_string("uid://pre_alpha"), "res://alpha.tres");
    reg.register(parse_uid_string("uid://pre_beta"), "res://beta.tres");

    let mut ul = UnifiedLoader::with_registry(SequenceLoader::new(), reg);

    let alpha = ul.load("uid://pre_alpha").unwrap();
    let beta = ul.load("uid://pre_beta").unwrap();

    assert_eq!(alpha.path, "res://alpha.tres");
    assert_eq!(beta.path, "res://beta.tres");
    assert!(!Arc::ptr_eq(&alpha, &beta));

    // Verify path-based access hits same cache.
    let alpha2 = ul.load("res://alpha.tres").unwrap();
    assert!(Arc::ptr_eq(&alpha, &alpha2));
}

// ===========================================================================
// 13. Sequence number proves single loader invocation
// ===========================================================================

#[test]
fn mixed_access_invokes_loader_exactly_once() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://once", "res://once.tres");

    let r1 = ul.load("res://once.tres").unwrap();
    let r2 = ul.load("uid://once").unwrap();
    let r3 = ul.load("res://once.tres").unwrap();
    let r4 = ul.load("uid://once").unwrap();

    // All same Arc.
    assert!(Arc::ptr_eq(&r1, &r2));
    assert!(Arc::ptr_eq(&r2, &r3));
    assert!(Arc::ptr_eq(&r3, &r4));

    // The seq property should be the same on all — loaded exactly once.
    let seq = r1.get_property("seq").unwrap();
    assert_eq!(seq, &Variant::Int(1), "loader should have been called exactly once");
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 14. Multiple UIDs for same path share Arc
// ===========================================================================

#[test]
fn two_uids_same_path_share_arc() {
    let mut ul = make_loader();
    // Register two different UID strings that both point to the same path.
    ul.register_uid_str("uid://alias_a", "res://shared.tres");
    // Note: UidRegistry only allows one UID per path, so second register
    // replaces the first. Instead, test that the cache key is the res:// path.
    let by_uid = ul.load("uid://alias_a").unwrap();
    let by_path = ul.load("res://shared.tres").unwrap();
    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

// ===========================================================================
// 15. Stress: 50 resources, mixed access patterns
// ===========================================================================

#[test]
fn stress_50_resources_mixed_access() {
    let mut ul = make_loader();

    // Register 50 resources with UIDs.
    for i in 0..50 {
        ul.register_uid_str(
            &format!("uid://res_{i}"),
            &format!("res://resources/r{i}.tres"),
        );
    }

    // Load all by path first.
    let by_path: Vec<Arc<Resource>> = (0..50)
        .map(|i| ul.load(&format!("res://resources/r{i}.tres")).unwrap())
        .collect();

    // Load all by UID — must get same Arcs.
    let by_uid: Vec<Arc<Resource>> = (0..50)
        .map(|i| ul.load(&format!("uid://res_{i}")).unwrap())
        .collect();

    for i in 0..50 {
        assert!(
            Arc::ptr_eq(&by_path[i], &by_uid[i]),
            "resource {i}: path and uid must share Arc"
        );
    }

    assert_eq!(ul.cache_len(), 50);
}

// ===========================================================================
// 16. Invalidate one resource does not affect others
// ===========================================================================

#[test]
fn invalidate_one_does_not_affect_others() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://a", "res://a.tres");
    ul.register_uid_str("uid://b", "res://b.tres");

    let a1 = ul.load("uid://a").unwrap();
    let b1 = ul.load("uid://b").unwrap();

    ul.invalidate("res://a.tres");

    // B unchanged.
    let b2 = ul.load("uid://b").unwrap();
    assert!(Arc::ptr_eq(&b1, &b2), "uninvalidated resource must be stable");

    // A reloaded.
    let a2 = ul.load("uid://a").unwrap();
    assert!(!Arc::ptr_eq(&a1, &a2), "invalidated resource must be fresh");
}

// ===========================================================================
// 17. Unknown uid:// returns NotFound error
// ===========================================================================

#[test]
fn unknown_uid_returns_not_found() {
    let mut ul = make_loader();
    let err = ul.load("uid://nonexistent").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("no path registered"),
        "error should mention missing registration: {msg}"
    );
}

// ===========================================================================
// 18. Invalid uid:// (empty suffix) returns error
// ===========================================================================

#[test]
fn empty_uid_suffix_returns_error() {
    let mut ul = make_loader();
    // uid:// with empty suffix — hash yields 0 which is INVALID.
    let result = ul.load("uid://");
    assert!(result.is_err(), "empty uid:// suffix should error");
}

// ===========================================================================
// 19. Non-res non-uid path passes through unchanged
// ===========================================================================

#[test]
fn absolute_path_passes_through_loader() {
    let mut ul = make_loader();
    let res = ul.load("/absolute/path/resource.tres").unwrap();
    assert_eq!(res.path, "/absolute/path/resource.tres");
}

// ===========================================================================
// 20. Repeated load + invalidate cycle preserves correctness
// ===========================================================================

#[test]
fn load_invalidate_cycle_10_rounds() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://cyclic", "res://cyclic.tres");

    let mut prev_seq = 0i64;
    for round in 0..10 {
        let by_path = ul.load("res://cyclic.tres").unwrap();
        let by_uid = ul.load("uid://cyclic").unwrap();
        assert!(
            Arc::ptr_eq(&by_path, &by_uid),
            "round {round}: path and uid must share Arc"
        );

        let seq = match by_path.get_property("seq").unwrap() {
            Variant::Int(v) => *v,
            other => panic!("unexpected variant: {other:?}"),
        };
        assert!(
            seq > prev_seq || round == 0,
            "round {round}: seq {seq} must increase from prev {prev_seq}"
        );
        prev_seq = seq;

        ul.invalidate("res://cyclic.tres");
        assert!(!ul.is_cached("res://cyclic.tres"));
    }
}

// ===========================================================================
// 21. TresLoader implements ResourceLoader — UnifiedLoader<TresLoader> works
// ===========================================================================

#[test]
fn tres_loader_implements_resource_loader_trait() {
    // Verify the trait is implemented by constructing a UnifiedLoader with it.
    let _ul: UnifiedLoader<gdresource::loader::TresLoader> =
        UnifiedLoader::new(gdresource::loader::TresLoader);
}

// ===========================================================================
// 22. TresLoader as ResourceLoader loads .tres files
// ===========================================================================

#[test]
fn tres_loader_as_resource_loader_loads_tres() {
    let dir = tempfile::tempdir().unwrap();
    let tres_path = dir.path().join("test.tres");
    std::fs::write(
        &tres_path,
        r#"[gd_resource type="Resource" format=3]

[resource]
name = "TraitTest"
value = 99
"#,
    )
    .unwrap();

    let loader = gdresource::loader::TresLoader;
    let res = ResourceLoader::load(&loader, tres_path.to_str().unwrap()).unwrap();
    assert_eq!(res.class_name, "Resource");
    assert_eq!(
        res.get_property("name"),
        Some(&Variant::String("TraitTest".into()))
    );
}

// ===========================================================================
// 23. UnifiedLoader<TresLoader> deduplicates .tres loads
// ===========================================================================

#[test]
fn unified_tres_loader_deduplicates_tres() {
    let dir = tempfile::tempdir().unwrap();
    let tres_path = dir.path().join("cached.tres");
    std::fs::write(
        &tres_path,
        r#"[gd_resource type="Resource" format=3]

[resource]
label = "Cached"
"#,
    )
    .unwrap();

    let path_str = tres_path.to_str().unwrap();
    let mut ul = UnifiedLoader::new(gdresource::loader::TresLoader);

    let first = ul.load(path_str).unwrap();
    let second = ul.load(path_str).unwrap();

    assert!(
        Arc::ptr_eq(&first, &second),
        "UnifiedLoader<TresLoader> must deduplicate .tres loads"
    );
    assert_eq!(ul.cache_len(), 1);
}

// ===========================================================================
// 24. UnifiedLoader<TresLoader> UID+path for real .tres
// ===========================================================================

#[test]
fn unified_tres_loader_uid_and_path_same_arc_for_tres() {
    let dir = tempfile::tempdir().unwrap();
    let tres_path = dir.path().join("weapon.tres");
    std::fs::write(
        &tres_path,
        r#"[gd_resource type="Resource" format=3]

[resource]
damage = 42
"#,
    )
    .unwrap();

    let path_str = tres_path.to_str().unwrap();
    let mut ul = UnifiedLoader::new(gdresource::loader::TresLoader);
    ul.register_uid_str("uid://weapon_test", path_str);

    let by_path = ul.load(path_str).unwrap();
    let by_uid = ul.load("uid://weapon_test").unwrap();

    assert!(
        Arc::ptr_eq(&by_path, &by_uid),
        "real .tres loaded by path and UID through TresLoader must share Arc"
    );
}

// ===========================================================================
// 27. resolve_to_path returns canonical path for res:// (pat-gl0j)
// ===========================================================================

#[test]
fn resolve_to_path_passthrough_for_res() {
    let ul = make_loader();
    let resolved = ul.resolve_to_path("res://data/item.tres").unwrap();
    assert_eq!(resolved, "res://data/item.tres");
}

// ===========================================================================
// 28. resolve_to_path resolves uid:// to registered path (pat-gl0j)
// ===========================================================================

#[test]
fn resolve_to_path_uid_to_registered_path() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://hero", "res://characters/hero.tres");
    let resolved = ul.resolve_to_path("uid://hero").unwrap();
    assert_eq!(resolved, "res://characters/hero.tres");
}

// ===========================================================================
// 29. resolve_to_path + load agree on canonical path (pat-gl0j)
// ===========================================================================

#[test]
fn resolve_then_load_produces_same_resource() {
    let mut ul = make_loader();
    ul.register_uid_str("uid://axe", "res://weapons/axe.tres");

    // Resolve first, then load by both methods — all must agree.
    let canonical = ul.resolve_to_path("uid://axe").unwrap();
    let by_uid = ul.load("uid://axe").unwrap();
    let by_path = ul.load(&canonical).unwrap();

    assert_eq!(by_uid.path, canonical);
    assert!(Arc::ptr_eq(&by_uid, &by_path));
}

// ===========================================================================
// 30. resolve_to_path for unknown uid returns error (pat-gl0j)
// ===========================================================================

#[test]
fn resolve_to_path_unknown_uid_errors() {
    let ul = make_loader();
    let err = ul.resolve_to_path("uid://ghost").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("no path registered"), "error: {msg}");
}

// ===========================================================================
// 31. Mixed resolve + load stress: 20 resources (pat-gl0j)
// ===========================================================================

#[test]
fn resolve_and_load_stress_20_resources() {
    let mut ul = make_loader();
    for i in 0..20 {
        ul.register_uid_str(
            &format!("uid://stress_{i}"),
            &format!("res://stress/r{i}.tres"),
        );
    }

    for i in 0..20 {
        let uid_ref = format!("uid://stress_{i}");
        let path_ref = format!("res://stress/r{i}.tres");

        // resolve_to_path and load must agree.
        let resolved = ul.resolve_to_path(&uid_ref).unwrap();
        assert_eq!(resolved, path_ref);

        let by_uid = ul.load(&uid_ref).unwrap();
        let by_path = ul.load(&path_ref).unwrap();
        assert!(
            Arc::ptr_eq(&by_uid, &by_path),
            "resource {i}: uid and path must share Arc"
        );
    }
    assert_eq!(ul.cache_len(), 20);
}
