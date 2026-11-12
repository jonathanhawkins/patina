//! pat-ghfv0.35: Interaction-sequence oracle — open → 3D → select → inspector set.
//!
//! Drives the served editor over HTTP through a scripted workflow golden and
//! asserts mode, selection, and inspector property state match the expected
//! post-sequence snapshot (Godot workflow goldens live under
//! `fixtures/editor_oracle/interaction_sequences/`).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use serde_json::Value;

static LOCK: Mutex<()> = Mutex::new(());

fn start_server() -> (EditorServerHandle, u16) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(0, state);
    let port = handle.port();
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
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

fn fixtures_root() -> PathBuf {
    // engine-rs/crates/gdeditor -> engine-rs/fixtures
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn sequence_path() -> PathBuf {
    fixtures_root().join("editor_oracle/interaction_sequences/open_3d_select_set.json")
}

#[test]
fn interaction_sequence_open_3d_select_set() {
    let _g = LOCK.lock().unwrap();
    let seq: Value = serde_json::from_str(
        &std::fs::read_to_string(sequence_path()).expect("read interaction sequence golden"),
    )
    .expect("parse sequence json");

    let expect = seq.get("expect").expect("expect block");
    let expected_mode = expect["mode"].as_str().unwrap();
    let expected_class = expect["selection_class"].as_str().unwrap();
    let expected_prop = &expect["inspector_property"];

    let (handle, port) = start_server();

    // open project (fixtures root so scenes resolve)
    let fixtures = fixtures_root();
    let open_body = format!(
        r#"{{"path":{}}}"#,
        serde_json::to_string(&fixtures.to_string_lossy()).unwrap()
    );
    let open = http_post(port, "/api/project/open", &open_body);
    assert!(
        open.contains("200 OK") || open.contains("\"ok\""),
        "project open should succeed: {open}"
    );

    // load scene
    let scene_rel = seq["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["action"] == "load_scene")
        .and_then(|s| s["path"].as_str())
        .unwrap_or("scenes/minimal_3d.tscn");
    let scene_abs = fixtures.join(scene_rel);
    let load_body = format!(
        r#"{{"path":{}}}"#,
        serde_json::to_string(&scene_abs.to_string_lossy()).unwrap()
    );
    let load = http_post(port, "/api/scene/load", &load_body);
    assert!(
        load.contains("200 OK") || load.contains("ok"),
        "scene load should succeed: {load}"
    );

    // switch to 3D
    let mode_resp = http_post(port, "/api/editor/mode", r#"{"mode":"3d"}"#);
    assert!(
        mode_resp.contains("200 OK") || mode_resp.contains("3d"),
        "set mode 3d: {mode_resp}"
    );
    let mode_get = http_get(port, "/api/editor/mode");
    let mode_body = extract_body(&mode_get);
    assert!(
        mode_body.contains(expected_mode),
        "mode should be {expected_mode}, got {mode_body}"
    );

    // Find a MeshInstance3D (or expected class) via the loaded scene graph.
    let tree_resp = http_get(port, "/api/scene");
    let tree_body = extract_body(&tree_resp);
    let tree_json: Value = serde_json::from_str(tree_body).unwrap_or(Value::Null);

    fn find_class_id(v: &Value, class: &str) -> Option<u64> {
        if let Some(obj) = v.as_object() {
            let cls = obj
                .get("class")
                .or_else(|| obj.get("type"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            let id = obj
                .get("id")
                .or_else(|| obj.get("node_id"))
                .and_then(|i| i.as_u64().or_else(|| i.as_i64().map(|x| x as u64)));
            if cls == class {
                if let Some(id) = id {
                    return Some(id);
                }
            }
            for (_k, child) in obj {
                if let Some(found) = find_class_id(child, class) {
                    return Some(found);
                }
            }
        } else if let Some(arr) = v.as_array() {
            for item in arr {
                if let Some(found) = find_class_id(item, class) {
                    return Some(found);
                }
            }
        }
        None
    }

    let node_id = find_class_id(&tree_json, expected_class).unwrap_or_else(|| {
        // Fallback: raw body must mention the class; pick the first numeric id.
        let class_token = format!("\"{expected_class}\"");
        assert!(
            tree_body.contains(&class_token) || tree_body.contains(expected_class),
            "no {expected_class} in scene tree: {tree_body}"
        );
        // Scan for `"id": N` or `"node_id": N`
        let mut found = None;
        for key in ["\"id\":", "\"node_id\":"] {
            if let Some(idx) = tree_body.find(key) {
                let rest = &tree_body[idx + key.len()..];
                let digits: String = rest
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(n) = digits.parse::<u64>() {
                    found = Some(n);
                    break;
                }
            }
        }
        found.expect("numeric id in scene tree payload")
    });

    let sel = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{node_id}}}"#),
    );
    assert!(
        sel.contains("200 OK") || sel.contains("ok"),
        "select node: {sel}"
    );

    // set property visible = false (tagged Variant JSON — see gdvariant::from_json)
    let prop_name = expected_prop["name"].as_str().unwrap();
    let prop_val = expected_prop["value"].as_bool().unwrap();
    let set_body = format!(
        r#"{{"node_id":{node_id},"property":"{prop_name}","value":{{"type":"Bool","value":{prop_val}}}}}"#
    );
    let set = http_post(port, "/api/property/set", &set_body);
    assert!(
        set.contains("200 OK") || set.contains("ok"),
        "property set: {set}"
    );

    // Assert inspector / selected reflects the *value* (not just HTTP 200 or
    // property name presence — set-then-read must observe the written state).
    let selected = http_get(port, "/api/selected");
    let sel_body = extract_body(&selected);
    let sel_json: Value = serde_json::from_str(sel_body).unwrap_or(Value::Null);
    let props = sel_json
        .get("properties")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let prop = props
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(prop_name))
        .unwrap_or_else(|| {
            panic!("selected inspector must list property `{prop_name}`, body: {sel_body}")
        });
    // Godot-tagged Variant JSON: { "type": "Bool", "value": false }
    let got = prop
        .pointer("/value/value")
        .or_else(|| prop.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        got,
        Value::Bool(prop_val),
        "inspector property `{prop_name}` must be {prop_val} after set, got {got} in {prop}"
    );

    // Final mode still 3d.
    let mode_final_resp = http_get(port, "/api/editor/mode");
    let mode_final = extract_body(&mode_final_resp);
    assert!(
        mode_final.contains(expected_mode),
        "mode remains {expected_mode}: {mode_final}"
    );

    drop(handle);
}

#[test]
fn interaction_sequence_golden_file_exists_and_is_well_formed() {
    let raw = std::fs::read_to_string(sequence_path()).expect("golden file must exist");
    let v: Value = serde_json::from_str(&raw).expect("valid json");
    assert_eq!(v["id"], "open_3d_select_set");
    assert!(v["steps"].as_array().unwrap().len() >= 4);
    assert_eq!(v["expect"]["mode"], "3d");
}
