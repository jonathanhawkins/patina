//! Visual-fidelity (DOM-structure) test for the Debugger → Profiler sub-tab.
//!
//! Godot's EditorProfiler (editor_profiler.cpp:680-787) is a toolbar
//! [Start (activate) · Clear · Autostart · "Measure:" OptionButton (Frame Time
//! (ms)/Average Time (ms)/Frame %/Physics Frame %) · "Time:" OptionButton
//! (Inclusive/Self) · "Frame #:" SpinBox] above an HSplit of a `variables` Tree
//! with columns Name | Time | Calls, and a frame-time graph. Patina's Profiler
//! was just FPS/Avg/Max stats + a bare canvas — nothing like Godot.
//!
//! Enumerated 1:1 from the Godot source.

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

/// The Profiler toolbar must match Godot: Start, Clear, Autostart, Measure:
/// (with the four modes), Time: (Inclusive/Self), Frame #:.
#[test]
fn profiler_renders_editorprofiler_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"profiler-toolbar\"",
        "id=\"profiler-activate\"", // activate Start/Stop (editor_profiler.cpp:680)
        ">Start<",
        "id=\"profiler-clear\"",     // Clear (687)
        "id=\"profiler-autostart\"", // Autostart checkbox (693)
        "id=\"profiler-measure\"",   // display_mode OptionButton (705)
        "Frame Time (ms)",           // its first item (707)
        "Average Time (ms)",         // (708)
        "Physics Frame %",           // (710)
        "id=\"profiler-time\"",      // display_time OptionButton (721)
        ">Inclusive<",               // (724)
        ">Self<",                    // (726)
        "id=\"profiler-frame\"",     // cursor_metric_edit "Frame #:" (744)
        "Frame #:",
    ] {
        assert!(
            html.contains(needle),
            "Profiler toolbar is missing `{needle}` (Godot editor_profiler.cpp)"
        );
    }
    handle.stop();
}

/// The split must have the Name | Time | Calls function tree and the graph.
#[test]
fn profiler_renders_tree_columns_and_graph() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"profiler-split\"",
        "id=\"profiler-tree\"",
        "id=\"profiler-tree-head\"",
        ">Name<",                         // variables column 0 (editor_profiler.cpp:763)
        ">Time<",                         // column 1 (767)
        ">Calls<",                        // column 2 (771)
        "id=\"profiler-func-breakdown\"", // the rows body (kept id)
        "id=\"profiler-graph-canvas\"",   // the graph (kept id)
        "function setupProfilerToolbar",  // wired
        "setupProfilerToolbar()",
    ] {
        assert!(
            html.contains(needle),
            "Profiler split is missing `{needle}`"
        );
    }
    handle.stop();
}
