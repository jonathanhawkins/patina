//! The Game view's frame stream (`GET /api/viewport`), the picture a player
//! watches while a scene runs in the browser editor.
//!
//! Play looked like ~5 fps whatever the game ran at, and said 60 the whole time:
//! * the page fetched a frame on the 200 ms viewport poll timer;
//! * the server deflated a PNG of every frame (~40 ms in a debug build) that
//!   nothing read while streaming, capping publishes at ~14 fps;
//! * the frame stream shared the 60 req/s abuse limit with every other route;
//! * the status `fps` was `1 / delta` — the TARGET rate — so the readout was
//!   a constant 60.
//!
//! Now each request names the last frame it showed (`?after=<seq>`) and the
//! server answers with the next one as soon as it exists; the status reports
//! what was measured.

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::export::{encode_bmp, encode_png_fast};
use gdrender2d::renderer::FrameBuffer;
use gdscene::SceneTree;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

struct Response {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

fn request(port: u16, method: &str, path: &str, body: &str) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("an HTTP response head");
    let headers = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

fn get(port: u16, path: &str) -> Response {
    request(port, "GET", path, "")
}

fn frame_seq(r: &Response) -> u64 {
    r.headers
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("X-Frame-Seq")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or_else(|| panic!("no X-Frame-Seq header in:\n{}", r.headers))
}

fn status(port: u16) -> Value {
    serde_json::from_slice(&get(port, "/api/runtime/status").body).unwrap()
}

/// A small solid frame whose red channel encodes `k` in 8 bits (the half step
/// keeps the encoder's truncation from landing on `k - 1`).
fn frame(k: u8) -> FrameBuffer {
    FrameBuffer::new(8, 6, Color::new((k as f32 + 0.5) / 255.0, 0.25, 0.5, 1.0))
}

/// The `k` a [`frame`] was made from, read back from its BMP (BGRA pixels
/// after the 54-byte header, so red is byte 56).
fn frame_index(bmp: &[u8]) -> u8 {
    bmp[54 + 2]
}

fn server() -> EditorServerHandle {
    EditorServerHandle::start(0, EditorState::new(SceneTree::new()))
}

#[test]
fn a_frame_request_after_the_shown_frame_answers_with_the_next_one() {
    let server = server();
    let port = server.port();
    server.update_frame(frame(10));
    let first = get(port, "/api/viewport");
    assert_eq!(first.status, 200);
    assert_eq!(first.body, encode_bmp(&frame(10)));
    let shown = frame_seq(&first);

    // The next frame is published while the request is already waiting.
    let waiter = std::thread::spawn(move || get(port, &format!("/api/viewport?after={shown}")));
    std::thread::sleep(Duration::from_millis(80));
    server.update_frame(frame(11));
    let next = waiter.join().unwrap();
    assert_eq!(next.status, 200);
    assert_eq!(
        frame_seq(&next),
        shown + 1,
        "the answer must be the frame published after `{shown}`, not a repeat of it"
    );
    assert_eq!(next.body, encode_bmp(&frame(11)));
}

#[test]
fn a_frame_request_with_no_newer_frame_answers_with_the_current_one() {
    let server = server();
    let port = server.port();
    server.update_frame(frame(20));
    let shown = frame_seq(&get(port, "/api/viewport"));
    // A paused game publishes nothing: the request must still come back.
    let started = Instant::now();
    let again = get(port, &format!("/api/viewport?after={shown}"));
    let waited = started.elapsed();
    assert_eq!(again.status, 200);
    assert_eq!(frame_seq(&again), shown);
    assert_eq!(again.body, encode_bmp(&frame(20)));
    assert!(
        waited < Duration::from_secs(2),
        "a request for a frame that never comes must not hang: {waited:?}"
    );
}

/// Several viewers pull one stream while frames are published as fast as the
/// server can take them: every viewer sees sequence numbers only move forward,
/// and never gets a picture older than the number it was told.
#[test]
fn concurrent_viewers_of_a_fast_stream_never_go_backwards() {
    const FRAMES: u8 = 200;
    let server = server();
    let port = server.port();
    server.update_frame(frame(0));
    let base = frame_seq(&get(port, "/api/viewport"));
    let viewers: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                let mut last = base;
                let mut seen = Vec::new();
                let deadline = Instant::now() + Duration::from_secs(20);
                while last < base + FRAMES as u64 && Instant::now() < deadline {
                    let r = get(port, &format!("/api/viewport?after={last}"));
                    assert_eq!(r.status, 200);
                    let seq = frame_seq(&r);
                    assert!(seq >= last, "sequence went backwards: {last} -> {seq}");
                    // Frame k is published as seq base + k, so the picture
                    // must be at least as new as the number in the header.
                    let k = frame_index(&r.body) as u64;
                    assert!(
                        base + k >= seq,
                        "seq {seq} came with the older picture of frame {k}"
                    );
                    if seq > last {
                        seen.push(seq);
                    }
                    last = seq;
                }
                (last, seen)
            })
        })
        .collect();
    for k in 1..=FRAMES {
        server.update_frame(frame(k));
        if k % 8 == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    for viewer in viewers {
        let (last, seen) = viewer.join().unwrap();
        assert_eq!(
            last,
            base + FRAMES as u64,
            "a viewer never saw the final frame"
        );
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
    }
}

