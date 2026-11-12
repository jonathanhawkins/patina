//! pat-0q2kx.133 stage (1) — `Image`'s uncompressed format family and the
//! pixel operations Godot's `core/io/image.cpp` exposes.
//!
//! ⚠️ This does NOT satisfy the bead's acceptance (`image_class_parity`). That
//! test additionally needs stage (2) the orphaned tga/bmp/hdr/jpg decoders
//! registered on the loader, stage (3) `Image`/`ImageTexture` reachable from
//! GDScript, and stage (4) an `ImageTexture` on a `Sprite2D` reaching the
//! renderer. See the completion report. These tests cover the data layer those
//! stages build on.
//!
//! Before this, `ImageFormat` had exactly two variants (L8, RGBA8) and `Image`
//! had six operations — no convert, resize, mipmaps, blit, blend, crop, rotate,
//! region or used-rect — so nothing downstream could be built on it.
//!
//! An integration test, not a `--lib` unit test: `-p gdresource --lib` cannot
//! pass at HEAD (a pre-existing ctex/webp failure), so a unit test here would be
//! unverifiable.

use gdcore::math::Color;
use gdresource::image::{Image, ImageFormat, Interpolation};

const ALL_FORMATS: &[ImageFormat] = &[
    ImageFormat::L8,
    ImageFormat::La8,
    ImageFormat::R8,
    ImageFormat::Rg8,
    ImageFormat::Rgb8,
    ImageFormat::Rgba8,
    ImageFormat::Rf,
    ImageFormat::Rgf,
    ImageFormat::Rgbf,
    ImageFormat::Rgbaf,
];

fn approx(a: f32, b: f32, eps: f32, what: &str) {
    assert!(
        (a - b).abs() <= eps,
        "{what}: expected {b}, got {a} (tolerance {eps})"
    );
}

/// Every format sizes its buffer correctly and round-trips the channels it
/// actually stores.
#[test]
fn every_format_sizes_and_round_trips_its_channels() {
    for &fmt in ALL_FORMATS {
        let img = Image::create(4, 3, fmt);
        assert_eq!(
            img.get_data_size(),
            4 * 3 * fmt.bytes_per_pixel(),
            "{fmt:?} buffer must be width*height*bpp"
        );

        let mut img = Image::create(2, 2, fmt);
        let c = Color::new(0.25, 0.5, 0.75, 0.5);
        img.set_pixel(1, 1, c);
        let got = img.get_pixel(1, 1);

        // 8-bit formats quantise to 1/255; float formats are exact.
        let eps = if fmt.is_float() { 1e-6 } else { 1.0 / 255.0 };
        match fmt {
            // L8/LA8 store luminance, so all three colour channels read back as
            // the same Rec. 601 luma — not as r/g/b.
            ImageFormat::L8 | ImageFormat::La8 => {
                let luma = 0.25 * 0.299 + 0.5 * 0.587 + 0.75 * 0.114;
                approx(got.r, luma, eps, "L8 luma");
                approx(got.g, got.r, 1e-6, "L8 is grey");
                approx(got.b, got.r, 1e-6, "L8 is grey");
            }
            _ => {
                approx(got.r, c.r, eps, "red round-trip");
            }
        }
        if fmt.has_alpha() {
            approx(got.a, c.a, eps, "alpha round-trip");
        } else {
            approx(got.a, 1.0, 1e-6, "a format without alpha reads opaque");
        }
    }
}

/// Float formats are HDR: they must NOT clamp to 0..1 the way the byte formats
/// do. Storing 4.0 and reading back 1.0 would silently destroy HDR content.
#[test]
fn float_formats_keep_values_outside_zero_to_one() {
    let mut hdr = Image::create(1, 1, ImageFormat::Rgbf);
    hdr.set_pixel(0, 0, Color::new(4.0, -1.5, 12.25, 1.0));
    let got = hdr.get_pixel(0, 0);
    approx(got.r, 4.0, 1e-6, "HDR red");
    approx(got.g, -1.5, 1e-6, "HDR negative green");
    approx(got.b, 12.25, 1e-6, "HDR blue");

    let mut ldr = Image::create(1, 1, ImageFormat::Rgb8);
    ldr.set_pixel(0, 0, Color::new(4.0, -1.5, 0.5, 1.0));
    let got = ldr.get_pixel(0, 0);
    approx(got.r, 1.0, 1e-6, "byte format clamps high");
    approx(got.g, 0.0, 1e-6, "byte format clamps low");
}

