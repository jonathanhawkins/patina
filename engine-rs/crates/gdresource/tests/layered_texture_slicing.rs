//! pat-s3rrv.138 — a source image slices into the layers of a layered texture.
//!
//! Godot ground truth: `editor/import/resource_importer_layered_texture.cpp`
//! — `slice_w = width / hslices`, `slice_h = height / vslices`, and the slices
//! are collected with `vslices` in the OUTER loop and `hslices` in the inner
//! one (:370-385).
//!
//! The `GSTL` container half of this bead was already done: `ctex.rs` reads
//! `.ctexarray` / `.ccube` / `.ccubearray` / `.ctex3d` and all four are
//! registered. What did not exist was the SOURCE side — `grep -rn 'slices/'`
//! over `crates/` returned nothing, so a project that imported `sheet.png` as a
//! Texture2DArray got a plain 2D texture and its layer options were dropped.

use gdcore::math::Color;
use gdresource::ctex::{slice_source_into_layers, LayeredKind};
use gdresource::importers::ResourceFormatLoader;
use gdresource::Resource;
use gdvariant::Variant;
use std::path::Path;

/// A `w`x`h` image whose pixels are distinguishable by index: red carries the
/// index, so a mis-ordered slice is visible rather than merely wrong-sized.
fn source_image(w: i64, h: i64) -> Resource {
    let mut res = Resource::new("Texture2D");
    res.set_property("width", Variant::Int(w));
    res.set_property("height", Variant::Int(h));
    let pixels: Vec<Variant> = (0..w * h)
        .map(|i| Variant::Color(Color::new(i as f32, 0.0, 0.0, 1.0)))
        .collect();
    res.set_property("pixels", Variant::Array(pixels));
    res
}

fn layer_reds(res: &Resource, layer: usize) -> Vec<f32> {
    let layer = res
        .subresources
        .get(&format!("layer_{layer}"))
        .unwrap_or_else(|| panic!("layer_{layer} exists"));
    match layer.get_property("pixels") {
        Some(Variant::Array(a)) => a
            .iter()
            .map(|v| match v {
                Variant::Color(c) => c.r,
                other => panic!("expected Color, got {other:?}"),
            })
            .collect(),
        other => panic!("layer pixels: {other:?}"),
    }
}

/// **The acceptance clause.** A 3-frame horizontal strip yields three layers.
#[test]
fn a_three_frame_horizontal_strip_slices_into_three_layers() {
    // 6x2 source, sliced 3 across and 1 down => three 2x2 layers.
    let src = source_image(6, 2);
    let out = slice_source_into_layers(&src, LayeredKind::Texture2DArray, 3, 1, Path::new("s.png"))
        .expect("slices");

    assert_eq!(out.class_name, "CompressedTexture2DArray");
    assert_eq!(out.get_property("layers"), Some(&Variant::Int(3)));
    assert_eq!(out.get_property("layer_count"), Some(&Variant::Int(3)));
    // ⚠️ The layered resource reports the SLICE size, not the strip's. A
    // consumer sampling this as an array uses one layer's dimensions.
    assert_eq!(out.get_property("width"), Some(&Variant::Int(2)));
    assert_eq!(out.get_property("height"), Some(&Variant::Int(2)));
    assert_eq!(out.subresources.len(), 3);
}

/// 🔑 Slices are collected ROW-MAJOR — `vslices` outer, `hslices` inner.
///
/// ⚠️ This is the assertion a count-only test cannot make. A 3x1 strip and a
/// 1x3 column both produce three layers, so a column-major implementation
/// passes every `layers == 3` check and silently reorders the six faces of a
/// cubemap. The source pixels carry their own index in the red channel so the
/// order is checked directly rather than inferred.
#[test]
fn slices_are_collected_row_major() {
    // 4x2, sliced 2x2 => four 2x1 layers. Source indices laid out as
    //   row 0: 0 1 2 3
    //   row 1: 4 5 6 7
    // so row-major slicing gives [0,1], [2,3], [4,5], [6,7].
    let src = source_image(4, 2);
    let out = slice_source_into_layers(&src, LayeredKind::Texture2DArray, 2, 2, Path::new("s.png"))
        .expect("slices");

    assert_eq!(out.get_property("layers"), Some(&Variant::Int(4)));
    assert_eq!(layer_reds(&out, 0), vec![0.0, 1.0], "top-left slice");
    assert_eq!(
        layer_reds(&out, 1),
        vec![2.0, 3.0],
        "🔑 layer 1 must be the slice to the RIGHT of layer 0. Getting \
         [4, 5] here means the loops are nested the wrong way round — \
         column-major — which still yields four layers"
    );
    assert_eq!(layer_reds(&out, 2), vec![4.0, 5.0], "bottom-left slice");
    assert_eq!(layer_reds(&out, 3), vec![6.0, 7.0], "bottom-right slice");
}

