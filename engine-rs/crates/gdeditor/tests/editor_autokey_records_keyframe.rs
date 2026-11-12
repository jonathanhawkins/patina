//! pat-s3rrv.62 — **auto-key**: while armed, a property edit records a keyframe.
//!
//! Godot's auto-key writes a key on every inspector edit while the toggle is on,
//! which is what makes "move the node, get a keyframe" work
//! (`editor/animation/animation_track_editor.cpp`).
//!
//! Patina already had the FLAG and nothing else. `AnimationPlaybackState::
//! recording` was settable via `POST /api/animation/record`, reported in
//! `GET /api/animation/status`, and read by the panel — but **no code consumed
//! it**: `api_set_property` never looked at it, so arming auto-key flipped a
//! boolean and recorded nothing. This drives the real HTTP surface end to end.
//!
//! ⚠️ Named for what it covers. The bead's full acceptance
//! (`editor_animation_onion_autokey_served`) also asserts onion-skin ghost
//! pixels and an onion parameter in `EDITOR_HTML`; that half is not implemented
//! yet, so it is deliberately NOT claimed here.

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

// -- HTTP harness (mirrors editor_animation_tscn_roundtrip_and_preview.rs) ---

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

/// Finds a node's raw id by name anywhere in the scene JSON.
fn find_id(v: &serde_json::Value, name: &str) -> Option<u64> {
    if v["name"].as_str() == Some(name) {
        return v["id"].as_u64();
    }
    for c in v["children"].as_array()?.iter() {
        if let Some(id) = find_id(c, name) {
            return Some(id);
        }
    }
    None
}

fn node_id(port: u16, name: &str) -> u64 {
    let scene = json_of(&http_get(port, "/api/scene"));
    find_id(&scene["nodes"], name).unwrap_or_else(|| panic!("no node named {name}: {scene}"))
}

/// A sprite to animate plus an `AnimationPlayer` to own the clip.
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
    // `.tscn`-loaded players default `root_node` to `".."`, which is what makes
    // a track path like `"Sprite2D"` resolve to the player's SIBLING.
    player.root_node = "..".to_string();
    tree.attach_animation_player(player_node, player).unwrap();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// Creates a clip and makes it the CURRENT one, which is what auto-key writes
/// into.
///
/// ⚠️ Creating a clip does not select it. The only route that sets
/// `AnimationPlaybackState::animation_name` is `POST /api/animation/play`
/// (editor_server.rs:12937) — there is no "select without playing". So the
/// clip is opened by playing it, and the playhead is parked afterwards
/// (`play` also resets `current_time` to 0).
///
/// That coupling is a real parity gap, not a quirk of this test: Godot's
/// auto-key writes into the clip SELECTED in the animation panel and does not
/// require playback. Recorded in the bead rather than worked around silently.
fn open_clip(port: u16) {
    let created = json_of(&http_post(
        port,
        "/api/animation/create",
        r#"{"name":"slide","length":1.0}"#,
    ));
    assert_eq!(created["ok"], true, "create failed: {created}");
    let played = json_of(&http_post(
        port,
        "/api/animation/play",
        r#"{"name":"slide"}"#,
    ));
    assert_eq!(
        played["ok"], true,
        "play (to select the clip) failed: {played}"
    );
}

