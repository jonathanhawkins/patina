//! Visual-fidelity test for the top-bar renderer selector (pat-j8sfr).
//!
//! Godot's editor shows the rendering method as a functional dropdown
//! (OptionButton) listing Forward+, Mobile, and Compatibility
//! (upstream/godot/editor/editor_node.cpp — `_to_rendering_method_display_name`
//! maps forward_plus/mobile/gl_compatibility to those labels). Patina rendered
//! it as static "Forward+" text. This asserts the served top bar now exposes a
//! `<select>` dropdown with all three renderers.

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

/// pat-j8sfr: the renderer indicator is a functional dropdown listing Forward+,
/// Mobile, and Compatibility — not static text.
#[test]
fn top_bar_renderer_is_dropdown() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // The renderer is now a <select> dropdown.
    assert!(
        html.contains(r#"<select class="renderer-indicator" id="renderer-select""#),
        "the renderer indicator should be a <select> dropdown"
    );
    // All three Godot renderers are listed as options.
    for renderer in [">Forward+<", ">Mobile<", ">Compatibility<"] {
        assert!(
            html.contains(renderer),
            "renderer dropdown should list `{renderer}`"
        );
    }
    // It carries the renderer option values used by Godot's settings.
    for value in [
        r#"value="forward_plus""#,
        r#"value="mobile""#,
        r#"value="gl_compatibility""#,
    ] {
        assert!(
            html.contains(value),
            "renderer dropdown should have option `{value}`"
        );
    }
    // The old static text form must be gone.
    assert!(
        !html.contains(
            r#"<span class="renderer-indicator" title="Rendering method">Forward+</span>"#
        ),
        "the renderer should no longer be a static <span>"
    );

    handle.stop();
}
