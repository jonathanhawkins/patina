//! A cloned Godot project has `.import` sidecars but no `.godot/imported/`
//! artifacts (`.godot/` is never committed). Godot reimports from the source,
//! so the texture loads. Patina's importer treated the missing artifact as a
//! hard error, which made every non-PNG texture of a cloned project fail — the
//! official 2D Platformer demo (all `.webp`) drew every prop as a white box.

use gdscene::texture_cache::TextureCache;

#[test]
fn a_webp_with_a_fresh_clone_sidecar_loads_from_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("project.godot"), "config_version=5\n").unwrap();

    // A 3x2 opaque red image, WebP-encoded (lossless).
    let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]));
    let mut webp = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP)
        .unwrap();
    std::fs::write(root.join("rock.webp"), &webp).unwrap();
    // Exactly what Godot writes next to an imported texture — pointing at an
    // artifact this checkout never generated.
    std::fs::write(
        root.join("rock.webp.import"),
        "[remap]\n\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\n\
         path=\"res://.godot/imported/rock.webp-0123.ctex\"\n",
    )
    .unwrap();

    let mut cache = TextureCache::new(root.to_string_lossy().to_string());
    let tex = cache
        .get("res://rock.webp")
        .expect("a missing import artifact must fall back to the source, as Godot reimports it");
    assert_eq!((tex.width, tex.height), (3, 2));
    let px = tex.pixels[0];
    assert!(px.r > 0.99 && px.g < 0.01 && px.a > 0.99, "{px:?}");
}
