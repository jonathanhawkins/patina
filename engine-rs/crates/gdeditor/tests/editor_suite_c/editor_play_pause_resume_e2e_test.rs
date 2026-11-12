//! App-level e2e smoke test for the Play / Pause / Resume run workflow
//! (pat-niu1j.4, sub-task of pat-niu1j editor parity).
//!
//! Why app-level: the editor-parity gate was previously fooled by lib unit
//! tests that exercised isolated state structs. Those pass even when the
//! pause/resume flag is never reachable through the running editor. This test
//! boots the real server (`EditorServerHandle::start`) and drives it over HTTP
//! the way the editor UI does — `POST /api/runtime/{play,pause,step,stop}` and
//! `GET /api/runtime/status` — asserting the SERVED runtime state actually
//! reflects pause and resume, and that resume is a real un-pause (not cosmetic):
//! frame stepping is gated on the paused flag, so a resumed runtime rejects a
//! step exactly as a freshly-played one does.
//!
//! Deliberately a SINGLE `#[test]`: nextest launches one process per test, and
//! booting several servers from several concurrently-launched processes thrashes
//! under the host's code-sign/AMFI launch path. One test → one process → one
//! server. Harness mirrors `editor_mode_e2e_test.rs` (raw-TCP HTTP); integration
//! crates can't share helpers, so they're duplicated here.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..50 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 49 => thread::sleep(Duration::from_millis(100)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Fetches `GET /api/runtime/status` and returns the parsed JSON body.
fn status(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/runtime/status");
    assert!(resp.contains("200 OK"), "status ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("status json: {e}"))
}

/// Boots the real editor server once and drives the Play / Pause / Resume run
/// workflow over HTTP, asserting that the served runtime state reflects each
/// transition — and that resume genuinely un-pauses (pat-niu1j.4).
//
// NOTE: the fn name intentionally contains `editor_play_pause_resume` so a bare
// nextest positional filter (`cargo nextest run -p gdeditor
// editor_play_pause_resume`) matches it. nextest's substring filter matches the
// test NAME, not the binary id — a name lacking that stem yields "0 tests run".
#[test]
fn editor_play_pause_resume_reflects_runtime_state() {
    let (_handle, port) = make_server();

    // Baseline: nothing is running before Play.
    let s = status(port);
    assert_eq!(s["running"], false, "idle before play: {s}");

    // Play → running, not paused. The pat-nzgdu.19 auto-driver begins advancing
    // frames on its own, so `frame_count` is a non-negative number that may
    // already be climbing (no longer pinned to 0 the way manual-only play was).
    let play = http_post(port, "/api/runtime/play", "");
    assert!(play.contains("200 OK"), "play ok: {play}");
    let s = status(port);
    assert_eq!(s["running"], true, "running after play: {s}");
    assert_eq!(s["paused"], false, "not paused after play: {s}");
    assert!(
        s["frame_count"].as_u64().is_some(),
        "frame_count present after play: {s}"
    );

    // While running but NOT paused, a step is rejected: manual `/api/runtime/step`
    // is the single-step control and is gated on the paused flag, so this
    // distinguishes a live (auto-driven) runtime from a paused one.
    let step_live = http_post(port, "/api/runtime/step", "");
    assert!(
        step_live.contains("400"),
        "step while running-not-paused is rejected: {step_live}"
    );

    // Pause → the served state reports paused, which also halts the auto-driver.
    let pause = http_post(port, "/api/runtime/pause", "");
    assert!(pause.contains("200 OK"), "pause ok: {pause}");
    let s = status(port);
    assert_eq!(s["running"], true, "still running while paused: {s}");
    assert_eq!(s["paused"], true, "paused after pause: {s}");

    // Paused: the auto-driver is frozen, so the frame count is now a stable
    // baseline and each manual step advances it by exactly one.
    let base = s["frame_count"]
        .as_u64()
        .expect("paused frame_count is a number");
    for i in 1..=3u64 {
        let step = http_post(port, "/api/runtime/step", "");
        assert!(step.contains("200 OK"), "step {i} ok: {step}");
        let s = status(port);
        assert_eq!(
            s["frame_count"].as_u64().unwrap(),
            base + i,
            "manual step advances the paused frame count by exactly one: {s}"
        );
    }

    // Resume (pause toggles back) → the served state reports not paused, and the
    // frame counter is PRESERVED across the pause (resume is not a restart): it
    // continues from base+3 and the auto-driver resumes advancing, so it is
    // never reset to 0.
    let resume = http_post(port, "/api/runtime/pause", "");
    assert!(resume.contains("200 OK"), "resume ok: {resume}");
    let s = status(port);
    assert_eq!(s["running"], true, "still running after resume: {s}");
    assert_eq!(s["paused"], false, "not paused after resume: {s}");
    assert!(
        s["frame_count"].as_u64().unwrap() >= base + 3,
        "frame count preserved (not reset) across resume: {s}"
    );

    // Resume is a REAL un-pause: a step is now rejected again, exactly as it was
    // on the freshly-played (never-paused) runtime above. A cosmetic resume that
    // left `paused` set would instead accept the step.
    let step_resumed = http_post(port, "/api/runtime/step", "");
    assert!(
        step_resumed.contains("400"),
        "step after resume is rejected (genuinely un-paused): {step_resumed}"
    );

    // Stop → not running and the frame counter resets.
    let stop = http_post(port, "/api/runtime/stop", "");
    assert!(stop.contains("200 OK"), "stop ok: {stop}");
    let s = status(port);
    assert_eq!(s["running"], false, "not running after stop: {s}");
    assert_eq!(s["paused"], false, "not paused after stop: {s}");
    assert_eq!(s["frame_count"], 0, "frame count reset after stop: {s}");
}
