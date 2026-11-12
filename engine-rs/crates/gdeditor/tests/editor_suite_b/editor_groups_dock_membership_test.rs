//! App-level test for the Groups dock node membership (pat-jwuv1.5).
//!
//! Godot's Node dock Groups tab lets you add a node to / remove it from named
//! groups, operating on the engine's real group membership (so it shows in the
//! scene-tree group badge and saves to the .tscn). This drives the assembled
//! `EditorServerHandle`: POST /api/node/groups/add|remove and assert the node's
//! groups in GET /api/scene track the changes.

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
    tree.add_child(root, Node::new("World", "Node2D")).unwrap();
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

fn json_ok(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// Find a node by name in the /api/scene tree and return its id + groups.
fn node_groups(port: u16, name: &str) -> (u64, Vec<String>) {
    let scene = json_ok(&http_get(port, "/api/scene"));
    fn walk(n: &serde_json::Value, name: &str) -> Option<(u64, Vec<String>)> {
        if n["name"] == serde_json::json!(name) {
            let groups = n["groups"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|g| g.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            return Some((n["id"].as_u64().unwrap(), groups));
        }
        for c in n["children"].as_array().into_iter().flatten() {
            if let Some(r) = walk(c, name) {
                return Some(r);
            }
        }
        None
    }
    walk(&scene["nodes"], name).expect("node found in scene")
}

/// pat-jwuv1.5: adding/removing a node from named groups updates its real
/// membership reflected in the scene.
#[test]
fn editor_groups_dock_membership() {
    let (handle, port) = make_server();

    let (world_id, groups0) = node_groups(port, "World");
    assert!(groups0.is_empty(), "World starts in no groups");

    // Add to "enemies".
    let add = json_ok(&http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{world_id},"group":"enemies"}}"#),
    ));
    assert_eq!(add["ok"], serde_json::json!(true));
    assert_eq!(add["added"], serde_json::json!(true));
    assert!(
        node_groups(port, "World")
            .1
            .contains(&"enemies".to_string()),
        "World is now in 'enemies'"
    );

    // Adding the same group again is idempotent.
    let again = json_ok(&http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{world_id},"group":"enemies"}}"#),
    ));
    assert_eq!(
        again["added"],
        serde_json::json!(false),
        "re-adding does not duplicate"
    );

    // Add a second group.
    json_ok(&http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{world_id},"group":"players"}}"#),
    ));
    let mut g = node_groups(port, "World").1;
    g.sort();
    assert_eq!(g, vec!["enemies".to_string(), "players".to_string()]);

    // Remove "enemies".
    let rem = json_ok(&http_post(
        port,
        "/api/node/groups/remove",
        &format!(r#"{{"node_id":{world_id},"group":"enemies"}}"#),
    ));
    assert_eq!(rem["ok"], serde_json::json!(true));
    let after = node_groups(port, "World").1;
    assert_eq!(
        after,
        vec!["players".to_string()],
        "only 'players' remains after removing 'enemies'"
    );

    handle.stop();
}
