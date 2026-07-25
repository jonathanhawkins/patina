//! pat-0q2kx.45 — every engine load must come through the registered resource
//! format loaders, resolving `res://` and `.import` remaps.
//!
//! The pipeline was implemented and tested, but **nothing at runtime called it**:
//! the interpreter's `load()` guessed from the extension and handed back empty
//! placeholder resources, and the editor's texture cache used a PNG-only decoder.
//! `.jpg` was not even registered, so a JPEG was unloadable however it was reached.
//!
//! This exercises [`UnifiedLoader`] — the shared entry point — rather than the raw
//! format loader, so it proves the path a *consumer* takes: `res://` in, decoded
//! resource out, remaps followed.

use gdcore::math::Color;
use gdresource::importers::UnifiedLoader;
use gdresource::Resource;
use gdvariant::Variant;

/// Builds a minimal `GST2` `.ctex` wrapping raw RGBA8 pixels
/// (`DATA_FORMAT_IMAGE`), mirroring `gdresource::ctex`'s on-disk layout. Same
/// helper shape as `resource_loader_follows_import_remap.rs`.
fn build_ctex_image_rgba8(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GST2");
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&5u32.to_le_bytes());
    d.extend_from_slice(raw);
    d
}

fn int_prop(res: &Resource, key: &str) -> i64 {
    match res.get_property(key) {
        Some(Variant::Int(v)) => *v,
        other => panic!("expected Int for {key}, got {other:?}"),
    }
}

fn pixels_of(res: &Resource) -> Vec<Color> {
    match res.get_property("pixels") {
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        other => panic!("expected a pixels Array, got {other:?}"),
    }
}

/// **The acceptance test.** A project's assets load through the unified entry
/// point: a real WebP decodes to its true dimensions, and a source image whose
/// `.import` sidecar redirects to a compiled `.ctex` resolves to the artifact.
#[test]
fn runtime_load_follows_import_pipeline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // -- a real WebP, loaded by res:// path -----------------------------------
    //
    // WebP was decodable but unreachable from any runtime consumer. The fixture is
    // a genuine file, so this fails if the format dispatch regresses.
    let webp_src = std::path::Path::new("fixtures/icon.webp");
    let webp_bytes = std::fs::read(webp_src).expect("gdresource/fixtures/icon.webp must exist");
    std::fs::write(root.join("icon.webp"), &webp_bytes).expect("write webp");

    let loader = UnifiedLoader::new(root);
    let webp = loader
        .load("res://icon.webp")
        .expect("a WebP must load through the unified loader");
    assert_eq!(webp.class_name, "Texture2D");
    let (w, h) = (int_prop(&webp, "width"), int_prop(&webp, "height"));
    assert!(
        w > 0 && h > 0,
        "the WebP must report its real dimensions, got {w}x{h}",
    );
    assert_eq!(
        pixels_of(&webp).len(),
        (w * h) as usize,
        "the pixel array must match the declared extent",
    );

    // -- a source image remapped to a compiled artifact -----------------------
    //
    // The source is deliberately not a valid PNG: if the loader decoded the source
    // instead of following the sidecar, this fails. Godot's contract is that after
    // import the artifact is authoritative.
    std::fs::write(root.join("icon.png"), b"NOT-A-REAL-PNG-SOURCE").expect("source");
    std::fs::write(
        root.join("icon.png.import"),
        b"[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/icon.ctex\"\n",
    )
    .expect("sidecar");
    let imported = root.join(".godot/imported");
    std::fs::create_dir_all(&imported).expect("imported dir");
    let raw: [u8; 16] = [
        255, 0, 0, 255, // (0,0) red
        0, 255, 0, 255, // (1,0) green
        0, 0, 255, 255, // (0,1) blue
        255, 255, 255, 255, // (1,1) white
    ];
    std::fs::write(
        imported.join("icon.ctex"),
        build_ctex_image_rgba8(2, 2, &raw),
    )
    .expect("ctex artifact");

    let remapped = loader
        .load("res://icon.png")
        .expect("a remapped source must load its artifact through the unified loader");
    assert_eq!(remapped.class_name, "Texture2D");
    assert_eq!(
        (int_prop(&remapped, "width"), int_prop(&remapped, "height")),
        (2, 2),
        "dimensions must come from the artifact, not the source",
    );
    let px = pixels_of(&remapped);
    assert_eq!(px.len(), 4, "2x2 pixels from the artifact");
    assert!(
        px[0].r > 0.9 && px[0].g < 0.1 && px[0].b < 0.1,
        "first artifact pixel is red, got {:?}",
        px[0],
    );
}

/// The unified loader claims exactly the formats that have a compiled decoder, and
/// declines the rest — so this is about which registrations exist, not about
/// accepting everything.
///
/// `.jpg` is deliberately **not** in the claimed list: the JPEG decoder lives in
/// `gdresource/src/texture_import.rs`, which is declared as a module by no crate
/// and therefore never compiles, and neither `jpeg-decoder` nor `image`'s `jpeg`
/// feature is enabled in this workspace. Registering `.jpg` would hand callers a
/// loader that cannot exist (pat-0q2kx.45).
#[test]
fn loader_claims_compiled_formats_and_declines_the_rest() {
    let loader = UnifiedLoader::new("/project");
    for claimed in [
        "res://icon.webp",
        "res://icon.svg",
        "res://icon.png",
        "res://icon.ctex",
        "res://scene.tscn",
    ] {
        assert!(
            loader.can_load_path(claimed),
            "{claimed} must be claimed by a registered format loader",
        );
    }
    for unclaimed in [
        "res://photo.jpg",
        "res://photo.jpeg",
        "res://notes.xyz",
        "res://noextension",
    ] {
        assert!(
            !loader.can_load_path(unclaimed),
            "{unclaimed} must not be claimed",
        );
    }
}

/// `res://` resolves against the project root; an already-resolved filesystem path
/// passes through, so consumers holding either form need no special case.
#[test]
fn res_paths_resolve_against_the_project_root() {
    let loader = UnifiedLoader::new("/project");
    assert_eq!(
        loader.resolve("res://art/icon.png"),
        std::path::PathBuf::from("/project/art/icon.png"),
    );
    assert_eq!(
        loader.resolve("/tmp/already/resolved.png"),
        std::path::PathBuf::from("/tmp/already/resolved.png"),
    );
}