/// A cubemap derives its grid from `slices/arrangement`, ignoring h/v.
///
/// ⚠️ The h/v options are present in the very same sidecar, so feeding them to
/// a cubemap is the obvious mistake — and it produces a plausible grid with the
/// wrong face count. Godot's default arrangement is `1` (2x3) = six faces.
#[test]
fn a_cubemaps_grid_comes_from_its_arrangement_not_from_h_and_v() {
    // h/v deliberately set to a grid that would give 12 layers if honoured.
    for (arrangement, expect) in [(0i64, (1u32, 6u32)), (1, (2, 3)), (2, (3, 2)), (3, (6, 1))] {
        let grid = LayeredKind::Cubemap.slice_grid(4, 3, arrangement, 1, 1);
        assert_eq!(
            grid, expect,
            "arrangement {arrangement} must select {expect:?} regardless of the \
             h/v options"
        );
        assert_eq!(grid.0 * grid.1, 6, "every cubemap arrangement is six faces");
    }
    // A cubemap ARRAY multiplies one axis by `amount`, chosen by `layout`.
    assert_eq!(
        LayeredKind::CubemapArray.slice_grid(4, 3, 1, 0, 2),
        (4, 3),
        "layout 0 (Horizontal) scales the horizontal axis: 2x3 -> 4x3"
    );
    assert_eq!(
        LayeredKind::CubemapArray.slice_grid(4, 3, 1, 1, 2),
        (2, 6),
        "layout 1 (Vertical) scales the vertical axis: 2x3 -> 2x6"
    );
}

/// A grid finer than the image is an error, not a zero-sized layer.
#[test]
fn a_grid_too_fine_for_the_source_is_rejected() {
    let src = source_image(2, 2);
    assert!(
        slice_source_into_layers(&src, LayeredKind::Texture2DArray, 4, 1, Path::new("s.png"))
            .is_err(),
        "a 2px-wide image cannot yield four columns; a zero-width layer would \
         reach the sampler as an empty texture instead of an import error"
    );
}

/// **The wiring.** A sidecar carrying `slices/*` routes a real PNG through the
/// layered path, and one without it does not.
///
/// ⚠️ Both halves matter. Without the control, this passes against an importer
/// that made EVERY png a one-layer array.
#[test]
fn the_sidecar_selects_the_layered_path() {
    const PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/png16.png"
    ));
    let dir = tempfile::TempDir::new().unwrap();
    let png = dir.path().join("sheet.png");
    std::fs::write(&png, PNG).unwrap();

    // Control: no sidecar at all — an ordinary 2D texture.
    let plain = ResourceFormatLoader::with_defaults()
        .load_resource(&png)
        .expect("plain load");
    assert_eq!(
        plain.class_name, "Texture2D",
        "a png with no slice options must stay a plain 2D texture"
    );
    let src_w = match plain.get_property("width") {
        Some(Variant::Int(w)) => *w,
        other => panic!("width: {other:?}"),
    };

    std::fs::write(
        dir.path().join("sheet.png.import"),
        "[params]\nslices/horizontal=2\nslices/vertical=1\n",
    )
    .unwrap();
    let sliced = ResourceFormatLoader::with_defaults()
        .load_resource(&png)
        .expect("sliced load");
    assert_eq!(
        sliced.class_name, "CompressedTexture2DArray",
        "the slice options must select the layered path"
    );
    assert_eq!(sliced.get_property("layers"), Some(&Variant::Int(2)));
    // Asserted as a RELATION to the fixture's own width so the test does not
    // encode the fixture's dimensions.
    assert_eq!(
        sliced.get_property("width"),
        Some(&Variant::Int(src_w / 2)),
        "each layer is half the strip"
    );
}