/// `convert` moves pixels between formats and drops only what the target cannot
/// hold.
#[test]
fn convert_preserves_what_the_target_format_can_store() {
    let mut src = Image::create(2, 1, ImageFormat::Rgba8);
    src.set_pixel(0, 0, Color::new(1.0, 0.0, 0.0, 0.5));
    src.set_pixel(1, 0, Color::new(0.0, 1.0, 0.0, 1.0));

    let rgb = src.convert(ImageFormat::Rgb8);
    assert_eq!(rgb.get_format(), ImageFormat::Rgb8);
    approx(rgb.get_pixel(0, 0).r, 1.0, 1.0 / 255.0, "RGB8 keeps red");
    approx(
        rgb.get_pixel(0, 0).a,
        1.0,
        1e-6,
        "RGB8 has no alpha, so it reads opaque",
    );

    let f = src.convert(ImageFormat::Rgbaf);
    approx(f.get_pixel(0, 0).a, 0.5, 1.0 / 255.0, "RGBAF keeps alpha");

    // Converting to the same format is a plain copy.
    assert_eq!(src.convert(ImageFormat::Rgba8), src);
}

/// Resizing to the same size is identity for every filter — the half-texel
/// centre convention, which is what stops an upscale shifting the picture.
#[test]
fn resize_to_same_size_is_identity() {
    let mut src = Image::create(3, 3, ImageFormat::Rgba8);
    for y in 0..3 {
        for x in 0..3 {
            src.set_pixel(x, y, Color::new(x as f32 / 2.0, y as f32 / 2.0, 0.0, 1.0));
        }
    }
    for interp in [
        Interpolation::Nearest,
        Interpolation::Bilinear,
        Interpolation::Cubic,
        Interpolation::Lanczos,
    ] {
        let same = src.resized(3, 3, interp);
        for y in 0..3 {
            for x in 0..3 {
                let (a, b) = (same.get_pixel(x, y), src.get_pixel(x, y));
                approx(a.r, b.r, 1.5 / 255.0, &format!("{interp:?} identity r"));
                approx(a.g, b.g, 1.5 / 255.0, &format!("{interp:?} identity g"));
            }
        }
    }
}

/// A bilinear upscale interpolates between texels; nearest does not. Comparing
/// the two is what proves the filter is actually applied.
#[test]
fn bilinear_upscale_blends_where_nearest_does_not() {
    let mut src = Image::create(2, 1, ImageFormat::Rgba8);
    src.set_pixel(0, 0, Color::new(0.0, 0.0, 0.0, 1.0));
    src.set_pixel(1, 0, Color::new(1.0, 1.0, 1.0, 1.0));

    let bil = src.resized(4, 1, Interpolation::Bilinear);
    let near = src.resized(4, 1, Interpolation::Nearest);
    assert_eq!(bil.get_width(), 4);

    // Nearest can only ever emit the two source values.
    for x in 0..4 {
        let r = near.get_pixel(x, 0).r;
        assert!(
            r <= 1.5 / 255.0 || r >= 1.0 - 1.5 / 255.0,
            "nearest must not blend, got {r} at x={x}"
        );
    }
    // Bilinear must produce at least one genuinely intermediate value.
    let blended = (0..4)
        .map(|x| bil.get_pixel(x, 0).r)
        .any(|r| r > 4.0 / 255.0 && r < 1.0 - 4.0 / 255.0);
    assert!(blended, "bilinear upscale produced no intermediate value");

    // The picture must not drift: left stays dark, right stays bright.
    assert!(bil.get_pixel(0, 0).r < bil.get_pixel(3, 0).r);
}

/// Downscaling averages, so a checkerboard collapses toward mid-grey rather
/// than picking one phase.
#[test]
fn bilinear_downscale_averages_a_checkerboard() {
    let mut src = Image::create(4, 4, ImageFormat::Rgba8);
    for y in 0..4 {
        for x in 0..4 {
            let v = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
            src.set_pixel(x, y, Color::new(v, v, v, 1.0));
        }
    }
    let small = src.resized(2, 2, Interpolation::Bilinear);
    for y in 0..2 {
        for x in 0..2 {
            let r = small.get_pixel(x, y).r;
            assert!(
                (0.2..=0.8).contains(&r),
                "a downscaled checkerboard should tend to mid-grey, got {r}"
            );
        }
    }
}

