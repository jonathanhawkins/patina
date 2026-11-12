//! pat-mgixk.49: the web runtime renders the 3D scene, not a flat clear.
//!
//! Godot's web export runs the ordinary `Main::iteration`, which asks the
//! RenderingServer to draw every frame from the current `Camera3D`
//! (`platform/web/web_main.cpp` -> `main/main.cpp`). patina-web's entire
//! presentation surface was `WebRenderer::present_clear(r, g, b)` — a solid
//! colour fill with no scene submission, camera or instance list — so a 3D game
//! in the browser produced no scene pixel at all. The 2D side had had
//! `render_frame_rgba` since pat-mgixk.74; the 3D side had nothing.
//!
//! # The path driven
//!
//! ```text
//! PackedScene::from_tscn -> add_packed_scene_to_tree -> WebEngine::new
//!   -> WebEngine::frame(now_ms)        (the requestAnimationFrame tick)
//!   -> WebEngine::render_3d_rgba       (the same adapter the runner draws from)
//!      -> RenderServer3DAdapter::render_frame -> RGBA bytes for putImageData
//! ```
//!
//! ⚠️ It steps through [`WebEngine::frame`], the animation-frame entry point,
//! rather than poking `MainLoop::step`. The first `frame` call only establishes
//! the time origin and returns `None`, which is why the loop below keeps
//! advancing the timestamp until a step has actually happened — asserting on a
//! buffer rendered before any step would be asserting on frame zero.
//!
//! ⚠️ The background comes from the CAMERA-ONLY scene's own frame rather than
//! being assumed to be black. The renderer works in linear HDR and tonemaps on
//! the way out, so a cleared pixel is not the literal clear colour; hard-coding
//! one would test a guess about the post-process. That frame is also asserted to
//! be uniform, which is what makes the coverage comparison mean anything.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use patina_web::engine::WebEngine;

/// Canvas backing size. Small enough to stay fast; the cube still fills the
/// centre.
const W: u32 = 96;
const H: u32 = 72;

/// A red cube in front of the camera, lit by a directional light.
///
/// `mesh = "primitive:BoxMesh"` comes from the repo's own generator, whose
/// winding is already proven to survive back-face culling
/// (`solid_cube_produces_filled_pixels`); hand-authored geometry that rendered
/// nothing would make "the web runtime cannot render" and "the model is
/// invisible" indistinguishable. `albedo_color` is what makes it RED:
/// `render_server_3d`'s standard-material path activates only for a node with an
/// explicit `albedo_color`, so without it the cube draws in the default shade
/// and the hue assertion would be testing the fallback.
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

/// The browser clock the first tick establishes as the origin.
const ORIGIN: f64 = 1_000.0;

/// Builds the engine, steps it through the animation-frame tick, and returns the
/// 3D frame as RGBA bytes.
fn render_through_tick(tscn: &str) -> Vec<u8> {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    // `WebEngine::new` populates the ClassDB, which the 3D sync pass needs:
    // every collector keeps a node only if `is_class("Node3D")` answers yes, and
    // an unpopulated DB would silently yield an empty instance list.
    let mut engine = WebEngine::new(tree);

    // Advance until a tick has actually produced a step. The driver's first
    // callback only records the time origin.
    let mut stepped = 0;
    for i in 0..60 {
        if engine.frame(ORIGIN + (i as f64) * 16.7).is_some() {
            stepped += 1;
            if stepped >= 2 {
                break;
            }
        }
    }
    assert!(
        stepped >= 2,
        "the animation-frame tick must drive the loop; only {stepped} steps ran"
    );

    let buf = engine.render_3d_rgba(W, H);
    assert_eq!(
        buf.len(),
        (W as usize) * (H as usize) * 4,
        "the 3D frame must be a full RGBA buffer for the canvas backing size"
    );
    buf
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) as usize) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// Pixels differing from `background` in any colour channel.
fn differing(buf: &[u8], background: [u8; 4]) -> usize {
    buf.chunks_exact(4)
        .filter(|c| c[0] != background[0] || c[1] != background[1] || c[2] != background[2])
        .count()
}

#[test]
fn web_runtime_renders_3d_scene_to_canvas() {
    // -- a camera-only scene presents the clear colour only ------------------
    let empty = render_through_tick(EMPTY_SCENE);
    let background = px(&empty, 0, 0);
    let empty_differing = differing(&empty, background);
    assert_eq!(
        empty_differing, 0,
        "a camera-only scene must present a UNIFORM background; \
         {empty_differing} pixels differ. This is also the guard for the \
         coverage assertion below — a non-uniform background would satisfy it \
         with nothing in the scene"
    );

    // -- the red box covers pixels, and the centre of frame is red -----------
    let drawn = render_through_tick(RED_BOX_SCENE);
    let covered = differing(&drawn, background);
    assert!(
        covered > 0,
        "a lit MeshInstance3D must reach the presented buffer; only {covered} \
         pixels differ from the background. Zero is the pre-pat-mgixk.49 \
         behaviour: `present_clear` was patina-web's only draw call, so a 3D \
         scene produced a flat colour"
    );

    let centre = px(&drawn, W / 2, H / 2);
    assert!(
        centre[0] as i32 > centre[1] as i32 + 25 && centre[0] as i32 > centre[2] as i32 + 25,
        "the cube fills the centre of frame and its `albedo_color` is red, so \
         the centre pixel must be red-dominant; got {:?}. Equal channels mean \
         the material never reached the renderer and the cube drew in the \
         default shade",
        &centre[..3]
    );
}
