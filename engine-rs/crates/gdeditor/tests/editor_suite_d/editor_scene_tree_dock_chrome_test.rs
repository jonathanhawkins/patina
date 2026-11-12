//! Visual-fidelity (DOM-structure) test for the Scene Tree dock's permanent
//! toolbar chrome (pat-l695a).
//!
//! Godot's `SceneTreeDock` builds a fixed toolbar (`filter_hbc`) in its
//! constructor (upstream/godot/editor/docks/scene_tree_dock.cpp): an Add Child
//! Node button, an Instantiate Child Scene button, the "Filter Nodes" box
//! (with name/type/group filtering), Attach/Detach/Extend Script buttons, and
//! an "Extra scene options" menu. None of it depends on a selection — it is
//! permanent chrome, so its absence is immediately visible side-by-side.
//!
//! This boots the real `EditorServerHandle` and asserts the served `/editor`
//! document carries a counterpart for every widget the dock's constructor
//! always instantiates.

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

/// Every permanent toolbar widget the Godot `SceneTreeDock` constructor builds
/// has a counterpart in the served editor, plus the filter advertises its
/// name/type/group support.
#[test]
fn scene_tree_dock_renders_toolbar_chrome() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // The toolbar container itself.
        "id=\"scene-tree-toolbar\"",
        // Add Child Node (button_add) + its Godot tooltip.
        "id=\"st-add-child\"",
        "Add/Create a New Node.",
        // Instantiate Child Scene (button_instance).
        "id=\"st-instantiate\"",
        "Instantiate a scene file as a Node.",
        // Attach / Detach / Extend Script (button_create_script/detach/extend).
        "id=\"st-attach-script\"",
        "Attach a new or existing script to the selected node.",
        "id=\"st-detach-script\"",
        "Detach the script from the selected node.",
        "id=\"st-extend-script\"",
        "Extend the script of the selected node.",
        // Extra scene options (button_tree_menu).
        "id=\"st-tree-menu\"",
        "Extra scene options.",
        // The filter box advertises Godot's name/type/group filtering.
        "id=\"scene-search\"",
        "type:",
        "group:",
    ] {
        assert!(
            html.contains(needle),
            "served Scene Tree dock is missing toolbar chrome `{needle}` (Godot builds it unconditionally)"
        );
    }

    handle.stop();
}
