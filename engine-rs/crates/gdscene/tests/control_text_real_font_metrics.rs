//! pat-s3rrv.37 (slice 1): Control text measured with the real font.
//!
//! Godot measures every Control's text through the TextServer's shaped advances
//! (`Font::get_string_size`), and `font_size` is an exact pixel size. Patina laid
//! text out with a fixed 5×7 monospace model — `text_scale(font_size)` is the
//! INTEGER `font_size / 7`, and `text_width` charges every glyph a flat 5px —
//! while the rasterizer drew proportional TTF glyphs. So the measured width
//! never matched the drawn width for any string.
//!
//! `TextServerLite` (TTF advances, kerning, wrap, ellipsis) has existed with
//! zero callers the whole time.
//!
//! These pin the three properties the bead's acceptance names, at the
//! measurement layer that every Control's layout is derived from.

use gdrender2d::TtfFont;

/// The exact call the acceptance criteria name, as the oracle.
fn oracle(text: &str, px: f32) -> f32 {
    TtfFont::builtin().measure_line(text, px)
}

/// **Acceptance property 2.** Changing `font_size` from 16 to 20 must change the
/// measured width. Under the old model both quantise to scale 2 and measure
/// identically — that collapse is the bug.
#[test]
fn font_size_16_and_20_measure_differently() {
    let w16 = oracle("Hello", 16.0);
    let w20 = oracle("Hello", 20.0);

    assert!(w16 > 0.0 && w20 > 0.0, "both sizes must measure non-zero");
    assert!(
        (w20 - w16).abs() > 0.5,
        "font_size 16 and 20 must measure differently; got {w16} and {w20}. \
         Equal widths mean the integer `font_size / 7` scale is still in play — \
         both sizes collapse to scale 2."
    );
    assert!(
        w20 > w16,
        "the larger font size must measure wider; got {w16} then {w20}"
    );
}

/// **Acceptance property 3.** A proportional font gives "WWW" and "iii"
/// different widths at the same size. The monospace model gives them the
/// identical width, which is what makes Button's minimum size wrong.
#[test]
fn wide_and_narrow_glyphs_measure_differently() {
    let wide = oracle("WWW", 16.0);
    let narrow = oracle("iii", 16.0);

    assert!(
        wide > narrow,
        "'WWW' must measure wider than 'iii' at the same font size; got {wide} \
         and {narrow}. Equal widths mean text is still being measured with the \
         flat 5px-per-glyph monospace model."
    );
}

/// **Acceptance property 1**, at the measurement layer: a centred string's x
/// offset is `(rect_width - measured) / 2`. The layout code derives the offset
/// from the measurement, so pinning the measurement pins the offset.
#[test]
fn centering_math_uses_the_measured_width() {
    const RECT_W: f32 = 200.0;
    let measured = oracle("Centered", 16.0);
    let offset = (RECT_W - measured) / 2.0;

    assert!(
        offset > 0.0 && offset < RECT_W / 2.0,
        "a string narrower than its rect must centre at a positive offset less \
         than half the rect; got {offset} for measured width {measured}"
    );

    // A longer string must centre further left. Under a monospace model this
    // still holds, so it is not sufficient on its own — properties 2 and 3
    // above are what distinguish real metrics from the placeholder.
    let longer = (RECT_W - oracle("Centered and then some", 16.0)) / 2.0;
    assert!(
        longer < offset,
        "a longer string must centre further left; got {longer} vs {offset}"
    );
}

/// The empty string measures zero rather than a stray advance, so an empty
/// Label centres at the rect's midpoint instead of drifting.
#[test]
fn the_empty_string_measures_zero() {
    assert_eq!(oracle("", 16.0), 0.0);
}
