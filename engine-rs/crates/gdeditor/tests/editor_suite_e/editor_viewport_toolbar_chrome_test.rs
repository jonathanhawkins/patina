//! Visual-fidelity DOM tests for 2D/3D viewport toolbars (umbrella for pat-9bh6y).
//!
//! Complements `editor_3d_toolbar_chrome_test` with 2D toolbar assertions and a
//! combined smoke that both toolbars ship in the served document.

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
fn viewport_toolbars_2d_and_3d_present() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="vp3d-toolbar""#,
        r#"id="vp3d-tool-move""#,
        r#"id="vp3d-tool-rotate""#,
        r#"id="vp3d-tool-scale""#,
        r#"id="vp3d-snap""#,
        r#"id="vp2d-toolbar""#,
        r#"id="vp2d-pan""#,
        r#"id="vp2d-grid-snap""#,
        r#"id="vp2d-local-space""#,
    ] {
        assert!(
            html.contains(needle),
            "viewport toolbar missing chrome `{needle}`"
        );
    }
    handle.stop();
}
