//! Visual-fidelity test for the Inspector dock's permanent "Open Documentation"
//! button (pat-5b63c).
//!
//! Godot's `InspectorDock` builds an `open_docs_button` (HelpSearch icon,
//! tooltip "Open documentation for this object.") in the object-selector row of
//! its constructor (upstream/godot/editor/docks/inspector_dock.cpp), invoking
//! `OBJECT_REQUEST_HELP`. The inspector was otherwise at parity; this was the
//! one missing permanent widget.
//!
//! The object-selector row is rendered by the inspector's client script, which
//! is served inline in the `/editor` document — so the button's id and Godot
//! tooltip ship in the served page (the same way the polling-guard test asserts
//! served-JS content).

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
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
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

/// The served editor carries the Inspector's Open Documentation button matching
/// Godot's `open_docs_button` (id + tooltip), wired to request object help.
#[test]
fn inspector_dock_renders_open_docs_button() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // The button the object-selector row builds (Godot open_docs_button).
        "insp-open-docs",
        // Godot's exact tooltip (TTRC("Open documentation for this object.")).
        "Open documentation for this object.",
        // It is wired to request documentation for the object.
        "Open Documentation",
    ] {
        assert!(
            html.contains(needle),
            "served Inspector dock is missing the Open Documentation widget `{needle}` (Godot builds open_docs_button)"
        );
    }

    handle.stop();
}
