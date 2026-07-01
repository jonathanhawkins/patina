//! Regression test for pat-w1ub4.7: the active scene tab must show the loaded
//! scene's filename, not "Untitled".
//!
//! This exercises the STARTUP path that the editor entrypoint
//! (`examples/editor.rs`) actually uses: a scene is installed into the tree and
//! `scene_file` is set on `EditorState` directly, WITHOUT going through
//! `POST /api/scene/load`. The bug was that `api_get_scene_tabs` served the
//! default `{ name: "Untitled", path: "" }` tab even though `scene_file` (and
//! `/api/scene/info`) reported a real scene. App-level: boots the real editor
//! server over HTTP and asserts on the JSON the browser UI consumes.

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

/// Boots an editor server whose `EditorState` has `scene_file` pre-set, exactly
/// like `examples/editor.rs` does at startup (it installs the scene and sets
/// `state.scene_file = Some(path)` before `EditorServerHandle::start`). Returns
/// the handle, the bound port, the full scene path, and its basename.
fn make_server_with_loaded_scene() -> (EditorServerHandle, u16, String, String) {
    let port = free_port();

    // A real scene on disk, mirroring fixtures/scenes/demo_2d.tscn in spirit.
    let scene_path = std::env::temp_dir().join(format!("tab_label_startup_{port}.tscn"));
    std::fs::write(
        &scene_path,
        "[gd_scene load_steps=1 format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"Child\" type=\"Sprite2D\" parent=\".\"]\n",
    )
    .unwrap();
    let scene_path_str = scene_path.to_string_lossy().to_string();
    let basename = scene_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Build a scene tree (contents are irrelevant to the tab label; what matters
    // is that scene_file is set, as the example does).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Root", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
    );
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    // The crux: the startup path sets scene_file directly and does NOT relabel
    // the tab via the load endpoint.
    state.scene_file = Some(scene_path_str.clone());

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, scene_path_str, basename)
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

#[allow(dead_code)]
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

/// At startup with a scene loaded, `GET /api/scene/tabs` must report the active
/// tab's name as the scene's filename (with its path set and not modified), and
/// it must agree with `GET /api/scene/info`'s `scene_file`. This is the exact
/// mismatch the bead reported: tabs said "Untitled" while info said the file.
#[test]
fn editor_tab_label_reflects_scene_filename() {
    let (handle, port, scene_path_str, basename) = make_server_with_loaded_scene();

    // /api/scene/info already reports the loaded file at startup (control).
    let info = http_get(port, "/api/scene/info");
    let iv: serde_json::Value = serde_json::from_str(extract_body(&info)).unwrap();
    assert_eq!(
        iv["scene_file"].as_str(),
        Some(scene_path_str.as_str()),
        "precondition: scene/info should report the loaded scene at startup: {iv}"
    );

    // The active tab must reflect that same scene, not the placeholder.
    let tabs = http_get(port, "/api/scene/tabs");
    let tv: serde_json::Value = serde_json::from_str(extract_body(&tabs)).unwrap();
    let active = tv["active_tab_index"].as_u64().unwrap() as usize;
    let tab = &tv["tabs"][active];

    assert_eq!(
        tab["name"].as_str(),
        Some(basename.as_str()),
        "active tab label should be the scene filename at startup, not 'Untitled': {tv}"
    );
    assert_ne!(
        tab["name"].as_str(),
        Some("Untitled"),
        "tab must not read 'Untitled' while a scene is loaded: {tv}"
    );
    assert_eq!(
        tab["path"].as_str(),
        Some(scene_path_str.as_str()),
        "active tab path should point at the loaded scene: {tv}"
    );
    assert_eq!(
        tab["modified"].as_bool(),
        Some(false),
        "a freshly loaded scene is not modified: {tv}"
    );

    // And the two surfaces must agree (the original bug was their disagreement).
    assert_eq!(
        tab["path"].as_str(),
        iv["scene_file"].as_str(),
        "scene/tabs active path must match scene/info scene_file: tabs={tv} info={iv}"
    );

    std::fs::remove_file(&scene_path_str).ok();
    handle.stop();
}

/// Guard: with no scene loaded, the lone tab must still read "Untitled" (the
/// fix must not fabricate a filename when `scene_file` is unset).
#[test]
fn editor_tab_label_untitled_when_no_scene_loaded() {
    let port = free_port();
    let tree = SceneTree::new();
    let state = EditorState::new(tree); // scene_file stays None
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let tabs = http_get(port, "/api/scene/tabs");
    let tv: serde_json::Value = serde_json::from_str(extract_body(&tabs)).unwrap();
    let active = tv["active_tab_index"].as_u64().unwrap() as usize;
    let tab = &tv["tabs"][active];
    assert_eq!(
        tab["name"].as_str(),
        Some("Untitled"),
        "with no scene loaded the tab stays 'Untitled': {tv}"
    );
    assert_eq!(
        tab["path"].as_str(),
        Some(""),
        "an unsaved tab has an empty path: {tv}"
    );

    handle.stop();
}
