//! pat-mgixk.49 acceptance: a headless run renders the 3D scene every frame and
//! writes it out.
//!
//! Godot's `Main::iteration` asks the RenderingServer to draw every frame, from
//! the current `Camera3D` (`main/main.cpp`). This runner stepped frames and
//! rendered nothing: `grep render|ppm|png|screenshot` in `patina-runner/src/main.rs`
//! found no 3D call, and `render_scene_frame` — the one bridge to
//! `RenderServer3DAdapter` — had only a test caller. Outside the editor process
//! no 3D pixel was produced at all.
//!
//! # The path driven
//!
//! ```text
//! patina-runner <scene.tscn> --frames 3 --frame-size 96x72 --render-to out.png
//!   -> main -> PackedScene::from_tscn -> add_packed_scene_to_tree
//!      -> MainLoop::new -> run_main_loop
//!         -> SceneRenderer3D::render (once per frame, ONE adapter for the run)
//!            -> RenderServer3DAdapter::render_frame
//!      -> write_render_to -> a PNG on disk
//! ```
//!
//! ⚠️ It spawns the REAL binary rather than calling `SceneRenderer3D` directly.
//! That matters more than usual here: `SceneRenderer3D` and the frame type it
//! returns already existed and were already green, so a test that constructed
//! one itself would pass with the frame loop and the CLI flag completely
//! unwired — which is the entire remaining gap this bead names. Spawning the
//! binary is the only way the assertion can tell "a running game draws" from
//! "the library can draw if someone calls it".
//!
//! ⚠️ It asserts on the written FILE, decoded back, not on an in-memory frame.
//! The image is the half a headless run is judged on; an assertion on pixels
//! the process still holds would pass with the encoder broken.
//!
//! ⚠️ The background is read from the camera-only scene's own frame instead of
//! being assumed. The renderer works in linear HDR and tonemaps on the way out,
//! so a cleared pixel is not the literal clear colour; hard-coding one would
//! test a guess about the post-process. (Same reasoning as the sibling
//! `runner_renders_3d_scene_frame`.) That frame is also asserted to be UNIFORM,
//! which is what makes the "coverage > 0" comparison below mean anything: a
//! non-uniform background would satisfy it with no cube in the scene.

use std::path::{Path, PathBuf};
use std::process::Command;

use gdrender2d::texture::{load_png, Texture2D};

/// Frame size. Small enough to stay fast; the cube still fills the centre.
const W: u32 = 96;
const H: u32 = 72;

/// A red cube in front of the camera, lit by a directional light.
///
/// `mesh = "primitive:BoxMesh"` rather than hand-authored geometry: it comes
/// from the repo's own generator, whose winding is already proven to survive
/// back-face culling (`solid_cube_produces_filled_pixels`). A fixture that
/// rendered nothing would make "the runner cannot render" and "the model is
/// invisible" indistinguishable.
///
/// `albedo_color` is what makes it RED: `render_server_3d`'s standard-material
/// path activates only for a node with an explicit `albedo_color`, so without
/// that line the cube renders in the default shade and the hue assertion below
/// would be testing the fallback instead of the scene's own material.
fn red_box_scene() -> PathBuf {
    fixture("red_box.tscn")
}

/// The same camera with nothing to look at.
fn camera_only_scene() -> PathBuf {
    fixture("camera_only.tscn")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/render3d_frame"
    ))
    .join(name)
}

/// A unique output path per scene. `std::env::temp_dir` rather than a tempdir
/// crate: patina-runner has no dev-dependencies and this test is not worth
/// adding one.
fn out_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "patina_runner_render3d_{}_{}.png",
        tag,
        std::process::id()
    ))
}

/// Runs the real binary over `scene` with `--render-to` and decodes the PNG.
fn render_through_runner(scene: &Path, tag: &str) -> (PathBuf, Texture2D) {
    let out = out_path(tag);
    let _ = std::fs::remove_file(&out);

    let exe = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(exe)
        .arg(scene)
        // More than one frame on purpose: the renderer is built ONCE before the
        // loop and drawn from after every step, so a run that only survives a
        // single frame (a per-frame adapter that cannot be rebuilt, a borrow
        // that only works once) fails here rather than passing by accident.
        .arg("--frames")
        .arg("3")
        .arg("--frame-size")
        .arg(format!("{W}x{H}"))
        .arg("--render-to")
        .arg(&out)
        .output()
        .expect("the runner binary must be spawnable");

    assert!(
        output.status.success(),
        "the runner must exit cleanly with --render-to.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.exists(),
        "`--render-to` must have written {}. An unknown-argument error here means \
         the flag is not wired into the runner at all, which is this bead's gap. \
         stderr:\n{}",
        out.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    let tex = load_png(&out.to_string_lossy()).unwrap_or_else(|| {
        panic!(
            "the runner wrote {}, but it does not decode as a PNG — the image \
             output is the half a headless run is judged on",
            out.display()
        )
    });
    (out, tex)
}

/// Counts pixels differing from `background` by more than a quantisation step.
fn differing(tex: &Texture2D, background: gdcore::math::Color) -> usize {
    tex.pixels
        .iter()
        .filter(|c| {
            (c.r - background.r).abs() > 1.0 / 255.0
                || (c.g - background.g).abs() > 1.0 / 255.0
                || (c.b - background.b).abs() > 1.0 / 255.0
        })
        .count()
}

/// **The acceptance test (pat-mgixk.49).**
#[test]
fn runner_writes_rendered_3d_frame() {
    // -- a scene with no 3D content writes the clear colour only -------------
    let (empty_path, empty) = render_through_runner(&camera_only_scene(), "empty");
    assert_eq!(
        (empty.width, empty.height),
        (W, H),
        "the written image must be the size `--frame-size` asked for; got {}x{} \
         at {}",
        empty.width,
        empty.height,
        empty_path.display()
    );

    let background = empty.get_pixel(0, 0);
    let empty_differing = differing(&empty, background);
    assert_eq!(
        empty_differing, 0,
        "a camera-only scene must write a UNIFORM background image; \
         {empty_differing} pixels differ. This is also the guard for the \
         coverage assertion below — a non-uniform background would make it \
         meaningless"
    );

    // -- the red box scene covers pixels, and its centre is red --------------
    let (box_path, drawn) = render_through_runner(&red_box_scene(), "redbox");
    let covered = differing(&drawn, background);
    assert!(
        covered > 0,
        "a lit MeshInstance3D must reach the WRITTEN image; only {covered} \
         pixels differ from the background in {}. Zero means the run produced \
         no scene pixels, which is the bug this bead is about",
        box_path.display()
    );

    let centre = drawn.get_pixel(W / 2, H / 2);
    assert!(
        centre.r > centre.g + 0.1 && centre.r > centre.b + 0.1,
        "the cube fills the centre of frame and its `albedo_color` is red, so \
         the centre pixel of the written image must be red-dominant; got \
         r={:.3} g={:.3} b={:.3}. Equal channels mean the material never \
         reached the renderer and the cube drew in the default shade",
        centre.r,
        centre.g,
        centre.b
    );
}
