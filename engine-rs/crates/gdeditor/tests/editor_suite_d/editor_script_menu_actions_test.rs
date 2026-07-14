//! Wiring test for the script editor's File/Search/Debug menu actions
//! (pat-kgfgf).
//!
//! Godot's ScriptEditor File/Search/Debug menus perform real actions
//! (upstream/godot/editor/script/script_editor_plugin.cpp:4328-4385). Patina's
//! scriptMenuAction only handled New and logged the rest. This asserts the
//! served editor exposes the menu items AND wires the actions to real work
//! (save, find, find-in-files, close, debugger step/break/continue).

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

/// pat-kgfgf: the File/Search/Debug menu items exist and their actions are wired
/// to real behavior (not just logged).
#[test]
fn script_menu_actions_perform() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // Menu items present (File / Search / Debug) — distinctive labels as they
    // ship in the inline menu definitions.
    for item in [
        "Save As...", "Save All", "Close All",
        "Find...", "Find Next", "Replace...", "Find in Files...",
        "Step Into", "Step Over",
    ] {
        assert!(html.contains(item), "script menu missing item `{item}`");
    }

    // Actions are wired (the dispatcher performs real work, not a bare log).
    for wired in [
        "case 'save':", "saveScript()",
        "case 'save_as':",
        "case 'close':", "scriptCloseActive",
        "case 'close_all':", "scriptCloseAll",
        "case 'find':", "scriptFindInEditor",
        "case 'find_in_files':", "scriptFindInFiles",
        "/api/search?q=",
        "case 'step_into':", "debug-btn-step-in",
        "case 'continue':", "debug-btn-continue",
    ] {
        assert!(html.contains(wired), "script menu action not wired: `{wired}`");
    }

    // The old "wired incrementally" stub is gone.
    assert!(
        !html.contains("Remaining actions are wired into the code editor incrementally"),
        "script menu actions should no longer be stubbed"
    );

    handle.stop();
}
