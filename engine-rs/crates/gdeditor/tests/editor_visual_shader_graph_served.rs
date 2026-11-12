//! pat-s3rrv.154 acceptance: a VisualShader resource is visible and editable
//! through the served editor.
//!
//! The runtime has read these resources for a while (`visual_shader_load.rs`),
//! but the editor had no route to them at all — 0 occurrences of VisualShader
//! in editor_ui.rs or editor_server.rs — so a project could RUN a visual shader
//! its author could not see.
//!
//! ⚠️ The write path had a trap worth naming. The .tres loader rewrites
//! `SubResource("id")` into the STRING `"SubResource:id"`, and `TresSaver` had
//! no step to turn it back — it re-quoted it. Our own loader accepts the string
//! form, so a Patina round trip looked perfectly clean while the file Godot
//! read had an ordinary String where a resource reference belonged, and the
//! graph silently lost its nodes. That is why this test asserts on the SAVED
//! TEXT and not only on a reload.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
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

fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_vshader_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A minimal graph: one colour constant, unconnected. The output node is NOT in
/// the file — Godot leaves it implicit at id 0 — which is what makes the id
/// allocator's "never below 2" rule matter.
const GRAPH_TRES: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[resource]
mode = 1
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/2/position = Vector2(120, 80)
"#;

fn serve(dir: &std::path::Path) -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());
    let port = free_port();
    // `EditorServerHandle` has no `Drop`, so the listener thread outlives this
    // helper and serves for the rest of the test.
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// GET returns the graph the loader already understands.
#[test]
fn get_returns_the_loaded_graph() {
    let dir = unique_dir("get");
    std::fs::write(dir.join("graph.tres"), GRAPH_TRES).unwrap();
    let port = serve(&dir);

    let resp = http_get(port, "/api/visual_shader?path=res://graph.tres");
    let body = body_of(&resp);

    assert!(body.contains("\"ok\":true"), "GET failed: {resp}");
    assert!(
        body.contains("\"id\":2"),
        "the node row is missing from the served graph: {body}"
    );
    // The CONCRETE class, not the 9-variant codegen enum — that enum drops every
    // class it does not recognise, so serving it would hide nodes the file has.
    assert!(
        body.contains("VisualShaderNodeColorConstant"),
        "the node's Godot class should be reported: {body}"
    );
    assert!(
        body.contains("\"position\":[120"),
        "the node position should be served: {body}"
    );
}

/// Add + connect, then assert the SAVED TEXT.
#[test]
fn adding_a_node_and_connecting_it_rewrites_the_tres() {
    let dir = unique_dir("write");
    let file = dir.join("graph.tres");
    std::fs::write(&file, GRAPH_TRES).unwrap();
    let port = serve(&dir);

    let add = http_post(
        port,
        "/api/visual_shader/node",
        r#"{"path":"res://graph.tres","type":"VisualShaderNodeColorConstant","position":[40,60]}"#,
    );
    assert!(body_of(&add).contains("\"ok\":true"), "add failed: {add}");
    // Ids never reuse 0 or 1: the output node is implicit at 0 and absent from
    // the file, so an allocator counting only what it can see would collide.
    assert!(
        body_of(&add).contains("\"id\":3"),
        "expected the next free id to be 3: {add}"
    );

    let conn = http_post(
        port,
        "/api/visual_shader/connect",
        r#"{"path":"res://graph.tres","from":3,"from_port":0,"to":0,"to_port":0}"#,
    );
    assert!(
        body_of(&conn).contains("\"ok\":true"),
        "connect failed: {conn}"
    );

    let saved = std::fs::read_to_string(&file).expect("the .tres is still readable");

    // ⚠️ The reference must be written in Godot's CALL syntax. `TresSaver` used
    // to re-quote the loader's normalised `"SubResource:id"` string, which our
    // own loader reads back happily — so only asserting on a reload would pass
    // while Godot saw a String and lost the node.
    assert!(
        saved.contains(r#"nodes/fragment/3/node = SubResource("n3")"#),
        "the new node must be written as a SubResource reference, not a quoted \
         string; saved file was:\n{saved}"
    );
    assert!(
        saved.contains(r#"[sub_resource type="VisualShaderNodeColorConstant" id="n3"]"#),
        "the new node needs its own sub_resource section:\n{saved}"
    );
    assert!(
        saved.contains("nodes/fragment/connections = PackedInt32Array(3, 0, 0, 0)"),
        "the connection quad must be written:\n{saved}"
    );
    // The pre-existing node must survive the rewrite — a save built from the
    // COMPILED graph would drop every class its 9-variant enum cannot name.
    assert!(
        saved.contains(r#"nodes/fragment/2/node = SubResource("Color_1")"#),
        "the original node was lost in the rewrite:\n{saved}"
    );

    // And it still parses back into the graph the editor serves.
    let resp = http_get(port, "/api/visual_shader?path=res://graph.tres");
    let body = body_of(&resp);
    assert!(
        body.contains("\"id\":3") && body.contains(r#""from":3"#),
        "the rewritten file should serve the new node and its connection: {body}"
    );
}

/// The served client can actually reach the route.
#[test]
fn the_client_references_the_visual_shader_route() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("/api/visual_shader"),
        "the editor client has no reference to the visual shader route"
    );
    assert!(
        html.contains("openVisualShader"),
        "the panel function is missing"
    );
}