/// Every `(node_path, property)` pair currently on the `slide` clip.
///
/// Used to tell "no track was written" apart from "a track was written under a
/// path this test does not recognise" — the two produce an identical empty key
/// list otherwise, and that ambiguity hid a real bug once already.
fn track_paths(port: u16) -> Vec<(String, String)> {
    let anim = json_of(&http_get(port, "/api/animation?name=slide"));
    anim["tracks"]
        .as_array()
        .map(|ts| {
            ts.iter()
                .map(|t| {
                    (
                        t["node_path"].as_str().unwrap_or("<none>").to_string(),
                        t["property"].as_str().unwrap_or("<none>").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Counts keyframes on the `Sprite2D:position` track of `slide`.
///
/// ⚠️ The track path is RELATIVE to the AnimationPlayer's `root_node` (`".."`
/// here, so the player's parent) — `"Sprite2D"`, not `"/root/Main/Sprite2D"`.
/// That is the form playback and the editor preview resolve against, and the
/// form Godot writes to `.tscn`.
fn position_keys(port: u16) -> Vec<f64> {
    let anim = json_of(&http_get(port, "/api/animation?name=slide"));
    let tracks = anim["tracks"].as_array().cloned().unwrap_or_default();
    for t in tracks {
        // ⚠️ The emitted key is `property`, NOT `property_path` — the server
        // renames it on the way out, and asserting the struct's field name
        // would silently match nothing and report an empty track.
        if t["node_path"].as_str() == Some("Sprite2D") && t["property"].as_str() == Some("position")
        {
            return t["keyframes"]
                .as_array()
                .map(|ks| ks.iter().filter_map(|k| k["time"].as_f64()).collect())
                .unwrap_or_default();
        }
    }
    Vec::new()
}

/// **The claim.** With auto-key armed, a property edit lands a keyframe at the
/// playhead; with it disarmed, the same edit lands none.
#[test]
fn editor_autokey_records_keyframe() {
    let (_h, port) = server();
    open_clip(port);
    let sprite = node_id(port, "Sprite2D");

    // -- disarmed: the control that makes the positive case meaningful -------
    //
    // Without this, a test that only checks the armed case would pass even if
    // auto-key recorded on EVERY edit regardless of the toggle.
    let set = json_of(&http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite},"property":"position","value":{{"type":"Vector2","value":[10.0,0.0]}}}}"#
        ),
    ));
    assert_eq!(set["ok"], true, "property set failed: {set}");
    assert!(
        position_keys(port).is_empty(),
        "auto-key is disarmed, so no keyframe should exist yet — got {:?}",
        position_keys(port)
    );

    // -- arm auto-key and park the playhead ---------------------------------
    let rec = json_of(&http_post(
        port,
        "/api/animation/record",
        r#"{"enabled":true}"#,
    ));
    assert_eq!(rec["ok"], true, "record toggle failed: {rec}");
    let seek = json_of(&http_post(
        port,
        "/api/animation/seek",
        // `api_seek_animation` reads only `time`.
        r#"{"time":0.5}"#,
    ));
    assert_eq!(seek["ok"], true, "seek failed: {seek}");

    // -- armed: the same edit now records ------------------------------------
    let set = json_of(&http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite},"property":"position","value":{{"type":"Vector2","value":[42.0,0.0]}}}}"#
        ),
    ));
    assert_eq!(set["ok"], true, "property set failed: {set}");

    // ⚠️ Check the track path BEFORE the key count. A track written under the
    // wrong path yields an empty key list that is indistinguishable from "auto
    // key did nothing", which is precisely how the absolute-path bug below hid.
    let paths = track_paths(port);
    assert!(
        paths.contains(&("Sprite2D".to_string(), "position".to_string())),
        "auto-key must write the track path RELATIVE to the AnimationPlayer's \
         root_node — `Sprite2D`, not an absolute `/root/Main/Sprite2D`, which \
         resolves to nothing at playback. Tracks on the clip: {paths:?}"
    );

    let keys = position_keys(port);
    assert_eq!(
        keys.len(),
        1,
        "an armed auto-key edit must record exactly one keyframe, got {keys:?}. \
         An empty list means `recording` is still being ignored by \
         api_set_property — the flag existed long before anything read it."
    );
    assert!(
        (keys[0] - 0.5).abs() < 1e-6,
        "the keyframe must land at the PLAYHEAD (0.5), got {}",
        keys[0]
    );
}

/// Auto-key with no clip selected is a no-op, not an error — editing a property
/// with the toggle left on and nothing open is an ordinary thing to do.
#[test]
fn autokey_without_a_selected_clip_is_harmless() {
    let (_h, port) = server();
    let sprite = node_id(port, "Sprite2D");

    let rec = json_of(&http_post(
        port,
        "/api/animation/record",
        r#"{"enabled":true}"#,
    ));
    assert_eq!(rec["ok"], true);

    let set = json_of(&http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite},"property":"position","value":{{"type":"Vector2","value":[7.0,0.0]}}}}"#
        ),
    ));
    assert_eq!(
        set["ok"], true,
        "the edit itself must still succeed with auto-key armed and no clip: {set}"
    );
}
