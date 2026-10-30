//! pat-s3rrv.225 — `EditorFeatureProfile` exists, persists as a `.profile`
//! file, and actually hides what it disables.
//!
//! The menu entry was dead: `editor_menu.rs` mapped
//! `MenuAction::EditorFeatureProfile` to the bare string
//! `OpenDialog("Manage Feature Profiles")` and nothing consumed it. There was
//! no profile resource, no save/load, no ClassDB entry and no filtering.
//!
//! ⚠️ **The bead under-counts the Feature enum.** It lists nine features;
//! 4.6.1 has **eleven** — `SIGNALS_DOCK` and `GROUPS_DOCK` are missing from the
//! bead, and `NODE_DOCK` is deprecated upstream but still holds its ordinal.
//! Dropping the deprecated one would renumber every constant after it, so a
//! script's `FEATURE_GAME` would silently select a different feature. These
//! tests pin all eleven and their ordinals.

use gdeditor::create_dialog::NodeCatalog2D;
use gdeditor::feature_profile::{
    active_profile, is_class_disabled, is_feature_disabled, set_active_profile, EditorFeature,
    EditorFeatureProfile,
};
use gdobject::class_db::{class_exists, class_has_method, is_parent_class, register_all_classes};

/// Serializes every test that touches the process-global active profile.
///
/// The old shape here was a bare `set_active_profile(None)` at the top and
/// bottom of each test, under a comment claiming each test "owns" the global.
/// It does not: under the default parallel harness one test's clear drops
/// another's profile mid-assertion while its own profile filters theirs, which
/// is why a DIFFERENT subset of these failed on every run and all of them
/// passed under `--test-threads=1` (pat-21oab.3). Setting the global again on
/// entry is a *rearm*, not isolation — the writes interleave and no assertion
/// in the test can see that it happened.
static PROFILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds [`PROFILE_LOCK`] for the rest of the test and clears the active
/// profile on drop — including on an unwind, so a failing test cannot leave its
/// profile installed for whichever test runs next.
struct ProfileGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        set_active_profile(None);
    }
}

/// Takes the lock and clears the active profile. Bind it with
/// `let _guard = profile_guard();` — binding to plain `_` would drop the guard
/// immediately and release the lock before the test body ran.
///
/// Poison-tolerant, matching `gdcore::file_access`'s `ROOTS_LOCK`: a panicking
/// test must fail on its own rather than poison the mutex and cascade into
/// every other test in the binary.
fn profile_guard() -> ProfileGuard {
    let lock = PROFILE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    set_active_profile(None);
    ProfileGuard(lock)
}

// ---------------------------------------------------------------------------
// ClassDB surface
// ---------------------------------------------------------------------------

#[test]
fn the_class_is_registered_with_its_eleven_methods() {
    register_all_classes();
    assert!(
        class_exists("EditorFeatureProfile"),
        "the class must exist — `class_exists` was false before this bead"
    );
    assert!(
        is_parent_class("EditorFeatureProfile", "RefCounted"),
        "Godot derives it from RefCounted"
    );
    for m in [
        "set_disabled_class",
        "is_disabled_class",
        "set_disabled_class_editor",
        "is_disabled_class_editor",
        "set_disabled_property",
        "is_disabled_property",
        "set_disabled_feature",
        "is_disabled_feature",
        "get_feature_name",
        "save_to_file",
        "load_from_file",
    ] {
        assert!(
            class_has_method("EditorFeatureProfile", m),
            "EditorFeatureProfile must bind {m}"
        );
    }
}

#[test]
fn the_feature_ordinals_match_godot() {
    // ⚠️ Ordinals are the wire format a script passes to set_disabled_feature.
    for (i, f) in EditorFeature::ALL.iter().enumerate() {
        assert_eq!(f.as_godot(), i as i64, "{f:?} must keep ordinal {i}");
        assert_eq!(EditorFeature::from_godot(i as i64), Some(*f));
    }
    assert_eq!(
        EditorFeature::ALL.len(),
        11,
        "4.6.1 defines eleven features"
    );
    assert_eq!(
        EditorFeature::from_godot(11),
        None,
        "FEATURE_MAX is not a feature"
    );
    assert_eq!(EditorFeature::from_godot(-1), None);
    // Names are user-facing labels, not derivable from the enum name.
    assert_eq!(EditorFeature::ThreeD.name(), "3D Editor");
    assert_eq!(EditorFeature::Game.name(), "Game View");
}

// ---------------------------------------------------------------------------
// The profile itself
// ---------------------------------------------------------------------------

#[test]
fn disabling_and_re_enabling_round_trips() {
    let mut p = EditorFeatureProfile::new();
    assert!(!p.is_disabled_class("Sprite2D"));
    p.set_disabled_class("Sprite2D", true);
    assert!(p.is_disabled_class("Sprite2D"));
    p.set_disabled_class("Sprite2D", false);
    assert!(
        !p.is_disabled_class("Sprite2D"),
        "setting false must remove it, not leave it disabled"
    );

    p.set_disabled_feature(EditorFeature::ThreeD, true);
    assert!(p.is_disabled_feature(EditorFeature::ThreeD));
    assert!(
        !p.is_disabled_feature(EditorFeature::Script),
        "disabling one feature must not disable its neighbours"
    );

    p.set_disabled_property("Sprite2D:texture", true);
    assert!(p.is_disabled_property("Sprite2D:texture"));
    p.set_disabled_class_editor("Sprite2D", true);
    assert!(p.is_disabled_class_editor("Sprite2D"));
    assert!(
        !p.is_disabled_class("Node2D"),
        "the four sets are independent — a disabled editor is not a disabled class"
    );
}

