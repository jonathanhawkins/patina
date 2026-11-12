//! pat-nzgdu.105: Quick Open (fuzzy) dialog + Batch Rename (counter pattern).
//!
//! Guards:
//!  1. Static — EDITOR_HTML defines the Quick Open dialog + `quickOpen` fuzzy
//!     search and the `batchRename` wiring against `/api/node/batch_rename`.
//!  2. Functional — `POST /api/node/batch_rename` with a `node_%d` pattern
//!     renames a 3-node selection to node_1 / node_2 / node_3 in the scene tree.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guards.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_quick_open_and_batch_rename() {
    assert!(
        EDITOR_HTML.contains("quick-open-dialog"),
        "EDITOR_HTML must contain the Quick Open dialog"
    );
    assert!(
        EDITOR_HTML.contains("quickOpen"),
        "EDITOR_HTML must define quickOpen (fuzzy file search over /api/filesystem)"
    );
    assert!(
        EDITOR_HTML.contains("/api/node/batch_rename"),
        "EDITOR_HTML must reference the batch-rename endpoint"
    );
    assert!(
        EDITOR_HTML.contains("batchRename"),
        "EDITOR_HTML must define the batchRename wiring"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional: batch_rename applies the counter pattern.
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

#[test]
fn batch_rename_applies_counter_pattern() {
    let port = free_port();

    // Scene: root -> Foo, Bar, Baz.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("Foo", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("Bar", "Node2D")).unwrap();
    let c = tree.add_child(root, Node::new("Baz", "Node2D")).unwrap();

    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    // Batch rename the 3-node selection with the `node_%d` counter pattern.
    let body = format!(
        r#"{{"node_ids":[{},{},{}],"pattern":"node_%d"}}"#,
        a.raw(),
        b.raw(),
        c.raw()
    );
    let resp = http_post(port, "/api/node/batch_rename", &body);
    assert!(
        resp.contains("200 OK"),
        "batch_rename should 200, got:\n{resp}"
    );

    // The selection is renamed node_1 / node_2 / node_3 in scene order.
    let g = handle.state().lock().unwrap();
    assert_eq!(
        g.scene_tree.get_node(a).map(|n| n.name().to_string()),
        Some("node_1".to_string())
    );
    assert_eq!(
        g.scene_tree.get_node(b).map(|n| n.name().to_string()),
        Some("node_2".to_string())
    );
    assert_eq!(
        g.scene_tree.get_node(c).map(|n| n.name().to_string()),
        Some("node_3".to_string())
    );
    drop(g);

    handle.stop();
}
