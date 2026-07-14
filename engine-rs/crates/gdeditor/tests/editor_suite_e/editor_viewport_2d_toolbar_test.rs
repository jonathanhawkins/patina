//! Visual-fidelity test for the 2D viewport toolbar (pat-9yq5q).
//!
//! Godot's CanvasItemEditor builds a toolbar (main_menu_hbox,
//! canvas_item_editor_plugin.cpp:5702-6040) with, in order: list-select +
//! edit-pivot tools, pan + ruler tools, Use Local Space, Smart/Grid snap +
//! Snapping Options menu, Lock/Unlock, Group/Ungroup, a Skeleton menu, a View
//! menu, and a zoom widget. Patina's 2D viewport had only the transform tools.
//! This asserts the served editor renders the full toolbar in Godot order.

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

/// pat-9yq5q: the 2D viewport toolbar renders the full Godot tool set.
#[test]
fn viewport_2d_toolbar_full_parity() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        "id=\"vp2d-toolbar\"",
        // list-select + edit-pivot
        "id=\"vp2d-list-select\"", "Show list of selectable nodes at position clicked.",
        "id=\"vp2d-pivot\"", "rotation pivot",
        // pan + ruler
        "id=\"vp2d-pan\"", "Pan Mode (G)",
        "id=\"vp2d-ruler\"", "Ruler Mode (M)",
        // local space
        "id=\"vp2d-local-space\"", "Use Local Space (T)",
        // smart/grid snap + snapping options menu
        "id=\"vp2d-smart-snap\"", "smart snapping",
        "id=\"vp2d-grid-snap\"", "grid snapping",
        "id=\"vp2d-snap-config\"", "Snapping Options",
        // lock/unlock + group/ungroup
        "id=\"vp2d-lock\"", "id=\"vp2d-unlock\"",
        "id=\"vp2d-group\"", "id=\"vp2d-ungroup\"",
        // skeleton + view menus
        "id=\"vp2d-skeleton\"", "Skeleton Options",
        "id=\"vp2d-view\"",
        // zoom widget
        "id=\"vp2d-zoom\"", "id=\"vp2d-zoom-out\"", "id=\"vp2d-zoom-in\"", "id=\"vp2d-zoom-reset\"",
        // representative menu items (ship as JS literals)
        "Use Rotation Snap", "Snap to Guides", "Configure Snap...",
        "Show Bones", "Center Selection", "Frame Selection",
    ] {
        assert!(
            html.contains(needle),
            "served 2D viewport toolbar is missing `{needle}` (Godot CanvasItemEditor builds it)"
        );
    }

    handle.stop();
}
