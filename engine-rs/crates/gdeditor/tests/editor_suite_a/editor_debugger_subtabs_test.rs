//! Debugger sub-tabs live data + session actions (pat-j906k).
//!
//! Godot's ScriptEditorDebugger populates each sub-tab when a session is active
//! (script_editor_debugger.cpp:2062-2386): the Errors list, the Evaluator, the
//! Visual Profiler / Video RAM / Misc / Network Profiler, and the Break /
//! Skip-Breakpoints / Copy-Error actions. This drives the assembled server's
//! debug endpoints and asserts the served panel wires the sub-tabs.

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
fn debugger_subtabs_show_live_data() {
    let (handle, port) = make_server();

    // Errors tab: record a runtime error/warning, list it, clear it.
    assert!(http_post(
        port,
        "/api/debug/record_error",
        r#"{"level":"error","message":"Null instance"}"#
    )
    .contains("200 OK"));
    assert!(http_post(
        port,
        "/api/debug/record_error",
        r#"{"level":"warning","message":"Deprecated call"}"#
    )
    .contains("200 OK"));
    let errs = body_json(&http_get(port, "/api/debug/errors"));
    let arr = errs["errors"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "Errors tab lists recorded errors/warnings");
    assert_eq!(arr[0]["level"], serde_json::json!("error"));
    assert_eq!(arr[1]["level"], serde_json::json!("warning"));

    // Evaluator: evaluate an expression.
    let ev = body_json(&http_post(
        port,
        "/api/debug/evaluate",
        r#"{"expression":"2 + 3"}"#,
    ));
    assert_eq!(
        ev["result"],
        serde_json::json!("5"),
        "Evaluator computes expressions"
    );

    // Break + Skip-Breakpoints act on the session.
    assert_eq!(
        body_json(&http_post(port, "/api/debug/break", "{}"))["state"],
        serde_json::json!("break")
    );
    assert_eq!(
        body_json(&http_post(
            port,
            "/api/debug/skip_breakpoints",
            r#"{"enabled":true}"#
        ))["skip_breakpoints"],
        serde_json::json!(true)
    );

    // Video RAM / Misc / Network sub-tab data.
    let info = body_json(&http_get(port, "/api/debug/info"));
    assert!(
        !info["video_ram"].as_array().unwrap().is_empty(),
        "Video RAM lists resources"
    );
    assert!(info["misc"].is_object(), "Misc data present");
    assert!(info["network"].is_object(), "Network data present");

    // Clear empties the Errors list.
    assert!(http_post(port, "/api/debug/clear_errors", "{}").contains("200 OK"));
    assert_eq!(
        body_json(&http_get(port, "/api/debug/errors"))["errors"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // The served panel wires the sub-tab data loads + Evaluator + actions.
    let html = http_get(port, "/editor");
    for needle in [
        "function loadDebuggerSubtab",
        "function renderDebugErrors",
        "function renderDebugInfo",
        "id=\"dbg-eval-input\"",
        "id=\"dbg-eval-run\"",
        "/api/debug/evaluate",
        "/api/debug/skip_breakpoints",
        "/api/debug/info",
        "id=\"dbg-vram-list\"",
        "id=\"net-rpc-body\"",
    ] {
        assert!(html.contains(needle), "debugger panel missing `{needle}`");
    }

    handle.stop();
}
