//! App-level test for 3D rubber-band (marquee) multi-select — Godot's
//! spatial-editor left-drag box select. Boots the real `EditorServerHandle`,
//! posts a screen rectangle to `/api/viewport/3d/box_select`, and asserts the
//! `Node3D`s whose origins project inside it become the selection while an empty
//! region selects nothing.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::{node3d, SceneTree};
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

/// Scene: World(Node3D) → A,B (MeshInstance3D) both near the camera focus
/// (origin), so they project into the centre of the default editor viewport.
fn make_server() -> (EditorServerHandle, u16, u64, u64) {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node3D")).unwrap();
    let a = tree
        .add_child(world, Node::new("A", "MeshInstance3D"))
        .unwrap();
    let b = tree
        .add_child(world, Node::new("B", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, a, gdcore::math::Vector3::new(0.0, 0.0, 0.0));
    node3d::set_position(&mut tree, b, gdcore::math::Vector3::new(1.5, 0.0, 0.0));

    let port = free_port();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, a.raw(), b.raw())
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

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON body")
}

#[test]
fn box_select_selects_projected_nodes() {
    let (handle, port, a_raw, b_raw) = make_server();

    // A wide rectangle covering the whole viewport selects both nodes.
    let v = body_json(&http_post(
        port,
        "/api/viewport/3d/box_select",
        "{\"x1\":0,\"y1\":0,\"x2\":2000,\"y2\":2000,\"add\":false}",
    ));
    // `selected_nodes` is an array of raw ids (bare numbers), matching /select.
    let sel: Vec<u64> = v["selected_nodes"]
        .as_array()
        .expect("selected_nodes array")
        .iter()
        .map(|n| n.as_u64().unwrap())
        .collect();
    assert!(sel.contains(&a_raw), "wide box selects A (at the origin)");
    assert!(sel.contains(&b_raw), "wide box selects B (near the origin)");

    // A 2x2 rectangle in the top-left corner catches neither (both project to
    // the centre of the viewport), and replaces the previous selection.
    let v = body_json(&http_post(
        port,
        "/api/viewport/3d/box_select",
        "{\"x1\":0,\"y1\":0,\"x2\":2,\"y2\":2,\"add\":false}",
    ));
    assert_eq!(
        v["selected_nodes"].as_array().unwrap().len(),
        0,
        "an empty corner region clears the selection"
    );

    handle.stop();
}
