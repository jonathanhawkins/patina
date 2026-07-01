//! Regression test for pat-m6rbm: the scene `modified` flag must clear when
//! the undo stack is rewound back to the saved revision, and re-arm when a
//! redo moves past it again.
//!
//! REPRO (from the bead): fresh load -> /api/scene/info modified:false;
//! POST /api/node/add -> modified:true; POST /api/undo -> modified STILL true
//! (BUG). Godot tracks the undo-stack position relative to the last save and
//! clears the dirty flag when undo returns to that position. This test drives
//! the real editor server over HTTP and asserts the corrected behavior.
//!
//! Harness (free_port/make_server/http_get/http_post/extract_body) is copied
//! from crates/gdeditor/tests/editor_parity_test.rs so this file is
//! self-contained.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
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
    let mut main = Node::new("Main", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
    );
    tree.add_child(root, main).unwrap();
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

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

/// Reads the `modified` boolean from `GET /api/scene/info`.
fn scene_modified(port: u16) -> bool {
    let resp = http_get(port, "/api/scene/info");
    assert!(
        resp.contains("200 OK"),
        "GET /api/scene/info should succeed, got: {resp}"
    );
    let info: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    info["modified"]
        .as_bool()
        .expect("scene/info must report a boolean `modified` field")
}

/// pat-m6rbm: undoing back to the saved revision must clear the `modified`
/// flag, and redoing past it must set it again. Before the fix the flag was
/// latched true on any mutation and never cleared by undo.
#[test]
fn editor_modified_flag_clears_on_undo_to_saved() {
    let (handle, port) = make_server();
    let main_id = get_main_node_id(port);

    // (1) A freshly constructed editor state is at its saved revision.
    assert_eq!(
        scene_modified(port),
        false,
        "a freshly loaded scene starts unmodified"
    );

    // (2) Adding a node mutates the scene and marks it modified.
    let add_body =
        format!(r#"{{"parent_id":{main_id},"name":"UndoChild","class_name":"Sprite2D"}}"#);
    let add_resp = http_post(port, "/api/node/add", &add_body);
    assert!(
        add_resp.contains("200 OK"),
        "POST /api/node/add should succeed, got: {add_resp}"
    );
    let add_json: serde_json::Value =
        serde_json::from_str(extract_body(&add_resp)).unwrap();
    assert!(
        add_json["id"].as_u64().is_some(),
        "add response should include the new node id"
    );
    assert_eq!(
        scene_modified(port),
        true,
        "adding a node marks the scene modified"
    );

    // (3) Undoing back to the saved revision must clear the modified flag.
    //     This is the core of pat-m6rbm — previously it stayed true.
    let undo_resp = http_post(port, "/api/undo", "");
    assert!(
        undo_resp.contains("200 OK"),
        "POST /api/undo should succeed, got: {undo_resp}"
    );
    assert_eq!(
        scene_modified(port),
        false,
        "undoing back to the saved state must clear `modified` (pat-m6rbm)"
    );

    // (4) Redoing past the saved revision must re-mark the scene modified.
    let redo_resp = http_post(port, "/api/redo", "");
    assert!(
        redo_resp.contains("200 OK"),
        "POST /api/redo should succeed, got: {redo_resp}"
    );
    assert_eq!(
        scene_modified(port),
        true,
        "redoing past the saved state must set `modified` again"
    );

    // (5) Undo once more to confirm the flag toggles back to clean (not a
    //     one-shot), proving `modified` is derived from the saved revision.
    let undo_resp2 = http_post(port, "/api/undo", "");
    assert!(
        undo_resp2.contains("200 OK"),
        "second POST /api/undo should succeed, got: {undo_resp2}"
    );
    assert_eq!(
        scene_modified(port),
        false,
        "undo must again clear `modified` when returning to the saved revision"
    );

    handle.stop();
}
