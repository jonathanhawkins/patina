//! pat-h7sz7 on the real game: the 3D platformer's sky is a custom
//! `shader_type sky` sampling a `samplerCube` skybox, and its metal robot
//! reflects that sky wherever no ReflectionProbe covers it (and in every
//! probe's 1 m fade band). Patina reflected black there; Godot reflects the
//! skybox (blue above, green hills at the horizon).
//!
//! This pins the wiring from the game's own files to the radiance map the
//! shaders sample: the Play environment must reflect its sky (default
//! `reflected_light_source = BG`, `background_mode = Sky`), and the radiance
//! map's zenith must be the skybox's blue, not black and not the default
//! procedural sky. It also pins Godot's probe order for the stage: smallest
//! box first.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::play_3d::Play3DScene;
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

#[test]
fn platformer_3d_reflects_its_skybox_and_orders_its_probes() {
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
    std::thread::sleep(Duration::from_millis(500));
    let (env, probes) = {
        let state = handle.state();
        let mut guard = state.lock().unwrap();
        let st = &mut *guard;
        // `prepare_play_3d_frame` lays out Controls in place (pat-9ww3e), so
        // it takes the tree mutably.
        let tree = match st.run_main_loop.as_mut() {
            Some(ml) => ml.tree_mut(),
            None => &mut st.scene_tree,
        };
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
            64,
            36,
        );
        match prepared.scene {
            Play3DScene::Camera { vp, inputs } => (
                vp.environment.environment.clone(),
                inputs.env_effects.reflection_probes.clone(),
            ),
            _ => panic!("platformer_3d plays through its Camera3D"),
        }
    };
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();

    let sky = gdrender3d::renderer::sky_radiance_for(
        &env,
        &[],
        gdrender3d::renderer::SKY_RADIANCE_RESOLUTION,
    )
    .expect(
        "the platformer's sky background with the default BG reflection source reflects the sky",
    );
    let up = sky
        .cubemap
        .sample_direction(gdcore::math::Vector3::new(0.0, 1.0, 0.0));
    assert!(
        up.b > 0.1 && up.b > up.r * 1.3,
        "the radiance zenith must be the skybox's blue: {up:?}"
    );
    // Not the default procedural sky (top colour (0.385, 0.454, 0.55) sRGB):
    // the custom cubemap shader really ran.
    let procedural = gdserver3d::sky::ProceduralSkyMaterial::default()
        .sky_top_color
        .srgb_to_linear();
    assert!(
        (up.r - procedural.r).abs() + (up.g - procedural.g).abs() + (up.b - procedural.b).abs()
            > 0.02,
        "the radiance map must come from the skybox cubemap, not a fallback sky: {up:?}"
    );

    // Godot's order: smallest box first. Reflection3 (6 x 6 x 12) is the
    // smallest of the stage's seven probes; Reflection1/5 the largest.
    assert_eq!(probes.len(), 7, "the stage authors seven probes");
    let order = gdserver3d::reflection_probe::probe_processing_order(&probes);
    let sizes: Vec<f32> = order
        .iter()
        .map(|&i| (probes[i].size * 0.5).length())
        .collect();
    assert!(
        sizes.windows(2).all(|w| w[0] <= w[1]),
        "smallest first: {sizes:?}"
    );
    assert_eq!(
        probes[order[0]].size,
        gdcore::math::Vector3::new(6.0, 6.0, 12.0)
    );
}
