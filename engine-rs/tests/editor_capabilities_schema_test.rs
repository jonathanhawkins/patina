//! pat-r5udv: `GET /api/capabilities` returns a deterministic machine-readable
//! route schema.
//!
//! Acceptance: the document lists every route with its method, path, params,
//! request body schema, and response shape; output is stable across runs.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn fixture_state() -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    EditorState::new(tree)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return s;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("could not connect to editor server on port {port}");
}

fn http_get_body(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).ok();
    response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .to_string()
}

#[test]
fn capabilities_returns_well_formed_deterministic_schema() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    let body1 = http_get_body(port, "/api/capabilities");
    let body2 = http_get_body(port, "/api/capabilities");
    assert_eq!(
        body1, body2,
        "capabilities output must be byte-deterministic across runs"
    );

    let v: serde_json::Value =
        serde_json::from_str(&body1).expect("capabilities body must be valid JSON");
    assert_eq!(v["version"], serde_json::json!(1), "schema version must be 1");
    let routes = v["routes"]
        .as_array()
        .expect("`routes` must be a JSON array");
    assert!(
        !routes.is_empty(),
        "capabilities must enumerate at least one route"
    );

    // The route list must be sorted by (method, path) so two clients always
    // agree on the order without doing their own sort.
    let mut prev: Option<(String, String)> = None;
    for entry in routes {
        let method = entry["method"]
            .as_str()
            .expect("each route must have a string `method`")
            .to_string();
        let path = entry["path"]
            .as_str()
            .expect("each route must have a string `path`")
            .to_string();
        // Validate the four required shape fields.
        assert!(
            entry["params"].is_array(),
            "route {method} {path} must have an array `params`"
        );
        assert!(
            entry.get("body_schema").is_some(),
            "route {method} {path} must include a `body_schema` field"
        );
        assert!(
            entry.get("response_shape").is_some(),
            "route {method} {path} must include a `response_shape` field"
        );
        if let Some(last) = prev.as_ref() {
            assert!(
                last <= &(method.clone(), path.clone()),
                "routes must be sorted by (method, path); {last:?} preceded ({method:?}, {path:?})"
            );
        }
        prev = Some((method, path));
    }

    // Sanity: the endpoint must list itself so clients can discover it from
    // the same document they're consuming.
    let self_entry = routes.iter().find(|r| {
        r["method"].as_str() == Some("GET") && r["path"].as_str() == Some("/api/capabilities")
    });
    assert!(
        self_entry.is_some(),
        "capabilities document must list /api/capabilities itself"
    );

    handle.stop();
}
