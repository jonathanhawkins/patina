//! pat-s3rrv.199: glyph outlines become flattened contours.
//!
//! `TextMesh::generate_from_contours` could always extrude glyph contours and
//! had **zero non-test callers**, because nothing in the tree could produce a
//! contour — there was no outline source at all. These tests cover the source.
//!
//! The font is this crate's own `test_assets/Cantarell-Regular.ttf`, already
//! used by the font importer tests.

use gdresource::glyph_outline::{text_outline, TextOutline, DEFAULT_CURVE_STEP};

fn font() -> Vec<u8> {
    let path = format!(
        "{}/test_assets/Cantarell-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn outline(text: &str) -> TextOutline {
    text_outline(&font(), text, 16.0, 0.01, DEFAULT_CURVE_STEP).expect("the font parses")
}

/// A glyph with curves produces closed polygons.
#[test]
fn a_glyph_produces_flattened_contours() {
    let out = outline("A");
    assert!(!out.contours.is_empty(), "'A' must produce geometry");
    for c in &out.contours {
        assert!(
            c.len() >= 3,
            "a contour needs at least three points to bound an area, got {}",
            c.len()
        );
    }
    assert!(out.advance > 0.0, "'A' advances the pen");
}

/// ⚠️ The contract with `generate_from_contours`: contours are closed
/// **implicitly**.
///
/// TrueType outlines routinely end by returning to their start point. Leaving
/// that duplicate in produces a zero-area triangle at the seam of every glyph —
/// which triangulates without error and renders as a sliver.
#[test]
fn a_contour_does_not_repeat_its_first_point() {
    let out = outline("Bog");
    assert!(!out.contours.is_empty());
    for (i, c) in out.contours.iter().enumerate() {
        let first = c[0];
        let last = c[c.len() - 1];
        assert!(
            (first.x - last.x).abs() > 1e-6 || (first.y - last.y).abs() > 1e-6,
            "contour {i} repeats its first point ({first:?} == {last:?}); \
             the loop must be implicit"
        );
    }
}

/// A letter with a counter yields the counter as its own contour.
///
/// This is what proves contours are separated rather than concatenated into one
/// run-on polygon — and it is also the shape of the known limitation: nothing
/// marks the inner loop as a hole, so `generate_from_contours` extrudes it as
/// solid. Recording it here so the behaviour is deliberate, not accidental.
#[test]
fn a_counter_arrives_as_its_own_contour() {
    let out = outline("o");
    assert_eq!(
        out.contours.len(),
        2,
        "'o' is an outer loop plus its counter, got {} contour(s)",
        out.contours.len()
    );
}

/// ⚠️ A space has no outline but must still move the pen.
///
/// `outline_glyph` returns `None` for a glyph with no contours, which is not an
/// error. Treating it as one would collapse every following glyph onto the
/// space's position — the whole run would overlap into an unreadable pile.
#[test]
fn a_space_advances_the_pen_without_geometry() {
    let single = outline("A");
    let spaced = outline("A A");

    assert!(
        spaced.advance > single.advance * 2.0,
        "a space must contribute advance (one 'A' {}, 'A A' {})",
        single.advance,
        spaced.advance
    );
    // Two 'A's worth of contours, and nothing from the space itself.
    assert_eq!(spaced.contours.len(), single.contours.len() * 2);
}

/// Glyphs lay out left to right rather than stacking on the origin.
#[test]
fn glyphs_advance_along_x() {
    let out = outline("AA");
    let max_x = out
        .contours
        .iter()
        .flatten()
        .map(|p| p.x)
        .fold(f32::MIN, f32::max);
    let single = outline("A");
    let single_max = single
        .contours
        .iter()
        .flatten()
        .map(|p| p.x)
        .fold(f32::MIN, f32::max);

    assert!(
        max_x > single_max,
        "the second 'A' must sit to the right of the first ({max_x} vs {single_max})"
    );
}

/// A finer `curve_step` subdivides curves more.
///
/// Asserted as a strict inequality on a curved glyph: `curve_step` that did
/// nothing would give identical counts, which is the failure worth catching.
#[test]
fn a_finer_curve_step_produces_more_points() {
    let f = font();
    let coarse = text_outline(&f, "o", 16.0, 0.01, 2.0).expect("parses");
    let fine = text_outline(&f, "o", 16.0, 0.01, 0.01).expect("parses");

    let count = |o: &TextOutline| o.contours.iter().map(|c| c.len()).sum::<usize>();
    assert!(
        count(&fine) > count(&coarse),
        "a finer tolerance must subdivide more (fine {}, coarse {})",
        count(&fine),
        count(&coarse)
    );
}

/// `font_size * pixel_size` scales the glyph, independent of the font's grid.
#[test]
fn font_size_and_pixel_size_scale_the_glyph() {
    let f = font();
    let small = text_outline(&f, "A", 16.0, 0.01, DEFAULT_CURVE_STEP).expect("parses");
    let large = text_outline(&f, "A", 32.0, 0.01, DEFAULT_CURVE_STEP).expect("parses");

    let height = |o: &TextOutline| {
        let max = o
            .contours
            .iter()
            .flatten()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        let min = o
            .contours
            .iter()
            .flatten()
            .map(|p| p.y)
            .fold(f32::MAX, f32::min);
        max - min
    };

    let ratio = height(&large) / height(&small);
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "doubling font_size must double the glyph, got ratio {ratio}"
    );
    assert!(
        (large.advance / small.advance - 2.0).abs() < 0.05,
        "the advance scales with it too"
    );
}

