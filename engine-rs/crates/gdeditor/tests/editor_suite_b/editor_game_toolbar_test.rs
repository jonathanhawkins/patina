//! Visual-fidelity (DOM-structure) test for the Game view toolbar.
//!
//! Godot's GameView builds a runtime-interaction toolbar
//! (game_view_plugin.cpp:1233-1370): process controls (Suspend, Next Frame,
//! speed, Reset Speed) | input/node-type toggles (Input, 2D, 3D) | selection
//! tools (Select, List Select, Hide Selection, Selection Options) | Mute |
//! camera override + embed options. Patina's Game tab only had Play/Pause/Stop
//! (which in Godot live in the top-bar run controls, not the Game toolbar), so
//! the tab looked nothing like Godot's.
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

/// The Game tab must render Godot's runtime toolbar groups.
#[test]
fn game_toolbar_renders_godot_controls() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"game-toolbar\"",
        "id=\"game-suspend\"",     // Suspend (SceneTree pause)
        "id=\"game-next-frame\"",  // Next Frame
        "id=\"game-speed\"",       // game speed
        "id=\"game-reset-speed\"", // Reset Speed
        "id=\"game-input\"",       // node_type "Input"
        ">Input<",
        "id=\"game-node-2d\"",           // node_type "2D"
        "id=\"game-node-3d\"",           // node_type "3D"
        "id=\"game-select-single\"",     // SELECT_MODE_SINGLE
        "id=\"game-select-list\"",       // SELECT_MODE_LIST
        "id=\"game-hide-selection\"",    // hide_selection
        "id=\"game-selection-options\"", // selection_options_menu
        "id=\"game-mute\"",              // debug_mute_audio_button
        "id=\"game-camera-override\"",   // camera_override_button
        "id=\"game-camera-menu\"",       // camera_override_menu
        "id=\"game-embed-options\"",     // embed_options_menu
    ] {
        assert!(
            html.contains(needle),
            "served Game tab toolbar is missing `{needle}` (Godot game_view_plugin.cpp)"
        );
    }
    handle.stop();
}

/// The toolbar must be wired, and the empty-state must use Godot's exact prompt.
#[test]
fn game_toolbar_is_wired_and_uses_godot_prompt() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    assert!(
        html.contains("function setupGameToolbar") && html.contains("setupGameToolbar()"),
        "Game toolbar must be set up at boot"
    );
    assert!(
        html.contains("Press play to start the game."),
        "Game empty-state must use Godot's exact prompt 'Press play to start the game.'"
    );
    // Play/Pause/Stop must NOT be in the Game toolbar (Godot keeps them in the
    // top-bar run controls); the redundant in-panel buttons were removed.
    assert!(
        !html.contains("id=\"game-play-btn\""),
        "the redundant in-panel Play button should be gone (play lives in the top bar)"
    );
    handle.stop();
}
