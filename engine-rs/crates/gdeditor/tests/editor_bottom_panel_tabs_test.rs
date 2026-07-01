//! Visual-fidelity test for the bottom-panel tab set (pat-bc0tv).
//!
//! Godot's bottom panel includes an Audio (bus layout) panel
//! (editor_audio_buses.cpp) and a Shader Editor panel (shader_editor_plugin.cpp)
//! in its tab strip. Patina had Audio but was missing the Shader Editor tab.
//! This asserts the served bottom-panel tab strip now has both, and the Shader
//! Editor tab is backed by a real editor panel.

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

/// pat-bc0tv: the bottom-panel tab strip includes Audio and Shader Editor tabs.
#[test]
fn bottom_panel_has_audio_and_shader_tabs() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // Audio (bus layout) tab + its panel.
    assert!(
        html.contains(r#"data-tab="audio">Audio<"#),
        "bottom panel should have an Audio tab"
    );
    assert!(html.contains(r#"id="audio-panel""#), "Audio tab should be backed by the bus-layout panel");

    // Shader Editor tab + its panel.
    assert!(
        html.contains(r#"id="shader-tab-btn""#) && html.contains(">Shader Editor<"),
        "bottom panel should have a Shader Editor tab"
    );
    assert!(
        html.contains(r#"class="bottom-content-tab" data-tab="shader""#),
        "the Shader Editor tab should have a matching content panel"
    );
    assert!(
        html.contains(r#"id="shader-source""#),
        "the Shader Editor panel should have a source editor"
    );
    // The shader editor is wired to the source endpoint (not inert chrome).
    assert!(
        html.contains("/api/shader_source"),
        "the Shader Editor should load/save via the shader_source endpoint"
    );

    handle.stop();
}