/// `get_used_rect` finds the alpha bounding box; formats without alpha are
/// fully used, and a fully transparent image has an empty rect.
#[test]
fn used_rect_bounds_the_visible_pixels() {
    let mut img = Image::create(8, 8, ImageFormat::Rgba8);
    assert_eq!(
        img.get_used_rect(),
        (0, 0, 0, 0),
        "a fully transparent image uses nothing"
    );

    img.set_pixel(2, 3, Color::new(1.0, 1.0, 1.0, 1.0));
    img.set_pixel(5, 6, Color::new(1.0, 1.0, 1.0, 1.0));
    assert_eq!(
        img.get_used_rect(),
        (2, 3, 4, 4),
        "the rect must span both opaque pixels inclusively"
    );

    let opaque = Image::create(4, 2, ImageFormat::Rgb8);
    assert_eq!(
        opaque.get_used_rect(),
        (0, 0, 4, 2),
        "a format with no alpha channel is entirely used"
    );
}

/// crop / get_region / rotate move pixels to the right places.
#[test]
fn crop_region_and_rotate_place_pixels_correctly() {
    let mut src = Image::create(3, 2, ImageFormat::Rgba8);
    // Encode each pixel's x in red and y in green so a move is traceable.
    for y in 0..2 {
        for x in 0..3 {
            src.set_pixel(x, y, Color::new(x as f32 / 4.0, y as f32 / 4.0, 0.0, 1.0));
        }
    }

    let region = src.get_region(1, 0, 2, 2);
    assert_eq!((region.get_width(), region.get_height()), (2, 2));
    approx(
        region.get_pixel(0, 0).r,
        1.0 / 4.0,
        1.0 / 255.0,
        "region origin maps to source x=1",
    );

    let mut cropped = src.clone();
    cropped.crop(2, 1);
    assert_eq!((cropped.get_width(), cropped.get_height()), (2, 1));
    approx(
        cropped.get_pixel(1, 0).r,
        1.0 / 4.0,
        1.0 / 255.0,
        "crop keeps x=1",
    );

    // 90° CW: (x, y) -> (H-1-y, x). Source (2,0) lands at (1,2).
    let mut cw = src.clone();
    cw.rotate_90_cw();
    assert_eq!(
        (cw.get_width(), cw.get_height()),
        (2, 3),
        "90° swaps the axes"
    );
    approx(
        cw.get_pixel(1, 2).r,
        2.0 / 4.0,
        1.0 / 255.0,
        "CW moved (2,0)->(1,2)",
    );

    // CCW then CW returns the original.
    let mut round = src.clone();
    round.rotate_90_ccw();
    round.rotate_90_cw();
    assert_eq!(round, src, "CCW followed by CW must be identity");

    // 180° twice is identity.
    let mut twice = src.clone();
    twice.rotate_180();
    twice.rotate_180();
    assert_eq!(twice, src, "rotating 180° twice must be identity");
}

/// blit overwrites; blend composites source-over. The difference is the whole
/// point of having both.
#[test]
fn blit_overwrites_where_blend_composites() {
    let mut src = Image::create(2, 2, ImageFormat::Rgba8);
    src.fill(Color::new(1.0, 0.0, 0.0, 0.5)); // half-transparent red

    let mut blitted = Image::create(4, 4, ImageFormat::Rgba8);
    blitted.fill(Color::new(0.0, 0.0, 1.0, 1.0)); // opaque blue
    let mut blended = blitted.clone();

    blitted.blit_rect(&src, 0, 0, 2, 2, 1, 1);
    blended.blend_rect(&src, 0, 0, 2, 2, 1, 1);

    // Blit takes the source verbatim, alpha included.
    let b = blitted.get_pixel(1, 1);
    approx(b.a, 0.5, 1.0 / 255.0, "blit copies the source alpha");
    approx(b.b, 0.0, 1.0 / 255.0, "blit discards the destination blue");

    // Blend keeps the opaque destination alpha and mixes the colours.
    let c = blended.get_pixel(1, 1);
    approx(c.a, 1.0, 1.0 / 255.0, "blending over opaque stays opaque");
    assert!(
        c.r > 0.2 && c.b > 0.2,
        "blend must mix both colours, got r={} b={}",
        c.r,
        c.b
    );

    // Pixels outside the destination rect are untouched by either.
    approx(
        blitted.get_pixel(0, 0).b,
        1.0,
        1e-6,
        "outside the rect is untouched",
    );
    approx(
        blended.get_pixel(3, 3).b,
        1.0,
        1e-6,
        "outside the rect is untouched",
    );
}

