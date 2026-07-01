//! Wiring test for the script editor's open-scripts list + members overview
//! (pat-f64ui).
//!
//! Godot's ScriptEditor lists open scripts in a filterable script_list, shows
//! the active script's methods in a filterable members_overview (with an
//! alpha-sort toggle), and centers the script name label
//! (upstream/godot/editor/script/script_editor_plugin.cpp). Patina had the
//! chrome but never populated it. This asserts the served editor carries the
//! list/members/label chrome (incl. the alpha-sort toggle) and the population
//! wiring that fills them when a script is opened.

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

/// pat-f64ui: the script editor has a filterable open-scripts list, a
/// filterable members overview with an alpha-sort toggle, a centered name
/// label, and the wiring that populates them on open.
#[test]
fn script_editor_populates_list_and_members() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // Chrome: list + filter, members + method filter + alpha-sort, name label.
    for needle in [
        r#"id="script-list""#,
        r#"id="script-filter""#,
        "Filter Scripts",
        r#"id="script-members""#,
        r#"id="script-method-filter""#,
        "Filter Methods",
        r#"id="script-members-sort""#,
        "Sort alphabetically",
        r#"id="script-name-label""#,
    ] {
        assert!(html.contains(needle), "script editor missing chrome `{needle}`");
    }

    // Wiring: opening a script populates the list, members, and label.
    for fn_name in [
        "function updateScriptDocChrome",
        "function extractScriptMethods",
        "function renderScriptMembers",
        "function renderScriptList",
    ] {
        assert!(html.contains(fn_name), "script editor missing population wiring `{fn_name}`");
    }
    // renderScript (which loads a document) drives the chrome update.
    assert!(
        html.contains("updateScriptDocChrome(path, content)"),
        "renderScript should populate the list/members/label on open"
    );

    handle.stop();
}