#[test]
fn a_profile_survives_save_and_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mobile.profile");

    let mut p = EditorFeatureProfile::new();
    p.set_disabled_class("Sprite2D", true);
    p.set_disabled_feature(EditorFeature::ThreeD, true);
    p.save_to_file(&path).expect("profile saves");

    let loaded = EditorFeatureProfile::load_from_file(&path).expect("profile loads");
    assert_eq!(loaded, p, "the round trip must be lossless");
    assert!(loaded.is_disabled_class("Sprite2D"));
    assert!(loaded.is_disabled_feature(EditorFeature::ThreeD));
}

#[test]
fn a_file_that_is_not_a_profile_is_refused() {
    // ⚠️ Without the `type` check any JSON object would load as an *empty*
    // profile — disabling nothing, which looks identical to a working load.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("not.profile");
    std::fs::write(&path, r#"{"type":"editor_settings","foo":1}"#).unwrap();
    assert!(
        EditorFeatureProfile::load_from_file(&path).is_err(),
        "a file with the wrong type tag must be refused, not silently accepted"
    );
}

// ---------------------------------------------------------------------------
// The profile is actually applied
// ---------------------------------------------------------------------------

#[test]
fn a_disabled_class_leaves_the_create_node_list() {
    let _guard = profile_guard();
    let catalog = NodeCatalog2D::new();
    let target = catalog
        .entries()
        .first()
        .expect("the catalog is not empty")
        .class_name
        .clone();

    let before = catalog.visible_entries().len();
    assert!(
        catalog
            .visible_entries()
            .iter()
            .any(|e| e.class_name == target),
        "precondition: {target} is offered before any profile is selected"
    );

    let mut p = EditorFeatureProfile::new();
    p.set_disabled_class(&target, true);
    set_active_profile(Some(p));

    assert!(
        !catalog
            .visible_entries()
            .iter()
            .any(|e| e.class_name == target),
        "{target} must disappear from Create Node once the profile disables it"
    );
    assert_eq!(
        catalog.visible_entries().len(),
        before - 1,
        "exactly one entry may vanish — a filter that empties the list would \
         also satisfy the assertion above"
    );
    // ⓘ The unfiltered accessor still lists it: the Manage-Profiles dialog has
    // to show every class in order to offer it for disabling.
    assert!(
        catalog.entries().iter().any(|e| e.class_name == target),
        "`entries()` stays unfiltered on purpose"
    );
}

#[test]
fn a_disabled_feature_leaves_the_enabled_set() {
    let _guard = profile_guard();
    let mut p = EditorFeatureProfile::new();
    p.set_disabled_feature(EditorFeature::ThreeD, true);
    set_active_profile(Some(p));

    assert!(is_feature_disabled(EditorFeature::ThreeD));
    let enabled = active_profile()
        .expect("a profile is active")
        .enabled_features();
    assert!(
        !enabled.contains(&EditorFeature::ThreeD),
        "the 3D screen must not be offered by the served editor"
    );
    assert_eq!(
        enabled.len(),
        10,
        "the other ten features stay available — disabling one must not hide all"
    );
}

#[test]
fn with_no_profile_selected_nothing_is_hidden() {
    // The control that keeps every pre-existing editor path unchanged.
    let _guard = profile_guard();
    assert!(!is_class_disabled("Sprite2D"));
    for f in EditorFeature::ALL {
        assert!(!is_feature_disabled(f), "{f:?} must be enabled by default");
    }
    let catalog = NodeCatalog2D::new();
    assert_eq!(
        catalog.visible_entries().len(),
        catalog.entries().len(),
        "with no profile the visible catalog is the whole catalog"
    );
}

#[test]
fn a_saved_profile_still_filters_after_a_reload() {
    // The persistence half of the acceptance: save, drop the in-memory profile,
    // load it back, and the same class is still hidden.
    let _guard = profile_guard();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restart.profile");
    let catalog = NodeCatalog2D::new();
    let target = catalog.entries()[0].class_name.clone();

    let mut p = EditorFeatureProfile::new();
    p.set_disabled_class(&target, true);
    p.save_to_file(&path).expect("saves");

    set_active_profile(None);
    assert!(
        catalog
            .visible_entries()
            .iter()
            .any(|e| e.class_name == target),
        "precondition: clearing the profile restores the entry"
    );

    set_active_profile(Some(
        EditorFeatureProfile::load_from_file(&path).expect("loads"),
    ));
    assert!(
        !catalog
            .visible_entries()
            .iter()
            .any(|e| e.class_name == target),
        "the setting must survive a reload via the .profile file"
    );
}
