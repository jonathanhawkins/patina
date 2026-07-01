//! Visual-fidelity test for the run toolbar's Movie Maker mode toggle
//! (pat-uqj6w).
//!
//! Godot's `EditorRunBar` builds a `write_movie_button` (MainMovie icon,
//! toggle, tooltip "Enable Movie Maker mode.\nThe project will run at stable
//! FPS and the visual and audio output will be recorded to a video file.")
//! (upstream/godot/editor/run/editor_run_bar.cpp:707). Patina's run toolbar
//! lacked it. This asserts the served run toolbar now includes the toggle.

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

/// pat-uqj6w: the run toolbar includes the Movie Maker mode toggle with Godot's
/// tooltip, alongside the other run buttons, wired to toggle the mode.
#[test]
fn top_bar_has_movie_maker_toggle() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    assert!(
        html.contains(r#"id="btn-movie-maker""#),
        "run toolbar should have a Movie Maker toggle button"
    );
    assert!(
        html.contains("Enable Movie Maker mode."),
        "Movie Maker button should carry Godot's tooltip"
    );
    assert!(
        html.contains("movie-maker-btn"),
        "Movie Maker button should carry its movie-maker-btn class"
    );
    // It sits alongside the other run-toolbar buttons.
    for sibling in ["btn-play", "btn-stop", "btn-play-custom"] {
        assert!(
            html.contains(&format!("id=\"{sibling}\"")),
            "run toolbar should still have `{sibling}` next to the Movie Maker toggle"
        );
    }
    // It is wired to toggle the mode (not inert chrome).
    assert!(
        html.contains("Movie Maker mode "),
        "Movie Maker button should be wired to toggle the mode"
    );

    handle.stop();
}
