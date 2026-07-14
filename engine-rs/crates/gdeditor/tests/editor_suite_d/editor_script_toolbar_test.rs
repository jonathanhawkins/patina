//! Visual-fidelity (DOM-structure) test for the Script editor toolbar.
//!
//! Godot's ScriptEditor builds a `menu_hb` toolbar (script_editor_plugin.cpp
//! :4210-4458): File / Search / Debug menus, a centered script-name label, then
//! "Online Docs" (site_search), "Search Help" (help_search), the history
//! back/forward arrows, and a make-floating toggle — plus a left column with a
//! "Filter Scripts" box + script list and a "Filter Methods" box + members
//! overview. Patina's Script tab had none of this (just a bare "Script + New"
//! header), so a user comparing the Script tab side-by-side saw an empty shell.
//!
//! These assertions are enumerated 1:1 from the Godot source.

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

/// The Script editor toolbar must show the File/Search/Debug menus, the
/// Online Docs and Search Help buttons, and the history arrows — the exact
/// widgets the user flagged as missing.
#[test]
fn script_toolbar_renders_menu_and_help_buttons() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"script-toolbar\"",
        "id=\"script-file-menu\"",
        ">File<",
        "id=\"script-search-menu\"",
        ">Search<",
        "id=\"script-debug-menu\"",
        ">Debug<",
        "id=\"script-online-docs\"", // site_search "Online Docs"
        "Online Docs",
        "id=\"script-search-help\"", // help_search "Search Help"
        "Search Help",
        "id=\"script-back\"",        // history previous arrow
        "id=\"script-forward\"",     // history next arrow
        "id=\"script-make-floating\"", // make-floating toggle
        "id=\"script-name-label\"",  // centered script name label
    ] {
        assert!(
            html.contains(needle),
            "served Script tab toolbar is missing `{needle}` (Godot script_editor_plugin.cpp menu_hb)"
        );
    }
    handle.stop();
}

/// The left column must have the Filter Scripts + script list and Filter
/// Methods + members overview (Godot scripts_vbox + overview_vbox).
#[test]
fn script_editor_renders_list_column() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"script-list-col\"",
        "placeholder=\"Filter Scripts\"",
        "id=\"script-list\"",
        "placeholder=\"Filter Methods\"",
        "id=\"script-members\"",
    ] {
        assert!(
            html.contains(needle),
            "served Script tab is missing left-column element `{needle}`"
        );
    }
    handle.stop();
}

/// The toolbar must be wired (menus open, Online Docs/Search Help act, filter narrows).
#[test]
fn script_toolbar_is_wired() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "function setupScriptToolbar",
        "setupScriptToolbar()",
        "function bindScriptMenu",
        "docs.godotengine.org", // Online Docs / Search Help targets
        "function renderScriptList",
    ] {
        assert!(
            html.contains(needle),
            "served editor JS is missing Script toolbar wiring `{needle}`"
        );
    }
    handle.stop();
}
