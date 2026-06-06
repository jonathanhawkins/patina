//! pat-dnyao: Acceptance gate for Scene Tree dock DOM parity across five
//! reference scenes.
//!
//! Acceptance: the rendered Scene Tree DOM (the JSON tree the editor serves
//! to the Scene Tree dock via `GET /api/scene`) is structurally equivalent
//! to the Godot reference snapshot for each scene. Structural equivalence
//! is defined as: same node names, same Godot class names, and same child
//! arity at every position in the tree. Cosmetic differences — node IDs,
//! file paths, `visible`/`is_instance`/`has_script` flags, properties —
//! are ignored because the Scene Tree dock displays only name + class +
//! hierarchy.
//!
//! Five reference scenes are exercised:
//!   1. `minimal.tscn`           — single-node baseline
//!   2. `hierarchy.tscn`         — three-deep 2D hierarchy
//!   3. `multi_layer_2d.tscn`    — wide 2D scene with many sibling types
//!   4. `main.tscn`              — flat 2D scene with three siblings
//!   5. `minimal_3d.tscn`        — wide 3D scene (Camera3D, MeshInstance3D, etc.)
//!
//! Each scene has a matching Godot oracle snapshot under
//! `fixtures/golden/scenes/<name>.json` produced by the Patina oracle
//! tooling against upstream Godot.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;

