//! pat-p3p1m: times the Play frame of a real 3D project in isolation, phase by
//! phase, exactly as the editor binary's render loop runs it:
//! `prepare_play_3d_frame` (under the state lock) then `render` (off it).
//!
//! Ignored by default; it is a measurement, not a check. Run it on a RELEASE
//! build against a project directory:
//!
//! ```text
//! PATINA_PLAY3D_PROJECT=/path/to/project PATINA_PLAY3D_SCENE=game.tscn \
//!   cargo nextest run --release -p gdeditor --test play_3d_frame_timing --run-ignored all --no-capture
//! ```

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

#[test]
#[ignore = "measurement: set PATINA_PLAY3D_PROJECT and run on a release build"]
fn play_3d_frame_timing() {
    let Ok(project) = std::env::var("PATINA_PLAY3D_PROJECT") else {
        eprintln!("SKIP: PATINA_PLAY3D_PROJECT not set");
        return;
    };
    let scene = std::env::var("PATINA_PLAY3D_SCENE").unwrap_or_else(|_| "game.tscn".into());
    let (w, h) = (995u32, 807u32);
    gdobject::class_db::register_all_classes();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));
    request(port, "POST", "/api/project/open", &format!(r#"{{"path":"{project}"}}"#));
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{project}/{scene}"}}"#),
    );
    assert!(load.contains("\"ok\""), "load: {load}");
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");
    std::thread::sleep(Duration::from_millis(500));

    let frames = 40;
    let (mut prep_total, mut render_total) = (Duration::ZERO, Duration::ZERO);
    let mut kind = "";
    for i in 0..frames {
        let t0 = Instant::now();
        let prepared = {
            let state = handle.state();
            let st = state.lock().unwrap();
            let tree = st.run_main_loop.as_ref().map(|ml| ml.tree()).unwrap_or(&st.scene_tree);
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
        let t1 = Instant::now();
        let img = prepared.render();
        let t2 = Instant::now();
        assert_eq!((img.width, img.height), (w, h));
        kind = match prepared.scene {
            gdeditor::play_3d::Play3DScene::Camera { .. } => "camera",
            gdeditor::play_3d::Play3DScene::EditorCamera { .. } => "editor camera",
            gdeditor::play_3d::Play3DScene::NoCamera { .. } => "no camera",
        };
        // The first frames pay one-time costs (GPU init, probe capture,
        // texture decode); report them separately.
        if i < 3 {
            eprintln!(
                "frame {i}: prepare {:.1} ms, render {:.1} ms",
                (t1 - t0).as_secs_f64() * 1e3,
                (t2 - t1).as_secs_f64() * 1e3
            );
        } else {
            prep_total += t1 - t0;
            render_total += t2 - t1;
        }
    }
    let n = (frames - 3) as f64;
    eprintln!(
        "{kind} at {w}x{h}: prepare {:.2} ms, render {:.2} ms, total {:.2} ms per frame (mean of {n}); software fallbacks {}",
        prep_total.as_secs_f64() * 1e3 / n,
        render_total.as_secs_f64() * 1e3 / n,
        (prep_total + render_total).as_secs_f64() * 1e3 / n,
        gdeditor::viewport_3d_render::software_fallback_frames(),
    );
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();
}
