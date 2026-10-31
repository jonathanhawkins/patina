//! pat-mgixk.46 acceptance — the runner emits a real 2D canvas frame as a PNG.
//!
//! Godot ends every `Main::iteration` with `RenderingServer::draw`, so the root
//! viewport's canvas is rendered wherever the game runs — not only in the editor
//! (`main/main.cpp`).
//!
//! # What this bead actually still needed
//!
//! Most of the runtime 2D path was already in place when this bead was picked
//! up, by two other pieces of work:
//!
//! * pat-mgixk.45 moved the render list out of the editor. `collect_canvas_items`,
//!   `rasterize_render_list*`, `camera2d_view_transform`, `apply_stretch_transform`
//!   and `render_canvas_from_list` now live in `gdscene::canvas_render`, with
//!   `TextureCache` in `gdscene::texture_cache` — none of them in `gdeditor`.
//! * pat-mgixk.28 added `--frame-out` / `--frame-size` to the runner and wired
//!   the four-call path (collect, camera, rasterize, encode), including a
//!   `TextureCache` rooted at the project directory so `res://` sprites draw
//!   real pixels.
//!
//! What was missing was the format this bead's acceptance asks for: the writer
//! only emitted a binary PPM. So the change under test is PNG output, and this
//! file is what proves the whole path end to end through the real binary.
//!
//! # The path driven
//!
//! ```text
//! patina-runner <project_dir> --frames 3 --frame-size 96x96 --frame-out out.png
//!   -> main -> project_boot::boot_project -> run_project_main_loop
//!      -> write_frame_out
//!     -> gdscene::canvas_render::{collect_canvas_items, camera2d_view_transform,
//!        apply_camera2d_view, rasterize_render_list}
//!       -> FrameBuffer::save_png -> a PNG on disk
//! ```
//!
//! ⚠️ It spawns the real binary rather than calling the writer, because the
//! point is that a *running game* produces pixels. A direct call would pass with
//! the CLI flag unwired.
//!
//! ⚠️ It enters by PROJECT DIRECTORY, not by loose `.tscn`, and that is load
//! bearing. `project_boot` seeds the ClassDB (`project_boot.rs:379`), so
//! `Node::is_class("CanvasItem")` answers true and
//! `gdscene::canvas_render::collect_canvas_items` can see the scene. The
//! loose-`.tscn` entry point in `main.rs` never seeds it — `ClassDB::new()`
//! starts empty and nothing fills it lazily — so `collect_canvas_items` filters
//! the whole scene out and the SAME fixture renders as nothing but the clear
//! colour there. Measured: 256 rect pixels and 64 sprite pixels by project
//! directory, 0 of 9216 by loose `.tscn`. That gap is a separate defect in a
//! different entry point and is reported rather than fixed here; booting by
//! project directory is also what a shipped game does.
//!
//! ⚠️ Every assertion is a pixel COUNT plus a centroid RELATION, never an exact
//! coordinate. The fixture declares no `Camera2D`, and `camera2d_view_transform`
//! is still consulted, so the drawn content may be uniformly translated by a
//! view this test has no business predicting. Probing fixed coordinates would
//! make the test fail whenever that convention changed, which measures the
//! convention rather than "did the game draw". The sibling
//! `runner_frame_out.rs` uses centroids for the same reason.

use std::path::{Path, PathBuf};
use std::process::Command;

use gdrender2d::texture::load_png;

/// Framebuffer size. Big enough that the two shapes cannot overlap after any
/// plausible view translation, small enough to stay fast.
const SIZE: u32 = 96;

/// The fixture's `ColorRect` is 16x16 = 256 px. The bar allows for edge
/// coverage and any clipping at the frame border.
const MIN_RECT_PX: usize = 150;

/// The sprite texture is 8x8 = 64 px. Same bar the sibling acceptance uses for
/// its own 8x8 texture.
const MIN_SPRITE_PX: usize = 32;

/// The fixture PROJECT directory, holding `project.godot`, `main.tscn` and the
/// `res://sprite.png` the scene references.
fn fixture_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/canvas_frame"
    ))
}

/// A unique output path. `std::env::temp_dir` rather than a tempdir crate:
/// patina-runner has no dev-dependencies and this test is not worth adding one.
fn out_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "patina_runner_canvas_frame_{}.png",
        std::process::id()
    ))
}

/// The fixture's `ColorRect` colour, (51, 153, 204) in 8 bits.
fn is_rect(c: &gdcore::math::Color) -> bool {
    let (r, g, b) = (c.r * 255.0, c.g * 255.0, c.b * 255.0);
    (40.0..70.0).contains(&r) && (135.0..170.0).contains(&g) && (185.0..220.0).contains(&b)
}

