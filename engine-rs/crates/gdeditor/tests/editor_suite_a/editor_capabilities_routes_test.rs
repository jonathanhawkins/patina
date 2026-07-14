//! Regression test for pat-k6ggf: GET /api/capabilities must reflect the live
//! router (no missing routes) and publish body_schema field names that the live
//! handlers actually accept (e.g. `class_name`, not `class`; `property`, not
//! `name`).
//!
//! App-level: boots the real EditorServerHandle over HTTP and (a) asserts every
//! route the bug named as missing is present in the document, (b) probes every
//! param-less GET route the document advertises and asserts it is actually
//! served (not a 404 route-miss), and (c) self-tests POST /api/node/add and
//! POST /api/property/set using exactly the field names the document publishes,
//! asserting HTTP 200 (i.e. the handler accepts the documented body).

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

#[test]
fn editor_capabilities_matches_live_routes() {
    let (handle, port) = make_server();

    // ---- Fetch and parse the capabilities document. ----
    let resp = http_get(port, "/api/capabilities");
    assert!(
        resp.contains("200 OK"),
        "GET /api/capabilities should return 200, got:\n{resp}"
    );
    let doc: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("capabilities body should be valid JSON");
    let routes = doc["routes"]
        .as_array()
        .expect("capabilities.routes should be an array");

    // Build the set of documented (METHOD, PATH) pairs, and index by path for
    // body_schema lookups.
    let documented: std::collections::HashSet<(String, String)> = routes
        .iter()
        .map(|r| {
            (
                r["method"].as_str().unwrap_or("").to_string(),
                r["path"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();

    // ---- (a) Every route the bug named as missing must now be documented. ----
    // These all respond 200 from the live router but were absent from the
    // hand-maintained capabilities list.
    let must_be_documented: &[(&str, &str)] = &[
        ("GET", "/api/editor/main_view"),
        ("POST", "/api/editor/mode"),
        ("GET", "/api/logs"),
        ("GET", "/api/debug/state"),
        ("POST", "/api/scene/tabs/open"),
        ("GET", "/api/scene/tabs"),
        ("GET", "/api/monitors/frame_times"),
        ("GET", "/api/profiler"),
        // The two routes whose schemas were wrong must of course be present too.
        ("POST", "/api/node/add"),
        ("POST", "/api/property/set"),
    ];
    for (method, path) in must_be_documented {
        assert!(
            documented.contains(&(method.to_string(), path.to_string())),
            "/api/capabilities is missing live route {method} {path}; it is served by the router and responds 200"
        );
    }

    // ---- Document self-consistency: every param-less GET it advertises must
    // actually be served (not a 404 route-miss). This proves the doc reflects
    // real routes. Restricted to GETs with no params so a 200 is unambiguous
    // and the probe stays fully read-only. ----
    for r in routes {
        let method = r["method"].as_str().unwrap_or("");
        let path = r["path"].as_str().unwrap_or("");
        let params = r["params"].as_array().map(|a| a.len()).unwrap_or(0);
        if method != "GET" || params != 0 {
            continue;
        }
        // The viewport frame endpoints serve the latest rendered frame from a
        // cache that is empty on a freshly-booted headless server, so they
        // legitimately answer 404 ("no frame yet") despite being real, routed
        // endpoints. That is an empty-state response, not a stale-doc phantom,
        // so exclude them from the route-miss probe.
        if path == "/api/viewport" || path == "/api/viewport/png" {
            continue;
        }
        let probe = http_get(port, path);
        let status_line = probe.lines().next().unwrap_or("");
        assert!(
            !status_line.contains(" 404"),
            "GET {path} is advertised by /api/capabilities but the router returns 404 (phantom route). Status: {status_line}"
        );
    }

    // ---- (b) Self-test POST /api/node/add against its PUBLISHED schema. ----
    // The published body_schema must name `class_name` (the field the handler
    // deserializes), not `class`.
    let add_schema = routes
        .iter()
        .find(|r| r["method"] == "POST" && r["path"] == "/api/node/add")
        .map(|r| &r["body_schema"])
        .expect("POST /api/node/add must be documented");
    assert!(
        add_schema.get("class_name").is_some(),
        "POST /api/node/add body_schema must publish field `class_name`, got: {add_schema}"
    );
    assert!(
        add_schema.get("class").is_none(),
        "POST /api/node/add body_schema must NOT publish the bogus field `class` (the handler rejects it), got: {add_schema}"
    );

    let main_id = get_main_node_id(port);
    let add_body = format!(r#"{{"parent_id":{main_id},"name":"CapChild","class_name":"Node2D"}}"#);
    let add_resp = http_post(port, "/api/node/add", &add_body);
    assert!(
        add_resp.contains("200 OK"),
        "POST /api/node/add using the published body_schema fields should return 200, got:\n{add_resp}"
    );
    assert!(
        !add_resp.contains("missing class_name"),
        "POST /api/node/add must accept the documented `class_name` field, got:\n{add_resp}"
    );
    let added: serde_json::Value = serde_json::from_str(extract_body(&add_resp))
        .expect("node/add response should be JSON");
    let new_id = added["id"]
        .as_u64()
        .expect("node/add should return the new node id");

    // ---- (b') Self-test POST /api/property/set against its PUBLISHED schema. ----
    // The published body_schema must name `property`, not `name`.
    let prop_schema = routes
        .iter()
        .find(|r| r["method"] == "POST" && r["path"] == "/api/property/set")
        .map(|r| &r["body_schema"])
        .expect("POST /api/property/set must be documented");
    assert!(
        prop_schema.get("property").is_some(),
        "POST /api/property/set body_schema must publish field `property`, got: {prop_schema}"
    );
    assert!(
        prop_schema.get("name").is_none(),
        "POST /api/property/set body_schema must NOT publish the bogus field `name` (the handler rejects it), got: {prop_schema}"
    );

    let set_body = format!(
        r#"{{"node_id":{new_id},"property":"position","value":{{"type":"Vector2","value":[5.0,6.0]}}}}"#
    );
    let set_resp = http_post(port, "/api/property/set", &set_body);
    assert!(
        set_resp.contains("200 OK"),
        "POST /api/property/set using the published `property` field should return 200, got:\n{set_resp}"
    );
    assert!(
        !set_resp.contains("missing property"),
        "POST /api/property/set must accept the documented `property` field, got:\n{set_resp}"
    );

    handle.stop();
}