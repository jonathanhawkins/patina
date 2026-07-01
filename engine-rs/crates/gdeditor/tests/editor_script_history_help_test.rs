//! Visual-fidelity / wiring test for the script editor's document-history
//! arrows and in-editor Search Help dialog (pat-nrwy2).
//!
//! Godot's ScriptEditor has script_back/script_forward buttons that walk the
//! edited-document history (enabled per position), and a Search Help action
//! that opens an in-editor reference search dialog (EditorHelpSearch), not the
//! web docs (upstream/godot/editor/script/script_editor_plugin.cpp:4436-4450 +
//! editor_help_search.cpp). Patina's Search Help opened the web docs and the
//! history arrows were inert. This asserts the served editor wires both.

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

/// pat-nrwy2: the back/forward arrows walk the document history and Search Help
/// opens an in-editor reference dialog (not the web docs).
#[test]
fn script_editor_history_and_help_search() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // History arrows + their navigation wiring.
    assert!(html.contains(r#"id="script-back""#), "script editor should have a back arrow");
    assert!(html.contains(r#"id="script-forward""#), "script editor should have a forward arrow");
    assert!(
        html.contains("scriptHistoryGo") && html.contains("scriptDocHistory"),
        "the arrows should navigate an edited-document history stack"
    );

    // In-editor Search Help dialog.
    assert!(
        html.contains("openScriptHelpSearch") && html.contains("help-search-overlay"),
        "Search Help should open an in-editor reference dialog"
    );
    assert!(
        html.contains("Search the reference documentation"),
        "the Help dialog should search the reference documentation"
    );
    // It must no longer just redirect to the web docs search.
    assert!(
        !html.contains("docs.godotengine.org/en/stable/search.html"),
        "Search Help should no longer redirect to the web docs search page"
    );

    handle.stop();
}
