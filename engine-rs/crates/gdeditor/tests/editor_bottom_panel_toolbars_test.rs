//! Visual-fidelity (DOM-structure) tests for the bottom panel's Output and
//! Debugger toolbars (pat-mcwjp, pat-t5lb5).
//!
//! Godot's bottom panel gives the Output log a toolbar (Clear, Copy, Collapse,
//! Filter Messages, and Standard/Errors/Warnings/Editor message-type toggles —
//! upstream/godot/editor/editor_log.cpp:505-617) and the Debugger a toolbar
//! (Skip Breakpoints, Ignore Error Breaks, Copy Error, Step Into/Over/Out,
//! Break, Continue — upstream/godot/editor/debugger/script_editor_debugger.cpp
//! :2082-2139). These assert the served editor renders each toolbar's controls
//! 1:1 from the Godot constructors.

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

/// pat-mcwjp: the Output panel renders its toolbar — Clear, Copy, a Collapse
/// toggle, a "Filter Messages" box, and Standard/Errors/Warnings/Editor toggles.
#[test]
fn output_panel_renders_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"output-clear\"",   // Clear (editor_log.cpp clear_button)
        "Clear Output",
        "id=\"output-copy\"",    // Copy (copy_button)
        "Copy Selection",
        "id=\"output-collapse\"", // Collapse-duplicates toggle (collapse_button)
        "Collapse duplicate messages",
        "id=\"output-search\"",  // Filter Messages box (search_box)
        "Filter Messages",
        // Standard / Errors / Warnings / Editor message-type toggles.
        "id=\"output-filter-std\"",
        "id=\"output-filter-error\"",
        "id=\"output-filter-warning\"",
        "id=\"output-filter-editor\"",
    ] {
        assert!(
            html.contains(needle),
            "served Output panel toolbar is missing `{needle}` (Godot EditorLog builds it)"
        );
    }
    handle.stop();
}

/// pat-t5lb5: the Debugger renders its full toolbar — Skip Breakpoints, Ignore
/// Error Breaks, Copy Error, Step Into/Over/Out, Break, and Continue.
#[test]
fn debugger_panel_renders_full_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"debug-btn-skip-bp\"",        // Skip Breakpoints (skip_breakpoints)
        "Skip Breakpoints",
        "id=\"debug-btn-ignore-errors\"",  // Ignore Error Breaks (ignore_error_breaks)
        "Ignore Error Breaks",
        "id=\"debug-btn-copy-error\"",     // Copy Error (copy)
        "Copy Error",
        "id=\"debug-btn-step-in\"",        // Step Into (step)
        "id=\"debug-btn-step-over\"",      // Step Over (next)
        "id=\"debug-btn-step-out\"",       // Step Out (out)
        "id=\"debug-btn-break\"",          // Break (dobreak)
        "title=\"Break\"",
        "id=\"debug-btn-continue\"",       // Continue (docontinue)
    ] {
        assert!(
            html.contains(needle),
            "served Debugger toolbar is missing `{needle}` (Godot ScriptEditorDebugger builds it)"
        );
    }
    handle.stop();
}
