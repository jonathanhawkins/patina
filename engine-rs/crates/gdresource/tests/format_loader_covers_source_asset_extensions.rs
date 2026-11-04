//! pat-s3rrv.51: source-asset extensions resolve through the loader registry.
//!
//! `ResourceFormatLoader::with_defaults` registered 14 extensions and none of
//! `.tga`/`.bmp`/`.hdr`/`.gltf`/`.glb`, so `load("res://sprite.tga")` answered
//! "unsupported extension" while `image_tga_bmp::decode_tga`,
//! `radiance_hdr::decode_hdr` and `gltf_import::import_gltf` sat in the SAME
//! CRATE, compiled, with zero consumers. The decoders were never the gap; the
//! registration was.
//!
//! Godot resolves every source-asset extension through a registered loader
//! (core/io/image_loader.cpp, editor/import/resource_importer_texture.cpp,
//! modules/gltf/), which is what lets `load()`, the editor and the renderer
//! agree about what a file contains.
//!
//! Everything here goes through `UnifiedLoader::load("res://…")` — the path the
//! editor texture cache takes — rather than calling the decoders directly, so a
//! decoder that works while the registry does not still fails this test.

use gdresource::importers::{ResourceFormatLoader, UnifiedLoader};
use gdvariant::Variant;

/// The glTF fixture that already lives in this crate.
const QUAD_GLB: &[u8] = include_bytes!("../fixtures/quad.glb");

// ---------------------------------------------------------------------------
// Fixture builders — byte layouts taken from each decoder's own header checks
// ---------------------------------------------------------------------------

/// An uncompressed 32-bit top-origin TGA (image type 2).
///
/// TGA stores pixels BGRA, which `read_pixel_rgba` swaps back — so the bytes
/// written here are deliberately NOT in RGBA order.
fn make_tga(width: u16, height: u16, bgra: [u8; 4]) -> Vec<u8> {
    let mut v = vec![0u8; 18];
    v[2] = 2; // uncompressed truecolor
    v[12..14].copy_from_slice(&width.to_le_bytes());
    v[14..16].copy_from_slice(&height.to_le_bytes());
    v[16] = 32; // bits per pixel
    v[17] = 0x20; // top origin
    for _ in 0..(width as usize * height as usize) {
        v.extend_from_slice(&bgra);
    }
    v
}

/// An uncompressed 32-bit top-down BMP (`BITMAPINFOHEADER`, `BI_RGB`).
///
/// Height is written NEGATIVE for top-down rows; at 32bpp each row is already
/// 4-byte aligned, so no padding is needed.
fn make_bmp(width: i32, height: i32, bgra: [u8; 4]) -> Vec<u8> {
    let pixel_offset: u32 = 54;
    let pixels = (width as usize) * (height.unsigned_abs() as usize) * 4;
    let mut v = Vec::with_capacity(pixel_offset as usize + pixels);
    v.extend_from_slice(b"BM");
    v.extend_from_slice(&(pixel_offset + pixels as u32).to_le_bytes()); // file size
    v.extend_from_slice(&0u32.to_le_bytes()); // reserved
    v.extend_from_slice(&pixel_offset.to_le_bytes());
    v.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    v.extend_from_slice(&width.to_le_bytes());
    v.extend_from_slice(&(-height).to_le_bytes()); // negative = top-down
    v.extend_from_slice(&1u16.to_le_bytes()); // planes
    v.extend_from_slice(&32u16.to_le_bytes()); // bpp
    v.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    v.extend_from_slice(&(pixels as u32).to_le_bytes());
    v.extend_from_slice(&[0u8; 16]); // ppm + palette counts
    for _ in 0..(width as usize * height.unsigned_abs() as usize) {
        v.extend_from_slice(&bgra);
    }
    v
}

/// A flat Radiance `.hdr`. `(128, 0, 0, 128)` decodes to 0.5 red:
/// `128 * 2^(128 - 136) = 128/256`.
fn make_hdr(width: usize, height: usize) -> Vec<u8> {
    let mut v =
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n").into_bytes();
    for _ in 0..(width * height) {
        v.extend_from_slice(&[128, 0, 0, 128]);
    }
    v
}

fn int_prop(res: &gdresource::resource::Resource, key: &str) -> i64 {
    match res.get_property(key) {
        Some(Variant::Int(i)) => *i,
        other => panic!("{key}: expected Int, got {other:?}"),
    }
}

