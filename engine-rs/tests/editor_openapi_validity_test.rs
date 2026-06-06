//! pat-yuyqw: Validate `prd/editor_openapi.yaml` as OpenAPI 3 and check it
//! covers every route reported by `GET /api/capabilities`.
//!
//! Acceptance: the file parses with the `openapiv3` crate AND lists every
//! route the live `/api/capabilities` endpoint exposes.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

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
fn openapi_yaml_is_valid_openapi3_and_covers_capabilities() {
    let spec_path = repo_root().join("prd/editor_openapi.yaml");
    let yaml = std::fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("must read {}: {e}", spec_path.display()));

    // (1) Parse as OpenAPI 3 using the openapiv3 crate.
    let spec: openapiv3::OpenAPI = serde_yaml::from_str(&yaml)
        .expect("prd/editor_openapi.yaml must parse with openapiv3");
    assert!(
        spec.openapi.starts_with("3."),
        "spec must declare OpenAPI 3.x, got {}",
        spec.openapi
    );
    assert!(
        !spec.paths.paths.is_empty(),
        "OpenAPI document must list at least one path"
    );

    // (2) Collect (method, path) pairs from the OpenAPI document.
    use std::collections::BTreeSet;
    let mut openapi_routes: BTreeSet<(String, String)> = BTreeSet::new();
    for (path, ref_or_item) in &spec.paths.paths {
        let item = match ref_or_item {
            openapiv3::ReferenceOr::Item(p) => p,
            openapiv3::ReferenceOr::Reference { .. } => continue,
        };
        if item.get.is_some() {
            openapi_routes.insert(("GET".to_string(), path.clone()));
        }
        if item.post.is_some() {
            openapi_routes.insert(("POST".to_string(), path.clone()));
        }
        if item.put.is_some() {
            openapi_routes.insert(("PUT".to_string(), path.clone()));
        }
        if item.delete.is_some() {
            openapi_routes.insert(("DELETE".to_string(), path.clone()));
        }
        if item.patch.is_some() {
            openapi_routes.insert(("PATCH".to_string(), path.clone()));
        }
    }

    // (3) Pull the live capabilities document and confirm every route there
    // has an entry in the OpenAPI spec.
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));
    let body = http_get_body(port, "/api/capabilities");
    handle.stop();

    let caps: serde_json::Value =
        serde_json::from_str(&body).expect("/api/capabilities body must be valid JSON");
    let routes = caps["routes"]
        .as_array()
        .expect("capabilities must list `routes`");
    let mut capabilities_routes: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in routes {
        let method = entry["method"]
            .as_str()
            .expect("route method must be a string")
            .to_string();
        let path = entry["path"]
            .as_str()
            .expect("route path must be a string")
            .to_string();
        capabilities_routes.insert((method, path));
    }

    let missing: Vec<(String, String)> = capabilities_routes
        .difference(&openapi_routes)
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "OpenAPI spec is missing {} route(s) reported by /api/capabilities: {:?}",
        missing.len(),
        missing
    );
}
