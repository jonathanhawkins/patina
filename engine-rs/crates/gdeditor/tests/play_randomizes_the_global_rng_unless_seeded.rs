//! Godot 4 randomizes the global RNG at startup, so a game that never calls
//! `randomize()` still plays differently each run. Patina's generator started
//! at a fixed seed and nothing reseeded it, so every Play of the official demo
//! "Dodge the Creeps" spawned the same enemies in the same places.
//!
//! A `{"seed": n}` Play pins the stream instead — what the agent interface's
//! frame-exact `play` sends, so a recorded timeline replays the same game.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn http(port: u16, request: &str) -> String {
    let mut stream = (0..40)
        .find_map(|_| {
            TcpStream::connect(format!("127.0.0.1:{port}"))
                .ok()
                .or_else(|| {
                    thread::sleep(Duration::from_millis(50));
                    None
                })
        })
        .expect("connect");
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    resp
}

fn post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// `rolled` as the running script reports it.
fn rolled(port: u16) -> i64 {
    let resp = http(
        port,
        "GET /api/runtime/node?path=/root/Main HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    let body: serde_json::Value =
        serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap();
    body["properties"]
        .as_array()
        .and_then(|props| props.iter().find(|p| p["name"] == "rolled"))
        .and_then(|p| p["value"]["value"].as_i64())
        .unwrap_or_else(|| panic!("`rolled` missing: {body}"))
}

/// One Play with `body`, returning what the script drew in `_ready`.
fn play_once(port: u16, body: &str) -> i64 {
    assert!(post(port, "/api/runtime/play", body).contains("200 OK"));
    post(port, "/api/runtime/pause", "");
    let value = rolled(port);
    post(port, "/api/runtime/stop", "");
    value
}

#[test]
fn play_randomizes_the_rng_and_a_seed_pins_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.gd"),
        "extends Node\nvar rolled = 0\nfunc _ready():\n    self.rolled = randi()\n",
    )
    .unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node");
    main.set_property("_script_path", Variant::String("main.gd".into()));
    tree.add_child(root, main).unwrap();
    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Unseeded: three runs, not all the same (a collision of all three on a
    // 63-bit draw would be astronomically unlikely).
    let runs = [
        play_once(port, "{}"),
        play_once(port, "{}"),
        play_once(port, "{}"),
    ];
    assert!(
        runs[0] != runs[1] || runs[1] != runs[2],
        "every unseeded Play drew the same number: {runs:?}"
    );

    // Seeded: identical.
    let a = play_once(port, r#"{"seed": 7}"#);
    let b = play_once(port, r#"{"seed": 7}"#);
    assert_eq!(a, b, "the same seed replays the same stream");

    handle.stop();
}