fn pixels(res: &gdresource::resource::Resource) -> Vec<Variant> {
    match res.get_property("pixels") {
        Some(Variant::Array(a)) => a.clone().to_vec(),
        other => panic!("pixels: expected Array, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** Every newly registered extension loads through
/// `UnifiedLoader` with real decoded data.
#[test]
fn format_loader_covers_source_asset_extensions() {
    // (1) The registry admits them at all. `can_load` is what the editor's
    // "can I show this file" checks consult.
    let rfl = ResourceFormatLoader::with_defaults();
    for ext in [".tga", ".bmp", ".hdr", ".gltf", ".glb"] {
        assert!(
            rfl.can_load(ext),
            "{ext} must be registered — its decoder already existed and compiled \
             with zero consumers, so an unregistered extension is the whole bug",
        );
    }

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.tga"), make_tga(4, 2, [10, 20, 30, 255])).expect("tga");
    std::fs::write(dir.path().join("b.bmp"), make_bmp(4, 2, [10, 20, 30, 255])).expect("bmp");
    std::fs::write(dir.path().join("c.hdr"), make_hdr(4, 2)).expect("hdr");
    std::fs::write(dir.path().join("d.glb"), QUAD_GLB).expect("glb");
    let loader = UnifiedLoader::new(dir.path().to_path_buf());

    // (2) TGA and BMP produce the SAME pixel-bearing Texture2D shape the PNG
    // and WebP paths do — width/height/pixels/has_pixel_data — so a caller
    // never has to care which extension it asked for.
    for (file, kind) in [("res://a.tga", "TGA"), ("res://b.bmp", "BMP")] {
        let res = loader
            .load(file)
            .unwrap_or_else(|e| panic!("{kind} load: {e:?}"));
        assert_eq!(res.class_name, "Texture2D", "{kind} class");
        assert_eq!(int_prop(&res, "width"), 4, "{kind} width");
        assert_eq!(int_prop(&res, "height"), 2, "{kind} height");
        assert_eq!(
            pixels(&res).len(),
            8,
            "{kind} must carry exactly width*height pixels",
        );
        // BGRA in, RGBA out: the decoder swaps, so reading the bytes back in
        // written order would mean the swap was skipped.
        let Some(Variant::Color(c)) = pixels(&res).first().cloned() else {
            panic!("{kind}: first pixel is not a Color");
        };
        assert!(
            (c.r - 30.0 / 255.0).abs() < 1e-4 && (c.b - 10.0 / 255.0).abs() < 1e-4,
            "{kind} stores BGRA and must come back RGBA; got r={} b={}",
            c.r,
            c.b,
        );
    }

    // (3) HDR keeps LINEAR float values. This is the assertion that catches the
    // tempting bug: dividing by 255 like the 8-bit paths do would turn 0.5 into
    // 0.00196 and silently destroy the dynamic range that is the whole point of
    // loading an HDR.
    let hdr = loader.load("res://c.hdr").expect("hdr load");
    assert_eq!(hdr.class_name, "Texture2D");
    assert_eq!(int_prop(&hdr, "width"), 4);
    assert_eq!(int_prop(&hdr, "height"), 2);
    assert_eq!(pixels(&hdr).len(), 8, "one Color per pixel");
    let Some(Variant::Color(c)) = pixels(&hdr).first().cloned() else {
        panic!("hdr: first pixel is not a Color");
    };
    assert!(
        (c.r - 0.5).abs() < 1e-4,
        "RGBE (128,0,0,128) is 0.5 linear red; got {} — a value near 0.002 means \
         the 8-bit /255 conversion was applied to float data",
        c.r,
    );
    assert!(
        (c.a - 1.0).abs() < 1e-6,
        "Radiance carries no alpha, so it must be opaque",
    );

    // (4) `.glb` reaches the glTF parser through the registry rather than only
    // through a direct `gltf_import::import_gltf` call.
    let glb = loader.load("res://d.glb").expect("glb load");
    assert_eq!(
        glb.class_name, "PackedScene",
        "a .glb must load as a PackedScene, as Godot's scene importer reports \
         (pat-mgixk.23) — the parsed document rides along on the same resource",
    );
    assert_eq!(
        int_prop(&glb, "mesh_count"),
        1,
        "and carry the fixture's real mesh count, not an empty placeholder",
    );
}

/// The registry's own count moves with the registrations (pat-s3rrv.51).
///
/// ⚠️ `format_loader_with_defaults` in `importers.rs` asserted `12` while
/// `with_defaults` registered `14` — it was RED AT HEAD, stale by two, before
/// this bead touched anything. Anyone adding an extension has to move it, so it
/// is pinned here too, next to the list it describes.
#[test]
fn the_default_registry_counts_every_extension_it_registers() {
    let rfl = ResourceFormatLoader::with_defaults();
    let expected = [
        ".png",
        ".webp",
        ".svg",
        ".ctex",
        ".wav",
        ".ttf",
        ".otf",
        ".tres",
        ".tscn",
        ".res",
        ".scn",
        ".vs",
        ".gdshader",
        ".gdshaderinc",
        ".tga",
        ".bmp",
        ".hdr",
        ".gltf",
        ".glb",
        // Registered by later beads and never added here, which left this
        // test red: Godot's compressed cubemap / texture-array / 3D-texture
        // artifacts (`ResourceFormatLoaderCompressedTexture{Layered,3D}`) and
        // the WOFF web-font containers `FontFile` loads.
        ".ccube",
        ".ccubearray",
        ".ctexarray",
        ".ctex3d",
        ".woff",
        ".woff2",
    ];
    for ext in expected {
        assert!(rfl.can_load(ext), "{ext} must be registered");
    }
    assert_eq!(
        rfl.extension_count(),
        expected.len(),
        "the count and the list must not drift — a mismatch means an extension \
         was registered without being spelled out here",
    );
}

/// Extensions this bead deliberately did NOT add still report unsupported
/// (pat-s3rrv.51).
///
/// `.jpg`/`.jpeg` and `.ogg`/`.mp3` need `texture_import.rs` / `audio_import.rs`,
/// which are ORPHANED — never declared in `lib.rs`, so rustc has never compiled
/// them — and `.obj` needs `gdserver3d`, which `gdresource` does not depend on.
/// Wiring those is follow-up work; this pins the boundary honestly rather than
/// letting a later reader assume the whole extension list landed.
#[test]
fn the_orphaned_module_extensions_are_still_unregistered() {
    let rfl = ResourceFormatLoader::with_defaults();
    for ext in [".jpg", ".jpeg", ".ogg", ".mp3", ".obj"] {
        assert!(
            !rfl.can_load(ext),
            "{ext} is NOT part of pat-s3rrv.51's delivered scope — if this now \
             passes, update this test and say so rather than leaving the \
             boundary undocumented",
        );
    }
}
