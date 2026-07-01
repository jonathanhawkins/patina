//! App-level integration test for the FileSystem dock **context-menu backends**
//! (pat-ypgqq), driving the assembled `EditorServerHandle` over HTTP.
//!
//! Godot's `FileSystemDock` right-click menu's actions operate on the target:
//! Duplicate copies the file, Copy Absolute Path / Copy UID surface the path and
//! resource UID, and Edit Dependencies / View Owners list what a file references
//! and what references it (upstream/godot/editor/docks/filesystem_dock.cpp).
//!
//! Here we exercise the server side those actions call: `GET
//! /api/filesystem/file_info` (abs path, uid, dependencies, owners) and `POST
//! /api/filesystem/duplicate`.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
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

/// A temp project where `scenes/main.tscn` has a UID header and depends on
/// `icon.png`, and `scenes/level.tscn` references `main.tscn` (so main's owner
/// is level).
fn make_project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("project.godot"), "[application]\n").unwrap();
    std::fs::write(dir.path().join("icon.png"), "PNG").unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("scenes/main.tscn"),
        "[gd_scene load_steps=2 format=3 uid=\"uid://abcd1234\"]\n\
         [ext_resource type=\"Texture2D\" path=\"res://icon.png\" id=\"1\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scenes/level.tscn"),
        "[gd_scene load_steps=2 format=3]\n\
         [ext_resource type=\"PackedScene\" path=\"res://scenes/main.tscn\" id=\"1\"]\n",
    )
    .unwrap();
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
    http_request_str(port, &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))
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

fn open_project(port: u16, root: &std::path::Path) {
    let resp = json_ok(&http_post(
        port,
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, root.display().to_string().replace('\\', "\\\\")),
    ));
    assert_eq!(resp["ok"], serde_json::json!(true));
}

/// pat-ypgqq: the context-menu actions operate on their target — file_info
/// resolves abs path / uid / dependencies / owners, and Duplicate copies.
#[test]
fn fs_dock_context_menu_backend_actions() {
    let (handle, port) = make_server();
    let project = make_project();
    let root = project.path().to_path_buf();
    open_project(port, &root);

    // file_info surfaces what Copy Absolute Path / Copy UID / Edit Dependencies
    // / View Owners act on.
    let info = json_ok(&http_get(port, "/api/filesystem/file_info?path=res://scenes/main.tscn"));
    assert!(
        info["abs_path"].as_str().unwrap().ends_with("scenes/main.tscn"),
        "abs_path resolves under the project: {info}"
    );
    assert_eq!(info["uid"], serde_json::json!("uid://abcd1234"), "uid read from header");
    let deps: Vec<String> = info["dependencies"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert!(deps.contains(&"res://icon.png".to_string()), "dependency parsed: {deps:?}");
    let owners: Vec<String> = info["owners"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert!(
        owners.contains(&"res://scenes/level.tscn".to_string()),
        "owner (level.tscn references main.tscn) found: {owners:?}"
    );

    // A file without a UID header still gets a (synthesized) copyable UID.
    let lvl = json_ok(&http_get(port, "/api/filesystem/file_info?path=res://scenes/level.tscn"));
    assert!(
        lvl["uid"].as_str().unwrap().starts_with("uid://"),
        "synthesized uid for a header-less file: {lvl}"
    );

    // Duplicate copies the file next to itself with a derived name.
    let dup = json_ok(&http_post(port, "/api/filesystem/duplicate", r#"{"path":"res://scenes/main.tscn"}"#));
    assert_eq!(dup["new_path"], serde_json::json!("res://scenes/main_copy.tscn"));
    assert!(
        root.join("scenes/main_copy.tscn").is_file(),
        "duplicate produced a real file on disk"
    );

    // A second duplicate of the same source must not clobber the first.
    let dup2 = json_ok(&http_post(
        port,
        "/api/filesystem/duplicate",
        r#"{"path":"res://scenes/main.tscn","new_name":"main_v2.tscn"}"#,
    ));
    assert_eq!(dup2["new_path"], serde_json::json!("res://scenes/main_v2.tscn"));
    assert!(root.join("scenes/main_v2.tscn").is_file());

    handle.stop();
}
