//! pat-0q2kx.217 — `PortableCompressedTexture2D` exists, is registered, and
//! round-trips an image losslessly.
//!
//! Godot's `PortableCompressedTexture2D` keeps its compressed payload **inside
//! the resource**, which is what makes a `.tres` self-contained — no
//! `.godot/imported/` sidecar. The class was absent here entirely: no
//! implementation and no ClassDB entry, so a scene referencing the type failed
//! to load outright.

use gdobject::class_db::class_exists;
use gdobject::{class_get_integer_constant, is_parent_class, register_viewport_classes};
use gdresource::image::{Image, ImageFormat};
use gdresource::portable_compressed_texture::{
    PortableCompressedTexture2D, PortableCompressionMode,
};

/// A small image with varied colour *and* alpha, so a round-trip that silently
/// dropped or flattened a channel could not pass.
fn sample_image(w: u32, h: u32) -> Image {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            data.push((x * 37 % 256) as u8);
            data.push((y * 53 % 256) as u8);
            data.push(((x + y) * 11 % 256) as u8);
            data.push(if (x + y) % 3 == 0 { 128 } else { 255 });
        }
    }
    Image::create_from_data(w, h, ImageFormat::Rgba8, data)
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** The class is registered, and an image compressed
/// with `MODE_LOSSLESS` comes back identical.
#[test]
fn portable_compressed_texture2d_roundtrip() {
    // -- (a) ClassDB knows the type ------------------------------------------
    register_viewport_classes();
    assert!(
        class_exists("PortableCompressedTexture2D"),
        "ClassDB.class_exists(PortableCompressedTexture2D) must be true, or a \
         .tscn/.tres referencing the type cannot load",
    );
    assert!(
        is_parent_class("PortableCompressedTexture2D", "Texture2D"),
        "it must be a Texture2D, which is what makes it assignable as one",
    );

    // -- (b) a lossless round-trip is byte-exact -----------------------------
    let src = sample_image(9, 6);
    let tex =
        PortableCompressedTexture2D::create_from_image(&src, PortableCompressionMode::Lossless)
            .expect("MODE_LOSSLESS compresses");

    assert_eq!(tex.get_size(), (9, 6), "size is preserved");

    let back = tex.get_image().expect("get_image decodes the payload");
    assert_eq!(back.get_width(), src.get_width());
    assert_eq!(back.get_height(), src.get_height());
    assert_eq!(
        back.get_data(),
        src.get_data(),
        "get_image() must match the source exactly for MODE_LOSSLESS",
    );
}

// ===========================================================================
// Details
// ===========================================================================

/// The payload is a genuine PNG, not raw bytes wearing a header — otherwise
/// "compressed texture" would be a misnomer and `.tres` files would balloon.
#[test]
fn payload_is_real_compressed_png() {
    let flat = Image::create_from_data(48, 48, ImageFormat::Rgba8, vec![200u8; 48 * 48 * 4]);
    let tex =
        PortableCompressedTexture2D::create_from_image(&flat, PortableCompressionMode::Lossless)
            .expect("compresses");

    assert_eq!(
        &tex.compressed_bytes()[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "payload must carry the PNG magic",
    );
    let raw_len = 48 * 48 * 4;
    assert!(
        tex.compressed_bytes().len() < raw_len / 4,
        "a flat image must compress well below raw size ({raw_len}), got {}",
        tex.compressed_bytes().len(),
    );
}

/// The full `CompressionMode` enum is registered — including **ASTC (6)**,
/// which Godot 4 has and which is the easiest constant to lose when the enum is
/// transcribed from memory rather than from the XML.
#[test]
fn compression_mode_enum_is_registered_including_astc() {
    register_viewport_classes();
    for (name, value) in [
        ("COMPRESSION_MODE_LOSSLESS", 0),
        ("COMPRESSION_MODE_LOSSY", 1),
        ("COMPRESSION_MODE_BASIS_UNIVERSAL", 2),
        ("COMPRESSION_MODE_S3TC", 3),
        ("COMPRESSION_MODE_ETC2", 4),
        ("COMPRESSION_MODE_BPTC", 5),
        ("COMPRESSION_MODE_ASTC", 6),
    ] {
        assert_eq!(
            class_get_integer_constant("PortableCompressedTexture2D", name),
            Some(value),
            "{name} must be registered with Godot's value",
        );
    }
}

/// A non-RGBA source is handed back in its own format rather than silently
/// widened to RGBA8 — a caller that put in `L8` expects `L8` out.
#[test]
fn non_rgba_source_format_is_restored() {
    let gray = Image::create_from_data(
        4,
        2,
        ImageFormat::L8,
        vec![0, 40, 80, 120, 160, 200, 240, 255],
    );
    let tex =
        PortableCompressedTexture2D::create_from_image(&gray, PortableCompressionMode::Lossless)
            .expect("compresses");

    let back = tex.get_image().expect("decodes");
    assert_eq!(
        back.get_format(),
        ImageFormat::L8,
        "the source format must be restored, not left as RGBA8",
    );
    assert_eq!(back.get_width(), 4);
    assert_eq!(back.get_height(), 2);
}

/// An unsupported mode still produces a usable texture and preserves what was
/// asked for, so a `.tres` round-trips — with `is_lossless()` telling the truth
/// about what was actually stored.
#[test]
fn unsupported_mode_falls_back_visibly() {
    let src = sample_image(4, 4);
    for mode in [
        PortableCompressionMode::S3tc,
        PortableCompressionMode::Etc2,
        PortableCompressionMode::Bptc,
        PortableCompressionMode::Astc,
        PortableCompressionMode::BasisUniversal,
        PortableCompressionMode::Lossy,
    ] {
        let tex = PortableCompressedTexture2D::create_from_image(&src, mode)
            .unwrap_or_else(|e| panic!("{mode:?} must still produce a texture: {e:?}"));
        assert_eq!(tex.compression_mode(), mode, "{mode:?} must be preserved");
        assert!(
            !tex.is_lossless(),
            "{mode:?} must report that it is not byte-exact",
        );
        assert_eq!(
            tex.get_image().expect("decodes").get_data(),
            src.get_data(),
            "{mode:?} must still decode to the right pixels despite the fallback",
        );
    }
}
