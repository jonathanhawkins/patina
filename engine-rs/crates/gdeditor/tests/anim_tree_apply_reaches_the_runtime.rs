//! pat-s3rrv.1: the authored animation graph reaches a live AnimationTree.
//!
//! # The gap this closes
//!
//! `AnimTreeGraph` was pure editor state — 16 route call sites mutated it, one
//! GET read it back, and **nothing ever built an `AnimationNode` from it**.
//! Every state, transition and blend point authored in the served editor
//! evaporated while each route returned 200. `to_state_machine()` converted the
//! graph but had no caller for three increments; this route is the caller, and
//! `SceneTree::process_animation_trees` drives whatever it attaches.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            return s;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("editor server never accepted a connection on {port}");
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut s = connect_with_retry(port);
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).expect("write");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

/// A scene with a real `AnimationTree` node to attach to.
fn boot() -> (EditorServerHandle, u16) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    tree.add_child(main, Node::new("Tree", "AnimationTree"))
        .unwrap();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn author_a_graph(port: u16) {
    for (name, kind) in [("idle", "animation"), ("run", "animation")] {
        let resp = http_post(
            port,
            "/api/animation/tree/state/add",
            &format!(r#"{{"name":"{name}","kind":"{kind}"}}"#),
        );
        assert!(resp.contains("200 OK"), "state/add failed: {resp}");
    }
    let resp = http_post(
        port,
        "/api/animation/tree/transition/add",
        r#"{"from":"idle","to":"run","xfade":0.25}"#,
    );
    assert!(resp.contains("200 OK"), "transition/add failed: {resp}");
}

/// **The acceptance case.** What the editor authored is what the runtime gets.
#[test]
fn applying_the_graph_attaches_a_state_machine() {
    let (handle, port) = boot();
    author_a_graph(port);

    let resp = http_post(port, "/api/animation/tree/apply", r#"{"node":"Tree"}"#);
    assert!(
        resp.contains("200 OK"),
        "POST /api/animation/tree/apply should return 200, got:\n{resp}"
    );
    assert!(
        resp.contains(r#""states":2"#),
        "both authored states must reach the machine; got:\n{resp}"
    );
    assert!(
        resp.contains(r#""transitions":1"#),
        "and the authored transition; got:\n{resp}"
    );

    drop(handle);
}

/// ⚠️ The target must be an `AnimationTree`. Attaching to anything else would
/// store a graph `process_animation_trees` never looks at — the same silent
/// no-op this route exists to remove, reintroduced one level down.
#[test]
fn applying_to_a_non_animation_tree_is_refused() {
    let (handle, port) = boot();
    author_a_graph(port);

    let resp = http_post(port, "/api/animation/tree/apply", r#"{"node":"Main"}"#);
    assert!(
        resp.contains("400"),
        "attaching to a Node2D must be refused, got:\n{resp}"
    );
    assert!(
        !resp.contains(r#""ok":true"#),
        "and must not report success:\n{resp}"
    );

    drop(handle);
}

/// A name that does not exist is a 404, not a silent success.
#[test]
fn applying_to_a_missing_node_is_a_404() {
    let (handle, port) = boot();

    let resp = http_post(port, "/api/animation/tree/apply", r#"{"node":"Nope"}"#);
    assert!(resp.contains("404"), "expected 404, got:\n{resp}");

    drop(handle);
}
