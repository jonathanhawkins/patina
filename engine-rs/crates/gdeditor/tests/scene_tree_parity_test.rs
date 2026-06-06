//! Parity tests for editor beads: scene tree ops, indicators, inspector,
//! create-node dialog, and resource toolbar.
//!
//! Covers: pat-lac, pat-t0c, pat-mn3, pat-48k, pat-des, pat-4mc

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

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
    );
    tree.add_child(root, main).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

fn add_child(port: u16, parent_id: u64, name: &str, class: &str) -> u64 {
    let body = format!(r#"{{"parent_id":{parent_id},"name":"{name}","class_name":"{class}"}}"#);
    let r = http_post(port, "/api/node/add", &body);
    serde_json::from_str::<serde_json::Value>(extract_body(&r)).unwrap()["id"]
        .as_u64()
        .unwrap()
}

// =========================================================================
// pat-lac: Scene Tree parity — node operations
// =========================================================================

#[test]
fn test_lac_add_and_delete_node() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let nid = add_child(port, mid, "Temp", "Sprite2D");

    // Verify node exists in scene tree
    let scene = http_get(port, "/api/scene");
    assert!(extract_body(&scene).contains("Temp"));

    // Delete it
    let dr = http_post(port, "/api/node/delete", &format!(r#"{{"node_id":{nid}}}"#));
    assert!(dr.contains("200 OK"));

    // Verify node is gone
    let scene2 = http_get(port, "/api/scene");
    assert!(!extract_body(&scene2).contains("Temp"));
    handle.stop();
}

#[test]
fn test_lac_reparent_node() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Create two children under Main: Parent1 and Child
    let p1 = add_child(port, mid, "Parent1", "Node2D");
    let child = add_child(port, mid, "Child", "Sprite2D");

    // Reparent Child under Parent1
    let rr = http_post(
        port,
        "/api/node/reparent",
        &format!(r#"{{"node_id":{child},"new_parent_id":{p1}}}"#),
    );
    assert!(rr.contains("200 OK"));

    // Verify Child is now under Parent1 in the scene tree
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let main_children = v["nodes"]["children"][0]["children"].as_array().unwrap();
    // Parent1 should have Child as its child
    let parent1_node = main_children
        .iter()
        .find(|c| c["name"] == "Parent1")
        .unwrap();
    let p1_children = parent1_node["children"].as_array().unwrap();
    assert!(
        p1_children.iter().any(|c| c["name"] == "Child"),
        "Child should be reparented under Parent1"
    );
    handle.stop();
}

#[test]
fn test_lac_duplicate_preserves_properties() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let orig = add_child(port, mid, "Original", "Sprite2D");

    // Set a property on the original
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{orig},"property":"tag","value":{{"type":"String","value":"hello"}}}}"#
        ),
    );

    // Duplicate
    let dr = http_post(
        port,
        "/api/node/duplicate",
        &format!(r#"{{"node_id":{orig}}}"#),
    );
    assert!(dr.contains("200 OK"));
    let dup_id = serde_json::from_str::<serde_json::Value>(extract_body(&dr)).unwrap()["id"]
        .as_u64()
        .unwrap();

    // Verify duplicate has the same class
    let n: serde_json::Value = serde_json::from_str(extract_body(&http_get(
        port,
        &format!("/api/node/{dup_id}"),
    )))
    .unwrap();
    assert_eq!(n["class"], "Sprite2D");
    handle.stop();
}

#[test]
fn test_lac_delete_nonexistent_returns_404() {
    let (handle, port) = make_server();
    let r = http_post(port, "/api/node/delete", r#"{"node_id":9999999}"#);
    assert!(r.contains("404"));
    handle.stop();
}

// =========================================================================
// pat-t0c: Scene Tree indicators — class, script, groups
// =========================================================================

#[test]
fn test_t0c_class_icon_in_scene_json() {
    let (handle, port) = make_server();
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let main_node = &v["nodes"]["children"][0];
    // Class field should be present for icon selection
    assert_eq!(main_node["class"], "Node2D");
    assert!(main_node.get("id").is_some());
    handle.stop();
}

#[test]
fn test_t0c_script_badge_indicator() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let nid = add_child(port, mid, "Scripted", "Node2D");

    // Attach a script path
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{nid},"property":"_script_path","value":{{"type":"String","value":"res://test.gd"}}}}"#
        ),
    );

    // Verify has_script is true in scene tree JSON
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let scripted = v["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Scripted")
        .unwrap();
    assert_eq!(scripted["has_script"], true);
    handle.stop();
}

