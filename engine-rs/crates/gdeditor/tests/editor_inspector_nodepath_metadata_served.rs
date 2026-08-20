//! pat-s3rrv.151 acceptance: the Inspector's NodePath picker and Add Metadata
//! are real controls, not labelled stubs.
//!
//! Both shipped as placeholders: the `...` button carried the title "Pick node
//! (not yet implemented)" and had no click handler at all, and Add Metadata was
//! wired to a `logMessage` and nothing else. The NodePath branch was also
//! over-broad — it claimed `texture`, `script`, and ANY property whose name
//! merely contained "path", so a plain String got node-picker affordances.
//!
//! The metadata half is the one with depth. A node keeps metadata in `meta`, a
//! different map from `properties`, while a .tscn spells it as a `metadata/foo`
//! property row — so the value has to be readable through BOTH, and it is the
//! `get_meta` side that a script sees.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_3d_gizmo_handles_served.rs)
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

/// The two controls this bead is about are no longer labelled stubs.
///
/// ⚠️ The needles are the FULL stub strings, not the bare phrase "not yet
/// implemented" — the served client still carries that phrase for unrelated
/// controls (asset import, plugins view), so asserting on the phrase alone
/// would fail for reasons that have nothing to do with this bead, and would
/// start passing if someone implemented those instead.
#[test]
fn the_two_inspector_stubs_are_gone_from_the_served_client() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    assert!(
        !html.contains("Pick node (not yet implemented)"),
        "the NodePath picker button is still labelled a stub"
    );
    assert!(
        !html.contains("Add Metadata (not yet implemented)"),
        "Add Metadata is still labelled a stub"
    );

    // Positive controls: absence alone would also be satisfied by deleting the
    // buttons, so pin that the replacements are actually present.
    assert!(
        html.contains("openNodePathPicker"),
        "the picker function is missing"
    );
    assert!(
        html.contains("openAddMetadataDialog"),
        "the Add Metadata dialog function is missing"
    );
    assert!(
        html.contains("nodepath-picker-dialog") && html.contains("add-metadata-dialog"),
        "the dialog markup is missing"
    );
}

/// The NodePath editor no longer claims properties that are not NodePaths.
#[test]
fn the_nodepath_branch_is_narrowed_to_nodepath_properties() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    assert!(
        !html.contains("prop.name.indexOf('path') >= 0"),
        "a property is still routed to the NodePath editor merely for containing \
         \"path\" in its name"
    );
    // `script` keeps an editor — the resource picker, which is what it is.
    assert!(
        html.contains("script: 'Script'"),
        "script should route through the resource picker instead"
    );
}

/// Add Metadata round-trip: the endpoint writes metadata that is visible to a
/// script through `get_meta`, which is the form the acceptance names.
#[test]
fn add_metadata_endpoint_writes_metadata_readable_as_get_meta() {
    const SCENE: &str = "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node\"]\n";
    let scene = gdscene::packed_scene::PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // Returns the instanced scene's OWN root, which is the node under test.
    let node = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("scene instances");
    let raw = node.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(
        port,
        "/api/node/metadata",
        &format!(r#"{{"node_id":{raw},"name":"foo","type":"int","value":42}}"#),
    );
    assert!(
        body_of(&resp).contains("\"ok\":true"),
        "the endpoint rejected the write: {resp}"
    );

    // A scene must still be served afterwards — a 500 here would show as an
    // empty body and the assertion above could still have passed on a stale
    // connection.
    let scene = http_get(port, "/api/scene");
    assert!(
        body_of(&scene).contains("Main"),
        "the scene should still serve after the write: {scene}"
    );

    let state = handle.state();
    let state = state.lock().unwrap();
    let node_ref = state.scene_tree.get_node(node).expect("node still exists");

    assert_eq!(
        node_ref.get_meta("foo"),
        Variant::Int(42),
        "metadata written by the Inspector must be readable via get_meta, which \
         is what a script's get_meta(\"foo\") call reads"
    );
    assert_eq!(
        node_ref.get_property("metadata/foo"),
        Variant::Int(42),
        "and as the `metadata/foo` property row, which is what the saver writes \
         into the .tscn"
    );
}
