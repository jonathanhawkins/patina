//! App-level e2e smoke test for the main-screen mode switch (pat-3d87y).
//!
//! Root cause of the false "parity complete": the editor-parity gate only ran
//! lib unit tests on isolated model structs (e.g. `MainScreenSwitcher`), which
//! pass even when the model is never wired into the running editor. This test
//! boots the real server (`EditorServerHandle::start`), drives it over HTTP the
//! way a user does — `POST /api/editor/mode {3d|script|game|assetlib}` — and
//! asserts the SERVED central view (`GET /api/editor/main_view`) actually
//! corresponds to that mode (a distinct component, not the 2D viewport), rather
//! than the switch being cosmetic.
//!
//! Deliberately a SINGLE `#[test]`: nextest launches one process per test, and
//! booting several servers from several concurrently-launched processes thrashes
//! under the host's code-sign/AMFI launch path. One test → one process → one
//! server, exercising every mode in sequence. Harness mirrors
//! `editor_beads_test.rs` (raw-TCP HTTP); integration-test crates can't share
//! helpers, so they're duplicated here.

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

/// Switches the editor to `mode` and returns the served central-view JSON.
fn served_view(port: u16, mode: &str) -> serde_json::Value {
    let set = http_post(port, "/api/editor/mode", &format!(r#"{{"mode":"{mode}"}}"#));
    assert!(set.contains("200 OK"), "set mode {mode} ok: {set}");
    let resp = http_get(port, "/api/editor/main_view");
    assert!(resp.contains("200 OK"), "main_view for {mode} ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("main_view json {mode}: {e}"))
}

/// Boots the real editor server once and asserts that each main-screen mode
/// serves its own central view over HTTP — distinct from the 2D viewport and
/// mutually distinct — so the mode switch is not cosmetic (pat-3d87y).
// NOTE: the fn name intentionally contains the binary stem `editor_mode_e2e_test`
// so a bare nextest positional filter (`cargo nextest run -p gdeditor
// editor_mode_e2e_test`) matches it. nextest's substring filter matches the test
// NAME, not the binary id — a name lacking that stem yields "0 tests run" (exit 4).
#[test]
fn editor_mode_e2e_test_modes_serve_distinct_views() {
    let (_handle, port) = make_server();

    // 2D mode is the baseline: it IS the 2D canvas viewport.
    let two_d = served_view(port, "2d");
    assert_eq!(two_d["is_2d_viewport"], true, "2d mode is the 2D viewport: {two_d}");
    let two_d_component = two_d["component_id"].as_str().unwrap().to_string();

    // Each non-2D mode serves its own distinct, non-2D central view.
    let mut seen = std::collections::HashSet::new();
    seen.insert(two_d_component.clone());
    for (mode, expected) in [
        ("3d", "spatial_editor"),
        ("script", "script_editor"),
        ("game", "game_view"),
        ("assetlib", "asset_lib"),
    ] {
        let v = served_view(port, mode);
        let component = v["component_id"].as_str().unwrap();
        assert_eq!(v["mode"], mode, "served view reports the {mode} mode: {v}");
        assert_eq!(
            component, expected,
            "{mode} mode serves its own central view, got {component}"
        );
        assert_ne!(
            component, two_d_component,
            "{mode} mode's served view differs from the 2D viewport ({two_d_component})"
        );
        assert_eq!(
            v["is_2d_viewport"], false,
            "{mode} mode is not the 2D viewport: {v}"
        );
        assert!(
            seen.insert(component.to_string()),
            "mode {mode} served a duplicate central view: {component}"
        );
    }

    // All five modes (2d + the four above) serve mutually-distinct views.
    assert_eq!(seen.len(), 5, "all five modes serve distinct views: {seen:?}");
}