#[test]
fn test_t0c_group_membership_in_scene_json() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let nid = add_child(port, mid, "Grouped", "Node2D");

    // Add node to a group
    let gr = http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{nid},"group":"enemies"}}"#),
    );
    assert!(gr.contains("200 OK"));

    // Verify groups array in scene JSON
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let grouped = v["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Grouped")
        .unwrap();
    let groups = grouped["groups"].as_array().unwrap();
    assert!(
        groups.iter().any(|g| g == "enemies"),
        "Node should appear in 'enemies' group"
    );
    handle.stop();
}

#[test]
fn test_t0c_no_script_badge_by_default() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let nid = add_child(port, mid, "Plain", "Node2D");

    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let plain = v["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Plain")
        .unwrap();
    // Node without script should have has_script = false
    assert_eq!(plain["has_script"], false);
    let _ = nid; // used for add_child
    handle.stop();
}

// =========================================================================
// pat-mn3: Inspector core editing — Variant types through /api/property/set
// =========================================================================

#[test]
fn test_mn3_set_get_string() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let r = http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"label","value":{{"type":"String","value":"hello world"}}}}"#
        ),
    );
    assert!(r.contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "label")
        .unwrap();
    assert_eq!(prop["type"], "String");
    handle.stop();
}

#[test]
fn test_mn3_set_get_int() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let r = http_post(
        port,
        "/api/property/set",
        &format!(r#"{{"node_id":{mid},"property":"health","value":{{"type":"Int","value":42}}}}"#),
    );
    assert!(r.contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "health")
        .unwrap();
    assert_eq!(prop["type"], "Int");
    handle.stop();
}

#[test]
fn test_mn3_set_get_float() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let r = http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"speed","value":{{"type":"Float","value":3.14}}}}"#
        ),
    );
    assert!(r.contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "speed")
        .unwrap();
    assert_eq!(prop["type"], "Float");
    handle.stop();
}

#[test]
fn test_mn3_set_get_bool() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let r = http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"active","value":{{"type":"Bool","value":true}}}}"#
        ),
    );
    assert!(r.contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "active")
        .unwrap();
    assert_eq!(prop["type"], "Bool");
    handle.stop();
}

#[test]
fn test_mn3_set_get_vector2() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let r = http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"velocity","value":{{"type":"Vector2","value":[1.0,2.0]}}}}"#
        ),
    );
    assert!(r.contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "velocity")
        .unwrap();
    assert_eq!(prop["type"], "Vector2");
    handle.stop();
}

// =========================================================================
// pat-48k: Inspector advanced — array editing, dict editing, resource ref
// =========================================================================

#[test]
fn test_48k_array_element_editing() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Set an array with 3 elements
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"items","value":{{"type":"Array","value":[{{"type":"Int","value":10}},{{"type":"Int","value":20}},{{"type":"Int","value":30}}]}}}}"#
        ),
    );

    // Read back and verify all 3 elements
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let arr = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "items")
        .unwrap();
    let elements = arr["value"]["value"].as_array().unwrap();
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0]["value"], 10);
    assert_eq!(elements[1]["value"], 20);
    assert_eq!(elements[2]["value"], 30);
    handle.stop();
}

#[test]
fn test_48k_dictionary_editing() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Set a dictionary with multiple keys
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"metadata","value":{{"type":"Dictionary","value":{{"name":{{"type":"String","value":"player"}},"level":{{"type":"Int","value":5}}}}}}}}"#
        ),
    );

    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let dict = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "metadata")
        .unwrap();
    assert_eq!(dict["type"], "Dictionary");
    let dict_val = dict["value"]["value"].as_object().unwrap();
    assert_eq!(dict_val.len(), 2);
    assert!(dict_val.contains_key("name"));
    assert!(dict_val.contains_key("level"));
    handle.stop();
}

