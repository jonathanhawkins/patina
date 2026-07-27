//! App-level integration test for the FileSystem dock **split display mode**
//! (pat-96yv4), driving the assembled `EditorServerHandle` over HTTP.
//!
//! Godot's `FileSystemDock` has a "Change Split Mode" button that cycles the
//! dock between a single combined tree (`DISPLAY_MODE_TREE_ONLY`) and split
//! layouts with a separate file list (`DISPLAY_MODE_VSPLIT` / `_HSPLIT`),
//! persisting the choice — see `_change_split_mode`/`set_display_mode` in
//! upstream/godot/editor/docks/filesystem_dock.cpp.
//!
//! This drives the served surface: open a project, read the default mode from
//! `GET /api/filesystem`, cycle it through Godot's order via
//! `POST /api/filesystem/display_mode`, set a mode directly, confirm it landed
//! on disk, and re-open the project to prove the mode survives a reload.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

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

fn make_project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("project.godot"), "[application]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(dir.path().join("scenes/main.tscn"), "[gd_scene format=3]\n").unwrap();
    dir
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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
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

fn json_ok(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(extract_body(resp)).expect("response is valid JSON")
}

fn current_mode(port: u16) -> String {
    json_ok(&http_get(port, "/api/filesystem"))["display_mode"]
        .as_str()
        .expect("display_mode in filesystem response")
        .to_string()
}

fn open_project(port: u16, root: &std::path::Path) {
    let resp = json_ok(&http_post(
        port,
        "/api/project/open",
        &format!(
            r#"{{"path":"{}"}}"#,
            root.display().to_string().replace('\\', "\\\\")
        ),
    ));
    assert_eq!(resp["ok"], serde_json::json!(true), "project should open");
}

/// pat-96yv4 acceptance: the Change Split Mode button cycles tree-only ↔ split
/// (matching Godot's order) and the selected mode persists.
#[test]
fn fs_dock_split_display_mode_toggles() {
    let (handle, port) = make_server();
    let project = make_project();
    let root = project.path().to_path_buf();
    open_project(port, &root);

    // Default is the single combined tree.
    assert_eq!(current_mode(port), "tree_only");

    // Cycle through Godot's order: tree_only → vsplit → hsplit → tree_only.
    let cycle = |p: u16| {
        json_ok(&http_post(
            p,
            "/api/filesystem/display_mode",
            r#"{"action":"cycle"}"#,
        ))["display_mode"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(cycle(port), "vsplit");
    assert_eq!(
        current_mode(port),
        "vsplit",
        "cycle persists in the served state"
    );
    assert_eq!(cycle(port), "hsplit");
    assert_eq!(cycle(port), "tree_only");

    // Set a mode directly and confirm it was persisted to disk.
    let set = json_ok(&http_post(
        port,
        "/api/filesystem/display_mode",
        r#"{"mode":"vsplit"}"#,
    ));
    assert_eq!(set["display_mode"], serde_json::json!("vsplit"));
    let mode_file = root.join(".godot").join("editor").join("fs_display_mode");
    assert_eq!(
        std::fs::read_to_string(&mode_file).unwrap().trim(),
        "vsplit",
        "display mode persisted to {mode_file:?}"
    );

    // Reload the editor (re-open the project): the mode survives.
    open_project(port, &root);
    assert_eq!(
        current_mode(port),
        "vsplit",
        "split mode persists across an editor reload"
    );

    handle.stop();
}
