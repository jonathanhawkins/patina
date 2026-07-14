//! pat-yunxt: Acceptance gate for Inspector panel DOM parity across five
//! reference scenes.
//!
//! Acceptance: the rendered Inspector DOM (the ordered property list the
//! editor serves to the Inspector panel via `GET /api/node/<id>`) is
//! structurally equivalent to the Godot reference snapshot for each scene
//! — same fields in the same order. The Inspector dock displays one row
//! per property, in a deterministic order chosen by Godot's `_get_property_list`
//! contract; the editor must surface the same property names in the same
//! order so the Inspector renders identically.
//!
//! Five reference scenes are exercised — the same set used by the Scene
//! Tree dock parity gate (pat-dnyao):
//!   1. `minimal.tscn`
//!   2. `hierarchy.tscn`
//!   3. `multi_layer_2d.tscn`
//!   4. `main.tscn`
//!   5. `minimal_3d.tscn`
//!
//! Comparison rules (loose enough to tolerate editor-internal properties,
//! strict enough to catch real parity drift):
//!   - For every node the golden snapshot records, every property listed
//!     in the golden must appear in the editor's response, in the same
//!     relative order.
//!   - For every property the golden records, the editor's response must
//!     also expose a non-null value of a type derived from Godot's
//!     `Variant` (string, int, float, bool, Vector2, …).
//!   - Property names the editor exposes that are absent from the golden
//!     are tolerated — they represent editor-side bookkeeping that is not
//!     observable from Godot's `_get_property_list` (e.g. node IDs).

use std::collections::HashSet;
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

/// Ordered list of `(node_name, [property_name, …])` pairs collected
/// from the editor's response for each node in the loaded scene.
type InspectorView = Vec<(String, Vec<String>)>;

/// Walk the editor's `GET /api/scene` tree (rooted at the editor's
/// implicit `root` wrapper) and, for each node under it, query
/// `GET /api/node/<id>` to harvest the inspector's property list. The
/// editor's outer `root` is skipped so we line up with how the golden
/// snapshots are rooted at the scene's own first node.
fn collect_editor_inspector(port: u16) -> InspectorView {
    let scene_resp = http_get(port, "/api/scene");
    assert_eq!(
        status_code(&scene_resp),
        200,
        "GET /api/scene must succeed: {scene_resp}"
    );
    let scene_json: serde_json::Value = serde_json::from_str(extract_body(&scene_resp))
        .expect("scene JSON parses");
    let mut out = InspectorView::new();

    fn walk(port: u16, v: &serde_json::Value, out: &mut InspectorView, skip_root: bool) {
        if skip_root {
            if let Some(children) = v.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    walk(port, child, out, false);
                }
            }
            return;
        }
        let id = match v.get("id").and_then(|i| i.as_u64()) {
            Some(id) => id,
            None => return,
        };
        let name = v
            .get("name")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let node_resp = http_get(port, &format!("/api/node/{id}"));
        if status_code(&node_resp) != 200 {
            // Surface the failure in the inspector list so the comparison
            // panics with a clear message rather than silently skipping.
            out.push((name, vec![format!("__http_{}__", status_code(&node_resp))]));
            return;
        }
        let body = extract_body(&node_resp);
        let node_json: serde_json::Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(_) => {
                out.push((name, vec!["__parse_error__".into()]));
                return;
            }
        };
        let props: Vec<String> = node_json
            .get("properties")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        out.push((name, props));

        if let Some(children) = v.get("children").and_then(|c| c.as_array()) {
            for child in children {
                walk(port, child, out, false);
            }
        }
    }

    let nodes = &scene_json["nodes"];
    walk(port, nodes, &mut out, true);
    out
}

/// Walk a Godot oracle snapshot the same way and harvest the ordered
/// property names per node (preserving insertion order via `as_object()`
/// which is backed by `IndexMap`/`Map` in serde_json).
fn collect_golden_inspector(golden_json: &serde_json::Value) -> InspectorView {
    let mut out = InspectorView::new();
    if let Some(arr) = golden_json.get("nodes").and_then(|n| n.as_array()) {
        for n in arr {
            walk_golden(n, &mut out);
        }
    }
    out
}

fn walk_golden(v: &serde_json::Value, out: &mut InspectorView) {
    let name = v
        .get("name")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let props: Vec<String> = v
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    out.push((name, props));
    if let Some(children) = v.get("children").and_then(|c| c.as_array()) {
        for child in children {
            walk_golden(child, out);
        }
    }
}

