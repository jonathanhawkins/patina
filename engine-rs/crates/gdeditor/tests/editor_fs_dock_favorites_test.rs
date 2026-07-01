//! App-level integration test for the FileSystem dock **Favorites** feature
//! (pat-k6600), driving the assembled `EditorServerHandle` over HTTP rather
//! than poking the model directly.
//!
//! Godot's `FileSystemDock` lets you right-click a directory to favorite it;
//! favorited dirs are pinned under the always-present "★ Favorites:" header and
//! persist across editor reloads/restarts, written one `res://` path per line
//! to `<project>/.godot/editor/favorites`
//! (upstream/godot/editor/settings/editor_settings.cpp `set_favorites`).
//!
//! This test exercises the full served surface: open a project, favorite a
//! directory via `POST /api/filesystem/favorite`, confirm it appears in
//! `GET /api/filesystem`'s `favorites` array AND landed on disk, simulate a
//! reload by re-opening the project (which reloads favorites from disk), and
//! finally unfavorite it and confirm it disappears from both the API and disk.

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

/// A temp project with a `project.godot` manifest and a `scenes/` directory
/// holding a real `.tscn` (so the directory shows up in the scan and exists on
/// disk for the favorite's `is_dir` check).
fn make_project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("project.godot"), "[application]\n").unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("scenes/main.tscn"),
        "[gd_scene format=3]\n",
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

/// The `favorites` array from `GET /api/filesystem`.
fn favorites(port: u16) -> Vec<serde_json::Value> {
    let v = json_ok(&http_get(port, "/api/filesystem"));
    v["favorites"]
        .as_array()
        .expect("filesystem response has a favorites array")
        .clone()
}

fn fav_paths(port: u16) -> Vec<String> {
    favorites(port)
        .iter()
        .filter_map(|f| f["path"].as_str().map(str::to_string))
        .collect()
}

/// pat-k6600 acceptance: right-clicking a directory adds/removes a favorite;
/// favorited dirs appear under the ★ Favorites header and persist across
/// reloads; unfavoriting removes them.
#[test]
fn fs_dock_favorites_pin_and_persist() {
    let (handle, port) = make_server();
    let project = make_project();
    let root = project.path().to_path_buf();

    // Root the editor at the project so the dock lists its res:// tree.
    let opened = json_ok(&http_post(
        port,
        "/api/project/open",
        &format!(
            r#"{{"path":"{}"}}"#,
            root.display().to_string().replace('\\', "\\\\")
        ),
    ));
    assert_eq!(opened["ok"], serde_json::json!(true), "project should open");

    // Initially there are no favorites.
    assert!(
        fav_paths(port).is_empty(),
        "a fresh project starts with no favorites"
    );

    // Favorite the `scenes` directory (the right-click "Add to Favorites").
    let added = json_ok(&http_post(
        port,
        "/api/filesystem/favorite",
        r#"{"path":"res://scenes","action":"add"}"#,
    ));
    assert_eq!(added["is_favorite"], serde_json::json!(true));

    // It now appears under the Favorites header, flagged as a directory.
    let favs = favorites(port);
    assert_eq!(favs.len(), 1, "exactly one favorite pinned");
    assert_eq!(favs[0]["path"], serde_json::json!("res://scenes"));
    assert_eq!(favs[0]["name"], serde_json::json!("scenes"));
    assert_eq!(
        favs[0]["is_dir"],
        serde_json::json!(true),
        "favorited dir is marked is_dir so the folder icon shows"
    );

    // ...and it was persisted to Godot's per-project favorites file.
    let fav_file = root.join(".godot").join("editor").join("favorites");
    let on_disk = std::fs::read_to_string(&fav_file).expect("favorites file written");
    assert!(
        on_disk.lines().any(|l| l.trim() == "res://scenes"),
        "favorite persisted to {fav_file:?}; contents: {on_disk:?}"
    );

    // Reload the editor (re-open the same project). The dock reloads favorites
    // from disk, so the pin survives.
    let reopened = json_ok(&http_post(
        port,
        "/api/project/open",
        &format!(
            r#"{{"path":"{}"}}"#,
            root.display().to_string().replace('\\', "\\\\")
        ),
    ));
    assert_eq!(reopened["ok"], serde_json::json!(true));
    assert_eq!(
        fav_paths(port),
        vec!["res://scenes".to_string()],
        "favorite persists across an editor reload"
    );

    // Unfavorite it — it leaves both the API and the on-disk file.
    let removed = json_ok(&http_post(
        port,
        "/api/filesystem/favorite",
        r#"{"path":"res://scenes","action":"remove"}"#,
    ));
    assert_eq!(removed["is_favorite"], serde_json::json!(false));
    assert!(
        fav_paths(port).is_empty(),
        "unfavoriting removes it from the Favorites header"
    );
    let after = std::fs::read_to_string(&fav_file).unwrap_or_default();
    assert!(
        !after.lines().any(|l| l.trim() == "res://scenes"),
        "unfavoriting rewrites the favorites file without the entry; got {after:?}"
    );

    handle.stop();
}
