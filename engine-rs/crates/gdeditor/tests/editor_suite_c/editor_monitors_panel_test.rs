//! Visual-fidelity + functional test for the Debugger → Monitors sub-tab.
//!
//! Godot's EditorPerformanceProfiler (editor_performance_profiler.cpp:413-444)
//! is an HSplit: a "Monitor | Value" Tree of all performance monitors grouped by
//! category (Time, Memory, Object, Raster, Video, Physics 2D, Physics 3D,
//! Navigation, Audio) each with a checkbox, plus a graph that plots the checked
//! monitors, with the empty-state "Pick one or more items from the list to
//! display the graph." Patina's Monitors tab was just FPS/Avg/Max + a bare
//! canvas. Enumerated 1:1 from the source.

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

/// The Monitors tab must render the Monitor|Value tree + graph + empty-state.
#[test]
fn monitors_renders_tree_and_graph() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"monitors-split\"",
        "id=\"monitor-tree\"",
        "id=\"monitor-tree-head\"",
        ">Monitor<", // column 0 (editor_performance_profiler.cpp:420)
        ">Value<",   // column 1 (422)
        "id=\"monitor-tree-body\"",
        "id=\"monitor-graph-canvas\"", // graph (kept id)
        "id=\"monitor-graph-empty\"",
        "Pick one or more items from the list to display the graph.", // info_message (441)
    ] {
        assert!(
            html.contains(needle),
            "Monitors tab is missing `{needle}` (Godot editor_performance_profiler.cpp)"
        );
    }
    handle.stop();
}

/// The monitor spec must cover Godot's categories + representative monitors, and be wired.
#[test]
fn monitors_spec_covers_godot_categories() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "MONITOR_SPEC",
        "function buildMonitorTree",
        "function drawMonitorGraph",
        // categories
        "'Time'",
        "'Memory'",
        "'Object'",
        "'Raster'",
        "'Video'",
        "'Physics 2D'",
        "'Physics 3D'",
        "'Navigation'",
        "'Audio'",
        // representative monitors
        "time/fps",
        "object/nodes",
        "object/orphan_nodes",
        "raster/total_draw_calls",
        "physics_3d/active_objects",
        "navigation/regions",
    ] {
        assert!(
            html.contains(needle),
            "Monitors spec/wiring is missing `{needle}`"
        );
    }
    handle.stop();
}
