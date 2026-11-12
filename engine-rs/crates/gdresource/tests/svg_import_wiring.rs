//! pat-u3l8c.90 — SVG rasterizer wiring for the real Godot default icon.
//!
//! Every Godot project ships a `res://icon.svg`, so the default-project load
//! path routes through the SVG rasterizer. The rasterizer + `.svg` loader
//! registration landed in `image_svg.rs` (pat-nzgdu.44) and are unit-tested
//! there against a synthetic rect+circle SVG. This integration test closes the
//! acceptance's specific "default icon.svg loads as a texture" clause by driving
//! the *real* Godot default `icon.svg` (vendored from `upstream/godot/icon.svg`)
//! end-to-end through the public `ResourceFormatLoader`, the same way
//! `webp_import_wiring.rs` drives the real `icon.webp`.

use gdvariant::Variant;

use gdresource::importers::ResourceFormatLoader;

/// Godot's own 1024×1024 default project icon, vendored from
/// `upstream/godot/icon.svg`. Pure geometry (paths + circles, no text/fonts/
/// external refs), so it rasterizes with `usvg::Options::default()`.
const ICON_SVG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/icon.svg"));
const ICON_W: i64 = 1024;
const ICON_H: i64 = 1024;

/// Acceptance: the default `icon.svg` loads through the *registered* loader to a
/// `Texture2D` carrying decoded pixels — proving the real asset flows on the
/// same load path PNG/WebP use, not an extension-only classification.
#[test]
fn default_icon_svg_loads_pixels() {
    let rfl = ResourceFormatLoader::with_defaults();
    assert!(rfl.can_load(".svg"), ".svg must be a registered loader");

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("icon.svg");
    std::fs::write(&path, ICON_SVG).unwrap();

    let res = rfl
        .load_resource(&path)
        .expect("load default icon.svg via registry");
    assert_eq!(
        res.class_name, "Texture2D",
        "icon.svg imports as a Texture2D"
    );
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
                "one Color per pixel (1024*1024)",
            );
            assert!(
                px.to_vec().iter().all(|p| matches!(p, Variant::Color(_))),
                "every pixel is a Color",
            );
            // A real rasterization is not uniformly transparent-black: the logo
            // fills the centre, so at least one pixel carries visible colour.
            assert!(
                px.to_vec().iter().any(|p| match p {
                    Variant::Color(c) => c.r > 0.01 || c.g > 0.01 || c.b > 0.01 || c.a > 0.01,
                    _ => false,
                }),
                "the rasterized icon must not be uniformly transparent-black",
            );
        }
        other => panic!("expected pixels array, got {other:?}"),
    }
}

/// The real icon honours Godot's scale contract: rasterizing at 2.0 rounds to
/// `ceil(dim*2)` on each axis → four times the pixel count of scale 1.0.
#[test]
fn default_icon_svg_scale_doubles_dimensions() {
    let (w1, h1, c1) =
        gdresource::image_svg::rasterize_svg_bytes(ICON_SVG, 1.0).expect("rasterize at 1.0");
    assert_eq!((w1, h1), (ICON_W as u32, ICON_H as u32), "intrinsic size");

    let (w2, h2, c2) =
        gdresource::image_svg::rasterize_svg_bytes(ICON_SVG, 2.0).expect("rasterize at 2.0");
    assert_eq!(
        (w2, h2),
        (2 * ICON_W as u32, 2 * ICON_H as u32),
        "scale 2.0 doubles each dimension",
    );
    assert_eq!(
        c2.len(),
        4 * c1.len(),
        "scale 2.0 yields 4× the pixel count",
    );
}
