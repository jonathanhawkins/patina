//! pat-nzgdu.133 — the resource loader must follow a source asset's `.import`
//! sidecar `[remap] path` to the compiled `.ctex` artifact under
//! `.godot/imported/`, rather than re-decoding the source. The remap resolution
//! (`resolve_import_artifact`) is already wired into `import_image`/`import_webp`;
//! this proves the end-to-end path from a source `res://icon.png` to the
//! artifact's pixels.

use gdcore::math::Color;
use gdresource::importers::ResourceFormatLoader;
use gdresource::Resource;
use gdvariant::Variant;

/// Builds a minimal `GST2` `.ctex` wrapping raw RGBA8 pixels
/// (`DATA_FORMAT_IMAGE`), mirroring `gdresource::ctex`'s on-disk layout.
fn build_ctex_image_rgba8(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GST2"); // magic
    d.extend_from_slice(&1u32.to_le_bytes()); // version
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // df flags
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
    d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
    // Inner image header: data_format=IMAGE(0), w16, h16, mipmaps=0, RGBA8(5).
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

#[test]
fn resource_loader_follows_import_remap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // A project root (so `res://` remap paths resolve).
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");
    // The *source* image — deliberately NOT a valid PNG. If the loader re-decoded
    // the source instead of following the remap, this would fail; it must not be
    // read at all because the sidecar redirects to the artifact.
    std::fs::write(root.join("icon.png"), b"NOT-A-REAL-PNG-SOURCE").expect("source");
    // The `.import` sidecar remapping to the compiled artifact.
    std::fs::write(
        root.join("icon.png.import"),
        b"[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/icon.ctex\"\n",
    )
    .expect("sidecar");
    // The compiled `.ctex` artifact: a 2x2 RGBA8 image whose first pixel is red.
    let imported = root.join(".godot/imported");
    std::fs::create_dir_all(&imported).expect("imported dir");
    let raw: [u8; 16] = [
        255, 0, 0, 255, // (0,0) red
        0, 255, 0, 255, // (1,0) green
        0, 0, 255, 255, // (0,1) blue
        255, 255, 255, 255, // (1,1) white
    ];
    std::fs::write(imported.join("icon.ctex"), build_ctex_image_rgba8(2, 2, &raw))
        .expect("ctex artifact");

    // Load the SOURCE path; the loader must follow the remap to the `.ctex`.
    let res = ResourceFormatLoader::with_defaults()
        .load_resource(&root.join("icon.png"))
        .expect("load resource via import remap");

    assert_eq!(res.class_name, "Texture2D");
    // Dimensions/format come from the `.ctex` header (2x2), not the source.
    assert_eq!((int_prop(&res, "width"), int_prop(&res, "height")), (2, 2));

    // Pixels come from the artifact (first pixel red), proving the compiled
    // artifact was loaded rather than a re-decode of the (invalid) source.
    let pixels = match res.get_property("pixels") {
        Some(Variant::Array(a)) => a,
        other => panic!("expected pixels array, got {other:?}"),
    };
    assert_eq!(pixels.len(), 4, "2x2 pixels from the artifact");
    match &pixels[0] {
        Variant::Color(c) => assert!(
            c.r > 0.9 && c.g < 0.1 && c.b < 0.1,
            "artifact's first pixel is red, got {c:?}"
        ),
        other => panic!("expected Color, got {other:?}"),
    }
}