/// The sprite texture's magenta, (255, 0, 255).
///
/// ⚠️ Disjoint from [`is_rect`] by construction: this needs red AND blue high
/// with green low, the rect needs red low. No pixel can satisfy both, so neither
/// count can be met by the other shape's pixels.
fn is_sprite(c: &gdcore::math::Color) -> bool {
    c.r > 0.78 && c.g < 0.24 && c.b > 0.78
}

/// `(count, mean_x, mean_y)` over the pixels a predicate picks.
fn centroid(
    tex: &gdrender2d::texture::Texture2D,
    pick: impl Fn(&gdcore::math::Color) -> bool,
) -> (usize, f32, f32) {
    let (mut n, mut sx, mut sy) = (0usize, 0.0f32, 0.0f32);
    for (i, px) in tex.pixels.iter().enumerate() {
        if pick(px) {
            n += 1;
            sx += (i as u32 % tex.width) as f32;
            sy += (i as u32 / tex.width) as f32;
        }
    }
    if n == 0 {
        return (0, f32::NAN, f32::NAN);
    }
    (n, sx / n as f32, sy / n as f32)
}

/// Runs the real binary over the fixture scene and returns the decoded PNG.
fn render_fixture(out: &Path) -> gdrender2d::texture::Texture2D {
    let exe = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(exe)
        .arg(fixture_dir())
        .arg("--frames")
        .arg("3")
        .arg("--frame-size")
        .arg(format!("{SIZE}x{SIZE}"))
        .arg("--frame-out")
        .arg(out)
        .output()
        .expect("the runner binary must be spawnable");

    assert!(
        output.status.success(),
        "the runner must exit cleanly.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.exists(),
        "`--frame-out` must have written {}. stderr:\n{}",
        out.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    load_png(&out.to_string_lossy()).unwrap_or_else(|| {
        panic!(
            "the written file must decode as a PNG; `--frame-out` used to emit \
             only a binary PPM, which this loader rejects"
        )
    })
}

/// **The acceptance test.**
#[test]
fn runner_writes_2d_canvas_frame() {
    let out = out_path();
    let tex = render_fixture(&out);

    assert_eq!(
        (tex.width, tex.height),
        (SIZE, SIZE),
        "the PNG must be the size `--frame-size` asked for"
    );

    let (rect_n, rect_x, rect_y) = centroid(&tex, is_rect);
    let (sprite_n, sprite_x, _sprite_y) = centroid(&tex, is_sprite);
    let lit = tex.pixels.iter().filter(|c| c.r + c.g + c.b > 0.01).count();

    // -- the ColorRect rasterized -------------------------------------------
    assert!(
        rect_n >= MIN_RECT_PX,
        "the ColorRect must reach the framebuffer: found {rect_n} pixels of its \
         colour for a 16x16 rect, wanted >= {MIN_RECT_PX}. ZERO means nothing \
         rasterized at all — the whole frame is the clear colour, which is the \
         pre-pat-mgixk.28 behaviour of having no 2D draw path."
    );

    // -- the Sprite2D drew TEXTURE pixels -----------------------------------
    //
    // ⚠️ Counting magenta rather than "not the clear colour" is deliberate. A
    // sprite that drew as an untextured white quad is exactly what a failed
    // `res://` texture load looks like, and "not black" would accept it. Only a
    // successful decode of `res://sprite.png` produces this colour.
    assert!(
        sprite_n >= MIN_SPRITE_PX,
        "the sprite must draw its texture's magenta: found {sprite_n} pixels for \
         an 8x8 texture, wanted >= {MIN_SPRITE_PX}. Zero with the rect present \
         means the quad drew but the texture never loaded."
    );

    // -- they are two distinct shapes, in the authored order ----------------
    //
    // A relation, not a coordinate: the fixture puts the rect at x 8..24 and the
    // sprite near x 48, so the sprite is right of the rect under ANY uniform
    // view translation. This is what survives a change of camera convention.
    assert!(
        sprite_x > rect_x + 10.0,
        "the sprite must be drawn to the right of the rect, as the fixture \
         places them: rect centroid x = {rect_x:.1}, sprite centroid x = \
         {sprite_x:.1}. Equal centroids would mean one shape's pixels satisfied \
         both predicates."
    );
    assert!(
        rect_y.is_finite(),
        "the rect centroid must be a real position, not NaN"
    );

    // -- and the frame is not simply flooded --------------------------------
    //
    // The control: every assertion above counts coloured pixels, so a frame
    // filled entirely with one colour could satisfy one of them. Most of a
    // 96x96 frame holding two small shapes must still be the clear colour.
    assert!(
        lit < (SIZE * SIZE) as usize / 2,
        "most of the frame must still be the clear colour; {lit} of {} pixels \
         are lit, which means something flooded the buffer rather than drawing \
         two small shapes",
        SIZE * SIZE
    );

    let _ = std::fs::remove_file(&out);
}
