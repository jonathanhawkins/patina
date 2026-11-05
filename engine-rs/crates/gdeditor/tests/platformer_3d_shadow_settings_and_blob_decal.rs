//! pat-772py: the 3D platformer's Play frame carries the directional-shadow
//! settings Godot renders the stage's sun with, and the BlobShadow decal Godot
//! draws: a soft radial black blob that skips the robot.
//!
//! Driven the way the editor binary runs Play (open the project, play
//! `game.tscn`, `prepare_play_3d_frame` on the running tree), on the
//! UNMODIFIED `games/platformer_3d`. The assertions are on the render inputs.
//! The pixels are covered by `gdrender3d`'s `gpu_directional_shadow_fits_the_camera`
//! and `gpu_decal_honours_cull_mask`.
//!
//! What Godot reads, and Patina dropped before this bead:
//!
//! - `project.godot`: `directional_shadow/size = 8192`,
//!   `soft_shadow_filter_quality = 3`. Neither reached the renderer.
//! - `stage.tscn`'s sun: `shadow_blur = 1.5`, `directional_shadow_mode = 0`
//!   (orthogonal). Read by nothing.
//! - `player.tscn:179-185`, the BlobShadow: `texture_albedo` is an INLINE
//!   `GradientTexture2D` (black at the centre, transparent from radius 0.49).
//!   With no file behind it, the decal fell back to its flat white `modulate`
//!   over the whole box, the "grey rectangle".

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::play_3d::Play3DScene;
use gdeditor::viewport_3d_render::{Editor3DRenderInputs, SceneLightKind};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn project() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
        .to_string_lossy()
        .into_owned()
}

fn play_inputs() -> Editor3DRenderInputs {
    gdobject::class_db::register_all_classes();
    let project = project();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));
    request(
        port,
        "POST",
        "/api/project/open",
        &format!(r#"{{"path":"{project}"}}"#),
    );
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{project}/game.tscn"}}"#),
    );
    assert!(load.contains("\"ok\""), "load: {load}");
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");
    std::thread::sleep(Duration::from_millis(800));
    let inputs = {
        let state = handle.state();
        let st = state.lock().unwrap();
        let tree = st
            .run_main_loop
            .as_ref()
            .map(|ml| ml.tree())
            .unwrap_or(&st.scene_tree);
        let clear = st
            .run_project_settings
            .as_ref()
            .unwrap_or(&st.project_settings)
            .default_clear_color();
        let prepared = gdeditor::play_3d::prepare_play_3d_frame(
            tree,
            &st.editor_viewport_3d,
            st.runtime_camera_override,
            clear,
            320,
            180,
        );
        match prepared.scene {
            Play3DScene::Camera { inputs, .. } => inputs,
            _ => panic!("the platformer plays through its own Camera3D"),
        }
    };
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();
    inputs
}

/// Both tests share one Play session: it is the slow part.
fn inputs() -> &'static Editor3DRenderInputs {
    static INPUTS: std::sync::OnceLock<Editor3DRenderInputs> = std::sync::OnceLock::new();
    INPUTS.get_or_init(play_inputs)
}

#[test]
fn the_sun_carries_godots_directional_shadow_settings() {
    let sun = inputs()
        .scene_lights
        .iter()
        .find(|l| l.kind == SceneLightKind::Directional && l.shadow_enabled)
        .expect("stage.tscn's shadowed DirectionalLight3D");
    let p = sun.directional_shadow;
    assert_eq!(p.atlas_size, 8192, "project.godot directional_shadow/size");
    assert_eq!(
        p.soft_quality, 3,
        "project.godot soft_shadow_filter_quality"
    );
    assert_eq!(p.blur, 1.5, "stage.tscn shadow_blur");
    assert_eq!(p.mode, 0, "stage.tscn directional_shadow_mode (orthogonal)");
    assert_eq!(sun.directional_shadow_max_distance, 55.0);
    assert_eq!(sun.shadow_bias, 0.02);
    // SOFT_MEDIUM: 8 Vogel taps at quality radius 2 (Godot's
    // `directional_soft_shadow_filter_set_quality`).
    assert_eq!(p.soft_samples_and_radius(), (8, 2.0));
}

#[test]
fn the_blob_shadow_is_a_soft_black_disc_that_skips_layer_2() {
    let decals = &inputs().env_effects.decals;
    assert_eq!(decals.len(), 1, "the player's BlobShadow is the only Decal");
    let d = &decals[0];
    assert_eq!(
        d.cull_mask, 1048573,
        "every layer but 2, where the Robot mesh lives"
    );
    assert!(
        (d.albedo_mix - 0.8).abs() < 1e-6,
        "albedo_mix {}",
        d.albedo_mix
    );
    let img = d
        .albedo_image
        .as_ref()
        .expect("the inline GradientTexture2D must resolve to pixels, not fall back to modulate");
    let at = |u: f32, v: f32| {
        let x = ((u * img.width as f32) as u32).min(img.width - 1);
        let y = ((v * img.height as f32) as u32).min(img.height - 1);
        img.pixels[(y * img.width + x) as usize]
    };
    let centre = at(0.5, 0.5);
    assert!(
        centre.r < 0.01 && centre.g < 0.01 && centre.b < 0.01 && centre.a > 0.95,
        "the gradient is opaque black inside offset 0.2: {centre:?}"
    );
    for (u, v) in [(0.02, 0.02), (0.98, 0.5), (0.5, 0.02)] {
        let c = at(u, v);
        assert!(
            c.a < 0.05,
            "transparent past radius 0.49 (fill_to (0.5, 0.01)), got {c:?} at ({u}, {v})"
        );
    }
    let mid = at(0.5, 0.5 - 0.3);
    assert!(
        mid.a > 0.1 && mid.a < 0.9,
        "a soft falloff between: {mid:?}"
    );
}
