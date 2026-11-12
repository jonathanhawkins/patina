//! pat-nzgdu.112 — `load_resource` on advanced PNG variants (16-bit depth,
//! palette + `tRNS` alpha, and Adam7 interlace) must return a `Texture2D` with a
//! fully populated `pixels` array, not an empty one.
//!
//! The hand-rolled `decode_png_pixels` fast path only handles non-interlaced
//! 8-bit truecolor/grayscale; these variants previously loaded with correct
//! width/height but `has_pixel_data=false` and no pixels. The `image`-crate
//! fallback (its `png` feature) now decodes them on the same `.png` registry
//! path. Fixtures are hand-generated PNGs validated byte-for-byte (incl. a full
//! Adam7 de-interlace round-trip) at authoring time.

use gdcore::math::Color;
use gdresource::importers::ResourceFormatLoader;
use gdresource::Resource;
use gdvariant::Variant;
use std::sync::Arc;

const PNG16: &[u8] = include_bytes!("fixtures/png16.png");
const PNG_PALETTE: &[u8] = include_bytes!("fixtures/png_palette_trns.png");
const PNG_INTERLACED: &[u8] = include_bytes!("fixtures/png_interlaced.png");

fn load(bytes: &[u8], name: &str) -> Arc<Resource> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write png");
    ResourceFormatLoader::with_defaults()
        .load_resource(&path)
        .expect("load png via registry")
}

fn dims(res: &Resource) -> (i64, i64) {
    let g = |k| match res.get_property(k) {
        Some(Variant::Int(v)) => *v,
        other => panic!("expected Int for {k}, got {other:?}"),
    };
    (g("width"), g("height"))
}

fn pixels(res: &Resource) -> Vec<Color> {
    match res.get_property("pixels") {
        Some(Variant::Array(a)) => a
            .to_vec()
            .iter()
            .map(|v| match v {
                Variant::Color(c) => *c,
                other => panic!("pixels entry not a Color: {other:?}"),
            })
            .collect(),
        other => panic!("no pixels array: {other:?}"),
    }
}

#[test]
fn png_advanced_variants_decode() {
    // (1) 16-bit RGBA — the fast path bails on bit_depth != 8; the image crate
    // downsamples to 8-bit RGBA. First fixture pixel is full red.
    let r16 = load(PNG16, "sixteen.png");
    assert_eq!(r16.class_name, "Texture2D");
    assert!(
        matches!(
            r16.get_property("has_pixel_data"),
            Some(Variant::Bool(true))
        ),
        "16-bit PNG must decode to real pixels"
    );
    assert_eq!(dims(&r16), (2, 2));
    let px = pixels(&r16);
    assert_eq!(px.len(), 4, "16-bit pixels fully populated (2x2)");
    assert!(
        px[0].r > 0.9 && px[0].g < 0.1 && px[0].b < 0.1,
        "16-bit first pixel should be red, got {:?}",
        px[0]
    );

    // (2) Palette (color_type 3) + tRNS — the fast path bails on palette. Index 0
    // is opaque red; index 1 is blue with tRNS alpha ~0.5.
    let rp = load(PNG_PALETTE, "palette.png");
    assert!(
        matches!(rp.get_property("has_pixel_data"), Some(Variant::Bool(true))),
        "palette PNG must decode to real pixels"
    );
    let px = pixels(&rp);
    assert_eq!(px.len(), 4);
    assert!(
        px[0].r > 0.9 && px[0].g < 0.1 && px[0].b < 0.1,
        "palette index 0 should be opaque red, got {:?}",
        px[0]
    );
    assert!(
        px[1].b > 0.9 && px[1].r < 0.1,
        "palette index 1 should be blue, got {:?}",
        px[1]
    );
    assert!(
        px[1].a > 0.4 && px[1].a < 0.6,
        "tRNS must apply the palette entry's alpha (~0.5), got {}",
        px[1].a
    );

    // (3) Adam7-interlaced 8x8 — the fast path bails on interlace. The corner
    // pixel (7,7) of the authored gradient is ~(0.878 gray); recovering it proves
    // de-interlacing placed pixels correctly (a mis-decode would scramble them).
    let ri = load(PNG_INTERLACED, "interlaced.png");
    assert!(
        matches!(ri.get_property("has_pixel_data"), Some(Variant::Bool(true))),
        "interlaced PNG must decode to real pixels"
    );
    assert_eq!(dims(&ri), (8, 8));
    let px = pixels(&ri);
    assert_eq!(px.len(), 64, "interlaced pixels fully populated (8x8)");
    let corner = px[7 * 8 + 7];
    assert!(
        (corner.r - 0.878).abs() < 0.02
            && (corner.g - 0.878).abs() < 0.02
            && (corner.b - 0.878).abs() < 0.02,
        "de-interlaced pixel (7,7) should be the gradient's ~0.878 gray, got {corner:?}"
    );
}
