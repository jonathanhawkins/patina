//! pat-nzgdu.104: served animation track editor wired to keyframe copy/paste +
//! track reorder.
//!
//! The server routes (/api/animation/keyframe/copy|paste, /track/reorder) already
//! existed but the served track editor never called them. This guards:
//!  1. Static — EDITOR_HTML now references the copy + paste endpoints and defines
//!     the clipboard wiring.
//!  2. Functional — copying a keyframe and pasting at a time offset creates a
//!     duplicate key at the new time in the Animation resource, driven through
//!     the real HTTP handlers (which hold the clipboard on EditorState).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::animation::{Animation, AnimationTrack, KeyFrame, TransitionType};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_keyframe_clipboard() {
    assert!(
        EDITOR_HTML.contains("/api/animation/keyframe/copy"),
        "EDITOR_HTML must POST /api/animation/keyframe/copy"
    );
    assert!(
        EDITOR_HTML.contains("/api/animation/keyframe/paste"),
        "EDITOR_HTML must POST /api/animation/keyframe/paste"
    );
    assert!(
        EDITOR_HTML.contains("/api/animation/track/reorder"),
        "EDITOR_HTML must POST /api/animation/track/reorder for track up/down"
    );
    assert!(
        EDITOR_HTML.contains("copyKeyframes"),
        "EDITOR_HTML must define the keyframe copy action"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional copy -> paste round-trip through the live handlers.
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

#[test]
fn copy_paste_keyframe_creates_key_at_new_time() {
    let port = free_port();

    // An animation "test" with one track carrying a keyframe at t=0.
    let mut state = EditorState::new(SceneTree::new());
    let mut anim = Animation::new("test", 5.0);
    let mut track = AnimationTrack::new("Node:position");
    track.add_keyframe(KeyFrame::new(0.0, Variant::Float(1.0), TransitionType::Linear));
    anim.tracks.push(track);
    state.animations.insert("test".to_string(), anim);

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Copy the keyframe at index 0 of track 0 (the /api/vcs-style server clipboard).
    let copy = http_post(
        port,
        "/api/animation/keyframe/copy",
        r#"{"animation":"test","track_index":0,"keyframe_indices":[0]}"#,
    );
    assert!(copy.contains("200 OK"), "copy should return 200, got:\n{copy}");

    // Paste with a +1.0s offset -> a duplicate key at t = 0.0 + 1.0 = 1.0.
    let paste = http_post(
        port,
        "/api/animation/keyframe/paste",
        r#"{"animation":"test","track_index":0,"time_offset":1.0}"#,
    );
    assert!(paste.contains("200 OK"), "paste should return 200, got:\n{paste}");

    // Read the animation back: track 0 now has a key at t=1.0.
    {
        let s = handle.state().lock().unwrap();
        let anim = s.animations.get("test").expect("animation should still exist");
        let times: Vec<f64> = anim.tracks[0].keyframes().iter().map(|k| k.time).collect();
        assert!(
            times.iter().any(|&t| (t - 1.0).abs() < 1e-9),
            "copy@0.0 + paste@offset 1.0 should create a keyframe at t=1.0; times: {times:?}"
        );
    }

    handle.stop();
}
