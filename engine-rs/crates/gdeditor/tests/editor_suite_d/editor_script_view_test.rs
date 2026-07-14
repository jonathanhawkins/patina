//! App-level regression test for pat-qtmbv.
//!
//! The editor's SCRIPT main-screen mode was cosmetic: clicking SCRIPT only
//! opened the small bottom Script panel; the central area kept showing the 2D
//! viewport. The fix gives the central area a script-editor container and
//! relocates the existing code editor into it in script mode. This test boots
//! the real server and asserts (1) the served `/editor` HTML carries the
//! central script-editor container wired into the mode switch, and (2) the
//! server reports a non-2D `script_editor` main view in script mode.

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
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn editor_script_mode_serves_script_editor_view() {
    let (_h, port) = make_server();

    // (1) The served editor must have a central script-editor container, wired
    // into the mode switch (so Script mode swaps the central view).
    let html = http_get(port, "/editor");
    let body = html.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        body.contains(r#"id="script-editor-panel""#),
        "served /editor must contain the central script-editor panel"
    );
    assert!(
        body.contains(r#"id="script-main-host""#),
        "central script editor must have a host element for the relocated editor"
    );
    assert!(
        body.contains("showScriptMode"),
        "the mode switch must drive a central script-editor view (showScriptMode)"
    );
    assert!(
        body.contains("showScriptMode(mode === 'script')"),
        "the mode-button handler must show the script editor when entering script mode"
    );

    // (2) The server reports a non-2D script_editor main view in script mode.
    let _ = http_post(port, "/api/editor/mode", r#"{"mode":"script"}"#);
    let main_view = http_get(port, "/api/editor/main_view");
    assert!(
        main_view.contains("\"is_2d_viewport\":false"),
        "script mode must not serve the 2D viewport; got: {main_view}"
    );
    assert!(
        main_view.contains("script"),
        "script mode main view should name the script editor component; got: {main_view}"
    );
}
