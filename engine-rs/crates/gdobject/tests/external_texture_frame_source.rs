//! pat-nzgdu.209 — the `ExternalTexture` ClassDB surface.
//!
//! Covers the acceptance's checkable clause (`class_exists('ExternalTexture')`
//! and it inherits `Texture2D`).
//!
//! **The bead's other clause — "displays frames from a bound video/camera
//! source" — describes something Godot's ExternalTexture does not do, so it is
//! deliberately not implemented.** Per its own header, an external texture is
//! one "as defined by `OES_EGL_image_external` (GLES) or
//! `VK_ANDROID_external_memory_android_hardware_buffer` (Vulkan)"
//! (scene/resources/external_texture.h): it takes an **opaque uint64 platform
//! buffer handle**. Godot's implementation contains no reference to video,
//! camera, or `getUserMedia` — those are `CameraFeed`/`CameraServer` and
//! `VideoStreamPlayer`. Building a `<video>` frame pump onto this class would
//! add behaviour Godot lacks, which is a parity regression, not a feature. See
//! the bead notes.
//!
//! The renderer-independent half (size guards, buffer-id handling, size
//! reporting) is ported in `gdrender2d::external_texture`.
//!
//! Schema diffed against Godot 4.6.1.stable's `extension_api.json`; defaults
//! against its doc XML and `external_texture.h`'s member initialisers.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_viewport_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()` matters: `get_property_list` returns base-first, so a plain
/// `.find()` would return the most-BASE match on a shadowed name, while
/// `instantiate` lets the most-DERIVED default win. Searching in reverse agrees
/// with what an instance actually gets.
fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// Methods `class` declares that `base` does not.
fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's clause: registered, and inherits `Texture2D`.
#[test]
fn external_texture_frame_source() {
    register_viewport_classes();

    assert!(class_exists("ExternalTexture"));
    assert!(
        is_parent_class("ExternalTexture", "Texture2D"),
        "the acceptance's chain: ExternalTexture <- Texture2D"
    );
    // An unregistered intermediate severs the chain silently (pat-nzgdu.98), so
    // walk the whole way up rather than checking only the immediate parent.
    assert!(
        is_parent_class("ExternalTexture", "Texture"),
        "ExternalTexture must reach Texture through Texture2D"
    );
    assert!(is_parent_class("ExternalTexture", "Resource"));
    assert!(is_parent_class("ExternalTexture", "Object"));

    // `GDREGISTER_CLASS(ExternalTexture)` (register_scene_types.cpp:1056) — a
    // script constructs one and hands it a platform buffer handle.
    assert!(!is_abstract("ExternalTexture"));
    assert!(can_instantiate("ExternalTexture"));
    let obj = instantiate("ExternalTexture").expect("ExternalTexture instantiates");
    assert_eq!(obj.get_class(), "ExternalTexture");
}

/// ⚠️ `size` defaults to **Vector2(256, 256)**, not zero.
///
/// `Size2 size = Size2(256, 256)` is a member initialiser in
/// `external_texture.h`; the constructor body is EMPTY. Reading only
/// `ExternalTexture::ExternalTexture() {}` suggests a zero default, and a zero
/// default would be self-concealing: `set_size` rejects non-positive sizes, so a
/// texture that started at 0x0 would report 0x0 forever unless explicitly sized.
#[test]
fn size_defaults_to_256_square_not_zero() {
    register_viewport_classes();

    match default_of("ExternalTexture", "size") {
        Variant::Vector2(v) => assert!(
            (v.x - 256.0).abs() < 1e-6 && (v.y - 256.0).abs() < 1e-6,
            "size defaults to Vector2(256, 256), got {v:?}"
        ),
        other => panic!("size must be a Vector2, got {other:?}"),
    }
    assert_ne!(
        default_of("ExternalTexture", "size"),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "a zero default would be wrong AND self-concealing (set_size rejects 0)"
    );

    // The doc XML lists exactly one own member; `resource_local_to_scene` is an
    // `overrides="Resource"` entry, not ExternalTexture's own.
    let base = props("Texture2D");
    let own: Vec<String> = props("ExternalTexture")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert_eq!(
        own,
        vec!["size"],
        "ExternalTexture binds exactly one member"
    );
}

/// ⚠️ Exactly THREE methods are bound (external_texture.cpp:35-41), and
/// `get_size` is NOT among them.
///
/// ExternalTexture overrides the `get_size` virtual, but the *binding* is
/// Texture2D's — so it must be inherited, not re-declared. Re-registering it
/// here would invent a duplicate that Godot's ClassDB does not have.
#[test]
fn it_binds_exactly_godots_three_methods_and_inherits_get_size() {
    register_viewport_classes();

    let mut own = own_methods("ExternalTexture", "Texture2D");
    own.sort();
    assert_eq!(
        own,
        vec![
            "get_external_texture_id",
            "set_external_buffer_id",
            "set_size"
        ],
        "Godot binds exactly these three on ExternalTexture"
    );

    assert_eq!(arity("ExternalTexture", "set_size"), 1);
    assert_eq!(arity("ExternalTexture", "get_external_texture_id"), 0);
    assert_eq!(arity("ExternalTexture", "set_external_buffer_id"), 1);

    // Inherited, not re-declared — reachable all the same, which is the point.
    assert!(
        class_has_method("ExternalTexture", "get_size"),
        "get_size must be reachable through Texture2D"
    );
    assert!(class_has_method("Texture2D", "get_size"));
    assert!(class_has_method("ExternalTexture", "get_width"));
    assert!(class_has_method("ExternalTexture", "get_height"));
}

/// ⚠️ It is a RESOURCE, not a node — you assign one to a TextureRect's
/// `texture`, you do not put it in the tree.
#[test]
fn it_is_a_resource_not_a_node() {
    register_viewport_classes();

    assert!(!is_parent_class("ExternalTexture", "Node"));
    assert!(!is_parent_class("ExternalTexture", "CanvasItem"));

    // Not vacuous: Resource really is registered and really does derive Object,
    // so the ancestry assertions above discriminate.
    assert!(class_exists("Resource"));
    assert!(is_parent_class("Resource", "Object"));
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["ExternalTexture", "Texture2D"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_viewport_classes();
    let first = ids();
    register_viewport_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
