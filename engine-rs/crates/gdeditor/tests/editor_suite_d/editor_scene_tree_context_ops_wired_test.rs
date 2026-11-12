//! pat-nzgdu.4 — scene-tree Change-Type / Make-Local / Editable-Children /
//! Access-as-Unique-Name / Lock / Warnings wired into the served context menu.
//!
//! Guards the client-wiring gap (a "false-done": the server handlers all exist
//! and pass server-side tests, but the served `EDITOR_HTML` never called them —
//! green tests, dead UI). Part 1 inspects the static `EDITOR_HTML` constant and
//! asserts it now issues an `api('POST', ...)` call for each of the five
//! endpoints (plus the warnings GET) reachable from the right-click menu. Part 2
//! boots the real editor server and drives add-node -> change_type ->
//! `/api/scene` reflecting the new class, proving the wired endpoint mutates the
//! live scene tree.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

// ---- Part 1: the served EDITOR_HTML actually calls each endpoint ----

#[test]
fn editor_html_wires_all_scene_tree_context_ops() {
    // Each of the five ops issues an api('POST', '<endpoint>', ...) call —
    // previously all five counts were zero (dead UI over green server tests).
    for endpoint in [
        "/api/node/change_type",
        "/api/node/make_local",
        "/api/node/editable_children",
        "/api/node/unique_name",
        "/api/node/lock",
    ] {
        let call = format!("api('POST', '{endpoint}'");
        assert!(
            HTML.contains(&call),
            "EDITOR_HTML must issue {call}...) — endpoint {endpoint} is otherwise dead UI"
        );
    }

    // The warning badge fetches per-node configuration warnings.
    assert!(
        HTML.contains("/api/node/warnings"),
        "EDITOR_HTML must fetch /api/node/warnings for the warning badge"
    );

    // Each op is reachable from the right-click context menu.
    for action in [
        "data-action=\"change-type\"",
        "data-action=\"make-local\"",
        "data-action=\"editable-children\"",
        "data-action=\"unique-name\"",
        "data-action=\"lock\"",
    ] {
        assert!(
            HTML.contains(action),
            "the #context-menu must offer a menu item with {action}"
        );
    }
}

// ---- Part 2: change_type actually mutates the live scene tree ----

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
            Err(e) => panic!("failed to connect to editor server: {e}"),
        }
    }
    unreachable!()
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn change_type_mutates_scene_tree_class() {
    let (_handle, port) = make_server();

    let scene: serde_json::Value =
        serde_json::from_str(body_of(&http_get(port, "/api/scene"))).unwrap();
    let main_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add a Node2D child, then change its type to Sprite2D via the endpoint the
    // context menu now calls.
    let add = http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{main_id},"name":"Sub","class_name":"Node2D"}}"#),
    );
    let sub_id = serde_json::from_str::<serde_json::Value>(body_of(&add)).unwrap()["id"]
        .as_u64()
        .expect("added node returns an id");

    let resp = http_post(
        port,
        "/api/node/change_type",
        &format!(r#"{{"node_id":{sub_id},"class_name":"Sprite2D"}}"#),
    );
    assert!(
        resp.contains("200 OK"),
        "change_type should return 200: {resp}"
    );

    // /api/node/<id> now reflects the new class_name.
    let scene: serde_json::Value =
        serde_json::from_str(body_of(&http_get(port, "/api/scene"))).unwrap();
    let sub = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_u64() == Some(sub_id))
        .expect("Sub retained in place after change_type");
    assert_eq!(sub["class"], "Sprite2D", "node class changed to Sprite2D");
    assert_eq!(sub["name"], "Sub", "node name retained through change_type");
}
