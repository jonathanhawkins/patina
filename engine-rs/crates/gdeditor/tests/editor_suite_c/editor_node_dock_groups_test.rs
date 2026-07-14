//! Visual-fidelity test for the Node dock Groups tab (pat-3uujp).
//!
//! Godot's `GroupsEditor` (upstream/godot/editor/docks/groups_editor.cpp) builds
//! an "Add a new group" button + a "Filter Groups" box (Search icon), then a
//! tree with "Scene Groups" and "Global Groups" headers whose rows carry a
//! membership checkbox and a copy-name button, plus a right-click menu (Convert
//! to Global/Scene Group, Rename, Delete). The Node dock is rendered by the
//! served inline script, so all of that chrome ships in the `/editor` document.

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

/// The served Node dock Groups tab carries Godot's GroupsEditor chrome.
#[test]
fn node_dock_groups_tab_chrome() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // Add a new group button (Godot `add`, tooltip "Add a new group.").
        "group-add-btn",
        "Add a new group.",
        // Filter Groups box + Search icon (Godot `filter`, set_right_icon(Search)).
        "group-filter",
        "Filter Groups",
        "group-filter-icon",
        // Scene Groups / Global Groups tree headers (local_root / global_root).
        "Scene Groups",
        "Global Groups",
        // Per-group membership checkbox + copy-name button.
        "group-check",
        "group-copy-btn",
        "Copy group name",
        // Right-click menu (convert scene<->global, rename, delete).
        "Convert to Global Group",
        "Convert to Scene Group",
    ] {
        assert!(
            html.contains(needle),
            "served Node dock Groups tab is missing `{needle}` (Godot's GroupsEditor builds it)"
        );
    }

    handle.stop();
}
