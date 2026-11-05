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

    let frames: usize = std::env::var("PATINA_PLAY3D_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40)
        .max(4);
    let (mut prep_total, mut render_total) = (Duration::ZERO, Duration::ZERO);
    let mut kind = "";
    for i in 0..frames {
        let t0 = Instant::now();
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

/// Renders one Play frame of a real 3D project from a DETERMINISTIC state and
/// either saves it (`PATINA_PLAY3D_DUMP=<file>`) or compares it byte for byte
/// against a saved one (`PATINA_PLAY3D_COMPARE=<file>`) (pat-414k3).
///
/// The before/after oracle for render-performance work: dump on the old build,
/// compare on the new one, and an optimisation that changed the picture shows
/// up as differing pixels. Deterministic because Play is never pressed: the
/// frame is the loaded scene through its current Camera3D, rendered by the
/// same `prepare_play_3d_frame` + `render` pair the editor binary runs.
#[test]
#[ignore = "measurement: set PATINA_PLAY3D_PROJECT and PATINA_PLAY3D_DUMP or _COMPARE"]
fn play_3d_frame_dump() {
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
    // Every runtime frame advances by the fixed `delta_time`, so the game state
    // is a function of the frame COUNT alone: pause straight after Play, then
    // step to a fixed frame.
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");
    request(port, "POST", "/api/runtime/pause", "");
    const TARGET_FRAME: u64 = 30;
    loop {
        let (count, paused) = {
            let state = handle.state();
            let st = state.lock().unwrap();
            (st.runtime_frame_count, st.is_paused)
        };
        assert!(paused, "the runtime must be paused");
        assert!(count <= TARGET_FRAME, "the driver ran past frame {TARGET_FRAME} before pausing");
        if count == TARGET_FRAME {
            break;
        }
        // The server rate-limits to 60 requests a second.
        std::thread::sleep(Duration::from_millis(20));
        request(port, "POST", "/api/runtime/step", "");
    }
    let mut frames = Vec::new();
    for _ in 0..6 {
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
                false,
                clear,
                w,
                h,
            )
        };
        assert!(prepared.draws_world(), "the scene must have a current Camera3D");
        frames.push(prepared.render().data);
    }
    handle.stop();
    // The first frames pay probe capture; the last three must already agree, or
    // the comparison below would measure noise.
    for i in 1..frames.len() {
        let (n, bbox) = pixel_diff(&frames[i - 1], &frames[i], w);
        eprintln!("frame {} vs {i}: {n} pixels differ, bbox {bbox:?}", i - 1);
    }
    assert!(frames[3] == frames[4] && frames[4] == frames[5], "renders of one paused frame differ");
    let frame = frames.pop().unwrap();
    if let Ok(path) = std::env::var("PATINA_PLAY3D_DUMP") {
        std::fs::write(&path, &frame).unwrap();
        eprintln!("wrote {} bytes to {path}", frame.len());
    }
    if let Ok(path) = std::env::var("PATINA_PLAY3D_COMPARE") {
        let reference = std::fs::read(&path).unwrap();
        assert_eq!(reference.len(), frame.len());
        let mut differing = 0usize;
        let mut max_delta = 0u8;
        for (a, b) in reference.chunks_exact(4).zip(frame.chunks_exact(4)) {
            if a != b {
                differing += 1;
                for k in 0..4 {
                    max_delta = max_delta.max(a[k].abs_diff(b[k]));
                }
            }
        }
        eprintln!("compare: {differing} of {} pixels differ, max channel delta {max_delta}", w * h);
        assert_eq!(differing, 0, "the frame changed");
    }
}

/// `(differing pixels, bounding box (x0, y0, x1, y1))` between two BGRA frames.
fn pixel_diff(a: &[u8], b: &[u8], w: u32) -> (usize, Option<(u32, u32, u32, u32)>) {
    let mut n = 0;
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for (i, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        if pa != pb {
            n += 1;
            let (x, y) = (i as u32 % w, i as u32 / w);
            bbox = Some(match bbox {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    (n, bbox)
}
