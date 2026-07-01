//! Output panel collapse-duplicates + message categories (pat-d6uah).
//!
//! Godot's EditorLog merges consecutive duplicate lines into one entry showing
//! the occurrence count (Collapse), and tags each message with a type
//! (std/error/warning/editor) that the type-filter buttons count
//! (editor_log.cpp:436,598-617). This asserts `/api/logs` tags each message
//! with a category, and the served output panel wires the collapse merge +
//! occurrence-count badge.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
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

fn http(port: u16, req: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http(port, &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

#[test]
fn output_panel_collapse_and_categories() {
    let (handle, port) = make_server();

    // Trigger an editor log line (adding to a group logs "Added group ...").
    let scene = body_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().expect("World id");
    let add = body_json(&http_post(port, "/api/node/groups/add", &format!(r#"{{"node_id":{world_id},"group":"enemies"}}"#)));
    assert_eq!(add["ok"], serde_json::json!(true));

    // /api/logs tags each message with a category; the editor action is "editor".
    let logs = body_json(&http_get(port, "/api/logs"));
    let arr = logs.as_array().expect("logs is an array");
    assert!(!arr.is_empty(), "there is at least one log entry");
    for e in arr {
        let cat = e["category"].as_str().expect("each log entry has a category");
        assert!(
            matches!(cat, "std" | "error" | "warning" | "editor"),
            "category is a known type, got {cat:?}"
        );
    }
    assert!(
        arr.iter().any(|e| e["category"] == serde_json::json!("editor")),
        "the editor action produced an 'editor'-category log"
    );

    // The served output panel wires collapse-merging + the occurrence badge.
    let html = http_get(port, "/editor");
    for needle in [
        "collapseLogs",                  // the collapse state
        "last.e.message === e.message",  // consecutive-duplicate merge
        "log-count",                     // the occurrence-count badge
        "e.category || logCat",          // category-driven type
    ] {
        assert!(html.contains(needle), "output panel missing collapse/category wiring `{needle}`");
    }

    handle.stop();
}