/// The mask variants gate per pixel on the mask's alpha.
#[test]
fn mask_variants_copy_only_where_the_mask_is_opaque() {
    let mut src = Image::create(2, 1, ImageFormat::Rgba8);
    src.fill(Color::new(1.0, 0.0, 0.0, 1.0));

    let mut mask = Image::create(2, 1, ImageFormat::Rgba8);
    mask.set_pixel(0, 0, Color::new(1.0, 1.0, 1.0, 1.0)); // opaque -> copy
    mask.set_pixel(1, 0, Color::new(1.0, 1.0, 1.0, 0.0)); // transparent -> skip

    let mut dst = Image::create(2, 1, ImageFormat::Rgba8);
    dst.fill(Color::new(0.0, 0.0, 1.0, 1.0));
    dst.blit_rect_mask(&src, &mask, 0, 0, 2, 1, 0, 0);

    approx(
        dst.get_pixel(0, 0).r,
        1.0,
        1.0 / 255.0,
        "masked-in pixel copied",
    );
    approx(
        dst.get_pixel(1, 0).b,
        1.0,
        1.0 / 255.0,
        "masked-out pixel must keep the destination",
    );
}

/// A blit whose rect runs off either edge clips instead of panicking or
/// wrapping to the far side.
#[test]
fn blit_clips_at_both_edges() {
    let mut src = Image::create(4, 4, ImageFormat::Rgba8);
    src.fill(Color::new(1.0, 0.0, 0.0, 1.0));
    let mut dst = Image::create(4, 4, ImageFormat::Rgba8);
    dst.fill(Color::new(0.0, 0.0, 1.0, 1.0));

    // Straddles the top-left corner and runs past the bottom-right.
    dst.blit_rect(&src, -2, -2, 8, 8, -1, -1);
    dst.blit_rect(&src, 0, 0, 4, 4, 3, 3);

    // Survived without panicking, and the far corner took the source.
    approx(
        dst.get_pixel(3, 3).r,
        1.0,
        1.0 / 255.0,
        "the in-bounds tail of a clipped blit still lands",
    );
}

/// Mipmaps halve down to 1×1 and exclude level 0.
#[test]
fn mipmap_chain_halves_to_one_by_one() {
    let src = Image::create(8, 4, ImageFormat::Rgba8);
    let chain = src.generate_mipmaps();
    let sizes: Vec<(u32, u32)> = chain
        .iter()
        .map(|m| (m.get_width(), m.get_height()))
        .collect();
    assert_eq!(
        sizes,
        vec![(4, 2), (2, 1), (1, 1)],
        "the chain halves each axis (floor, min 1) and omits the full-size level"
    );
    assert!(
        chain.iter().all(|m| m.get_format() == ImageFormat::Rgba8),
        "mipmaps keep the source format"
    );

    // A 1×1 source has no smaller level.
    assert!(Image::create(1, 1, ImageFormat::Rgba8)
        .generate_mipmaps()
        .is_empty());
}

/// fill_rect paints only inside the rect, and clips at the edges.
#[test]
fn fill_rect_paints_only_its_rect() {
    let mut img = Image::create(4, 4, ImageFormat::Rgba8);
    img.fill_rect(1, 1, 2, 2, Color::new(1.0, 1.0, 1.0, 1.0));

    assert_eq!(img.get_used_rect(), (1, 1, 2, 2), "only the rect is opaque");

    // A rect starting off-image clips rather than panicking.
    let mut edge = Image::create(4, 4, ImageFormat::Rgba8);
    edge.fill_rect(-2, -2, 3, 3, Color::new(1.0, 1.0, 1.0, 1.0));
    assert_eq!(
        edge.get_used_rect(),
        (0, 0, 1, 1),
        "the off-image part is clipped"
    );
}
