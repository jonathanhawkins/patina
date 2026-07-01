//! Regression test for the 2D viewport Scale tool shortcut (pat-kwznj).
//!
//! Godot binds the 2D editor's Scale Mode to the R key
//! (upstream/godot/editor/scene/canvas_item_editor_plugin.cpp:
//! `ED_SHORTCUT("canvas_item_editor/scale_mode", TTRC("Scale Mode"), Key::R)`),
//! alongside Select=Q, Move=W, Rotate=E. Patina's served editor showed the
//! Scale tool as "S" and had no scale key binding at all. This asserts the
//! served editor uses R for Scale — the button label/tooltip, the keyboard
//! handler, and the shortcut help — and no longer advertises S.

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

/// pat-kwznj: the 2D Scale tool uses R (Godot parity), not S.
#[test]
fn viewport_scale_tool_uses_r_key() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // The Scale tool button is labelled/tooltipped R.
    assert!(
        html.contains(r#"data-tool="scale" title="Scale (R)""#),
        "Scale tool button should be titled \"Scale (R)\""
    );
    // It must no longer advertise the wrong S shortcut.
    assert!(
        !html.contains(r#"title="Scale (S)""#),
        "Scale tool button must not still say \"Scale (S)\""
    );
    // The keyboard handler binds R (not S) to the scale tool.
    assert!(
        html.contains("if (e.key === 'r' || e.key === 'R') { setToolMode('scale'); return; }"),
        "R should be bound to the scale tool in the served keyboard handler"
    );
    // The shortcut help lists Scale tool -> R.
    assert!(
        html.contains(r#"<span class="help-desc">Scale tool</span><span class="help-key">R</span>"#),
        "the shortcut help should list Scale tool as R"
    );

    handle.stop();
}
