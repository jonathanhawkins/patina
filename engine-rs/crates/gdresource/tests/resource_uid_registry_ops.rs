//! pat-s3rrv.253 (engine half) — the process-global `ResourceUID` registry.
//!
//! Godot ground truth: `core/io/resource_uid.cpp` and `doc/classes/ResourceUID.xml`.
//!
//! # What was missing
//!
//! `UidRegistry` existed, but every instance was **owned** by something —
//! `UnifiedLoader` holds one by value — so there was no registry a script could
//! reach. Godot's `ResourceUID` is a process singleton, so these tests exercise
//! the global one.
//!
//! # 🔑 `add_id` and `set_id` are exact inverses
//!
//! ⚠️ `add_id` fails when the id EXISTS; `set_id` fails when it does NOT
//! (`ERR_FAIL_COND(unique_ids.has(p_id))` against
//! `ERR_FAIL_COND(!unique_ids.has(p_id))`). `UidRegistry::register` overwrites
//! unconditionally, so implementing both by delegating to it passes every
//! happy-path assertion while dropping both error cases. `b_*` is the guard.
//!
//! # ⚠️ Ids are namespaced per test
//!
//! The registry is process-global. `cargo nextest` runs each test in its own
//! process, but `cargo test` does not, so every test below uses its own id and
//! path range and none of them assumes an empty registry.

use gdcore::ResourceUid;
use gdresource::uid::{
    add_id, create_id, create_id_for_path, ensure_path, get_id_path, has_id, id_to_text,
    path_to_uid, remove_id, set_id, uid_to_path, UidError,
};

/// 🔑 **THE ACCEPTANCE CLAUSE.** add_id → get_id_path / has_id → remove_id.
#[test]
fn a_add_id_then_resolve_then_remove() {
    let id = ResourceUid::new(0x2530_0001);

    // ⚠️ Positive control for the assertion below: before registration the id is
    // well-formed but unknown.
    assert!(
        !has_id(id),
        "a fresh id must not be registered — if this fails, has_id is not \
         consulting the registry"
    );

    assert_eq!(add_id(id, "res://a.tres"), Ok(()));
    assert!(has_id(id));
    assert_eq!(get_id_path(id).as_deref(), Some("res://a.tres"));

    assert_eq!(remove_id(id), Ok(()));
    assert!(!has_id(id), "remove_id must clear the mapping");
    assert_eq!(get_id_path(id), None);
}

/// 🔑 `has_id` must ask the REGISTRY, not merely check the id is well-formed.
///
/// ⚠️ This is the regression assertion for the previous script binding, which
/// answered `id_to_text(id) != "uid://<invalid>"` — true for **every**
/// non-negative integer, so `has_id` could never return false for a real id.
#[test]
fn b_has_id_is_not_a_well_formedness_check() {
    let unregistered = ResourceUid::new(0x2530_0002);

    // The old implementation's own predicate still holds here…
    assert_ne!(
        id_to_text(unregistered.raw()),
        "uid://<invalid>",
        "the id IS well-formed — that is exactly why the old check passed"
    );
    // …but the id is not registered, so has_id must say so.
    assert!(
        !has_id(unregistered),
        "has_id returned true for an id that was never added"
    );
}

/// 🔑 The inverse guards. `add_id` rejects a duplicate; `set_id` rejects an unknown.
///
/// ⚠️ Each negative is paired with its positive control, so the test cannot pass
/// by having both operations simply fail.
#[test]
fn c_add_and_set_reject_opposite_conditions() {
    let id = ResourceUid::new(0x2530_0003);
    let absent = ResourceUid::new(0x2530_0004);

    // add_id: positive, then the duplicate rejection.
    assert_eq!(add_id(id, "res://first.tres"), Ok(()));
    assert_eq!(
        add_id(id, "res://second.tres"),
        Err(UidError::AlreadyExists),
        "add_id is not an upsert — Godot ERR_FAIL_COND(unique_ids.has(p_id))"
    );
    assert_eq!(
        get_id_path(id).as_deref(),
        Some("res://first.tres"),
        "the rejected add must NOT have overwritten the mapping"
    );

    // set_id: the inverse — positive on the existing id, rejection on the absent.
    assert_eq!(set_id(id, "res://moved.tres"), Ok(()));
    assert_eq!(
        get_id_path(id).as_deref(),
        Some("res://moved.tres"),
        "set_id repoints an existing mapping"
    );
    assert_eq!(
        set_id(absent, "res://nowhere.tres"),
        Err(UidError::NotFound),
        "set_id on an unregistered id must fail — the exact inverse of add_id"
    );
    assert!(
        !has_id(absent),
        "the failed set_id must not have created it"
    );

    // remove_id shares set_id's precondition.
    assert_eq!(remove_id(absent), Err(UidError::NotFound));
    assert_eq!(remove_id(id), Ok(()));
}

