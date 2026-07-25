//! pat-0q2kx.71 — the inspector renders editors from PropertyHint metadata.
//!
//! The server has always emitted `hint`/`hint_string` in the node payload
//! (`property_entry`, fed from `inspector_schema`), but `createPropertyRow`
//! ignored both and matched on property *names* from a hand-written map. Any
//! enum outside that map — `texture_filter`, `texture_repeat`, `clip_children`,
//! every range-hinted float — degraded to a raw number box.
//!
//! These assert the two halves that have to meet: the payload really carries the
//! metadata for a property the old map never knew about, and the served renderer
//! really consumes it. The hint *parsers* are covered by executing them directly
//! (see the JS checks run during development); what a Rust test can pin is the
//! wiring and the round trip.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
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

fn http(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = if method == "GET" {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// **The acceptance.** An enum-hinted property the old name-map never covered
/// carries its metadata over the wire, and setting it by its dropdown value
/// round-trips.
#[test]
fn inspector_hint_editors_e2e() {
    // -- the renderer consumes hints, and does not depend on the name map -----
    for needle in [
        "parseEnumHint",
        "parseRangeHint",
        "makeEnumEditor",
        "makeRangeEditor",
        "makeFlagsEditor",
        "HINT_ENUM",
        "HINT_RANGE",
        "HINT_FLAGS",
        "hintIsLayers",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "served editor is missing the hint-driven renderer: {needle}"
        );
    }
    // The properties that DO carry a hint must no longer be listed in the name
    // map — two sources of truth is how the map went stale in the first place.
    for gone in [
        "texture_filter: [",
        "texture_repeat: [",
        "clip_children: [",
        "process_mode: [",
    ] {
        assert!(
            !EDITOR_HTML.contains(gone),
            "hint-covered property is still hardcoded in the name map: {gone}"
        );
    }

    // -- the payload carries the metadata ------------------------------------
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let sprite = tree
        .add_child(scene_root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    let sprite_raw = sprite.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let resp = http(port, "GET", &format!("/api/node/{sprite_raw}"), "");
    assert!(
        resp.contains("\"texture_filter\""),
        "Sprite2D payload must include texture_filter:\n{resp}"
    );
    assert!(
        resp.contains("\"hint\""),
        "the payload must carry hint metadata at all:\n{resp}"
    );
    assert!(
        resp.contains("\"hint_string\""),
        "the payload must carry hint_string:\n{resp}"
    );
    // texture_filter is PROPERTY_HINT_ENUM (2) with its labels inline — the
    // exact data the dropdown is built from.
    assert!(
        resp.contains("Nearest Mipmap"),
        "texture_filter's enum labels must reach the client:\n{resp}"
    );

    // -- setting it by the dropdown's value round-trips -----------------------
    let resp = http(
        port,
        "POST",
        "/api/property/set",
        &format!(
            r#"{{"node_id":{sprite_raw},"property":"texture_filter","value":{{"type":"Int","value":2}}}}"#
        ),
    );
    assert!(
        resp.contains("\"ok\":true") || resp.contains("200 OK"),
        "setting an enum property failed:\n{resp}"
    );

    // Scoped to texture_filter's own entry: a bare `"value":2` search would be
    // satisfied by any other int property that happens to be 2. Field order is
    // not assumed either — serde_json orders map keys alphabetically unless the
    // preserve_order feature is on.
    let resp = http(port, "GET", &format!("/api/node/{sprite_raw}"), "");
    let entry = resp
        .split(r#""texture_filter""#)
        .nth(1)
        .expect("texture_filter is present in the payload");
    let entry = &entry[..entry.len().min(300)];
    assert!(
        entry.contains("\"value\":2"),
        "the enum value must read back as 2 on texture_filter's own entry:\n{entry}"
    );
}

/// Range hints reach the client for a float property, so the editor can clamp.
#[test]
fn range_hint_metadata_reaches_the_client() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let cam = tree
        .add_child(scene_root, Node::new("Cam", "Camera3D"))
        .unwrap();
    let cam_raw = cam.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let resp = http(port, "GET", &format!("/api/node/{cam_raw}"), "");
    // Camera3D.fov is PROPERTY_HINT_RANGE with "1,179,0.1,degrees".
    assert!(
        resp.contains("\"fov\""),
        "Camera3D payload must include fov:\n{resp}"
    );
    assert!(
        resp.contains("1,179,0.1"),
        "fov's range hint must reach the client so the editor can clamp:\n{resp}"
    );
}

/// The clamp itself is implemented, not merely min/max attributes that a user
/// can type past.
#[test]
fn range_editor_clamps_on_commit() {
    let editor = EDITOR_HTML
        .split("function makeRangeEditor")
        .nth(1)
        .expect("makeRangeEditor exists")
        .split("function makeFlagsEditor")
        .next()
        .expect("the range editor body is delimited");

    assert!(
        editor.contains("n < range.min") && editor.contains("n > range.max"),
        "the committed value must be clamped to the hint's bounds, not just \
         advertised via input.min/max:\n{editor}"
    );
    assert!(
        editor.contains("range.orLess") && editor.contains("range.orGreater"),
        "Godot's or_less/or_greater make a bound soft; the clamp must respect \
         them:\n{editor}"
    );
}
