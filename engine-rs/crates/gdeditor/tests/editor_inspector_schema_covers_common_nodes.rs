//! pat-0q2kx.128 — the inspector serves a real class schema for the common node
//! and resource classes, not just the ones a captured oracle scene happened to
//! contain.
//!
//! Godot's `EditorInspector` enumerates a node's full `PROPERTY_USAGE_EDITOR`
//! schema through the inheritance chain, so every leaf class shows its own
//! members with their class defaults.
//!
//! Patina seeded 33 classes (`inspector_schema_generated.rs`, transcribed from
//! `fixtures/editor_oracle/inspector_*.json`) and `parent_of` mapped 48. Those
//! tables only ever covered what the oracle scenes used, so Line2D, Button,
//! GPUParticles2D, StandardMaterial3D, most of the GUI family and more returned
//! `None` from `editor_schema` and fell to `node_properties_json`'s stored-only
//! branch — a freshly created node showed a near-empty inspector.
//!
//! The fix regenerates the table with those classes synthesized from the
//! vendored `upstream/godot/doc/classes/*.xml` (membership + Godot's documented
//! defaults, walked base-first up `inherits`), and completes `parent_of` from
//! `godot_extension_api.json` so any class at all resolves to its nearest
//! seeded base.
//!
//! ⚠️ This drives the served `GET /api/node/<id>` JSON — the same path the
//! editor UI consumes — not `editor_schema` directly, so a schema that exists
//! but never reaches the wire would still fail.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Creates `class_name` under the root through the editor's own add-node route
/// and returns the served inspector JSON for it.
fn add_and_inspect(port: u16, root: u64, name: &str, class_name: &str) -> serde_json::Value {
    // NOTE: bind the response before borrowing its body — `extract_body`
    // borrows from the String, so inlining the call drops it while borrowed.
    let added = http(
        port,
        "POST",
        "/api/node/add",
        &format!(r#"{{"parent_id":{root},"name":"{name}","class_name":"{class_name}"}}"#),
    );
    assert!(
        added.contains("200 OK"),
        "adding a {class_name} should 200, got:\n{added}"
    );
    let added_json: serde_json::Value = serde_json::from_str(extract_body(&added))
        .unwrap_or_else(|e| panic!("add {class_name} response should be JSON ({e}):\n{added}"));
    let id = added_json["id"]
        .as_u64()
        .or_else(|| added_json["node_id"].as_u64())
        .unwrap_or_else(|| panic!("add {class_name} response has no id: {added_json}"));

    let fetched = http(port, "GET", &format!("/api/node/{id}"), "");
    assert!(
        fetched.contains("200 OK"),
        "GET /api/node/{id} for {class_name} should 200, got:\n{fetched}"
    );
    serde_json::from_str(extract_body(&fetched)).unwrap_or_else(|e| {
        panic!("/api/node/{id} for {class_name} should be JSON ({e}):\n{fetched}")
    })
}

/// The served property entry for `name`, or `None` when the schema omitted it.
fn prop<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    node["properties"]
        .as_array()?
        .iter()
        .find(|p| p["name"].as_str() == Some(name))
}

fn prop_or_panic<'a>(node: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    prop(node, name).unwrap_or_else(|| {
        let names: Vec<&str> = node["properties"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| p["name"].as_str()).collect())
            .unwrap_or_default();
        panic!(
            "{} should expose '{name}'; served {} properties: {names:?}",
            node["class"].as_str().unwrap_or("?"),
            names.len()
        )
    })
}

/// The `value` payload's inner scalar, however `to_json` tagged it.
fn value_of<'a>(entry: &'a serde_json::Value) -> &'a serde_json::Value {
    entry["value"].get("value").unwrap_or(&entry["value"])
}

