//! pat-mgixk.15 part 1: every animation-editor edit reaches the scene's
//! AnimationPlayer, so saving the scene persists it — and undo takes it back.
//!
//! **Wiring**: every edit below is a real `POST` to the editor server, and every
//! assertion reads either the server's own JSON or the `.tscn` bytes the server
//! wrote to disk. Nothing calls the handlers directly.
//!
//! **Godot ground truth**:
//! `editor/animation/animation_track_editor.cpp` mutates the `Animation`
//! resource the AnimationPlayer's library owns, through `undo_redo`. There is no
//! second copy, so a save always serialises the current clips and Ctrl+Z always
//! moves what the save would write.
//!
//! Patina keeps the panel's clips in `EditorState::animations` and mirrors them
//! onto the player with `write_animation_through`, which before this change was
//! called from only two places — keyframe-add and auto-key. Create, delete,
//! track add/delete/reorder, key remove, transition and paste never reached the
//! scene, `api_save_scene` never flushed, and undo updated the panel while
//! leaving the player untouched. So those edits were silently lost on save and
//! invisible to Play.
//!
//! ⚠️ Known gap this test deliberately does NOT assert: `scene_saver.rs` writes
//! `"transitions": PackedFloat32Array(1, 1, ...)` — a hardcoded `1` per key —
//! so a per-key transition cannot round-trip through a `.tscn` yet no matter
//! what the editor stores. The transition edit is therefore asserted where it
//! IS observable, on the clip the server reports back. Making the saver carry
//! real transitions is a `gdscene` change, outside this bead's file.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::animation::AnimationPlayer;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn post_ok(port: u16, path: &str, body: &str) -> String {
    let resp = http_post(port, path, body);
    assert!(
        resp.contains("200 OK"),
        "POST {path} {body} should succeed, got:\n{resp}"
    );
    resp
}

/// A scene that actually has an AnimationPlayer, which `make_test_server` in
/// `editor_test.rs` does not: `write_animation_through` has nowhere to put a
/// clip without one, so a fixture without a player would make every assertion
/// below pass or fail for the wrong reason.
///
/// `autoplay` is pre-set to `idle` so the delete test can show that removing a
/// clip also clears a reference that would otherwise be saved naming nothing.
fn make_animation_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world = Node::new("World", "Node2D");
    let world_id = tree.add_child(root, world).unwrap();

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    player.set_property("rotation", Variant::Float(0.0));
    tree.add_child(world_id, player).unwrap();

    let enemy = Node::new("Enemy", "Node2D");
    tree.add_child(world_id, enemy).unwrap();

    let anim_node = Node::new("Anim", "AnimationPlayer");
    let anim_id = tree.add_child(world_id, anim_node).unwrap();
    let mut ap = AnimationPlayer::new();
    ap.autoplay = Some("idle".to_string());
    tree.attach_animation_player(anim_id, ap).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

/// Saves the scene through the server and returns the bytes it wrote.
fn save_and_read(port: u16, dir: &std::path::Path, stem: &str) -> String {
    let path = dir.join(format!("{stem}.tscn"));
    let body = format!(r#"{{"path":"{}"}}"#, path.to_string_lossy());
    post_ok(port, "/api/scene/save", &body);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("saved scene should be readable: {e}"))
}

