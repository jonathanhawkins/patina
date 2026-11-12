//! Visual-fidelity test for the run toolbar's Play Custom Scene button
//! (pat-12qzs).
//!
//! Godot's `EditorRunBar` builds a `play_custom_scene_button` (PlayCustom icon,
//! tooltip "Play a custom scene.") alongside Play / Play Scene / Pause / Stop
//! (upstream/godot/editor/run/editor_run_bar.cpp:129). Patina's run toolbar had
//! Play / Pause / Stop / Play-Current but was missing Play Custom Scene. This
//! asserts the served run toolbar now includes it next to the other run buttons.

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

/// pat-12qzs: the run toolbar includes a Play Custom Scene button (PlayCustom,
/// "Play a custom scene.") alongside the other run buttons.
#[test]
fn top_bar_has_play_custom_scene() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // The Play Custom Scene button with Godot's tooltip.
    assert!(
        html.contains(r#"id="btn-play-custom""#),
        "run toolbar should have a Play Custom Scene button"
    );
    assert!(
        html.contains(r#"title="Play a custom scene.""#),
        "Play Custom Scene button should carry Godot's tooltip"
    );
    assert!(
        html.contains("play-custom"),
        "Play Custom Scene button should carry its play-custom class"
    );
    // It sits alongside the other run-toolbar buttons.
    for sibling in ["btn-play", "btn-pause", "btn-stop", "btn-play-current"] {
        assert!(
            html.contains(&format!("id=\"{sibling}\"")),
            "run toolbar should still have `{sibling}` next to Play Custom Scene"
        );
    }
    // It is wired to a click handler (not inert chrome).
    assert!(
        html.contains("Play Custom Scene: "),
        "Play Custom Scene button should be wired to play a chosen scene"
    );

    handle.stop();
}
