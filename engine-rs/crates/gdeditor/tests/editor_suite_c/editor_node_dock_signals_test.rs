//! Visual-fidelity test for the Node dock Signals tab's right-click context
//! menus + filter search icon (pat-9zrvd).
//!
//! Godot's `ConnectionsDock` builds a `signal_menu` (Connect... / Disconnect
//! All / Copy Name / Open Documentation) and a `slot_menu` (Edit... / Go to
//! Method / Disconnect), plus a "Filter Signals" search box with a right-aligned
//! Search icon (upstream/godot/editor/scene/connections_dialog.cpp). The Node
//! dock is rendered by the served inline script, so those menu labels and the
//! search widget ship in the `/editor` document.

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

/// The served Node dock carries the Signals tab's signal/connection context
/// menus and the Filter Signals box + Search icon, matching Godot.
#[test]
fn node_dock_signals_context_menus() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // signal_menu items (connections_dialog.cpp).
        "Disconnect All",
        "Copy Name",
        "Open Documentation",
        // slot_menu items.
        "Edit...",
        "Go to Method",
        // The Filter Signals box with its Search icon (built by the inline
        // node-dock script, so the id/class ship as JS string literals).
        "signal-search",
        "Filter Signals",
        "signal-search-icon",
    ] {
        assert!(
            html.contains(needle),
            "served Node dock Signals tab is missing `{needle}` (Godot's ConnectionsDock builds it)"
        );
    }

    // Both the signal "Disconnect All" and the slot "Disconnect" wire to the
    // disconnect endpoint.
    assert!(
        html.contains("/api/signal/disconnect"),
        "context-menu disconnect actions should call the disconnect endpoint"
    );

    handle.stop();
}
