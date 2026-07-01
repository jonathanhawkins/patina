//! App-level undo/redo regression tests (pat-undoredo).
//!
//! Live probing found that redo of a node deletion failed ("not found: node N")
//! and that undo of a deletion lost the node's properties and child subtree
//! (it re-created a bare empty node under a fresh id, so redo targeted a stale
//! id). These boot the real editor server and drive undo/redo over HTTP,
//! asserting a delete is fully reversible: undo restores the node WITH its
//! properties and children, and redo removes it again without error.

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

/// Scene: root → World → Target(Node2D, position set) → Inner(Node2D).
fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();
    let mut target = Node::new("Target", "Node2D");
    target.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(123.0, 456.0)),
    );
    let target_id = tree.add_child(world, target).unwrap();
    tree.add_child(target_id, Node::new("Inner", "Node2D"))
        .unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_get_json(port: u16, path: &str) -> serde_json::Value {
    let resp = http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    );
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

/// Returns (status_line, body) for a POST.
fn http_post(port: u16, path: &str, body: &str) -> (String, String) {
    let resp = http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let status = resp.lines().next().unwrap_or("").to_string();
    let b = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, b)
}

/// All (name, id) pairs in the scene.
fn nodes(port: u16) -> Vec<(String, u64)> {
    let scene = http_get_json(port, "/api/scene");
    let mut out = Vec::new();
    fn walk(n: &serde_json::Value, out: &mut Vec<(String, u64)>) {
        if let (Some(name), Some(id)) = (n.get("name").and_then(|v| v.as_str()), n.get("id").and_then(|v| v.as_u64())) {
            out.push((name.to_string(), id));
        }
        if let Some(ch) = n.get("children").and_then(|v| v.as_array()) {
            for c in ch {
                walk(c, out);
            }
        }
    }
    let rootish = scene.get("nodes").unwrap_or(&scene);
    walk(rootish, &mut out);
    out
}

fn id_of(port: u16, name: &str) -> Option<u64> {
    nodes(port).into_iter().find(|(n, _)| n == name).map(|(_, i)| i)
}

/// The `position` of a node as `[x, y]`, via GET /api/node/<id>.
fn position_of(port: u16, id: u64) -> Option<Vec<f64>> {
    let node = http_get_json(port, &format!("/api/node/{id}"));
    let props = node.get("properties")?.as_array()?;
    for p in props {
        if p.get("name").and_then(|v| v.as_str()) == Some("position") {
            let arr = p.get("value")?.get("value")?.as_array()?;
            return Some(arr.iter().filter_map(|v| v.as_f64()).collect());
        }
    }
    None
}

#[test]
fn editor_delete_undo_redo_round_trips() {
    let (_h, port) = make_server();
    let target = id_of(port, "Target").expect("Target present");
    let base_count = nodes(port).len();
    assert!(id_of(port, "Inner").is_some(), "Inner child present initially");

    // Delete Target (and its Inner subtree).
    let (st, _) = http_post(port, "/api/node/delete", &format!(r#"{{"node_id":{target}}}"#));
    assert!(st.contains("200"), "delete should succeed: {st}");
    assert!(id_of(port, "Target").is_none(), "Target removed");
    assert!(id_of(port, "Inner").is_none(), "Inner subtree removed with parent");
    assert_eq!(nodes(port).len(), base_count - 2, "two nodes removed");

    // Undo: the node AND its child come back, with properties intact.
    let (st, _) = http_post(port, "/api/undo", "");
    assert!(st.contains("200"), "undo should succeed: {st}");
    let restored = id_of(port, "Target").expect("Target restored by undo");
    assert!(
        id_of(port, "Inner").is_some(),
        "undo must restore the child subtree, not just an empty node"
    );
    assert_eq!(nodes(port).len(), base_count, "node count fully restored");
    assert_eq!(
        position_of(port, restored),
        Some(vec![123.0, 456.0]),
        "undo must restore the node's properties (position), not reset them"
    );

    // Redo: the node is removed again — must NOT 500 with a stale-id error.
    let (st, body) = http_post(port, "/api/redo", "");
    assert!(
        st.contains("200"),
        "redo of a delete must succeed (regression: stale original id); got {st} {body}"
    );
    assert!(id_of(port, "Target").is_none(), "redo removes Target again");
    assert!(id_of(port, "Inner").is_none(), "redo removes the subtree again");

    // And undo once more restores it again (repeatable).
    let (st, _) = http_post(port, "/api/undo", "");
    assert!(st.contains("200"));
    assert!(id_of(port, "Target").is_some(), "second undo restores again");
}
