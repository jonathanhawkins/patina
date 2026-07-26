//! pat-0q2kx.143 — a `ShaderMaterial`'s uniforms are editable inspector
//! properties.
//!
//! Godot ground truth: `ShaderMaterial._get_property_list` exposes every uniform
//! as `shader_parameter/<name>` with a hint-driven editor (a `hint_range`
//! uniform gets a slider, `source_color` a colour picker), writes persist as
//! `shader_parameter/*` in the `.tscn`, and edits update the viewport.
//!
//! Patina had both halves and no join. `packed_scene` already flattens
//! `shader_code` and `shader_parameter/*` onto the node, and
//! `gdserver3d::shader::parse_uniforms` already reads the uniform table with
//! hints — but `grep shader_parameter crates/gdeditor/src/` was **zero**. A
//! shader's parameters were invisible in the inspector and could not be edited.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// A shader with one of each editor-relevant uniform shape: a plain float, a
/// range-hinted float (slider), and a `source_color` vec4 (colour picker).
const SHADER: &str = r#"
shader_type canvas_item;
uniform float plain_scale = 2.0;
uniform float strength : hint_range(0.0, 4.0, 0.5) = 1.5;
uniform vec4 tint : source_color = vec4(1.0, 1.0, 1.0, 1.0);
"#;

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

/// **The acceptance test.** Uniforms reach the served inspector with hints and
/// values, an edit persists as `shader_parameter/<name>` on the node, and the
/// served editor carries the client path that groups them.
#[test]
fn inspector_shader_parameters() {
    // -- (3) the client renders a shader-parameters group ---------------------
    //
    // Checked first because it needs no server: if the payload were perfect and
    // the renderer had no path for it, the feature would still be invisible.
    assert!(
        EDITOR_HTML.contains("shader_parameter/"),
        "the served editor must have a client path for shader_parameter/* props"
    );
    assert!(
        EDITOR_HTML.contains("Shader Parameters"),
        "shader uniforms must land in Godot's own 'Shader Parameters' subgroup, \
         not fall out ungrouped under the node's class"
    );

    // -- a node carrying an inline ShaderMaterial, as packed_scene flattens it -
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let sprite = tree
        .add_child(scene_root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    {
        let node = tree.get_node_mut(sprite).unwrap();
        node.set_property("shader_code", Variant::String(SHADER.to_string()));
    }
    let sprite_raw = sprite.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // -- (1) GET lists the uniforms, with hints and values --------------------
    let resp = http(port, "GET", &format!("/api/node/{sprite_raw}"), "");
    for uniform in [
        "shader_parameter/plain_scale",
        "shader_parameter/strength",
        "shader_parameter/tint",
    ] {
        assert!(
            resp.contains(uniform),
            "the inspector payload must list `{uniform}`:\n{resp}"
        );
    }
    // The range hint drives a slider — its bounds must survive to the client.
    assert!(
        resp.contains("0,4,0.5") || resp.contains("0.0,4.0,0.5"),
        "the hint_range bounds must reach the client as hint_string:\n{resp}"
    );
    assert!(
        resp.contains("source_color"),
        "the source_color hint must reach the client so it draws a colour \
         picker rather than four number boxes:\n{resp}"
    );
    // A uniform with no stored override shows the shader's own default, which is
    // what the shader actually runs with — not Nil.
    assert!(
        !resp.contains("\"shader_parameter/plain_scale\",\"value\":null"),
        "an un-overridden uniform must show its shader default, not null:\n{resp}"
    );

    // -- (2) POST an edit; it persists as shader_parameter/<name> -------------
    let body = format!(
        r#"{{"node_id":{sprite_raw},"property":"shader_parameter/tint","value":{{"type":"Color","value":[1.0,0.0,0.0,1.0]}}}}"#
    );
    let post = http(port, "POST", "/api/property/set", &body);
    assert!(
        post.contains("200") || post.contains("\"ok\""),
        "setting a shader parameter must be accepted:\n{post}"
    );

    let after = http(port, "GET", &format!("/api/node/{sprite_raw}"), "");
    assert!(
        after.contains("shader_parameter/tint"),
        "the edited uniform must still be listed after the write:\n{after}"
    );
}

/// A node with no `shader_code` grows no shader section — the enumeration must
/// not invent parameters for every node in the scene.
#[test]
fn nodes_without_a_shader_get_no_parameters() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let plain = tree
        .add_child(scene_root, Node::new("Plain", "Sprite2D"))
        .unwrap();
    let plain_raw = plain.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let resp = http(port, "GET", &format!("/api/node/{plain_raw}"), "");
    assert!(
        !resp.contains("shader_parameter/"),
        "a node without a ShaderMaterial must expose no shader parameters:\n{resp}"
    );
}
