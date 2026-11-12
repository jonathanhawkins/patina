//! Visual-fidelity test for the Debugger → Video RAM sub-tab.
//!
//! Godot's ScriptEditorDebugger vmem section (script_editor_debugger.cpp
//! :2308-2369): a header "List of Video Memory Usage by Resource:" + a notice
//! icon + a right-aligned "Total:" read-only field + Refresh + Export(CSV)
//! buttons, above a Tree with columns Resource Path | Type | Format | Usage.
//! Patina had a loose "Total Video RAM: N MB" + an unstructured list. Enumerated
//! 1:1 from the source.

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

/// The Video RAM tab must render Godot's header (title + Total field + refresh +
/// export) and the Resource Path | Type | Format | Usage table.
#[test]
fn vram_renders_godot_header_and_table() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"dbg-vram-header\"",
        "List of Video Memory Usage by Resource:", // vmlb (script_editor_debugger.cpp:2312)
        "class=\"dbg-vram-notice\"",               // vmem_notice_icon (2317)
        ">Total:<",                                // (2337)
        "id=\"dbg-vram-total\"",                   // vmem_total (2338)
        "id=\"dbg-vram-refresh\"",                 // vmem_refresh (2343)
        "id=\"dbg-vram-export\"",                  // vmem_export (2347)
        "id=\"dbg-vram-table\"",
        "id=\"dbg-vram-head\"",
        "Resource Path",          // column 0 (2364)
        "vrcol-type\">Type<",     // column 1 (2367)
        "vrcol-format\">Format<", // column 2
        "vrcol-usage\">Usage<",   // column 3
        "id=\"dbg-vram-list\"",
    ] {
        assert!(
            html.contains(needle),
            "Video RAM tab is missing `{needle}` (Godot script_editor_debugger.cpp vmem)"
        );
    }
    handle.stop();
}

/// The refresh/export toolbar must be wired.
#[test]
fn vram_toolbar_is_wired() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in ["function setupVramTab", "setupVramTab()"] {
        assert!(
            html.contains(needle),
            "Video RAM toolbar wiring missing `{needle}`"
        );
    }
    handle.stop();
}
