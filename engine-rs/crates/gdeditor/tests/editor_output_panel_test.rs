//! Visual-fidelity (DOM-structure) test for the Output panel (Godot EditorLog).
//!
//! Godot's EditorLog (editor_log.cpp:505-617) is a log area + a "Filter
//! Messages" box, plus a right-side toolbar: Clear, Copy | Collapse, Show Search
//! | Standard Messages / Errors / Warnings / Editor Messages filter toggles
//! (each with a live count). Patina's Output panel was a bare `<div>` with no
//! toolbar at all. Enumerated 1:1 from the Godot source.

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

/// The Output panel must render Godot's toolbar: Clear, Copy, Collapse, Show
/// Search, the "Filter Messages" box, and the four message-type filter toggles.
#[test]
fn output_panel_renders_editorlog_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"output-panel-body\"",
        "id=\"output-log\"",
        "placeholder=\"Filter Messages\"", // search_box (editor_log.cpp:531)
        "id=\"output-toolbar\"",
        "id=\"output-clear\"",       // Clear (editor_log.cpp:547)
        "id=\"output-copy\"",        // Copy (556)
        "id=\"output-collapse\"",    // Collapse toggle (574)
        "id=\"output-show-search\"", // Show Search toggle (584)
        "id=\"output-filter-std\"",     // Standard Messages (598)
        "id=\"output-filter-error\"",   // Errors (604)
        "id=\"output-filter-warning\"", // Warnings (609)
        "id=\"output-filter-editor\"",  // Editor Messages (614)
        "id=\"output-count-std\"",      // per-filter live counts
        "id=\"output-count-error\"",
    ] {
        assert!(
            html.contains(needle),
            "served Output panel is missing `{needle}` (Godot editor_log.cpp)"
        );
    }
    handle.stop();
}

/// The toolbar must be wired (clear/copy/collapse/show-search, filter toggles,
/// search box, per-category counts).
#[test]
fn output_panel_is_wired() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "function setupOutputPanel",
        "setupOutputPanel()",
        "function applyOutputFilters",
        "function updateOutputCounts",
        "outputFilters",
    ] {
        assert!(
            html.contains(needle),
            "served editor JS is missing Output panel wiring `{needle}`"
        );
    }
    handle.stop();
}
