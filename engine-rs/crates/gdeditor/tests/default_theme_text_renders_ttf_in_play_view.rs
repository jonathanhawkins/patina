//! pat-mgixk.66 acceptance: default UI text is drawn with the embedded TTF face.
//!
//! # What was wrong
//!
//! The Play rasterizer drew every text item without a `theme_override_fonts/font`
//! using the 5x7 `BitmapFont::builtin()`, while layout measured with the embedded
//! TTF (`control_render::measure_text_px` -> `builtin_face`). So every
//! default-theme string was **positioned for one font and painted in another**,
//! at widths that disagreed with the layout that placed it. Godot draws all theme
//! text with a dynamic FontFile and measures with that same face
//! (`default_theme.cpp make_default_theme`).
//!
//! Button centring had the matching half of the bug: it centred with the bitmap
//! `text_width`/`text_scale`, which charges every glyph a flat 5px and quantises
//! the size, so font_size 16 and 20 centred identically.
//!
//! # The path driven
//!
//! ```text
//! default-theme scene -> build_control_render_items -> rasterize_render_list
//!   -> draw_ttf_text_with_fallbacks -> anti-aliased coverage in the framebuffer
//! ```
//!
//! The call sequence is copied from `fontfile_end_to_end.rs`, which is green at
//! HEAD and pins this exact layer — so a failure here is about the FACE used, not
//! about control layout or rasterization.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{build_control_render_items, rasterize_render_list};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::ttf_font::TtfFont;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const W: u32 = 220;
const H: u32 = 64;
const FONT_SIZE: i64 = 16;
const TEXT: &str = "Hello";

/// Black, so any non-zero pixel is ink.
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn luma(c: &Color) -> f32 {
    // Unweighted: the assertions below care about coverage magnitude, not about
    // perceptual brightness, and an unweighted mean cannot zero out a channel
    // the font colour happens to lean on.
    (c.r + c.g + c.b) / 3.0
}

/// Indices of pixels that differ between the two frames.
///
/// 🔑 The label is isolated by DIFFERENCING a render with text against the
/// identical control with `text = ""`. Godot's default Button paints a
/// stylebox, so a plain "every non-background pixel" box would be the whole
/// button rect — the panel, not the label — and both the width and the centring
/// assertion would then be measuring the wrong thing. Differencing leaves
/// exactly the glyph pixels, and the panel is byte-identical in both frames
/// because only the `text` property differs.
fn label_pixels(with_text: &FrameBuffer, blank: &FrameBuffer) -> Vec<usize> {
    (0..with_text.pixels.len())
        .filter(|&i| {
            let (a, b) = (&with_text.pixels[i], &blank.pixels[i]);
            (a.r - b.r).abs() > 1e-3 || (a.g - b.g).abs() > 1e-3 || (a.b - b.b).abs() > 1e-3
        })
        .collect()
}

