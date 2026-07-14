//! End-to-end guard for `Scene > Save Scene` / `Save Scene As` against the real
//! editor server (Godot `EditorNode::_save_scene`).
//!
//! Covers the parity fixes: an empty (rootless) scene is refused rather than
//! written as a bare file, and a successful save reflects the file on the active
//! tab (title = filename, modified cleared) — the "Save As updates the tab" bug.

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

fn json(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp)).unwrap_or_else(|e| panic!("json: {e}: {resp}"))
}

/// Godot refuses to save a scene with no root node.
#[test]
fn saving_an_empty_scene_is_refused() {
    let (handle, port) = make_server();
    // New Scene -> empty, rootless active tab.
    assert!(http_post(port, "/api/scene/new", "{}").contains("200 OK"));

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    let resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));
    assert!(
        resp.contains("400"),
        "saving an empty scene must be refused: {resp}"
    );
    assert!(
        resp.contains("root node is required"),
        "the refusal explains a root node is required: {resp}"
    );
    // Nothing was written (or at least not a valid scene) — the file stays empty.
    let written = std::fs::read_to_string(tmp.path()).unwrap_or_default();
    assert!(
        !written.contains("[gd_scene"),
        "no scene file was written for the empty scene: {written:?}"
    );
    handle.stop();
}

/// A successful save reflects the file on the active tab: title becomes the
/// filename and the modified marker clears (Save As / first-save behavior).
#[test]
fn saving_updates_the_active_tab_filename() {
    let (handle, port) = make_server();

    // Start from a fresh untitled scene, then give it a root so it can be saved
    // (an untitled tab is exactly the Save As case).
    assert!(http_post(port, "/api/scene/new", "{}").contains("200 OK"));
    let root_id = json(&http_get(port, "/api/scene"))["nodes"]["id"]
        .as_u64()
        .unwrap();
    assert!(http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{root_id},"name":"Root","class_name":"Node2D"}}"#),
    )
    .contains("200 OK"));

    // The active tab is still "Untitled" with no path before saving.
    let before = json(&http_get(port, "/api/scene/tabs"));
    let ai = before["active_tab_index"].as_u64().unwrap() as usize;
    assert_eq!(before["tabs"][ai]["path"].as_str(), Some(""));

    // Save As to a real path.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my_level.tscn");
    let path_str = path.to_str().unwrap().to_string();
    let resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path_str}"}}"#));
    assert!(resp.contains("200 OK"), "save succeeds: {resp}");
    assert!(path.exists(), "the scene file was written");

    // The active tab now shows the saved filename, with the path set and clean.
    let after = json(&http_get(port, "/api/scene/tabs"));
    let ai2 = after["active_tab_index"].as_u64().unwrap() as usize;
    let tab = &after["tabs"][ai2];
    assert_eq!(
        tab["name"].as_str(),
        Some("my_level.tscn"),
        "the tab title becomes the saved filename: {after}"
    );
    assert_eq!(
        tab["path"].as_str(),
        Some(path_str.as_str()),
        "the tab path is the saved file: {after}"
    );
    assert_eq!(
        tab["modified"].as_bool(),
        Some(false),
        "the tab is no longer modified after saving: {after}"
    );
    handle.stop();
}

/// Saving a normal scene writes a valid .tscn and reports the file via scene/info.
#[test]
fn saving_writes_a_valid_scene_file() {
    let (handle, port) = make_server();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.tscn");
    let path_str = path.to_str().unwrap().to_string();

    let resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path_str}"}}"#));
    assert!(resp.contains("200 OK"), "save ok: {resp}");
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[gd_scene"), "valid tscn header: {contents}");
    assert!(contents.contains("Main"), "the scene root is serialized: {contents}");

    let info = json(&http_get(port, "/api/scene/info"));
    assert_eq!(
        info["scene_file"].as_str(),
        Some(path_str.as_str()),
        "scene/info reports the saved file: {info}"
    );
    assert_eq!(info["modified"].as_bool(), Some(false));
    handle.stop();
}