/// `path_to_uid` / `uid_to_path` are inverse for a registered pair.
///
/// ⚠️ …and they are NOT symmetric when the lookup misses. Godot's `path_to_uid`
/// echoes its input (`if (id == INVALID_ID) return p_path;`) while `uid_to_path`
/// yields an empty string, because `get_id_path` fails to `String()`. Asserting
/// only the round trip would let a "both return empty" implementation pass.
#[test]
fn d_path_and_uid_round_trip_but_miss_differently() {
    let id = ResourceUid::new(0x2530_0005);
    let path = "res://round/trip.tres";
    assert_eq!(add_id(id, path), Ok(()));

    let uid_text = path_to_uid(path);
    assert_eq!(uid_text, id_to_text(id.raw()));
    assert_eq!(
        uid_to_path(&uid_text),
        path,
        "inverse for a registered pair"
    );

    // The asymmetric miss behaviour.
    assert_eq!(
        path_to_uid("res://never/registered.tres"),
        "res://never/registered.tres",
        "path_to_uid echoes an unregistered path rather than failing"
    );
    assert_eq!(
        uid_to_path("uid://abcdefgh"),
        "",
        "…while uid_to_path yields empty for an unknown uid"
    );

    assert_eq!(remove_id(id), Ok(()));
}

/// `create_id` yields a distinct, valid, non-negative id each call.
///
/// ⚠️ Godot masks the high bit so a UID can never be negative and so can never
/// collide with the `-1` invalid sentinel.
#[test]
fn e_create_id_is_distinct_and_never_negative() {
    let ids: Vec<ResourceUid> = (0..64).map(|_| create_id()).collect();

    for id in &ids {
        assert!(
            id.is_valid(),
            "create_id must not return the invalid sentinel"
        );
        assert!(
            id.raw() >= 0,
            "the high bit must be masked; got {}",
            id.raw()
        );
        assert_ne!(id_to_text(id.raw()), "uid://<invalid>");
    }

    let mut raws: Vec<i64> = ids.iter().map(|i| i.raw()).collect();
    raws.sort_unstable();
    let before = raws.len();
    raws.dedup();
    assert_eq!(before, raws.len(), "create_id returned a duplicate");
}

/// `create_id_for_path` is deterministic, and case-insensitive as Godot is.
///
/// ⚠️ Godot lowercases the path before hashing so that a case-insensitive
/// filesystem cannot shift ids. It also means two paths differing only in case
/// deliberately collide.
#[test]
fn f_create_id_for_path_is_deterministic() {
    let a = create_id_for_path("res://Assets/Player.tscn");
    let b = create_id_for_path("res://Assets/Player.tscn");
    assert_eq!(a.raw(), b.raw(), "same path must yield the same id");

    let lowered = create_id_for_path("res://assets/player.tscn");
    assert_eq!(
        a.raw(),
        lowered.raw(),
        "the path is lowercased first, so case must not shift the id"
    );

    let other = create_id_for_path("res://assets/enemy.tscn");
    assert_ne!(
        a.raw(),
        other.raw(),
        "different paths must not collapse to one id"
    );

    assert!(a.is_valid() && a.raw() >= 0);
}

/// `ensure_path` resolves a uid and passes anything else through untouched.
#[test]
fn g_ensure_path_only_touches_uid_strings() {
    let id = ResourceUid::new(0x2530_0006);
    let path = "res://ensure/target.tres";
    assert_eq!(add_id(id, path), Ok(()));

    assert_eq!(ensure_path(&id_to_text(id.raw())), path);

    // ⚠️ A plain path is returned verbatim — NOT looked up, NOT emptied.
    assert_eq!(
        ensure_path("res://plain/file.tres"),
        "res://plain/file.tres"
    );
    assert_eq!(ensure_path(""), "");

    assert_eq!(remove_id(id), Ok(()));
}

/// An invalid id is rejected by the mutating operations.
#[test]
fn h_invalid_ids_are_rejected() {
    assert_eq!(
        add_id(ResourceUid::INVALID, "res://x.tres"),
        Err(UidError::Invalid)
    );
    assert_eq!(
        set_id(ResourceUid::INVALID, "res://x.tres"),
        Err(UidError::Invalid)
    );
    assert!(!has_id(ResourceUid::INVALID));
    assert_eq!(get_id_path(ResourceUid::INVALID), None);
}
