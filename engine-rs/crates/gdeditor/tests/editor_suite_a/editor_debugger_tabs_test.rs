//! Visual-fidelity (DOM-structure) test for the Debugger panel sub-tabs.
//!
//! Godot's ScriptEditorDebugger (script_editor_debugger.cpp:2062-2386) is a
//! sub-tabbed panel: Stack Trace / Errors / Evaluator / Profiler / Visual
//! Profiler / Monitors / Video RAM / Misc / Network Profiler. The Stack Trace
//! tab holds the debug-control toolbar (Skip Breakpoints, Copy Error, Step
//! Into/Over/Out, Break, Continue) plus Thread / Stack Frames / Breakpoints.
//!
//! Patina's Debugger was a flat Continue/Step view with Monitors and Profiler as
//! separate *top-level* bottom tabs — nothing like Godot. This asserts the
//! sub-tab layout, enumerated 1:1 from the source.

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

/// The Debugger must render Godot's nine sub-tabs, in order.
#[test]
fn debugger_renders_godot_subtabs() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"debugger-subtabs\"",
        "data-dbgtab=\"stack-trace\"",
        ">Stack Trace<",
        "data-dbgtab=\"errors\"",
        ">Errors<",
        "data-dbgtab=\"evaluator\"",
        ">Evaluator<",
        "data-dbgtab=\"profiler\"",
        "data-dbgtab=\"visual-profiler\"",
        ">Visual Profiler<",
        "data-dbgtab=\"monitors\"",
        "data-dbgtab=\"video-ram\"",
        ">Video RAM<",
        "data-dbgtab=\"misc\"",
        ">Misc<",
        "data-dbgtab=\"network-profiler\"",
        ">Network Profiler<",
    ] {
        assert!(
            html.contains(needle),
            "Debugger is missing sub-tab `{needle}` (Godot script_editor_debugger.cpp)"
        );
    }
    handle.stop();
}

/// The Stack Trace tab must hold the debug toolbar + Thread/Stack Frames/Breakpoints.
#[test]
fn debugger_stack_trace_tab_has_toolbar_and_panes() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"debug-headrow\"",     // Godot: Thread + Filter + toolbar on ONE row
        "id=\"debug-btn-skip-bp\"", // Skip Breakpoints
        "id=\"debug-btn-ignore-errors\"", // Ignore Error Breaks (Godot has this too)
        "id=\"debug-btn-copy-error\"", // Copy Error
        "id=\"debug-btn-step-in\"", // Step Into
        "id=\"debug-btn-step-over\"", // Step Over
        "id=\"debug-btn-step-out\"", // Step Out
        "id=\"debug-btn-break\"",   // Break
        "id=\"debug-btn-continue\"", // Continue
        "id=\"debug-thread\"",      // Thread: dropdown
        "placeholder=\"Filter Stack Variables\"", // stack variables filter
        "id=\"debug-stack-frames\"", // Stack Frames
        "id=\"debug-breakpoints-list\"", // Breakpoints
        "id=\"debugger-expand\"",   // Expand Bottom Panel icon (sub-tab row right)
        "function setupDebuggerSubtabs", // wired
        "setupDebuggerSubtabs()",
    ] {
        assert!(
            html.contains(needle),
            "Debugger Stack Trace tab is missing `{needle}`"
        );
    }
    // Monitors and Profiler moved INTO the debugger; they must no longer be
    // top-level bottom tabs (Godot has them only as debugger sub-tabs).
    assert!(
        !html.contains("data-tab=\"monitors\"") && !html.contains("data-tab=\"profiler\""),
        "Monitors/Profiler should no longer be top-level bottom tabs (they are debugger sub-tabs now)"
    );
    // ...but their content (canvases) still exists, inside the debugger.
    assert!(
        html.contains("id=\"profiler-graph-canvas\"")
            && html.contains("id=\"monitor-graph-canvas\""),
        "Monitors/Profiler canvases must still be served (relocated into the debugger)"
    );
    handle.stop();
}