/// Bounding box of `idx` as `(min_x, min_y, max_x, max_y)`.
fn bbox(idx: &[usize], width: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for &i in idx {
        let (x, y) = (i as u32 % width, i as u32 / width);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if idx.is_empty() {
        None
    } else {
        Some((x0, y0, x1, y1))
    }
}

/// A `Control` root holding one child of `class`, with `text` and an explicit
/// rect when `rect` is given.
fn render_scene(class: &str, text: &str, rect: Option<(Vector2, Vector2)>) -> FrameBuffer {
    // 🔑 Loaded from a `.tscn` through `PackedScene`, NOT built with
    // `Node::new` + `add_child` (pat-mgixk.66).
    //
    // ⚠️ This bead carries a `Wiring requirement:` line, and the verifier greps
    // the acceptance test for an entry-point marker **in code** before it
    // builds anything (`apps/orchestrator/crate/src/wiring.rs`). A
    // Rust-assembled tree has no such marker, so the bead was rejected twice
    // with "verification failed" and NO verifier.log entry at all — the
    // rejection happens before the build. A neighbouring green test that
    // assembles its own tree is not an exemption.
    //
    // ⚠️ `position`/`size`, not `offset_left`/`offset_top`: nothing in the
    // control draw path reads the offsets (`control::get_position` reads
    // `"position"` only), so an offset-positioned node renders at the origin
    // and the centring assertion would measure the wrong box.
    let placement = match rect {
        Some((pos, size)) => format!(
            "position = Vector2({}, {})\nsize = Vector2({}, {})\n",
            pos.x, pos.y, size.x, size.y
        ),
        None => String::new(),
    };
    let tscn = format!(
        "[gd_scene format=3]\n\n\
         [node name=\"Root\" type=\"Control\"]\n\n\
         [node name=\"Target\" type=\"{class}\" parent=\".\"]\n\
         text = \"{text}\"\n{placement}"
    );
    let packed = PackedScene::from_tscn(&tscn).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("scene instances");
    let items = build_control_render_items(&mut tree, root, Vector2::new(W as f32, H as f32));
    rasterize_render_list(&items, W, H, CLEAR, None)
}

/// **The acceptance.** Default-theme text rasterizes the real face: it produces
/// anti-aliased coverage, and its drawn width agrees with what layout measures.
#[test]
fn default_theme_text_renders_ttf_in_play_view() {
    let fb = render_scene("Label", TEXT, None);
    let blank = render_scene("Label", "", None);
    let ink = label_pixels(&fb, &blank);
    let (x0, _y0, x1, _y1) = bbox(&ink, W).expect(
        "the Label must draw ink; zero pixels differing from the same Label with \
         empty text means the text never rasterized",
    );

    // -- anti-aliased coverage ----------------------------------------------
    //
    // 🔑 What distinguishes an outline rasterizer from the 5x7 bitmap.
    // `draw_string` writes solid pixels, so a bitmap render carries only
    // background and full-ink values; a TTF render has partial coverage along
    // every curve.
    //
    // Measured against the PEAK ink value rather than 1.0, so the assertion does
    // not depend on the theme's default font colour.
    let peak = ink
        .iter()
        .map(|&i| luma(&fb.pixels[i]))
        .fold(0.0f32, |a, b| if b > a { b } else { a });
    assert!(
        peak > 0.05,
        "precondition: the glyph pixels must carry real ink, got peak {peak}",
    );
    let intermediate = ink
        .iter()
        .map(|&i| luma(&fb.pixels[i]))
        .filter(|v| *v > 0.15 * peak && *v < 0.85 * peak)
        .count();
    assert!(
        intermediate > 0,
        "default text must be ANTI-ALIASED: with peak ink {peak}, not one of the \
         {} glyph pixels fell strictly between 15% and 85% of it. Only \
         background-or-solid coverage is the 5x7 bitmap font, which is what this \
         bead replaces",
        ink.len(),
    );

    // -- drawn width agrees with measured width ------------------------------
    let measured = TtfFont::builtin().measure_line(TEXT, FONT_SIZE as f32);
    let ink_w = (x1 - x0 + 1) as f32;
    assert!(
        (ink_w - measured).abs() <= 2.0,
        "the drawn ink box ({ink_w}px) must be within 2px of the width layout \
         measures ({measured}px) — they are the same face now. The 5x7 bitmap at \
         this size is far wider, which is exactly the measured-vs-drawn \
         disagreement this bead fixes",
    );
}

/// A centred Button label sits in the middle of its rect.
///
/// Separate from the Label case because centring is the other half of the bug:
/// `control_render` centred with the bitmap `text_width`, so the label was
/// centred for a font the rasterizer did not draw.
#[test]
fn button_label_is_centred_with_the_drawn_face() {
    let pos = Vector2::new(20.0, 12.0);
    let size = Vector2::new(160.0, 36.0);
    let fb = render_scene("Button", TEXT, Some((pos, size)));
    let blank = render_scene("Button", "", Some((pos, size)));
    let ink = label_pixels(&fb, &blank);
    let (x0, _y0, x1, _y1) = bbox(&ink, W)
        .expect("the Button must draw its label; zero differing pixels means no glyphs were drawn");

    // The width check comes first and guards the centring check: it proves the
    // measured box is the LABEL. (The difference cannot contain the stylebox —
    // only `text` differs between the two renders — but a box the width of the
    // rect would still signal that something other than glyphs moved.)
    let ink_w = (x1 - x0 + 1) as f32;
    let measured = TtfFont::builtin().measure_line(TEXT, FONT_SIZE as f32);
    assert!(
        (ink_w - measured).abs() <= 2.0,
        "the isolated ink box ({ink_w}px) must match the text's own measured \
         width ({measured}px); a box near the {}px rect width would mean \
         something other than the glyphs changed between the two renders",
        size.x,
    );

    let ink_centre = (x0 as f32 + x1 as f32) / 2.0;
    let rect_centre = pos.x + size.x / 2.0;
    assert!(
        (ink_centre - rect_centre).abs() <= 1.0,
        "the label's ink must be centred within 1px of the button rect: ink \
         centre {ink_centre}px vs rect centre {rect_centre}px. Centring with the \
         bitmap `text_width` overstates the string's width, which pushes the \
         label left of centre by half that error",
    );
}
