//! End-to-end guard for `Scene > New Scene` and the "Create Root Node" flow,
//! against the real editor server (Godot `EditorNode::new_scene` +
//! `SceneTreeDock` create-root buttons).
//!
//! New Scene must produce a fresh, empty, untitled scene (no root node) in its
//! own active tab, and the Create Root Node buttons must add the matching root
//! node type (2D Scene -> Node2D, 3D Scene -> Node3D, User Interface -> Control)
//! under the window root, becoming the edited scene's root.

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
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..50 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 49 => thread::sleep(Duration::from_millis(100)),
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

fn scene(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/scene");
    assert!(resp.contains("200 OK"), "GET /api/scene ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("scene json: {e}"))
}

/// New Scene opens an empty, untitled, active tab whose root has no children —
/// the precondition for the Create Root Node panel to show.
#[test]
fn new_scene_opens_an_empty_untitled_active_tab() {
    let (_handle, port) = make_server();

    // The seeded scene has one child (Main).
    let before = scene(port);
    assert_eq!(
        before["nodes"]["children"].as_array().unwrap().len(),
        1,
        "seed scene has a Main child: {before}"
    );

    let resp = http_post(port, "/api/scene/new", "{}");
    assert!(resp.contains("200 OK"), "new scene ok: {resp}");
    let body: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        body["ok"].as_bool(),
        Some(true),
        "new scene reports ok: {body}"
    );

    // Tabs: a second, Untitled, unsaved tab is now active.
    let tabs: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene/tabs"))).unwrap();
    let arr = tabs["tabs"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "a new tab was added: {tabs}");
    let active = tabs["active_tab_index"].as_u64().unwrap() as usize;
    assert_eq!(active, 1, "the new scene tab is active: {tabs}");
    assert!(
        arr[active]["name"]
            .as_str()
            .unwrap()
            .starts_with("Untitled"),
        "the new tab is Untitled: {tabs}"
    );
    assert_eq!(
        arr[active]["path"].as_str(),
        Some(""),
        "no path yet: {tabs}"
    );

    // The active scene is empty (no root node) -> Create Root Node panel shows.
    let after = scene(port);
    assert!(
        after["nodes"]["children"]
            .as_array()
            .map(|c| c.is_empty())
            .unwrap_or(true),
        "the new scene has no root node yet: {after}"
    );
}

/// The Create Root Node buttons add the matching root type under the window
/// root, exactly as the served `createRootNode()` does.
#[test]
fn create_root_node_buttons_add_matching_root_type() {
    for (kind_class, expect_view) in [("Node2D", "2d"), ("Node3D", "3d"), ("Control", "2d")] {
        let (_handle, port) = make_server();
        // Start a fresh empty scene.
        assert!(http_post(port, "/api/scene/new", "{}").contains("200 OK"));

        let empty = scene(port);
        let root_id = empty["nodes"]["id"].as_u64().unwrap();
        assert!(
            empty["nodes"]["children"]
                .as_array()
                .map(|c| c.is_empty())
                .unwrap_or(true),
            "scene starts empty for {kind_class}: {empty}"
        );

        // Click the Create Root button -> add <class> under the window root.
        let add = http_post(
            port,
            "/api/node/add",
            &format!(
                r#"{{"parent_id":{root_id},"name":"{kind_class}","class_name":"{kind_class}"}}"#
            ),
        );
        assert!(add.contains("200 OK"), "add root {kind_class} ok: {add}");

        // The scene now has exactly that root node.
        let after = scene(port);
        let children = after["nodes"]["children"].as_array().unwrap();
        assert_eq!(
            children.len(),
            1,
            "one root created for {kind_class}: {after}"
        );
        assert_eq!(
            children[0]["class"].as_str(),
            Some(kind_class),
            "the root is a {kind_class}: {after}"
        );
        assert_eq!(
            children[0]["name"].as_str(),
            Some(kind_class),
            "the root is named after its type (Godot default): {after}"
        );

        // Godot switches the main screen to fit the new root; the server should
        // recommend the matching editor for a freshly rooted scene.
        assert_eq!(
            after["recommended_view"].as_str(),
            Some(expect_view),
            "a {kind_class} root recommends the {expect_view} editor: {after}"
        );
    }
}
