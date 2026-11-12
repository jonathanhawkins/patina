//! pat-nzgdu.209 — ClassDB's `ExternalTexture` and the backend must not drift.
//!
//! The `Vector2(256, 256)` default is hardcoded **twice**: once in ClassDB
//! (`gdobject::class_db`, for introspection and `instantiate`) and once in the
//! backend (`gdrender2d::external_texture::ExternalTexture::default`, for the
//! renderer). `gdobject` and `gdrender2d` do not depend on each other, so they
//! cannot share a constant and **nothing else forces the two copies to agree**.
//!
//! This lives in gdscene because it is the crate that sees both — the same shape
//! as `fog_material_properties_drive_render_struct.rs` (pat-nzgdu.208), which
//! exists because that exact drift shipped a real rendering bug.
//!
//! Ground truth: `default="Vector2(256, 256)"` in Godot's `ExternalTexture.xml`,
//! from the `Size2 size = Size2(256, 256)` member initialiser in
//! `scene/resources/external_texture.h`.

use gdobject::class_db;
use gdrender2d::external_texture::ExternalTexture;
use gdvariant::Variant;

/// The registered default must equal the backend's `default()`.
#[test]
fn external_texture_classdb_default_matches_the_backend() {
    class_db::register_viewport_classes();

    let registered = class_db::get_property_list("ExternalTexture")
        .into_iter()
        .rev() // most-derived wins, matching `instantiate`
        .find(|p| p.name == "size")
        .expect("ExternalTexture must register `size`")
        .default_value;

    let backend = ExternalTexture::default();

    assert_eq!(
        registered,
        Variant::Vector2(backend.size()),
        "ClassDB's ExternalTexture.size default and the backend's default have \
         drifted — a script reading the property would disagree with what the \
         renderer actually reports"
    );

    // Pin the shared value to Godot's, so this cannot be "fixed" by making both
    // sides agree on the WRONG number.
    assert_eq!(
        registered,
        Variant::Vector2(gdcore::math::Vector2::new(256.0, 256.0)),
        "Godot's ExternalTexture defaults to 256x256 (external_texture.h)"
    );
}

/// The backend's reported dimensions must match the registered default too —
/// `get_width`/`get_height` read straight off `size` in Godot.
#[test]
fn the_backends_reported_dimensions_follow_the_registered_default() {
    class_db::register_viewport_classes();

    let backend = ExternalTexture::default();
    assert_eq!(backend.width(), 256);
    assert_eq!(backend.height(), 256);

    // ...and ClassDB agrees the accessors exist to report them.
    assert!(class_db::class_has_method("ExternalTexture", "get_width"));
    assert!(class_db::class_has_method("ExternalTexture", "get_height"));
    assert!(class_db::class_has_method("ExternalTexture", "get_size"));
}

/// ⚠️ A zero default would be **self-concealing**, which is why both sides are
/// pinned rather than just one: `set_size` rejects non-positive sizes, so a
/// texture that started at 0x0 would stay 0x0 forever and silently draw nothing.
#[test]
fn a_zero_default_would_be_unrecoverable_through_the_public_api() {
    let mut tex = ExternalTexture::default();

    assert!(
        !tex.set_size(gdcore::math::Vector2::new(0.0, 0.0)),
        "set_size must reject a zero size"
    );
    assert_eq!(
        tex.width(),
        256,
        "the rejected set must leave the size untouched"
    );
}