fn scratch_dir(stem: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("patina-mgixk15-{stem}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The `times` entries the saver wrote for `tracks/<idx>/keys` of the clip whose
/// sub-resource body contains `resource_name = "<clip>"`.
///
/// Parsed out of the file rather than string-matched whole, so the assertion is
/// about how many keys survived rather than about one exact formatting of them.
fn saved_key_times(tscn: &str, clip: &str) -> Vec<String> {
    let marker = format!("resource_name = \"{clip}\"");
    let start = match tscn.find(&marker) {
        Some(i) => i,
        None => return Vec::new(),
    };
    // The clip's body ends at the next section header.
    let rest = &tscn[start..];
    let body = match rest.find("\n[") {
        Some(i) => &rest[..i],
        None => rest,
    };
    let Some(line) = body
        .lines()
        .find(|l| l.trim_start().starts_with("\"times\": PackedFloat32Array("))
    else {
        return Vec::new();
    };
    let inner = line
        .split_once('(')
        .and_then(|(_, r)| r.rsplit_once(')'))
        .map(|(v, _)| v)
        .unwrap_or("");
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner.split(',').map(|v| v.trim().to_string()).collect()
}

// ===========================================================================
// 1. The acceptance: create / track / keys / remove / transition, then save
// ===========================================================================

#[test]
fn animation_editor_edits_persist_to_tscn() {
    let (handle, port) = make_animation_server();
    let dir = scratch_dir("persist");

    post_ok(
        port,
        "/api/animation/create",
        r#"{"name":"walk","length":1.0}"#,
    );
    post_ok(
        port,
        "/api/animation/track/add",
        r#"{"animation":"walk","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    post_ok(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    post_ok(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"position","time":0.5,"value":{"type":"Vector2","value":[100,0]}}"#,
    );

    // Two keys on one track, so removing one is observable as a count.
    let tscn = save_and_read(port, &dir, "two-keys");
    assert_eq!(
        saved_key_times(&tscn, "walk").len(),
        2,
        "both keyframes should be in the saved clip:\n{tscn}"
    );

    post_ok(
        port,
        "/api/animation/keyframe/remove",
        r#"{"animation":"walk","track_index":0,"keyframe_index":0}"#,
    );
    post_ok(
        port,
        "/api/animation/keyframe/transition",
        r#"{"animation":"walk","track_index":0,"keyframe_index":0,"transition":"nearest"}"#,
    );

    let tscn = save_and_read(port, &dir, "one-key");

    // The clip reached the AnimationPlayer at all — `api_create_animation` and
    // `api_add_track` never touched the scene before this change, so without it
    // the file has no Animation sub-resource whatsoever.
    assert!(
        tscn.contains("[sub_resource type=\"Animation\""),
        "the saved scene must carry an Animation sub-resource:\n{tscn}"
    );
    assert!(
        tscn.contains("resource_name = \"walk\""),
        "the saved scene must carry the 'walk' clip:\n{tscn}"
    );
    assert!(
        tscn.contains("\"walk\": SubResource("),
        "the AnimationLibrary must map 'walk' to its sub-resource:\n{tscn}"
    );
    // The explicit track add is in the file, addressed the way Godot writes it.
    assert!(
        tscn.contains("NodePath(\"Player:position\")"),
        "the added property track must be saved as a NodePath:\n{tscn}"
    );
    // ...and the key REMOVE reached the file too, not just the panel.
    assert_eq!(
        saved_key_times(&tscn, "walk").len(),
        1,
        "removing a keyframe must leave one key in the saved clip:\n{tscn}"
    );

    // The transition edit is asserted where the format can express it (see the
    // module header): the clip the server reports back.
    let resp = http_get(
        port,
        "/api/animation/keyframe/transition?animation=walk&track_index=0&keyframe_index=0",
    );
    assert!(resp.contains("200 OK"), "{resp}");
    assert!(
        extract_body(&resp).contains("nearest"),
        "the transition edit must stick: {}",
        extract_body(&resp)
    );

    handle.stop();
}

// ===========================================================================
// 2. Delete reaches the file (and takes a dangling autoplay with it)
// ===========================================================================

#[test]
fn deleting_an_animation_removes_it_from_the_saved_scene() {
    let (handle, port) = make_animation_server();
    let dir = scratch_dir("delete");

    for clip in ["walk", "idle"] {
        post_ok(
            port,
            "/api/animation/create",
            &format!(r#"{{"name":"{clip}","length":1.0}}"#),
        );
        post_ok(
            port,
            "/api/animation/keyframe/add",
            &format!(
                r#"{{"animation":"{clip}","track_node":"Player","track_property":"position","time":0.0,"value":{{"type":"Vector2","value":[0,0]}}}}"#
            ),
        );
    }

    let tscn = save_and_read(port, &dir, "both");
    assert!(
        tscn.contains("resource_name = \"idle\""),
        "precondition: 'idle' must be in the file before the delete:\n{tscn}"
    );
    // The fixture pre-set autoplay to 'idle', and the saver regenerates that
    // line from the player, so it is in the file now.
    assert!(
        tscn.contains("autoplay = \"idle\""),
        "precondition: autoplay should name 'idle':\n{tscn}"
    );

    post_ok(port, "/api/animation/delete", r#"{"name":"idle"}"#);
    let tscn = save_and_read(port, &dir, "deleted");

    assert!(
        !tscn.contains("resource_name = \"idle\""),
        "the deleted clip must be gone from the saved scene:\n{tscn}"
    );
    assert!(
        !tscn.contains("\"idle\": SubResource("),
        "the AnimationLibrary must no longer map 'idle':\n{tscn}"
    );
    // A clip can be deleted while something still names it as the autoplay; the
    // save would then write an autoplay the next load cannot resolve.
    assert!(
        !tscn.contains("autoplay = \"idle\""),
        "deleting the autoplay clip must clear the reference:\n{tscn}"
    );
    // The surviving clip is untouched — delete must not take the library with it.
    assert!(
        tscn.contains("resource_name = \"walk\""),
        "the other clip must survive the delete:\n{tscn}"
    );

    handle.stop();
}

// ===========================================================================
// 3. Undo moves the file, not just the panel
// ===========================================================================

#[test]
fn undo_after_a_track_add_removes_it_from_the_panel_and_the_file() {
    let (handle, port) = make_animation_server();
    let dir = scratch_dir("undo");

    post_ok(
        port,
        "/api/animation/create",
        r#"{"name":"walk","length":1.0}"#,
    );
    post_ok(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    post_ok(
        port,
        "/api/animation/track/add",
        r#"{"animation":"walk","node_path":"Enemy","property":"rotation","track_type":"property"}"#,
    );

    let tscn = save_and_read(port, &dir, "with-track");
    assert!(
        tscn.contains("NodePath(\"Enemy:rotation\")"),
        "precondition: the added track must be saved before the undo:\n{tscn}"
    );

    post_ok(port, "/api/undo", "{}");

    // The panel: the server's own view of the clip.
    let resp = http_get(port, "/api/animation?name=walk");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let tracks = v["tracks"].as_array().expect("tracks array");
    // `api_get_animation` reports a track as `node_path`/`property`.
    assert!(
        !tracks
            .iter()
            .any(|t| t["node_path"] == "Enemy" && t["property"] == "rotation"),
        "undo must remove the track from the panel: {v}"
    );
    assert!(
        tracks
            .iter()
            .any(|t| t["node_path"] == "Player" && t["property"] == "position"),
        "...without removing the track that predates the undone edit: {v}"
    );

    // ...and the file, which is the half that was broken: undo_one updated
    // `state.animations` and left the AnimationPlayer holding the undone track.
    let tscn = save_and_read(port, &dir, "undone");
    assert!(
        !tscn.contains("NodePath(\"Enemy:rotation\")"),
        "undo must remove the track from the saved scene too:\n{tscn}"
    );
    // The keyframe track that predates the undone edit is still there.
    assert!(
        tscn.contains("NodePath(\"Player:position\")"),
        "undo must not take the earlier track with it:\n{tscn}"
    );

    handle.stop();
}

// ===========================================================================
// 4. ...and so does the command-palette route, which was a third copy
// ===========================================================================

/// `POST /api/command/execute {"command":"undo"}` is the menu/palette path, and
/// it was a *separate* hand-written undo from `/api/undo` — the thinnest of the
/// three, dropping `undo()`'s Result and reversing neither the tilemap store nor
/// the animation map. Same user-visible action, different behaviour depending on
/// which route the client happened to call.
#[test]
fn the_command_palette_undo_also_reverses_an_animation_edit() {
    let (handle, port) = make_animation_server();
    let dir = scratch_dir("palette");

    post_ok(
        port,
        "/api/animation/create",
        r#"{"name":"walk","length":1.0}"#,
    );
    post_ok(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    post_ok(
        port,
        "/api/animation/track/add",
        r#"{"animation":"walk","node_path":"Enemy","property":"rotation","track_type":"property"}"#,
    );

    let tscn = save_and_read(port, &dir, "before");
    assert!(
        tscn.contains("NodePath(\"Enemy:rotation\")"),
        "precondition: the added track must be saved before the undo:\n{tscn}"
    );

    post_ok(port, "/api/command/execute", r#"{"command":"undo"}"#);

    let resp = http_get(port, "/api/animation?name=walk");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let tracks = v["tracks"].as_array().expect("tracks array");
    assert!(
        !tracks
            .iter()
            .any(|t| t["node_path"] == "Enemy" && t["property"] == "rotation"),
        "the palette undo must reverse the animation edit too: {v}"
    );

    let tscn = save_and_read(port, &dir, "after");
    assert!(
        !tscn.contains("NodePath(\"Enemy:rotation\")"),
        "...and that must reach the saved scene:\n{tscn}"
    );

    handle.stop();
}
