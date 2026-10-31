//! pat-mgixk.49 acceptance: a headless run renders the 3D scene and writes it out.
//!
//! `render_scene_frame` existed but had only a test caller — `run_main_loop`
//! stepped frames and dumped JSON, and `grep render|ppm|png|screenshot` in
//! `patina-runner/src/main.rs` returned nothing. So outside the editor process
//! no 3D pixel was produced, and a headless run had no way to show what it drew.
//!
//! This drives the whole chain the wiring requirement names — `.tscn` load,
//! `MainLoop` step, adapter `render_frame`, image on disk — and asserts on the
//! **written file**, decoded back, rather than on the in-memory frame. An
//! assertion on the frame alone would pass with the image output broken, which
//! is the half a headless runner is actually judged on.
//!
//! ⚠️ Two things here exist to stop it passing by accident:
//!
//! * The background is read from the EMPTY scene's own frame instead of being
//!   assumed. The renderer works in linear HDR and tonemaps on the way out, so a
//!   cleared pixel is not the literal clear colour; hard-coding one would test a
//!   guess about the post-process. (Same reasoning as the sibling
//!   `runner_renders_3d_scene_frame`.)
//! * The empty scene is asserted to be UNIFORM. That is the fixture guard for
//!   the coverage comparison below: if the background frame were not uniform,
//!   "coverage > 0" would be true of nothing in particular.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use patina_runner::render3d::SceneRenderer3D;

const W: u32 = 96;
const H: u32 = 72;

fn tree_from(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// A red cube square in front of the camera, lit by a directional light.
///
/// `mesh = "primitive:BoxMesh"` rather than hand-authored geometry: it comes
/// from the repo's own generator, whose winding is already proven to survive
/// back-face culling (`solid_cube_produces_filled_pixels`). A fixture that
/// rendered nothing would make "the runner cannot render" and "the model is
/// invisible" indistinguishable.
///
/// `albedo_color` is what makes it RED: `render_server_3d`'s standard-material
/// path activates only for a node with an explicit `albedo_color`, so without
/// this line the cube renders in the default shade and the hue assertion below
/// would be testing the fallback instead of the scene's own material.
const RED_BOX_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Sun" type="DirectionalLight3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 4, 0)

[node name="Cube" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"
albedo_color = Color(1, 0, 0, 1)
"#;

/// The same camera with nothing to look at.
const EMPTY_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true
"#;

fn out_path(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "patina_runner_frame_{}_{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("frame.png").to_string_lossy().into_owned()
}

/// Steps the scene the way a run does, then renders through the persistent
/// renderer and writes the frame.
///
/// The `MainLoop` step is not decoration: the wiring requirement is `.tscn` ->
/// `MainLoop` step -> adapter `render_frame` -> image, and a render that never
/// saw a stepped tree would not be the frame a running game shows.
fn run_and_write(tscn: &str, tag: &str) -> (String, gdrender2d::Texture2D) {
    let mut ml = MainLoop::new(tree_from(tscn));
    // Two frames at 60Hz: enough for `_ready`/`_process` to have run and for any
    // first-frame initialisation to settle, without depending on a long run.
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    let mut renderer = SceneRenderer3D::new(W, H);
    renderer.render(ml.tree());

    let path = out_path(tag);
    renderer
        .last_frame()
        .expect("a frame was just rendered")
        .save_png(&path)
        .expect("the frame must be writable as a PNG");

    let decoded = gdrender2d::load_png(&path).unwrap_or_else(|| {
        panic!("the runner wrote {path}, but it does not decode as a PNG — the \
                image output is the half a headless run is judged on")
    });
    (path, decoded)
}

/// **The acceptance test (pat-mgixk.49).**
#[test]
fn runner_writes_rendered_3d_frame() {
    gdobject::class_db::register_3d_classes();

    // -- a scene with no 3D content writes the clear colour only -------------
    let (empty_path, empty) = run_and_write(EMPTY_SCENE, "empty");
    assert_eq!(
        (empty.width, empty.height),
        (W, H),
        "the written image must be the requested size; got {}x{} at {empty_path}",
        empty.width,
        empty.height
    );

    let background = empty.get_pixel(0, 0);
    let differing = empty
        .pixels
        .iter()
        .filter(|c| {
            (c.r - background.r).abs() > 1.0 / 255.0
                || (c.g - background.g).abs() > 1.0 / 255.0
                || (c.b - background.b).abs() > 1.0 / 255.0
        })
        .count();
    assert_eq!(
        differing, 0,
        "a camera-only scene must write a UNIFORM background image; {differing} \
         pixels differ. This is also the guard for the coverage assertion below — \
         a non-uniform background would make it meaningless"
    );

    // -- the red box scene covers pixels, and its centre is red --------------
    let (box_path, drawn) = run_and_write(RED_BOX_SCENE, "redbox");
    let covered = drawn
        .pixels
        .iter()
        .filter(|c| {
            (c.r - background.r).abs() > 1.0 / 255.0
                || (c.g - background.g).abs() > 1.0 / 255.0
                || (c.b - background.b).abs() > 1.0 / 255.0
        })
        .count();
    assert!(
        covered > 0,
        "a lit MeshInstance3D must reach the WRITTEN image; only {covered} pixels \
         differ from the background in {box_path}. Zero means the run produced \
         no scene pixels, which is the bug this bead is about"
    );

    let centre = drawn.get_pixel(W / 2, H / 2);
    assert!(
        centre.r > centre.g + 0.1 && centre.r > centre.b + 0.1,
        "the cube fills the centre of frame and its `albedo_color` is red, so the \
         centre pixel of the written image must be red-dominant; got \
         r={:.3} g={:.3} b={:.3}. Equal channels mean the material never reached \
         the renderer and the cube drew in the default shade",
        centre.r,
        centre.g,
        centre.b
    );
}
