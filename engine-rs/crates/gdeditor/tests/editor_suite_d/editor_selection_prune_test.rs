//! Regression test for pat-yysml: undo/delete must not leave a dangling node id
//! in the selection set. The single-selection endpoint (`/api/selected`) and the
//! multi-selection endpoint (`/api/selected_nodes`) must always agree — they can
//! never report a node that no longer exists in the scene tree.
//!
//! App-level (HTTP) test per project methodology: it boots the real editor server
//! and drives it over the wire, exactly like a UI client / gdscript automation.

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

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
    );
    tree.add_child(root, main).unwrap();
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

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

/// The canonical multi-selection set as seen by dock/viewport/inspector.
fn selected_nodes(port: u16) -> Vec<u64> {
    let resp = http_get(port, "/api/selected_nodes");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["selected_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap())
        .collect()
}

/// The single-selection object as seen by the Inspector (`/api/selected`).
/// Returns the JSON value (which is `null` when nothing is selected).
fn selected_single(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/selected");
    serde_json::from_str(extract_body(&resp)).unwrap()
}

#[test]
fn editor_selection_pruned_on_undo_and_delete() {
    let (handle, port) = make_server();
    let parent = get_main_node_id(port);

    // ---- Case 1: undo of a node add must prune the auto-selected ghost id ----
    let add = http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{parent},"name":"ProbeNode","class_name":"Node2D"}}"#),
    );
    assert!(add.contains("200 OK"), "add node should succeed: {add}");
    let added_id = serde_json::from_str::<serde_json::Value>(extract_body(&add)).unwrap()["id"]
        .as_u64()
        .expect("add returns the new node id");

    // Adding a node focuses it: both endpoints agree it is selected.
    assert_eq!(
        selected_nodes(port),
        vec![added_id],
        "the new node becomes the multi-selection"
    );
    assert_eq!(
        selected_single(port)["id"].as_u64(),
        Some(added_id),
        "the new node is the single selection (/api/selected) too"
    );

    // Undo the add: the node is gone from the scene, so its id must NOT linger
    // in the selection. This is the core bug (pat-yysml).
    assert!(
        http_post(port, "/api/undo", "").contains("200 OK"),
        "undo should succeed"
    );
    assert!(
        selected_nodes(port).is_empty(),
        "after undoing the add, /api/selected_nodes must not contain the deleted id, got {:?}",
        selected_nodes(port)
    );
    assert!(
        selected_single(port).is_null(),
        "after undoing the add, /api/selected must be null"
    );
    // The two endpoints must AGREE — neither may report a ghost node.
    assert_eq!(
        selected_nodes(port).len(),
        if selected_single(port).is_null() {
            0
        } else {
            1
        },
        "/api/selected and /api/selected_nodes must never disagree"
    );

    // ---- Case 2: undo of a duplicate must prune the auto-selected ghost id ----
    // (comment #937: duplicate auto-selects the new node; undo removes it.)
    let dup = http_post(
        port,
        "/api/node/duplicate",
        &format!(r#"{{"node_id":{parent}}}"#),
    );
    assert!(dup.contains("200 OK"), "duplicate should succeed: {dup}");
    let dup_id = serde_json::from_str::<serde_json::Value>(extract_body(&dup)).unwrap()["id"]
        .as_u64()
        .expect("duplicate returns the new node id");
    assert_eq!(
        selected_nodes(port),
        vec![dup_id],
        "the duplicate becomes the selection"
    );
    assert!(
        http_post(port, "/api/undo", "").contains("200 OK"),
        "undo of duplicate should succeed"
    );
    assert!(
        selected_nodes(port).is_empty(),
        "after undoing the duplicate, the dangling id must be pruned, got {:?}",
        selected_nodes(port)
    );
    assert!(
        selected_single(port).is_null(),
        "after undoing the duplicate, /api/selected must be null"
    );

    // ---- Case 3: delete path must also leave no dangling selection ----
    let add2 = http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{parent},"name":"ToDelete","class_name":"Node2D"}}"#),
    );
    assert!(add2.contains("200 OK"), "second add should succeed: {add2}");
    let del_id = serde_json::from_str::<serde_json::Value>(extract_body(&add2)).unwrap()["id"]
        .as_u64()
        .expect("add returns the new node id");
    // Explicitly select it (it is already auto-selected, but be defensive).
    http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{del_id}}}"#),
    );
    assert_eq!(
        selected_nodes(port),
        vec![del_id],
        "node is selected before delete"
    );

    let del = http_post(
        port,
        "/api/node/delete",
        &format!(r#"{{"node_id":{del_id}}}"#),
    );
    assert!(del.contains("200 OK"), "delete should succeed: {del}");
    assert!(
        selected_nodes(port).is_empty(),
        "deleting the selected node must clear the selection, got {:?}",
        selected_nodes(port)
    );
    assert!(
        selected_single(port).is_null(),
        "after delete, /api/selected must be null"
    );

    handle.stop();
}