/// Unreadable bytes are `None`, not silently empty text.
///
/// An empty `TextOutline` would render as nothing and look exactly like a valid
/// font containing an empty string — the caller could not tell a broken font
/// from an intentionally blank label.
#[test]
fn an_unparseable_font_is_none_rather_than_empty() {
    assert!(text_outline(b"not a font", "A", 16.0, 0.01, DEFAULT_CURVE_STEP).is_none());
    assert!(text_outline(&[], "A", 16.0, 0.01, DEFAULT_CURVE_STEP).is_none());
}

/// Empty text is a valid, empty result — distinct from a broken font.
#[test]
fn empty_text_is_an_empty_outline_not_a_failure() {
    let out = text_outline(&font(), "", 16.0, 0.01, DEFAULT_CURVE_STEP).expect("the font parses");
    assert!(out.contours.is_empty());
    assert_eq!(out.advance, 0.0);
}

/// Godot's subdivision clamp, transcribed and guarded.
///
/// `primitive_meshes.cpp` computes `step = CLAMP(curve_step / chord, 0.01,
/// 0.5)` and walks `t` by it — so a curve is **never** fewer than 2 segments
/// and never more than 100, whatever the tolerance. Both ends matter:
///
/// - Without the upper clamp on `step` (0.5), an absurd `curve_step` collapses
///   every curve to a single chord and a round glyph becomes a lozenge.
/// - Without the lower clamp (0.01), a tolerance near zero asks for unbounded
///   subdivision and allocates without limit.
///
/// ⚠️ Also pins that the tolerance is measured in **pixel** space. Applied in
/// world units it would be ~100x too coarse at a typical `pixel_size` of 0.01,
/// putting every curve on the 2-segment floor — which is what the first
/// assertion here would then read as a pass, so the fine case is asserted too.
#[test]
fn curve_subdivision_is_clamped_at_both_ends() {
    let f = font();

    let coarse = text_outline(&f, "o", 16.0, 0.01, 10_000.0).expect("parses");
    for c in &coarse.contours {
        assert!(
            c.len() >= 6,
            "even an absurd tolerance must keep 2 segments per curve, got {} points",
            c.len()
        );
    }

    let fine = text_outline(&f, "o", 16.0, 0.01, 0.0).expect("parses");
    for c in &fine.contours {
        assert!(
            c.len() <= 100 * 8,
            "a zero tolerance must stay bounded by the 0.01 t-step, got {} points",
            c.len()
        );
    }

    // And the two really are different, so neither is silently pinned.
    let count = |o: &TextOutline| o.contours.iter().map(|c| c.len()).sum::<usize>();
    assert!(count(&fine) > count(&coarse));
}
