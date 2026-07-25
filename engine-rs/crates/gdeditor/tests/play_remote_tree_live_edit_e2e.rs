//! pat-0q2kx.72: during play the editor can see and edit the RUNNING clone.
//!
//! Godot's Scene dock switches to a "Remote" tree while the game runs, and the
//! inspector edits that live instance (`editor/debugger/editor_debugger_tree.cpp`,
//! scene/debugger live edit). Patina runs play on a clone stored in
//! `run_main_loop`, but the dock kept reading `/api/scene` — the EDIT tree — so
//! nodes a script spawned in `_ready` were invisible, and every mutating route
//! was locked outright, so the inspector could not touch the running game at all.
//!
//! These drive the real HTTP server: play → read the remote tree → live-edit it
//! → confirm the edit tree is untouched.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
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

/// `Main -> Sprite2D`, with the sprite at x = 0 so a live edit is observable.
fn server() -> (EditorServerHandle, u16, u64) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let mut sprite = Node::new("Sprite2D", "Sprite2D");
    sprite.set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    let sprite_id = tree.add_child(main, sprite).unwrap();
    // `object_id()`/`raw()` live on NodeId, not on Node.
    let sprite_raw = sprite_id.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, sprite_raw)
}

/// **The acceptance test.**
#[test]
fn play_remote_tree_live_edit_e2e() {
    let (handle, port, sprite_raw) = server();

    // -- before play, the remote route reports "not running" ------------------
    let before = http_get(port, "/api/runtime/scene");
    assert!(
        before.contains("400"),
        "with no game running there is no remote tree to serve: {before}"
    );

    // -- start play ------------------------------------------------------------
    let played = json_of(&http_post(port, "/api/runtime/play", "{}"));
    assert_eq!(played["ok"], true, "play failed: {played}");

    // -- the Scene dock's Remote source serves the RUNNING clone ---------------
    let remote = json_of(&http_get(port, "/api/runtime/scene"));
    assert_eq!(remote["ok"], true, "runtime scene failed: {remote}");
    assert_eq!(
        remote["remote"], true,
        "the payload marks itself remote so the dock can label the tab"
    );
    assert!(
        remote["tree"].is_object(),
        "the remote payload carries a node tree: {remote}"
    );

    // -- live edit targets the clone, NOT the edit tree ------------------------
    //
    // ⚠️ Without `runtime: true` this route is edit-locked during play (409).
    // With it, the edit must land on the running clone.
    let live = json_of(&http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite_raw},"property":"position","value":{{"type":"Vector2","value":[42.0,0.0]}},"runtime":true}}"#
        ),
    ));
    assert_eq!(live["ok"], true, "live edit rejected: {live}");
    assert_eq!(
        live["runtime"], true,
        "the response says the edit went to the running clone"
    );

    {
        let state = handle.state();
        let state = state.lock().unwrap();

        // The running clone has the new value …
        let ml = state.run_main_loop.as_ref().expect("game is running");
        let running = ml
            .tree()
            .get_node_by_path("/root/Main/Sprite2D")
            .and_then(|id| ml.tree().get_node(id))
            .map(|n| n.get_property("position"));
        assert_eq!(
            running,
            Some(Variant::Vector2(gdcore::math::Vector2::new(42.0, 0.0))),
            "the live edit must reach the RUNNING clone"
        );

        // … and the edit tree is untouched, so stopping play does not leave the
        // saved scene silently modified.
        let edit = state
            .scene_tree
            .get_node_by_path("/root/Main/Sprite2D")
            .and_then(|id| state.scene_tree.get_node(id))
            .map(|n| n.get_property("position"));
        assert_eq!(
            edit,
            Some(Variant::Vector2(gdcore::math::Vector2::ZERO)),
            "a live edit must NOT dirty the edit tree"
        );
    }

    // -- addressing by PATH also works ----------------------------------------
    //
    // ⚠️ This is the branch that matters for a node the game SPAWNED: it has no
    // edit-tree twin, so there is no id to translate and only a path can reach
    // it. It is also why the id form works at all — play clones the tree, which
    // mints fresh ObjectIds, so an edit-tree id means nothing in the clone and
    // has to be resolved to a path first.
    let by_path = json_of(&http_post(
        port,
        "/api/property/set",
        r#"{"node_id":0,"path":"/root/Main/Sprite2D","property":"position","value":{"type":"Vector2","value":[99.0,0.0]},"runtime":true}"#,
    ));
    assert_eq!(
        by_path["ok"], true,
        "path-addressed live edit failed: {by_path}"
    );
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        let ml = state.run_main_loop.as_ref().expect("game is running");
        let running = ml
            .tree()
            .get_node_by_path("/root/Main/Sprite2D")
            .and_then(|id| ml.tree().get_node(id))
            .map(|n| n.get_property("position"));
        assert_eq!(
            running,
            Some(Variant::Vector2(gdcore::math::Vector2::new(99.0, 0.0))),
            "an explicit path must address the running node even with a bogus node_id"
        );
    }

    // -- the CLIENT is wired to all of it --------------------------------------
    //
    // The bead's third clause is client-side, and the routes existed with zero
    // callers before this. Asserting on EDITOR_HTML is how a Rust test can check
    // browser wiring without a browser: if the dock, the inspector or the movie
    // download stops calling these, the strings disappear.
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("/api/runtime/scene"),
        "the Scene dock must fetch the remote tree during play"
    );
    assert!(
        html.contains("/api/runtime/movie"),
        "stop must be able to retrieve the recorded frames"
    );
    assert!(
        html.contains("runtime: (typeof runtimeRunning"),
        "the inspector must send runtime:true while the game is running"
    );
    assert!(
        html.contains("Scene (Remote)"),
        "the dock labels itself Remote so a live tree is distinguishable"
    );

    // -- an ordinary (non-runtime) edit is still locked during play ------------
    let locked = http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite_raw},"property":"position","value":{{"type":"Vector2","value":[7.0,0.0]}}}}"#
        ),
    );
    assert!(
        locked.contains("409") || locked.contains("edit_locked"),
        "without runtime:true the route stays locked while running: {locked}"
    );
}