#[test]
fn editor_inspector_schema_covers_common_nodes() {
    let port = free_port();
    let tree = SceneTree::new();
    let root = tree.root_id().raw();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    // -- Line2D: points / width / default_color / gradient / texture ---------
    let line = add_and_inspect(port, root, "Line", "Line2D");
    for name in ["points", "width", "default_color", "gradient", "texture"] {
        prop_or_panic(&line, name);
    }
    assert_eq!(
        value_of(prop_or_panic(&line, "width")).as_f64(),
        Some(10.0),
        "Line2D.width defaults to 10.0 in Godot"
    );
    // Inherited members must come through the same flattened schema.
    for name in ["position", "rotation", "scale", "visible", "modulate"] {
        prop_or_panic(&line, name);
    }

    // -- Button: text / icon / flat / alignment + Control anchors ------------
    let button = add_and_inspect(port, root, "Btn", "Button");
    for name in ["text", "icon", "flat", "alignment"] {
        prop_or_panic(&button, name);
    }
    assert_eq!(
        value_of(prop_or_panic(&button, "flat")).as_bool(),
        Some(false),
        "Button.flat defaults to false — a doc default of \"false\" must not \
         become true on the way through the generator"
    );
    assert_eq!(
        value_of(prop_or_panic(&button, "alignment")).as_i64(),
        Some(1),
        "Button.alignment defaults to CENTER (1) in Godot"
    );
    for name in ["anchor_left", "anchor_top", "anchor_right", "anchor_bottom"] {
        prop_or_panic(&button, name);
    }

    // -- GPUParticles2D: amount / lifetime / emitting ------------------------
    let particles = add_and_inspect(port, root, "Parts", "GPUParticles2D");
    for name in [
        "amount",
        "lifetime",
        "emitting",
        "one_shot",
        "explosiveness",
    ] {
        prop_or_panic(&particles, name);
    }
    assert_eq!(
        value_of(prop_or_panic(&particles, "amount")).as_i64(),
        Some(8),
        "GPUParticles2D.amount defaults to 8 in Godot"
    );
    assert_eq!(
        value_of(prop_or_panic(&particles, "lifetime")).as_f64(),
        Some(1.0),
        "GPUParticles2D.lifetime defaults to 1.0 in Godot"
    );
    assert_eq!(
        value_of(prop_or_panic(&particles, "emitting")).as_bool(),
        Some(true),
        "GPUParticles2D.emitting defaults to true in Godot"
    );

    handle.stop();
}

/// The control: these classes used to serve a near-empty, stored-only list. A
/// count floor catches a regression that keeps the property *names* resolving
/// while losing the flattened inheritance chain.
#[test]
fn common_classes_serve_a_full_schema_not_a_stored_only_stub() {
    let port = free_port();
    let tree = SceneTree::new();
    let root = tree.root_id().raw();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    for (class_name, floor) in [
        ("Line2D", 30usize),
        ("Button", 60),
        ("GPUParticles2D", 30),
        ("LineEdit", 60),
        ("TextureRect", 40),
    ] {
        let node = add_and_inspect(port, root, class_name, class_name);
        let count = node["properties"].as_array().map(|a| a.len()).unwrap_or(0);
        assert!(
            count >= floor,
            "{class_name} should serve its full class schema (>= {floor} props), \
             got {count} — that is the stored-only fallback, not a schema"
        );
    }

    handle.stop();
}

/// Every class Godot declares now resolves to *some* seeded schema, so a class
/// outside the synthesized list still inherits a sensible base instead of
/// dropping to stored-only.
#[test]
fn unseeded_classes_resolve_to_their_nearest_seeded_base() {
    use gdeditor::inspector_schema::{editor_schema, parent_of};

    // Not in the goldens and not in the synthesized list — these exercise the
    // completed `parent_of` fallback rather than a direct seed.
    for class_name in [
        "Path2D",
        "Marker2D",
        "VSplitContainer",
        "SubViewportContainer",
    ] {
        assert!(
            parent_of(class_name).is_some(),
            "{class_name} must have a known parent after the extension_api fallback"
        );
        let schema = editor_schema(class_name)
            .unwrap_or_else(|| panic!("{class_name} should resolve to a seeded ancestor's schema"));
        assert!(
            schema
                .iter()
                .any(|p| p.name == "name" || p.name == "visible"),
            "{class_name}'s resolved schema should carry the inherited base members"
        );
    }
}