#[test]
fn the_runtime_status_reports_measured_rates_not_the_target() {
    let server = server();
    let port = server.port();
    let idle = status(port);
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    assert_eq!(
        idle["build"], build,
        "a debug editor renders ~10x slower: {idle}"
    );
    assert_eq!(idle["sim_fps"], 0, "nothing is running: {idle}");
    assert_eq!(idle["render_fps"], 0, "nothing was published: {idle}");

    let started = Instant::now();
    for k in 0..12 {
        server.update_frame(frame(k));
    }
    server.note_render_time(Duration::from_millis(7));
    let busy = status(port);
    if started.elapsed() < Duration::from_millis(900) {
        assert_eq!(busy["render_fps"], 12, "{busy}");
    }
    let render_ms = busy["render_ms"].as_f64().unwrap();
    assert!((render_ms - 7.0).abs() < 0.05, "{busy}");
    assert_eq!(
        busy["sim_fps"], 0,
        "publishing frames steps no game: {busy}"
    );

    // The rate is over the LAST second: once the frames age out it reads 0.
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(status(port)["render_fps"], 0);
}

#[test]
fn a_running_game_reports_the_frames_it_actually_steps() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n\n[application]\nconfig/name=\"FrameRate\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();
    let server = server();
    let port = server.port();
    let open = request(
        port,
        "POST",
        "/api/project/open",
        &serde_json::json!({"path": dir.path().display().to_string()}).to_string(),
    );
    assert_eq!(open.status, 200, "{}", String::from_utf8_lossy(&open.body));
    let play = request(port, "POST", "/api/runtime/play", "{}");
    assert_eq!(play.status, 200, "{}", String::from_utf8_lossy(&play.body));
    std::thread::sleep(Duration::from_millis(1200));
    let running = status(port);
    request(port, "POST", "/api/runtime/stop", "{}");
    assert_eq!(running["running"], true, "{running}");
    let sim = running["sim_fps"].as_u64().unwrap();
    assert!(sim > 0, "the game thread steps the game: {running}");
    assert!(running["frame_count"].as_u64().unwrap() > 0, "{running}");
}

#[test]
fn the_frame_stream_and_game_input_are_not_rate_limited() {
    let server = server();
    let port = server.port();
    server.update_frame(frame(1));
    for i in 0..200 {
        let r = get(port, "/api/viewport?after=0");
        assert_eq!(r.status, 200, "frame fetch {i} was refused");
    }
    for i in 0..200 {
        let r = request(port, "POST", "/api/runtime/input/key_up", r#"{"key":"A"}"#);
        assert_ne!(r.status, 429, "key_up {i} was rate limited");
    }
    // The same burst on an ordinary route IS limited, so the loops above
    // really did exceed the budget. Four clients at once, so a slow machine
    // still crosses 60 requests within one second.
    let control: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                (0..100).any(|_| get(port, "/api/runtime/status").status == 429)
            })
        })
        .collect();
    let limited = control.into_iter().any(|c| c.join().unwrap());
    assert!(limited, "the control burst was never limited");
}

#[test]
fn the_png_frame_is_encoded_from_the_latest_frame_on_demand() {
    let server = server();
    let port = server.port();
    server.update_frame(frame(30));
    assert_eq!(
        get(port, "/api/viewport/png").body,
        encode_png_fast(&frame(30))
    );
    server.update_frame(frame(31));
    let png = get(port, "/api/viewport/png");
    assert_eq!(png.status, 200);
    assert_eq!(png.body, encode_png_fast(&frame(31)));
    assert_eq!(get(port, "/api/viewport/png").body, png.body);
    // A low-resolution camera-motion frame streams BMP only: the PNG keeps
    // the last full-quality frame.
    server.update_frame_ex(frame(32), false);
    assert_eq!(get(port, "/api/viewport/png").body, png.body);
    assert_eq!(get(port, "/api/viewport").body, encode_bmp(&frame(32)));
}

/// The served page pulls each new frame while a scene runs and shows the
/// measured rates, instead of the 200 ms poll and the nominal `fps` field. The
/// counter goes on the Game view's screen when that is what is showing: the
/// viewport container is hidden there, so a counter on it is never seen.
#[test]
fn the_game_view_pulls_each_new_frame_and_shows_a_measured_frame_rate() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    for needle in [
        "fetch('/api/viewport?after=' + _gameFrameSeq",
        "async function pumpGameFrame(gen)",
        "if (_gamePumpActive) return;",
        "if (runtimeRunning) startGameFramePump(); else stopGameFramePump();",
        "o.id = 'fps-overlay';",
        "var g = document.getElementById('game-screen');\n    if (g && g.offsetParent !== null) return g;",
        "runtimeSimFps = r.sim_fps",
        "runtimeBuild = r.build || ''",
    ] {
        assert!(
            html.contains(needle),
            "missing `{needle}` in the served editor"
        );
    }
    assert!(
        !html.contains("runtimeFps.toFixed(0)"),
        "the status bar still shows the target rate as the frame rate"
    );
}
