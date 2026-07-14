//! pat-nzgdu.8 — WebP image import wiring.
//!
//! Godot decodes source `.webp` via libwebp and stores default 2D textures as
//! WebP-in-`.ctex` (inner `DATA_FORMAT_WEBP == 2`), so opening any real
//! imported/exported project routes textures through a WebP artifact. Before
//! this bead Patina had no WebP decoder: `.ctex` WebP returned
//! `Unsupported("no WebP decoder in this build")` and `.webp` had no registered
//! loader. This test drives the public `ResourceFormatLoader` end-to-end for all
//! three entry points and asserts a populated `pixels` array — the same contract
//! the PNG path already satisfies.

use gdresource::importers::ResourceFormatLoader;
use gdvariant::Variant;

/// Godot's own 256x256 test WebP (VP8X), vendored from
/// `upstream/godot/tests/data/images/icon.webp`.
const ICON_WEBP: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/icon.webp"));
const ICON_W: i64 = 256;
const ICON_H: i64 = 256;

/// Wrap a WebP payload in a minimal `.ctex` (GST2) container with inner
/// `DATA_FORMAT_WEBP` — the layout `load_ctex_bytes` parses (level 0 =
/// `u32 size` + the WebP file).
fn build_ctex_webp(width: u32, height: u32, webp: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GST2"); // magic
    d.extend_from_slice(&1u32.to_le_bytes()); // version
    d.extend_from_slice(&width.to_le_bytes()); // original width
    d.extend_from_slice(&height.to_le_bytes()); // original height
    d.extend_from_slice(&0u32.to_le_bytes()); // df flags
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
    d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
    d.extend_from_slice(&2u32.to_le_bytes()); // inner data_format = DATA_FORMAT_WEBP
    d.extend_from_slice(&(width as u16).to_le_bytes()); // inner w
    d.extend_from_slice(&(height as u16).to_le_bytes()); // inner h
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmaps
    d.extend_from_slice(&5u32.to_le_bytes()); // Image::FORMAT_RGBA8 (unused for webp)
    d.extend_from_slice(&(webp.len() as u32).to_le_bytes()); // level-0 payload size
    d.extend_from_slice(webp);
    d
}

/// Assert a loaded resource is a Texture2D with the right dimensions and a
/// fully-populated pixel array.
fn assert_icon_texture(res: &gdresource::resource::Resource) {
    assert_eq!(res.class_name, "Texture2D");
    assert_eq!(res.get_property("width"), Some(&Variant::Int(ICON_W)));
    assert_eq!(res.get_property("height"), Some(&Variant::Int(ICON_H)));
    assert_eq!(
        res.get_property("has_pixel_data"),
        Some(&Variant::Bool(true))
    );
    match res.get_property("pixels") {
        Some(Variant::Array(px)) => {
            assert_eq!(
                px.len() as i64,
                ICON_W * ICON_H,
                "one Color per pixel (256*256)",
            );
            assert!(
                px.iter().all(|p| matches!(p, Variant::Color(_))),
                "every pixel is a Color",
            );
            // A real decode is not uniformly transparent-black: at least one
            // pixel must carry visible colour/alpha.
            assert!(
                px.iter().any(|p| match p {
                    Variant::Color(c) => c.r > 0.01 || c.g > 0.01 || c.b > 0.01 || c.a > 0.01,
                    _ => false,
                }),
                "decoded pixels must not be all-zero",
            );
        }
        other => panic!("expected pixels array, got {other:?}"),
    }
}

/// Acceptance #1: a loose `.webp` loads to a Texture2D with real pixels.
#[test]
fn loose_webp_loads_pixels() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("icon.webp");
    std::fs::write(&path, ICON_WEBP).unwrap();

    let rfl = ResourceFormatLoader::with_defaults();
    let res = rfl.load_resource(&path).unwrap();
    assert_icon_texture(&res);
}

/// Acceptance #2: a `.ctex` whose inner `data_format` is `DATA_FORMAT_WEBP`
/// decodes to pixels instead of returning `Unsupported`.
#[test]
fn ctex_webp_decodes_pixels() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("icon.ctex");
    std::fs::write(&path, build_ctex_webp(ICON_W as u32, ICON_H as u32, ICON_WEBP)).unwrap();

    let rfl = ResourceFormatLoader::with_defaults();
    let res = rfl.load_resource(&path).unwrap();
    assert_icon_texture(&res);
}

/// Acceptance #3: a source PNG whose `.import` sidecar redirects to a WebP
/// `.ctex` loads real pixels (the default imported-project path).
#[test]
fn png_import_sidecar_redirects_to_webp_ctex() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".godot/imported")).unwrap();

    // Deliberately-invalid source bytes: if the loader decoded the source
    // directly (no redirect) it would fail — proving the redirect happened.
    let png = root.join("icon.png");
    std::fs::write(&png, b"NOT A PNG").unwrap();
    std::fs::write(
        root.join("icon.png.import"),
        "[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/icon.ctex\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".godot/imported/icon.ctex"),
        build_ctex_webp(ICON_W as u32, ICON_H as u32, ICON_WEBP),
    )
    .unwrap();

    let rfl = ResourceFormatLoader::with_defaults();
    let res = rfl.load_resource(&png).unwrap();
    assert_icon_texture(&res);
}