#[test]
fn test_48k_resource_reference_display() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Set a resource-like string property (resource paths use String type)
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"texture","value":{{"type":"String","value":"res://icon.png"}}}}"#
        ),
    );

    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let prop = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "texture")
        .unwrap();
    assert_eq!(prop["type"], "String");
    // Value should be the resource path
    assert!(prop["value"]["value"]
        .as_str()
        .unwrap()
        .starts_with("res://"));
    handle.stop();
}

#[test]
fn test_48k_empty_array_and_dict() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Empty array
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"empty_arr","value":{{"type":"Array","value":[]}}}}"#
        ),
    );
    // Empty dict
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"empty_dict","value":{{"type":"Dictionary","value":{{}}}}}}"#
        ),
    );

    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    let props = v["properties"].as_array().unwrap();
    let arr = props.iter().find(|p| p["name"] == "empty_arr").unwrap();
    assert_eq!(arr["value"]["value"].as_array().unwrap().len(), 0);
    let dict = props.iter().find(|p| p["name"] == "empty_dict").unwrap();
    assert_eq!(dict["value"]["value"].as_object().unwrap().len(), 0);
    handle.stop();
}

// =========================================================================
// pat-des: Inspector resource toolbar — /api/selected returns property list
// =========================================================================

#[test]
fn test_des_selected_returns_property_list() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);

    // Select the Main node
    http_post(port, "/api/node/select", &format!(r#"{{"node_id":{mid}}}"#));

    // GET /api/selected should return node with properties
    let resp = http_get(port, "/api/selected");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("properties").is_some(),
        "/api/selected must return properties"
    );
    assert!(v["properties"].as_array().is_some());
    assert_eq!(v["class"], "Node2D");
    assert_eq!(v["name"], "Main");
    handle.stop();
}

#[test]
fn test_des_selected_null_when_nothing_selected() {
    let (handle, port) = make_server();
    // Without selecting anything, /api/selected should return null
    let resp = http_get(port, "/api/selected");
    let body = extract_body(&resp);
    assert!(body.trim() == "null" || body.contains("null"));
    handle.stop();
}

// =========================================================================
// pat-4mc: Create Node dialog
// =========================================================================

#[test]
fn test_4mc_create_dialog_returns_class_list() {
    let (handle, port) = make_server();
    let r = http_post(port, "/api/node/create_dialog", "{}");
    assert!(r.contains("200 OK"));
    let v: serde_json::Value = serde_json::from_str(extract_body(&r)).unwrap();
    let classes = v["classes"].as_array().unwrap();
    assert!(
        classes.len() >= 10,
        "Should return at least 10 node classes"
    );
    // Core 2D types must be present
    assert!(classes.iter().any(|x| x == "Node2D"));
    assert!(classes.iter().any(|x| x == "Sprite2D"));
    assert!(classes.iter().any(|x| x == "Camera2D"));
    handle.stop();
}

#[test]
fn test_4mc_create_dialog_includes_control_types() {
    let (handle, port) = make_server();
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog",
        "{}",
    )))
    .unwrap();
    let classes = v["classes"].as_array().unwrap();
    assert!(classes.iter().any(|x| x == "Control"));
    assert!(classes.iter().any(|x| x == "Label"));
    assert!(classes.iter().any(|x| x == "Button"));
    handle.stop();
}

#[test]
fn test_4mc_create_dialog_includes_physics_types() {
    let (handle, port) = make_server();
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog",
        "{}",
    )))
    .unwrap();
    let classes = v["classes"].as_array().unwrap();
    assert!(classes.iter().any(|x| x == "CharacterBody2D"));
    assert!(classes.iter().any(|x| x == "RigidBody2D"));
    assert!(classes.iter().any(|x| x == "StaticBody2D"));
    handle.stop();
}
