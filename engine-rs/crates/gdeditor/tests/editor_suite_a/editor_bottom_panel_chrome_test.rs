//! Visual-fidelity DOM tests for the editor bottom panel tab strip + toolbars.
//!
//! Godot's `EditorBottomPanel` always exposes Output / Debugger / Audio /
//! Animation-style tabs plus strip controls (pin/expand). Patina serves the
//! same permanent chrome in `#bottom-panel`.

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

fn editor_html(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET /editor HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

#[test]
fn bottom_panel_renders_tab_strip() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="bottom-panel""#,
        r#"id="bottom-panel-header""#,
        r#"data-tab="output""#,
        r#"data-tab="debugger""#,
        r#"data-tab="audio""#,
        r#"data-tab="animation""#,
        "Output",
        "Debugger",
        "Audio",
        "Animation",
    ] {
        assert!(
            html.contains(needle),
            "bottom panel missing tab strip chrome `{needle}`"
        );
    }
    handle.stop();
}

#[test]
fn bottom_panel_renders_strip_controls_and_output_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="bottom-pin""#,
        r#"id="bottom-expand""#,
        r#"id="bottom-panel-content""#,
        r#"id="output-clear""#,
        r#"id="output-search""#,
        "Filter Messages",
        r#"id="output-toolbar""#,
    ] {
        assert!(
            html.contains(needle),
            "bottom panel missing toolbar chrome `{needle}`"
        );
    }
    handle.stop();
}
