//! Network Profiler sub-tab parity (Godot EditorNetworkProfiler,
//! modules/multiplayer/editor/editor_network_profiler.cpp:318-446).
//!
//! Godot's EditorNetworkProfiler builds a FlowContainer toolbar — a Start/Stop
//! toggle, a Clear button, an Autostart checkbox, then right-aligned "Down" and
//! "Up" bandwidth read-outs — over an HSplitContainer holding two trees: the RPC
//! counters tree (Node | Incoming RPC | Outgoing RPC) and the replication tree
//! (Root | Synchronizer | Config | Count | Size). This boots the real editor
//! server over HTTP and asserts (a) the served panel renders all of that chrome,
//! (b) `/api/debug/info` publishes the full `network` payload, and (c) the
//! Start/Clear/Autostart actions drive the server state the way Godot's
//! `_activate_pressed` / `_clear_pressed` / `_autostart_toggled` do.

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
fn network_profiler_renders_godot_chrome() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    // Toolbar widgets the EditorNetworkProfiler constructor always builds.
    for needle in [
        "id=\"net-profiler-panel\"",
        "id=\"net-activate\"",
        "id=\"net-clear\"",
        "id=\"net-autostart\"",
        "&#9654; Start",
        "&#10006; Clear",
        "Autostart",
    ] {
        assert!(html.contains(needle), "Network Profiler toolbar missing `{needle}`");
    }

    // Down / Up bandwidth read-outs (incoming/outgoing LineEdits + labels).
    for needle in [
        "id=\"net-down-label\"",
        ">Down<",
        "id=\"net-up-label\"",
        ">Up<",
        "id=\"net-incoming\"",
        "id=\"net-outgoing\"",
    ] {
        assert!(html.contains(needle), "Network Profiler bandwidth chrome missing `{needle}`");
    }

    // RPC counters tree columns: Node | Incoming RPC | Outgoing RPC.
    for needle in ["id=\"net-rpc-body\"", ">Node<", ">Incoming RPC<", ">Outgoing RPC<"] {
        assert!(html.contains(needle), "RPC counters tree missing `{needle}`");
    }

    // Replication tree columns: Root | Synchronizer | Config | Count | Size.
    for needle in [
        "id=\"net-repl-body\"",
        ">Root<",
        ">Synchronizer<",
        ">Config<",
        ">Count<",
        ">Size<",
    ] {
        assert!(html.contains(needle), "Replication tree missing `{needle}`");
    }

    // JS wiring: rendering + humanize + the three toolbar endpoints.
    for needle in [
        "function renderNetworkProfiler",
        "function setupNetworkProfilerTab",
        "function humanizeSize",
        "/api/debug/network_profiler",
        "/api/debug/network_autostart",
        "/api/debug/network_clear",
    ] {
        assert!(html.contains(needle), "Network Profiler JS missing `{needle}`");
    }

    handle.stop();
}

#[test]
fn network_profiler_start_toggle_follows_run_state() {
    // Godot EditorNetworkProfiler::started()/stopped() (editor_network_profiler.cpp:211-225)
    // enable the Start toggle only while a game runs, honor Autostart on start,
    // and reset profiling off on stop. The browser mirrors that in
    // `syncNetProfilerRunState`, driven from `updatePlayButtonStates` so the
    // button tracks the live run state instead of only refreshing on tab open.
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    assert!(
        html.contains("function syncNetProfilerRunState"),
        "missing the started()/stopped() run-state sync helper"
    );
    // It must be invoked from the central run-state update so play/stop/poll all
    // refresh the Start toggle (not just tab activation).
    assert!(
        html.contains("syncNetProfilerRunState()"),
        "syncNetProfilerRunState must be called (from updatePlayButtonStates)"
    );
    // started(): enable the toggle and auto-begin profiling when Autostart is set.
    assert!(
        html.contains("act.disabled = false")
            && html.contains("net-autostart")
            && html.contains("enable: true"),
        "started() should enable the toggle and honor the Autostart checkbox"
    );

    handle.stop();
}

#[test]
fn network_profiler_info_payload_shape() {
    let (handle, port) = make_server();

    let info = body_json(&http_get(port, "/api/debug/info"));
    let net = &info["network"];
    assert!(net.is_object(), "debug/info publishes a `network` object");
    assert!(net["incoming_bandwidth"].is_number(), "network.incoming_bandwidth present (bytes/s)");
    assert!(net["outgoing_bandwidth"].is_number(), "network.outgoing_bandwidth present (bytes/s)");
    assert!(net["rpcs"].is_array(), "network.rpcs is the RPC counters list");
    assert!(net["syncs"].is_array(), "network.syncs is the replication list");
    assert_eq!(net["profiling"], serde_json::json!(false), "profiling starts off");
    assert_eq!(net["autostart"], serde_json::json!(false), "autostart starts off");

    handle.stop();
}

#[test]
fn network_profiler_toolbar_actions_drive_state() {
    let (handle, port) = make_server();

    // Start/Stop toggle (Godot _activate_pressed → enable_profiling).
    let r = body_json(&http_post(port, "/api/debug/network_profiler", r#"{"enable":true}"#));
    assert_eq!(r["profiling"], serde_json::json!(true), "enable=true turns profiling on");
    assert_eq!(
        body_json(&http_get(port, "/api/debug/info"))["network"]["profiling"],
        serde_json::json!(true),
        "profiling state is reflected in debug/info"
    );
    let r = body_json(&http_post(port, "/api/debug/network_profiler", r#"{"enable":false}"#));
    assert_eq!(r["profiling"], serde_json::json!(false), "enable=false turns profiling off");

    // Autostart checkbox (Godot _autostart_toggled → persisted project metadata).
    let r = body_json(&http_post(port, "/api/debug/network_autostart", r#"{"enabled":true}"#));
    assert_eq!(r["autostart"], serde_json::json!(true), "autostart persists when checked");
    assert_eq!(
        body_json(&http_get(port, "/api/debug/info"))["network"]["autostart"],
        serde_json::json!(true),
        "autostart state survives in debug/info"
    );

    // Clear (Godot _clear_pressed) reports the reset/empty state.
    let r = body_json(&http_post(port, "/api/debug/network_clear", "{}"));
    assert_eq!(r["ok"], serde_json::json!(true));
    assert!(r["rpcs"].as_array().unwrap().is_empty(), "clear empties the RPC list");
    assert!(r["syncs"].as_array().unwrap().is_empty(), "clear empties the replication list");

    handle.stop();
}
