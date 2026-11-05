//! 3D Play render time: a running game re-uploads nothing per frame that did
//! not change, on the official 3D platformer demo (`games/platformer_3d`,
//! unmodified), driven the way the editor binary runs Play.
//!
//! Two per-frame costs this guards, both found by profiling the Play frame
//! (it had gone from ~15 ms to ~65 ms since the GPU path landed):
//!
//! - every roughness / metallic map was broadcast into a FRESH `Arc` each
//!   frame (`load_orm_maps`), and the GPU path keys map uploads by `Arc`
//!   identity, so every ORM map of the level was re-uploaded and re-mipped on
//!   every frame;
//! - the spinning coins change the shadow casters every frame, and each change
//!   re-created and re-uploaded the whole 2048² f32 shadow atlas (16 MB) even
//!   though only the coins' texels can differ from the static base.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::viewport_3d_render::{gpu_render_stats, GpuRenderStats};
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

fn render_one(handle: &EditorServerHandle, w: u32, h: u32) -> GpuRenderStats {
    let prepared = {
        let state = handle.state();
        let mut st = state.lock().unwrap();
        let st = &mut *st;
        let tree = match st.run_main_loop.as_mut() {
            Some(ml) => ml.tree_mut(),
            None => &mut st.scene_tree,
        };
        let clear = st
            .run_project_settings
            .as_ref()
            .unwrap_or(&st.project_settings)
            .default_clear_color();
        gdeditor::play_3d::prepare_play_3d_frame(
            tree,
            &st.editor_viewport_3d,
            st.runtime_camera_override,
            clear,
            w,
            h,
        )
    };
    assert!(prepared.draws_world(), "game.tscn has a current Camera3D");
    let img = prepared.render();
    assert_eq!((img.width, img.height), (w, h));
    gpu_render_stats()
}

#[test]
fn a_running_platformer_frame_uploads_only_what_changed() {
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
    std::thread::sleep(Duration::from_millis(300));

    let (w, h) = (160, 120);
    // Warm up: first uploads, probe captures, and the static shadow casters
    // settling into the cached base (30 calls).
    let mut first = render_one(&handle, w, h);
    if first == GpuRenderStats::default() {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
        request(port, "POST", "/api/runtime/stop", "");
        handle.stop();
        return;
    }
    for _ in 0..34 {
        first = render_one(&handle, w, h);
        // The game advances on its own thread; give the coins time to turn.
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut last = first;
    const FRAMES: u64 = 8;
    for _ in 0..FRAMES {
        std::thread::sleep(Duration::from_millis(20));
        last = render_one(&handle, w, h);
    }
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();
    eprintln!("after warm-up {first:?}\nafter {FRAMES} more frames {last:?}");

    assert_eq!(
        last.map_uploads,
        first.map_uploads,
        "a running frame re-uploaded {} material maps over {FRAMES} frames — a map \
         reached the renderer as a new Arc (an ORM channel broadcast per frame?)",
        last.map_uploads - first.map_uploads
    );
    // The coins spin, so the shadow casters DO change: otherwise the upload
    // check below would pass without exercising anything.
    let rebuilds = last.shadow_rebuilds - first.shadow_rebuilds;
    assert!(
        rebuilds >= FRAMES / 2,
        "only {rebuilds} shadow rebuilds in {FRAMES} running frames"
    );
    let per_rebuild = (last.shadow_upload_texels - first.shadow_upload_texels) / rebuilds;
    let atlas = 2048u64 * 2048;
    assert!(
        per_rebuild * 8 < atlas,
        "each shadow rebuild uploaded {per_rebuild} texels; the moving casters cover \
         a small part of the {atlas}-texel atlas, so the whole atlas was re-uploaded"
    );
}
