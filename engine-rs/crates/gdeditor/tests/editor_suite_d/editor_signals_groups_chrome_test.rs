//! Visual-fidelity DOM tests for the Node dock (Signals + Groups) permanent chrome.
//!
//! Source: Godot `SignalsDock` / `GroupsEditor` always expose filter boxes and
//! section structure. Patina serves a permanent shell under `#node-dock`.

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

#[test]
fn node_dock_shell_is_present() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="node-dock""#,
        r#"data-rptab="node""#,
        "Node",
        "Inspector",
        r#"id="node-dock-chrome""#,
        r#"id="node-signals-chrome""#,
        r#"id="node-groups-chrome""#,
        "Filter Signals",
        "Filter Groups",
        r#"id="signal-search""#,
        r#"id="group-search""#,
    ] {
        assert!(
            html.contains(needle),
            "Node dock missing Signals/Groups chrome `{needle}`"
        );
    }
    handle.stop();
}

/// pat-2u9jl — the Node dock's chrome is *permanent*, the way Godot's is.
///
/// `ConnectionsDock` and `GroupsEditor` build their widgets once in their
/// constructors and only ever toggle visibility (`holder->hide()`), including
/// one empty-state Label each, worded differently:
/// `select_an_object` = "Select a single node or resource to edit its signals."
/// (connections_dialog.cpp) and `select_a_node` = "Select one or more nodes to
/// edit their groups." (groups_editor.cpp).
///
/// Patina served that shell but `renderNodeDock` then did
/// `getElementById('node-dock').innerHTML = ''`, destroying the permanent chrome
/// and both labels on the first selection — so they never returned when the
/// selection cleared. `node_dock_shell_is_present` above could not see it: it
/// only inspects the *initial* document, which was correct the whole time. The
/// selection-dependent lists now render into `#node-dock-dynamic` instead, and
/// nothing may fetch `#node-dock` to clear it again.
#[test]
fn node_dock_permanent_chrome_survives_a_selection() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // Godot's two distinct empty-state labels, verbatim.
        r#"id="node-signals-empty""#,
        "Select a single node or resource to edit its signals.",
        r#"id="node-groups-empty""#,
        "Select one or more nodes to edit their groups.",
        // The dynamic container that keeps the permanent chrome alive, and the
        // renderer targeting it.
        r#"id="node-dock-dynamic""#,
        "document.getElementById('node-dock-dynamic')",
        // The permanent chrome is hidden and restored, not freed.
        "permChrome.style.display = 'none'",
        "permChrome.style.display = ''",
    ] {
        assert!(
            html.contains(needle),
            "Node dock permanent chrome is missing `{needle}`"
        );
    }

    // The regression itself: no code may clear `#node-dock` wholesale again.
    // (This literal cannot match `getElementById('node-dock-dynamic')` — the
    // closing quote and paren are part of the needle.)
    assert!(
        !html.contains("getElementById('node-dock')"),
        "something still fetches #node-dock directly; clearing it destroys the \
         permanent Signals/Groups chrome Godot keeps for the dock's lifetime"
    );

    handle.stop();
}

#[test]
fn connect_signal_dialog_chrome_present() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        r#"id="connect-dialog-overlay""#,
        r#"id="connect-signal-name""#,
        r#"id="connect-method-name""#,
        "Connect Signal",
        r#"id="connect-deferred""#,
        r#"id="connect-oneshot""#,
    ] {
        assert!(
            html.contains(needle),
            "connect dialog missing chrome `{needle}`"
        );
    }
    handle.stop();
}
