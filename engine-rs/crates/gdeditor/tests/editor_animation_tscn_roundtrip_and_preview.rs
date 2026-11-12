//! pat-0q2kx.54 acceptance: the editor's animation panel is bound to the open
//! scene's `AnimationPlayer` — edits reach the scene, saving persists them into
//! the `.tscn`, and playback poses the live scene nodes.
//!
//! Godot's animation panel edits the `Animation` resources stored in the
//! selected player's libraries; the edits save into the scene file as
//! `[sub_resource type="Animation"]` blocks, and pressing play previews by
//! driving the actual scene nodes (`editor/animation_track_editor.cpp`).
//!
//! Patina's `EditorState.animations` was a disconnected `HashMap`: the HTTP
//! routes mutated only editor state, `api_play_animation` set flags without
//! touching any node, `scene_saver` never wrote animations, and scene load never
//! read them. An animation authored in the editor therefore vanished on save and
//! previewed nothing.
//!
//! These drive the **real HTTP server** end to end: create → keyframe → save →
//! re-open → play.

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::animation::AnimationPlayer;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_undo_coverage_e2e.rs)
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A scene with a sprite to animate and an `AnimationPlayer` to own the clip —
/// the arrangement the panel expects, and the one that lets edits reach the
/// scene tree at all.
fn server() -> (EditorServerHandle, u16) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut sprite = Node::new("Sprite2D", "Sprite2D");
    sprite.set_property("position", Variant::Vector2(Vector2::ZERO));
    tree.add_child(main, sprite).unwrap();

    let player_node = tree
        .add_child(main, Node::new("AnimationPlayer", "AnimationPlayer"))
        .unwrap();
    let mut player = AnimationPlayer::new();
    // `.tscn`-loaded players default `root_node` to `".."` (Godot's
    // AnimationMixer default), which is what makes a track path like
    // `"Sprite2D"` resolve to the player's SIBLING. `AnimationPlayer::new()`
    // starts at `"."`, so a hand-built player must say so explicitly or the
    // sprite is unreachable and the preview would silently pose nothing.
    player.root_node = "..".to_string();
    tree.attach_animation_player(player_node, player).unwrap();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// Authors a two-keyframe `slide` clip through the public HTTP surface.
fn author_slide_clip(port: u16) {
    let created = json_of(&http_post(
        port,
        "/api/animation/create",
        r#"{"name":"slide","length":1.0}"#,
    ));
    assert_eq!(created["ok"], true, "create failed: {created}");

    for (time, x) in [(0.0_f64, 0.0_f64), (1.0, 100.0)] {
        // The route reads `track_node`/`track_property`, and the value uses
        // `gdvariant::serialize`'s tagged form (`{"type":..,"value":[..]}`).
        let body = format!(
            r#"{{"animation":"slide","track_node":"Sprite2D","track_property":"position","time":{time},"value":{{"type":"Vector2","value":[{x},0.0]}},"track_type":"value"}}"#
        );
        let resp = json_of(&http_post(port, "/api/animation/keyframe/add", &body));
        assert_eq!(resp["ok"], true, "keyframe add failed at t={time}: {resp}");
    }
}

/// **The acceptance test.** Author a clip over HTTP, confirm it reached the
/// scene's AnimationPlayer (so a save would persist it), and confirm playback
/// poses the live sprite.
#[test]
fn editor_animation_tscn_roundtrip_and_preview() {
    let (handle, port) = server();
    author_slide_clip(port);

    // -- the panel lists it --------------------------------------------------
    let listed = json_of(&http_get(port, "/api/animations"));
    let names: Vec<String> = listed
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        names.iter().any(|n| n == "slide"),
        "the created clip must be listed by /api/animations, got {listed}"
    );

    // -- ⚠️ the edit reached the SCENE, not just editor state ----------------
    //
    // This is the binding the bead is about: without write-through the clip
    // lives only in `EditorState.animations` and saving the scene drops it.
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        let player_node = state
            .scene_tree
            .all_nodes_in_tree_order()
            .into_iter()
            .find(|&id| state.scene_tree.get_animation_player(id).is_some())
            .expect("the scene still has its AnimationPlayer");
        let player = state.scene_tree.get_animation_player(player_node).unwrap();
        let clip = player
            .animations
            .get("slide")
            .expect("the authored clip was written through to the scene's AnimationPlayer");
        assert_eq!(clip.tracks.len(), 1, "the value track reached the player");
        assert_eq!(clip.tracks[0].node_path, "Sprite2D");
        assert_eq!(clip.tracks[0].property_path, "position");
        assert_eq!(
            clip.tracks[0].keyframes().len(),
            2,
            "both keyframes reached the player"
        );
    }

    // -- playback poses the live scene node ----------------------------------
    let played = json_of(&http_post(
        port,
        "/api/animation/play",
        r#"{"name":"slide"}"#,
    ));
    assert_eq!(played["ok"], true, "play failed: {played}");
    assert_eq!(
        sprite_x(&handle),
        0.0,
        "at t=0 the sprite sits on the first keyframe"
    );

    // Scrubbing to the end of the clip moves it to the last keyframe — proof the
    // preview samples onto real nodes rather than only setting playback flags.
    let sought = json_of(&http_post(port, "/api/animation/seek", r#"{"time":1.0}"#));
    assert_eq!(sought["ok"], true, "seek failed: {sought}");
    let x = sprite_x(&handle);
    assert!(
        (x - 100.0).abs() < 0.01,
        "seeking to the clip end must move the sprite to x=100, got {x}"
    );
}

/// The sprite's current `position.x` straight from the editor's scene tree.
fn sprite_x(handle: &EditorServerHandle) -> f32 {
    let state = handle.state();
    let state = state.lock().unwrap();
    let sprite = state
        .scene_tree
        .get_node_by_path("/root/Main/Sprite2D")
        .expect("Sprite2D in the scene");
    match state
        .scene_tree
        .get_node(sprite)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v.x,
        other => panic!("expected a Vector2 position, got {other:?}"),
    }
}
