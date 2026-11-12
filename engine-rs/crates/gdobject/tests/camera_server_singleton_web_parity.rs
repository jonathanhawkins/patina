//! pat-nzgdu.179 — `CameraServer` / `CameraFeed` / `CameraTexture` class surface.
//!
//! **Web parity is the point.** Godot's `modules/camera` ships drivers for
//! android/linux/macos/windows and **nothing for web**, so on a web export the
//! singleton exists and reports zero feeds rather than erroring. A
//! `get_feed_count()` of 0 is therefore *correct behaviour*, not a stub — which
//! is why this bead's minimum acceptance is the no-op singleton + class surface.
//!
//! The interpreter already dispatches `CameraServer` as a singleton and mints
//! `CameraFeed` handles (tag 0xCA); this is the ClassDB half that was missing.
//! Schemas diffed against `extension_api.json` from Godot 4.6.1.stable.

use gdobject::class_db::is_abstract;
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, class_has_signal, get_property_list,
    instantiate, is_parent_class, register_viewport_classes,
};
// `GenericObject`'s only inherent method is `new()`; `get_class`/`get_property`
// et al. come from the `GodotObject` trait, which must be in scope to call them.
use gdobject::GodotObject;
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

#[test]
fn camera_server_family_is_registered_with_godot_parents() {
    register_viewport_classes();

    assert!(class_exists("CameraServer"));
    assert!(
        is_parent_class("CameraServer", "Object"),
        "CameraServer is a plain Object singleton — not a Node, not a Resource"
    );

    assert!(class_exists("CameraFeed"));
    assert!(
        is_parent_class("CameraFeed", "RefCounted"),
        "a feed is a live device handle (RefCounted), not a Resource"
    );
    assert!(
        !is_parent_class("CameraFeed", "Resource"),
        "CameraFeed is not a Resource"
    );

    assert!(class_exists("CameraTexture"));
    assert!(
        is_parent_class("CameraTexture", "Texture2D"),
        "CameraTexture is a Texture2D — that is what lets a TextureRect bind it"
    );
    assert!(is_parent_class("CameraTexture", "Object"));
}

/// The acceptance's "CameraTexture instantiates as a Texture2D usable on a
/// TextureRect" — at the ClassDB level.
#[test]
fn camera_texture_instantiates_as_a_texture2d() {
    register_viewport_classes();
    assert!(!is_abstract("CameraTexture"));
    let obj = instantiate("CameraTexture").expect("CameraTexture instantiates");
    // `get_class` is the GodotObject trait method — there is no `class_name()`
    // on GenericObject (that one lives on the private `ObjectBase`).
    assert_eq!(obj.get_class(), "CameraTexture");
    // It carries its own feed binding...
    assert_props(
        "CameraTexture",
        &["camera_feed_id", "which_feed", "camera_is_active"],
    );
    // ...and is a Texture2D, which is the property that makes it bindable.
    assert!(is_parent_class("CameraTexture", "Texture2D"));
}

/// ⚠️ `CameraServer.FeedImage` really does have duplicate values — this is
/// Godot's enum, not a transcription error. The index is *within* a feed's
/// datatype: the first plane of RGBA/YCbCr/Y all share 0, and only the second
/// CbCr plane is 1. Recollection would "fix" this into 0/1/2/3 and be wrong.
#[test]
fn feed_image_enum_has_godots_duplicate_plane_indices() {
    register_viewport_classes();
    assert!(class_has_enum("CameraServer", "FeedImage"));
    assert_eq!(
        class_get_integer_constant("CameraServer", "FEED_RGBA_IMAGE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("CameraServer", "FEED_YCBCR_IMAGE"),
        Some(0),
        "shares 0 with FEED_RGBA_IMAGE — both are a first plane"
    );
    assert_eq!(
        class_get_integer_constant("CameraServer", "FEED_Y_IMAGE"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("CameraServer", "FEED_CBCR_IMAGE"),
        Some(1),
        "the only second plane"
    );
}

#[test]
fn camera_feed_carries_its_datatype_and_position_enums() {
    register_viewport_classes();
    assert!(class_has_enum("CameraFeed", "FeedDataType"));
    assert!(class_has_enum("CameraFeed", "FeedPosition"));
    for (name, value) in [
        ("FEED_NOIMAGE", 0),
        ("FEED_RGB", 1),
        ("FEED_YCBCR", 2),
        ("FEED_YCBCR_SEP", 3),
        ("FEED_EXTERNAL", 4),
    ] {
        assert_eq!(class_get_integer_constant("CameraFeed", name), Some(value));
    }
    for (name, value) in [("FEED_UNSPECIFIED", 0), ("FEED_FRONT", 1), ("FEED_BACK", 2)] {
        assert_eq!(class_get_integer_constant("CameraFeed", name), Some(value));
    }
}

/// Godot exposes a feed's id/name/position as *methods*, not properties — only
/// these three are the property surface. Registering `id`/`name` as properties
/// would be inventing a schema Godot doesn't have.
#[test]
fn camera_feed_property_surface_is_only_the_three_godot_has() {
    register_viewport_classes();
    assert_props(
        "CameraFeed",
        &["feed_is_active", "feed_transform", "formats"],
    );
    for not_a_prop in ["id", "name", "position"] {
        assert!(
            !props("CameraFeed").iter().any(|p| p == not_a_prop),
            "`{not_a_prop}` is a method on CameraFeed in Godot, not a property"
        );
    }
    // The transform defaults to identity, so an untouched feed maps 1:1.
    let t = get_property_list("CameraFeed")
        .into_iter()
        .find(|p| p.name == "feed_transform")
        .expect("feed_transform exists");
    assert!(
        matches!(t.default_value, Variant::Transform2D(_)),
        "feed_transform is a Transform2D, got {:?}",
        t.default_value
    );
}

#[test]
fn camera_server_exposes_its_feed_signals() {
    register_viewport_classes();
    for sig in [
        "camera_feed_added",
        "camera_feed_removed",
        "camera_feeds_updated",
    ] {
        assert!(
            class_has_signal("CameraServer", sig),
            "CameraServer must declare {sig}"
        );
    }
    assert_props("CameraServer", &["monitoring_feeds"]);
    // Monitoring is off until a script asks for it — matching Godot, and on web
    // it stays 0-feed regardless.
    let m = get_property_list("CameraServer")
        .into_iter()
        .find(|p| p.name == "monitoring_feeds")
        .unwrap();
    assert_eq!(m.default_value, Variant::Bool(false));
}

#[test]
fn camera_feed_declares_its_frame_signals() {
    register_viewport_classes();
    assert!(class_has_signal("CameraFeed", "frame_changed"));
    assert!(class_has_signal("CameraFeed", "format_changed"));
}

#[test]
fn registration_is_idempotent() {
    register_viewport_classes();
    let before = props("CameraFeed");
    register_viewport_classes();
    assert_eq!(before, props("CameraFeed"));
}
