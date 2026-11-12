//! pat-nzgdu.187 — `AudioSample` / `AudioSamplePlayback` are ClassDB-registered.
//!
//! Covers the acceptance's ClassDB clause. The behavioural half — trigger a
//! registered sample, handle reports playing — is gdaudio's
//! `sample_pool_playback_handle`. See the bead notes.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json` and its
//! vendored C++ (`upstream/godot/servers/audio/audio_stream.h`).

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, get_method_list, get_property_list, instantiate, is_parent_class,
    register_audio_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;

/// Serializes every test in this file against the process-global ClassDB.
///
/// `register_audio_classes` is a check-then-act sequence: `class_exists(..)`
/// and `register_class(..)` take `global_db()`'s mutex SEPARATELY, so two
/// threads can both see a class as absent and both go on to register it.
/// `cargo test` runs these five as threads in one process, which is the only
/// harness where that can happen — nextest gives each test its own process and
/// therefore cannot show it at all (pat-21oab.3).
///
/// ⚠️ Bound once per test rather than taken inside a helper, because
/// `registration_is_idempotent` deliberately calls the registrar twice and
/// `std`'s `Mutex` is not reentrant — locking per call would deadlock.
///
/// Poison-tolerant, matching `gdcore::file_access`'s `ROOTS_LOCK`: a panicking
/// test must fail on its own rather than poison the mutex and cascade into the
/// other four.
static CLASS_DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn class_db_guard() -> std::sync::MutexGuard<'static, ()> {
    CLASS_DB_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

#[test]
fn audiosample_classes_are_registered_as_refcounted() {
    let _guard = class_db_guard();
    register_audio_classes();

    for class in ["AudioSample", "AudioSamplePlayback"] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} is a RefCounted handle, not a Resource — it is never saved"
        );
        assert!(
            !is_parent_class(class, "Resource"),
            "{class} must NOT be a Resource"
        );
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object"
        );

        // Both are GDREGISTER_CLASS'd in Godot (register_server_types.cpp:171-172),
        // so they instantiate.
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ Both classes expose **nothing** — no properties, no methods, no enums —
/// and that is exact, not laziness.
///
/// Godot's `AudioSample`/`AudioSamplePlayback` are plain data structs with
/// `GDCLASS(..., RefCounted)` and **no `_bind_methods()`**
/// (`servers/audio/audio_stream.h:43-74`). Their fields (`stream`, `loop_mode`,
/// `pitch_scale`, `bus`, …) are public C++ members, NOT bound properties. The
/// bead describes AudioSample as "wraps an AudioStream + LoopMode" — true of the
/// struct, invisible to ClassDB. Registering those as properties would invent a
/// schema Godot does not have, and a `.tscn`/script written against it would not
/// work in Godot.
#[test]
fn both_classes_are_opaque_handles_with_no_bound_members() {
    let _guard = class_db_guard();
    register_audio_classes();

    let base = props("RefCounted");
    for class in ["AudioSample", "AudioSamplePlayback"] {
        let own: Vec<String> = props(class)
            .into_iter()
            .filter(|p| !base.contains(p))
            .collect();
        assert!(
            own.is_empty(),
            "{class} binds no properties in Godot; found {own:?}"
        );

        let base_methods: Vec<String> = get_method_list("RefCounted")
            .into_iter()
            .map(|m| m.name)
            .collect();
        let own_methods: Vec<String> = get_method_list(class)
            .into_iter()
            .map(|m| m.name)
            .filter(|m| !base_methods.contains(m))
            .collect();
        assert!(
            own_methods.is_empty(),
            "{class} has no _bind_methods in Godot; found {own_methods:?}"
        );

        // Constants/enums/signals too — checked GENERALLY rather than by the
        // names I happen to expect, so a wrongly-named registration cannot slip
        // through.
        let base_consts = gdobject::get_integer_constant_list("RefCounted");
        let own_consts: Vec<String> = gdobject::get_integer_constant_list(class)
            .into_iter()
            .filter(|c| !base_consts.contains(c))
            .collect();
        assert!(
            own_consts.is_empty(),
            "{class} binds no constants in Godot; found {own_consts:?}"
        );

        let base_enums = gdobject::get_enum_list("RefCounted");
        let own_enums: Vec<String> = gdobject::get_enum_list(class)
            .into_iter()
            .filter(|e| !base_enums.contains(e))
            .collect();
        assert!(
            own_enums.is_empty(),
            "{class} binds no enums in Godot; found {own_enums:?}"
        );
    }
}

/// ⚠️ `AudioSample::LoopMode` is a **C++-only** enum: no `BIND_ENUM_CONSTANT`,
/// no `VARIANT_ENUM_CAST`, so it is absent from `extension_api.json` and
/// invisible to GDScript. Registering it would be inventing a ClassDB enum.
#[test]
fn loop_mode_is_not_a_classdb_enum() {
    let _guard = class_db_guard();
    register_audio_classes();
    for name in [
        "LOOP_DISABLED",
        "LOOP_FORWARD",
        "LOOP_PINGPONG",
        "LOOP_BACKWARD",
    ] {
        assert_eq!(
            gdobject::class_get_integer_constant("AudioSample", name),
            None,
            "{name} is a C++ enum value in Godot, not a bound ClassDB constant"
        );
    }
    assert!(!gdobject::class_has_enum("AudioSample", "LoopMode"));
}

/// The sample path's neighbour: `AudioStreamPlayback` is the *stream* path.
/// These are two different ways to make sound, and confusing them is the trap
/// this bead invites.
///
/// NOTE what is deliberately NOT asserted here: Patina marks `AudioStreamPlayback`
/// `.abstract_class()` (`class_db.rs:3594`, from pat-nzgdu.153), but Godot
/// registers it with `GDREGISTER_CLASS` (`register_server_types.cpp:167`), NOT
/// the abstract macro — so it is **instantiable** in Godot. Asserting Patina's
/// current flag would use a parity test to lock in a parity bug. Follow-up bead;
/// see the notes.
#[test]
fn sample_handles_are_distinct_from_the_stream_playback_path() {
    let _guard = class_db_guard();
    register_audio_classes();

    assert!(
        !is_parent_class("AudioSamplePlayback", "AudioStreamPlayback"),
        "AudioSamplePlayback is NOT an AudioStreamPlayback — it is a sibling \
         under RefCounted"
    );
    assert!(!is_parent_class("AudioSample", "AudioStreamPlayback"));
    // The stream path really does have a bound surface; the sample path has
    // none. That contrast is the whole point.
    assert!(
        !get_method_list("AudioStreamPlayback").is_empty(),
        "the stream path binds methods"
    );
}

/// ⚠️ Re-registering must not duplicate or drop these classes.
///
/// Comparing property lists would be VACUOUS here: both are empty by design, and
/// `get_property_list` also returns empty for an *unregistered* class — so
/// `assert_eq!([], [])` would pass even if the second call had deleted them.
#[test]
fn registration_is_idempotent() {
    let _guard = class_db_guard();
    register_audio_classes();
    let count = gdobject::class_count();

    register_audio_classes();
    assert_eq!(
        gdobject::class_count(),
        count,
        "re-registering must not add classes"
    );
    for class in ["AudioSample", "AudioSamplePlayback"] {
        assert!(class_exists(class), "{class} survives re-registration");
        assert!(is_parent_class(class, "RefCounted"));
        assert_eq!(
            get_method_list(class).len(),
            get_method_list("RefCounted").len(),
            "{class} gains no methods on re-registration"
        );
    }
}
