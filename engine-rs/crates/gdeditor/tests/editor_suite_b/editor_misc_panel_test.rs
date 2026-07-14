//! Debugger "Misc" sub-tab parity with Godot's ScriptEditorDebugger.
//!
//! Godot's misc tab (editor/debugger/script_editor_debugger.cpp:2383-2432) is a
//! 2-column GridContainer of read-only "Clicked Control:" / "Clicked Control
//! Type:" fields, a "Live Edit Root:" field with "Set From Tree" / "Clear"
//! buttons, a separator, then an "Export measures as CSV" button. Live Edit Root
//! defaults to "/root" and resets to it on Clear (_live_edit_clear, :1603-1620).
//!
//! This drives the assembled editor server's debug endpoints and asserts the
//! served panel ships the matching form + wiring.

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
fn misc_panel_serves_godot_form() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    // The Godot-parity misc form: labels, read-only fields, the Live Edit Root
    // row with both buttons, and the export button.
    for needle in [
        "Clicked Control:",
        "Clicked Control Type:",
        "Live Edit Root:",
        "Set From Tree",
        ">Clear<",
        "Export measures as CSV",
        "id=\"dbg-clicked-ctrl\"",
        "id=\"dbg-clicked-ctrl-type\"",
        "id=\"dbg-live-edit-root\"",
        "id=\"dbg-le-set\"",
        "id=\"dbg-le-clear\"",
        "id=\"dbg-export-csv\"",
        "function setupMiscTab",
        "function exportMeasuresCsv",
        "/api/debug/live_edit_root",
    ] {
        assert!(html.contains(needle), "misc panel missing `{needle}`");
    }

    // The old ad-hoc text dump must be gone (1:1 with Godot, no "Frames Captured").
    assert!(!html.contains("Frames Captured"), "misc tab should not show Godot-absent 'Frames Captured'");

    handle.stop();
}

#[test]
fn misc_info_defaults_and_clicked_control_empty() {
    let (handle, port) = make_server();
    let info = body_json(&http_get(port, "/api/debug/info"));
    let misc = &info["misc"];
    assert!(misc.is_object(), "misc data present");
    // Live Edit Root defaults to "/root" (Godot default).
    assert_eq!(misc["live_edit_root"], serde_json::json!("/root"));
    // Clicked-control read-outs are empty until a control is clicked at runtime,
    // matching Godot's empty LineEdits.
    assert_eq!(misc["clicked_control"], serde_json::json!(""));
    assert_eq!(misc["clicked_control_type"], serde_json::json!(""));
    handle.stop();
}

#[test]
fn live_edit_root_set_and_clear() {
    let (handle, port) = make_server();

    // "Set From Tree" persists the selected node path.
    let set = body_json(&http_post(port, "/api/debug/live_edit_root", r#"{"path":"/root/World"}"#));
    assert_eq!(set["live_edit_root"], serde_json::json!("/root/World"));
    assert_eq!(
        body_json(&http_get(port, "/api/debug/info"))["misc"]["live_edit_root"],
        serde_json::json!("/root/World"),
        "set path is reflected in the misc info"
    );

    // "Clear" (empty/missing path) resets to "/root".
    let cleared = body_json(&http_post(port, "/api/debug/live_edit_root", r#"{"path":""}"#));
    assert_eq!(cleared["live_edit_root"], serde_json::json!("/root"));
    assert_eq!(
        body_json(&http_get(port, "/api/debug/info"))["misc"]["live_edit_root"],
        serde_json::json!("/root"),
        "clear resets the live edit root to /root"
    );

    handle.stop();
}