const REFERENCE_SCENES: &[&str] = &[
    "minimal",
    "hierarchy",
    "multi_layer_2d",
    "main",
    "minimal_3d",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn http_raw(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

fn http_get(port: u16, path: &str) -> String {
    http_raw(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_raw(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn status_code(resp: &str) -> u16 {
    resp.lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn wait_for_server(port: u16) {
    for _ in 0..40 {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Structural representation of a single Scene Tree node: name + class +
/// recursive children. Cosmetic data is intentionally dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DomNode {
    name: String,
    class: String,
    children: Vec<DomNode>,
}

impl DomNode {
    fn format(&self, indent: usize) -> String {
        let pad = "  ".repeat(indent);
        let mut s = format!("{pad}{}({})\n", self.name, self.class);
        for c in &self.children {
            s.push_str(&c.format(indent + 1));
        }
        s
    }
}

/// Normalize the editor's `GET /api/scene` JSON into a `DomNode` tree.
/// The editor wraps the loaded scene under its own implicit `root` node;
/// we strip that wrapper so the comparison starts at the scene root the
/// way Godot serializes it.
fn normalize_editor(scene_json: &serde_json::Value) -> Option<DomNode> {
    let root = scene_json.get("nodes")?;
    let children = root.get("children")?.as_array()?;
    let scene_root = children.first()?;
    Some(node_from_editor(scene_root))
}

fn node_from_editor(v: &serde_json::Value) -> DomNode {
    let name = v
        .get("name")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let class = v
        .get("class")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let children = v
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(node_from_editor).collect())
        .unwrap_or_default();
    DomNode {
        name,
        class,
        children,
    }
}

/// Normalize a Godot oracle snapshot JSON into a `DomNode` tree. The
/// oracle's top-level `nodes` is an array (the scene's roots), and each
/// node carries `name`, `class`, `children`. We use the first entry as
/// the scene root.
fn normalize_golden(snapshot_json: &serde_json::Value) -> Option<DomNode> {
    let arr = snapshot_json.get("nodes")?.as_array()?;
    let scene_root = arr.first()?;
    Some(node_from_golden(scene_root))
}

fn node_from_golden(v: &serde_json::Value) -> DomNode {
    let name = v
        .get("name")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let class = v
        .get("class")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let children = v
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(node_from_golden).collect())
        .unwrap_or_default();
    DomNode {
        name,
        class,
        children,
    }
}

#[test]
fn dom_node_helper_drops_cosmetic_fields() {
    // Sanity: the editor's extra fields (id, visible, has_script…) are
    // dropped, so two trees with the same shape but different IDs compare
    // equal under our normalization.
    let with_id = serde_json::json!({
        "id": 42,
        "name": "X",
        "class": "Node",
        "path": "/root/X",
        "visible": true,
        "has_script": false,
        "children": []
    });
    let without_id = serde_json::json!({
        "name": "X",
        "class": "Node",
        "children": []
    });
    assert_eq!(node_from_editor(&with_id), node_from_editor(&without_id));
}

#[test]
fn editor_scene_tree_dom_parity_across_five_reference_scenes() {
    // Spin up an in-process editor server.
    let port = free_port();
    let state = EditorState::new(SceneTree::new());
    let handle = EditorServerHandle::start(port, state);
    wait_for_server(port);

    let mut failures: Vec<String> = Vec::new();

    for scene_name in REFERENCE_SCENES {
        let scene_path = repo_root().join(format!("fixtures/scenes/{scene_name}.tscn"));
        let golden_path = repo_root().join(format!("fixtures/golden/scenes/{scene_name}.json"));

        assert!(
            scene_path.exists(),
            "reference scene must exist at {}",
            scene_path.display()
        );
        assert!(
            golden_path.exists(),
            "godot oracle snapshot must exist at {}",
            golden_path.display()
        );

        // Load the scene into the editor.
        let body =
            serde_json::json!({ "path": scene_path.to_string_lossy() }).to_string();
        let load_resp = http_post(port, "/api/scene/load", &body);
        let load_status = status_code(&load_resp);
        if load_status != 200 {
            failures.push(format!(
                "[{scene_name}] /api/scene/load failed with HTTP {load_status}: {}",
                extract_body(&load_resp)
            ));
            continue;
        }

        // Fetch the rendered Scene Tree DOM (the JSON the dock consumes).
        let scene_resp = http_get(port, "/api/scene");
        let scene_status = status_code(&scene_resp);
        if scene_status != 200 {
            failures.push(format!(
                "[{scene_name}] /api/scene failed with HTTP {scene_status}: {}",
                extract_body(&scene_resp)
            ));
            continue;
        }

        let editor_json: serde_json::Value = match serde_json::from_str(extract_body(&scene_resp)) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("[{scene_name}] editor JSON parse error: {e}"));
                continue;
            }
        };
        let editor_tree = match normalize_editor(&editor_json) {
            Some(t) => t,
            None => {
                failures.push(format!(
                    "[{scene_name}] editor JSON is missing nodes/children/scene-root"
                ));
                continue;
            }
        };

        let golden_text = match std::fs::read_to_string(&golden_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!(
                    "[{scene_name}] cannot read {}: {e}",
                    golden_path.display()
                ));
                continue;
            }
        };
        let golden_json: serde_json::Value = match serde_json::from_str(&golden_text) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("[{scene_name}] golden JSON parse error: {e}"));
                continue;
            }
        };
        let golden_tree = match normalize_golden(&golden_json) {
            Some(t) => t,
            None => {
                failures.push(format!(
                    "[{scene_name}] golden JSON has no scene-root under nodes[0]"
                ));
                continue;
            }
        };

        if editor_tree != golden_tree {
            failures.push(format!(
                "[{scene_name}] Scene Tree DOM diverged from Godot reference:\n--- editor ---\n{}--- godot reference ---\n{}",
                editor_tree.format(0),
                golden_tree.format(0)
            ));
        }
    }

    handle.stop();

    assert!(
        failures.is_empty(),
        "DOM parity failed for {} of {} reference scenes:\n\n{}",
        failures.len(),
        REFERENCE_SCENES.len(),
        failures.join("\n\n")
    );
}

#[test]
fn dom_parity_test_covers_at_least_five_scenes() {
    // Guard the bead's "five reference scenes" requirement at compile time
    // so accidentally pruning the list breaks this test, not the main one.
    assert!(
        REFERENCE_SCENES.len() >= 5,
        "pat-dnyao requires at least five reference scenes; got {}",
        REFERENCE_SCENES.len()
    );
}
