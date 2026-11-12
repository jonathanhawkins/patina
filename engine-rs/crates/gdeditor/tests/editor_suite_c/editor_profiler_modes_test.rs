//! Profiler capture + measure/time modes (pat-wykfm).
//!
//! Godot's EditorProfiler has Start/Stop/Clear, a Measure dropdown (Frame Time
//! ms / Average Time ms / Frame % / Physics Frame %), a Time dropdown
//! (Inclusive/Self), a Frame# scrub, and a function tree of Name/Time/Calls
//! (editor_profiler.cpp). This drives the assembled server: capture controls +
//! recorded frames carry self/inclusive time + call counts, and the served
//! panel wires the modes + frame scrub to recompute the tree.

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
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

#[test]
fn profiler_modes_and_capture() {
    let (handle, port) = make_server();

    // Start capture.
    let start = body_json(&http_post(port, "/api/profiler/start", "{}"));
    assert_eq!(start["capturing"], serde_json::json!(true));

    // Record a frame whose functions carry inclusive/self time + call counts.
    let rec = r#"{"frame":1,"total_ms":16.0,"physics_ms":4.0,"functions":[{"name":"physics_step","time_ms":5.0,"self_ms":3.0,"calls":2},{"name":"render_2d","time_ms":8.0,"self_ms":8.0,"calls":1}]}"#;
    assert!(http_post(port, "/api/profiler/record", rec).contains("200 OK"));

    let prof = body_json(&http_get(port, "/api/profiler"));
    assert_eq!(prof["capturing"], serde_json::json!(true));
    assert_eq!(prof["count"], serde_json::json!(1));
    let frame = &prof["frames"][0];
    assert_eq!(
        frame["physics_ms"],
        serde_json::json!(4.0),
        "physics time captured for Physics Frame %"
    );
    let func = &frame["functions"][0];
    assert_eq!(func["name"], serde_json::json!("physics_step"));
    assert_eq!(func["time_ms"], serde_json::json!(5.0), "inclusive time");
    assert_eq!(
        func["self_ms"],
        serde_json::json!(3.0),
        "self time (Time: Self mode)"
    );
    assert_eq!(
        func["calls"],
        serde_json::json!(2),
        "call count (Calls column)"
    );

    // Clear empties the captured frames.
    assert!(http_post(port, "/api/profiler/clear", "{}").contains("200 OK"));
    assert_eq!(
        body_json(&http_get(port, "/api/profiler"))["count"],
        serde_json::json!(0)
    );

    // Stop ends capture.
    assert_eq!(
        body_json(&http_post(port, "/api/profiler/stop", "{}"))["capturing"],
        serde_json::json!(false)
    );
    assert_eq!(
        body_json(&http_get(port, "/api/profiler"))["capturing"],
        serde_json::json!(false)
    );

    // The served panel wires the measure/time modes + frame scrub.
    let html = http_get(port, "/editor");
    for needle in [
        "id=\"profiler-measure\"",
        "Average Time (ms)",
        "Frame %",
        "Physics Frame %",
        "id=\"profiler-time\"",
        ">Inclusive<",
        ">Self<",
        "id=\"profiler-frame\"",
        "function renderProfilerTree", // recompute from cached capture
        "/api/profiler/start",
        "/api/profiler/clear",
    ] {
        assert!(html.contains(needle), "profiler panel missing `{needle}`");
    }

    handle.stop();
}
