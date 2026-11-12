//! App-level regression test for pat-v4o3a.
//!
//! The AnimationTree state-machine view used to render as an always-visible,
//! unstyled text strip injected above the main dock row (it was a top-level
//! `#anim-tree-dialog` with no CSS, so it fell into default document flow).
//! Godot keeps that editor inside the ANIMATION bottom panel. This test boots
//! the real editor server, fetches the served `/editor` HTML, and asserts the
//! AnimationTree container now lives inside the bottom-panel host (within the
//! ANIMATION content tab) and is gated hidden until toggled — never a bare
//! strip above the dock layout.

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
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = {
        let mut s = None;
        for attempt in 0..20 {
            match TcpStream::connect(format!("127.0.0.1:{port}")) {
                Ok(c) => {
                    s = Some(c);
                    break;
                }
                Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("failed to connect: {e}"),
            }
        }
        s.unwrap()
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

#[test]
fn editor_animation_panel_contained_in_bottom_panel() {
    let (_handle, port) = make_server();
    let resp = http_get(port, "/editor");
    let html = resp.split("\r\n\r\n").nth(1).unwrap_or("");

    let tree_idx = html
        .find(r#"id="anim-tree-dialog""#)
        .expect("AnimationTree container must be present in served /editor HTML");

    // It must live inside the bottom-panel content host. Before the fix the
    // container sat near the top of the document (index well below the bottom
    // panel), so this both proves containment and catches the strip regression.
    let host_idx = html
        .find(r#"id="bottom-panel-content""#)
        .expect("#bottom-panel-content host must exist");
    assert!(
        tree_idx > host_idx,
        "AnimationTree container must be a descendant of #bottom-panel-content, \
         not a bare strip above the dock row (tree@{tree_idx} <= host@{host_idx})"
    );

    // And specifically within the ANIMATION content tab (the data-tab marker
    // that appears *after* the host opening — the earlier one is the header
    // button).
    let anim_content_idx = html[host_idx..]
        .find(r#"data-tab="animation""#)
        .map(|i| host_idx + i)
        .expect("ANIMATION content tab must exist inside the bottom-panel host");
    assert!(
        tree_idx > anim_content_idx,
        "AnimationTree container must sit inside the ANIMATION bottom tab \
         (tree@{tree_idx} <= anim-tab@{anim_content_idx})"
    );

    // Visibility is gated: a CSS rule reveals it only when `.open` is toggled,
    // so it is hidden by default even when the ANIMATION tab is active.
    assert!(
        html.contains("#anim-tree-dialog.open"),
        "AnimationTree view must be hidden until toggled (missing .open gate rule)"
    );
    assert!(
        html.contains("#anim-tree-dialog { display: none"),
        "AnimationTree container must default to display:none (no raw document flow)"
    );
}

// --- pat-605ob: animation timeline with tracks + editable keyframes ---

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = {
        let mut s = None;
        for attempt in 0..20 {
            match TcpStream::connect(format!("127.0.0.1:{port}")) {
                Ok(c) => {
                    s = Some(c);
                    break;
                }
                Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("failed to connect: {e}"),
            }
        }
        s.unwrap()
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// pat-605ob: the ANIMATION tab is a real timeline — create an animation, the
/// AnimationPlayer's tracks + keyframes are editable (add/remove) over HTTP, and
/// the served panel renders the timeline + track list.
#[test]
fn animation_timeline_tracks_and_keyframes() {
    let (_handle, port) = make_server();

    // Create an animation.
    assert!(http_post(port, "/api/animation/create", r#"{"name":"walk"}"#).contains("200 OK"));

    // Insert two keyframes on a Main.position:x track (the track auto-creates).
    // Values are typed variants, as the editor sends ({type, value}).
    assert!(http_post(port, "/api/animation/keyframe/add", r#"{"animation":"walk","track_node":"Main","track_property":"position:x","time":0.0,"value":{"type":"Float","value":0.0}}"#).contains("200 OK"));
    assert!(http_post(port, "/api/animation/keyframe/add", r#"{"animation":"walk","track_node":"Main","track_property":"position:x","time":1.0,"value":{"type":"Float","value":100.0}}"#).contains("200 OK"));

    // The timeline reflects the track + its two keyframes.
    let anim = body_json(&http_get(port, "/api/animation?name=walk"));
    let tracks = anim["tracks"].as_array().expect("tracks array");
    assert_eq!(tracks.len(), 1, "one track was created");
    assert_eq!(tracks[0]["node_path"], serde_json::json!("Main"));
    assert_eq!(tracks[0]["property"], serde_json::json!("position:x"));
    let kfs = tracks[0]["keyframes"].as_array().unwrap();
    assert_eq!(kfs.len(), 2, "two keyframes inserted");
    let times: Vec<f64> = kfs.iter().map(|k| k["time"].as_f64().unwrap()).collect();
    assert!(
        times.contains(&0.0) && times.contains(&1.0),
        "keyframes at 0 and 1: {times:?}"
    );

    // Keyframes are editable — remove one.
    assert!(http_post(
        port,
        "/api/animation/keyframe/remove",
        r#"{"animation":"walk","track_index":0,"keyframe_index":0}"#
    )
    .contains("200 OK"));
    let after = body_json(&http_get(port, "/api/animation?name=walk"));
    assert_eq!(
        after["tracks"][0]["keyframes"].as_array().unwrap().len(),
        1,
        "keyframe removed"
    );

    // The served ANIMATION panel renders the timeline + track list + toolbar.
    let html = http_get(port, "/editor");
    for needle in [
        "id=\"animation-panel\"",
        "id=\"anim-timeline-canvas\"", // the timeline (ruler/playhead/keyframes)
        "id=\"anim-tracks\"",          // the track list
        "id=\"anim-new-btn\"",
        "id=\"anim-play-btn\"",
        "function renderAnimTracks", // track rendering wiring
    ] {
        assert!(html.contains(needle), "animation panel missing `{needle}`");
    }
}
