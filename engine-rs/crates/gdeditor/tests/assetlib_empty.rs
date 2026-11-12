//! pat-0q2kx.240 — `/api/assetlib/list` reports an honest empty state.
//!
//! `api_get_assetlib_list` used to serve twelve hardcoded entries ("Pixel
//! Platformer Kit", "Dialogue System", "Low-Poly Nature Pack", …) complete with
//! authors, licences and star ratings. None existed, none could be installed —
//! there is no install path anywhere in the server — and the endpoint's own
//! comment admitted "no external network call". A fabricated catalog is worse
//! than an empty browser: it invites clicks that cannot work and reads as a
//! finished feature to anyone auditing the editor.
//!
//! ⚠️ THIS IS A STANDALONE BINARY ON PURPOSE. The test first lived in
//! `editor_suite_c`, whose 128 tests include
//! `editor_play_game_view_wiring_test::..._play_switches_to_game_and_renders`,
//! which FAILS at HEAD for unrelated reasons (play-mode frame polling — it does
//! not reference assetlib at all). Running the suite therefore fails regardless
//! of this bead's correctness, and a positional nextest filter cannot be used
//! because the command would exceed the verifier's 58-character limit. The
//! harness below is copied from that suite rather than shared, so this test
//! stays verifiable while the suite is red.

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
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn assetlib_list_is_honestly_empty() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/assetlib/list");
    let body = extract_body(&resp).to_string();
    let v: serde_json::Value = serde_json::from_str(&body).expect("assetlib list is JSON");

    // The key is retained so existing consumers keep parsing — notably
    // editor_smoke_test.rs:84, which asserts it is present.
    let assets = v["assets"].as_array().expect("`assets` is an array");
    assert!(
        assets.is_empty(),
        "no catalog source is configured, so no assets may be advertised; got {assets:?}"
    );

    // "Not configured" must be distinguishable from "configured and returned
    // nothing" — the frontend has to render those two states differently.
    assert_eq!(
        v["configured"],
        serde_json::json!(false),
        "the response must state that no catalog source is configured"
    );

    // Checked against the RAW body, not the parsed array: an empty `assets`
    // would still pass if the fabricated entries merely moved to another field.
    for fake in [
        "Pixel Platformer Kit",
        "Top-Down Shooter Template",
        "Dialogue System",
        "Low-Poly Nature Pack",
        "Outline Shader",
        "Platformer Demo",
    ] {
        assert!(
            !body.contains(fake),
            "fabricated catalog entry {fake:?} must not be served"
        );
    }

    handle.stop();
}