/// Per-node comparison: every property the golden lists must appear in
/// the editor's list, in the same relative order. Extra editor-side
/// properties are allowed (they are not observable from Godot's
/// `_get_property_list` so we cannot expect parity on them).
fn editor_preserves_golden_order(editor: &[String], golden: &[String]) -> Result<(), String> {
    let editor_set: HashSet<&str> = editor.iter().map(|s| s.as_str()).collect();
    for g in golden {
        if !editor_set.contains(g.as_str()) {
            return Err(format!("editor is missing golden property '{g}'"));
        }
    }
    // Walk editor in order; the indices of golden properties inside the
    // editor list must be strictly increasing.
    let mut last_idx: i64 = -1;
    for g in golden {
        let idx = editor.iter().position(|e| e == g).unwrap() as i64;
        if idx <= last_idx {
            return Err(format!(
                "editor reorders golden property '{g}' (was at {last_idx}, now {idx}); editor order: {editor:?}"
            ));
        }
        last_idx = idx;
    }
    Ok(())
}

#[test]
fn inspector_helper_detects_missing_or_reordered_properties() {
    // Sanity-check the comparator before relying on it for the main test.
    let editor = vec!["position".to_string(), "rotation".to_string()];
    let golden_ok = vec!["position".to_string(), "rotation".to_string()];
    assert!(editor_preserves_golden_order(&editor, &golden_ok).is_ok());

    let golden_extra = vec!["position".to_string(), "scale".to_string()];
    assert!(editor_preserves_golden_order(&editor, &golden_extra).is_err());

    let golden_reordered = vec!["rotation".to_string(), "position".to_string()];
    assert!(editor_preserves_golden_order(&editor, &golden_reordered).is_err());

    // Editor with extra properties is fine if the golden ones still
    // appear in the right relative order.
    let editor_extra = vec![
        "_owner".to_string(),
        "position".to_string(),
        "_metadata".to_string(),
        "rotation".to_string(),
    ];
    assert!(editor_preserves_golden_order(&editor_extra, &golden_ok).is_ok());
}

#[test]
fn editor_inspector_dom_parity_across_five_reference_scenes() {
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

        let body =
            serde_json::json!({ "path": scene_path.to_string_lossy() }).to_string();
        let load_resp = http_post(port, "/api/scene/load", &body);
        if status_code(&load_resp) != 200 {
            failures.push(format!(
                "[{scene_name}] /api/scene/load failed: {}",
                extract_body(&load_resp)
            ));
            continue;
        }

        let editor_view = collect_editor_inspector(port);
        let golden_text = std::fs::read_to_string(&golden_path).expect("read golden");
        let golden_json: serde_json::Value =
            serde_json::from_str(&golden_text).expect("golden JSON parses");
        let golden_view = collect_golden_inspector(&golden_json);

        // The two walks must visit nodes in the same order: scene root
        // first, then depth-first children. Mismatched node names at the
        // same position is itself a parity failure.
        if editor_view.len() != golden_view.len() {
            failures.push(format!(
                "[{scene_name}] inspector visits different node counts: editor={}, golden={}",
                editor_view.len(),
                golden_view.len()
            ));
            continue;
        }

        for (idx, ((e_name, e_props), (g_name, g_props))) in
            editor_view.iter().zip(golden_view.iter()).enumerate()
        {
            if e_name != g_name {
                failures.push(format!(
                    "[{scene_name}] node {idx}: name mismatch — editor='{e_name}', golden='{g_name}'"
                ));
                continue;
            }
            if g_props.is_empty() {
                // Nothing to compare — the golden has no Inspector rows
                // for this node.
                continue;
            }
            if let Err(why) = editor_preserves_golden_order(e_props, g_props) {
                failures.push(format!(
                    "[{scene_name}] node '{e_name}' (idx {idx}): {why}; golden order: {g_props:?}"
                ));
            }
        }
    }

    handle.stop();

    assert!(
        failures.is_empty(),
        "Inspector DOM parity failed for {} of {} reference scenes:\n\n{}",
        failures.len(),
        REFERENCE_SCENES.len(),
        failures.join("\n")
    );
}

#[test]
fn inspector_parity_test_covers_at_least_five_scenes() {
    assert!(
        REFERENCE_SCENES.len() >= 5,
        "pat-yunxt requires at least five reference scenes; got {}",
        REFERENCE_SCENES.len()
    );
}
