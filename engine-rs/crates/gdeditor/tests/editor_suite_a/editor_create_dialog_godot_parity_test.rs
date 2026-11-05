//! Godot CreateDialog 1:1 parity tests for the Create New Node dialog.
//!
//! Covers the server contract that mirrors `editor/gui/create_dialog.cpp`:
//! subsequence matching (`is_subsequence_ofn`), `_score_type` best-match
//! auto-selection, the Matches hierarchy with greyed abstract ancestors,
//! favorites (star toggle + drag reorder), recent tracking on node creation,
//! and the served UI's structure (Favorites/Recent/Search/Matches/Description
//! panes, no name prompt).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
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

fn make_server() -> (EditorServerHandle, u16) {
    let (handle, port, _root) = make_server_with_root();
    (handle, port)
}

/// Like `make_server`, plus the scene root's raw id.
///
/// Node ids come from a process-wide counter, so the root is id 1 only in a
/// fresh process. Under nextest every test gets one; under plain `cargo test`
/// the tests share a process and a hard-coded `1` names no node (pat-hukbz).
fn make_server_with_root() -> (EditorServerHandle, u16, u64) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node2D");
    tree.add_child(root, main).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, root.raw())
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

fn dialog_response(port: u16, body: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog",
        body,
    )))
    .expect("create_dialog response must be JSON")
}

/// Finds a node by class name in the serialized Matches tree.
fn find_in_tree<'a>(node: &'a serde_json::Value, class_name: &str) -> Option<&'a serde_json::Value> {
    if node["class_name"] == class_name {
        return Some(node);
    }
    node["children"]
        .as_array()?
        .iter()
        .find_map(|c| find_in_tree(c, class_name))
}

// ---- Matches tree: hierarchy + abstract ancestors (Godot _add_type) ----

#[test]
fn tree_rooted_at_node_with_full_hierarchy_on_empty_search() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    let tree = &v["tree"];
    assert_eq!(tree["class_name"], "Node", "tree must be rooted at Node");
    // Deep hierarchy nesting: Node → CanvasItem → Node2D → Sprite2D.
    let canvas_item = find_in_tree(tree, "CanvasItem").expect("CanvasItem in tree");
    assert!(
        find_in_tree(canvas_item, "Sprite2D").is_some(),
        "Sprite2D must nest under CanvasItem"
    );
    handle.stop();
}

#[test]
fn abstract_classes_greyed_and_not_creatable() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    let tree = &v["tree"];
    // Abstract bases render greyed (instantiable=false) like Godot's
    // font_disabled_color rows.
    for abstract_class in ["CanvasItem", "PhysicsBody2D", "CollisionObject3D", "Light3D"] {
        let node = find_in_tree(tree, abstract_class)
            .unwrap_or_else(|| panic!("{abstract_class} must appear in tree"));
        assert_eq!(
            node["instantiable"], false,
            "{abstract_class} must not be instantiable"
        );
    }
    // Concrete classes are creatable.
    for concrete in ["Node", "Sprite2D", "StaticBody2D", "DirectionalLight3D"] {
        let node = find_in_tree(tree, concrete).expect("concrete class in tree");
        assert_eq!(node["instantiable"], true, "{concrete} must be instantiable");
    }
    // Abstract classes never appear in the flat candidate list.
    let classes = v["classes"].as_array().unwrap();
    assert!(
        !classes.iter().any(|c| c == "CanvasItem"),
        "abstract CanvasItem must not be a creatable candidate"
    );
    handle.stop();
}

#[test]
fn editor_api_classes_hidden() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    assert!(
        find_in_tree(&v["tree"], "EditorPlugin").is_none(),
        "EditorPlugin is editor-API and must be hidden (Godot Show Editor filter off)"
    );
    let classes = v["classes"].as_array().unwrap();
    assert!(!classes.iter().any(|c| c == "EditorPlugin"));
    handle.stop();
}

// ---- Search: Godot subsequence matching + scoring ----

#[test]
fn search_mesh_matches_and_auto_selects_meshinstance3d() {
    let (handle, port) = make_server();
    // MeshInstance2D and MeshInstance3D score IDENTICALLY for "mesh" in
    // Godot's `_score_type` (same length, same prefix position), so the winner
    // is decided by `_is_type_preferred` — the Scene dock sets the preferred
    // type from the edited scene root's family (Control/Node2D/Node3D,
    // upstream editor/docks/scene_tree_dock.cpp:633-647) — and, on a tie, by
    // the alphabetical `type_info_list` order with a strict `>`
    // (create_dialog.cpp:131, :276). This fixture's root is a Node2D, so with
    // no preference MeshInstance2D legitimately wins; a 3D scene (preferred
    // Node3D) is what makes MeshInstance3D the auto-selection.
    let v2d = dialog_response(port, r#"{"search":"mesh","base_class":"Node"}"#);
    assert_eq!(v2d["best_match"], "MeshInstance2D", "tie breaks alphabetically");

    let v = dialog_response(
        port,
        r#"{"search":"mesh","base_class":"Node","preferred":"Node3D"}"#,
    );
    let tree = &v["tree"];
    let mesh = find_in_tree(tree, "MeshInstance3D").expect("MeshInstance3D matches 'mesh'");
    assert_eq!(mesh["matched"], true);
    let multi = find_in_tree(tree, "MultiMeshInstance3D").expect("MultiMeshInstance3D matches");
    assert_eq!(multi["matched"], true);
    // Ancestors are retained for context but not matched.
    let node3d = find_in_tree(tree, "Node3D").expect("Node3D ancestor retained");
    assert_eq!(node3d["matched"], false);
    // _score_type: prefix match on the shorter name wins.
    assert_eq!(v["best_match"], "MeshInstance3D");
    handle.stop();
}

#[test]
fn search_uses_subsequence_not_substring() {
    let (handle, port) = make_server();
    // "mi3d" is not a substring of MeshInstance3D but is a subsequence
    // (M..I..3D), exactly like Godot's is_subsequence_ofn.
    let v = dialog_response(port, r#"{"search":"mi3d","base_class":"Node"}"#);
    assert!(
        find_in_tree(&v["tree"], "MeshInstance3D").is_some(),
        "subsequence 'mi3d' must match MeshInstance3D"
    );
    handle.stop();
}

#[test]
fn search_light_keeps_abstract_parent_as_grey_ancestor() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"light","base_class":"Node"}"#);
    let tree = &v["tree"];
    let light = find_in_tree(tree, "Light3D").expect("Light3D ancestor retained");
    // Light3D itself matches the text but is abstract → shown only as a
    // non-matching, non-instantiable ancestor.
    assert_eq!(light["matched"], false, "abstract Light3D is not a candidate");
    assert_eq!(light["instantiable"], false);
    let dir = find_in_tree(tree, "DirectionalLight3D").expect("DirectionalLight3D");
    assert_eq!(dir["matched"], true);
    handle.stop();
}

#[test]
fn exact_match_always_wins_scoring() {
    let (handle, port) = make_server();
    // "Node" scores 1.0 as an exact match even though Node2D/Node3D also match.
    let v = dialog_response(port, r#"{"search":"Node","base_class":"Node"}"#);
    assert_eq!(v["best_match"], "Node");
    handle.stop();
}

#[test]
fn no_results_returns_null_tree_and_no_best_match() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"zzzzqqqq","base_class":"Node"}"#);
    assert!(v["tree"].is_null(), "no matches → no tree");
    assert!(v["best_match"].is_null(), "no matches → no best match");
    handle.stop();
}

#[test]
fn preferred_type_boosts_context_matches() {
    let (handle, port) = make_server();
    // "camera" matches Camera2D and Camera3D; the preferred base breaks the
    // tie per Godot's _is_type_preferred weighting.
    let v = dialog_response(
        port,
        r#"{"search":"camera","base_class":"Node","preferred":"Node3D"}"#,
    );
    assert_eq!(v["best_match"], "Camera3D");
    let v = dialog_response(
        port,
        r#"{"search":"camera","base_class":"Node","preferred":"Node2D"}"#,
    );
    assert_eq!(v["best_match"], "Camera2D");
    handle.stop();
}

// ---- Description panel data ----

#[test]
fn tree_nodes_carry_description_and_inheritance_chain() {
    let (handle, port) = make_server();
    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    let node = find_in_tree(&v["tree"], "Node").unwrap();
    assert!(
        node["description"]
            .as_str()
            .unwrap()
            .contains("Base class for all scene objects"),
        "Node must carry Godot's brief description"
    );
    let sprite = find_in_tree(&v["tree"], "Sprite2D").unwrap();
    let chain: Vec<&str> = sprite["inheritance_chain"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        chain,
        vec!["Sprite2D", "Node2D", "CanvasItem", "Node", "Object"],
        "chain must run to Object for the Class X < ... display"
    );
    handle.stop();
}

// ---- Favorites: star toggle + reorder ----

#[test]
fn favorite_toggle_roundtrip() {
    let (handle, port) = make_server();
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog/toggle_favorite",
        r#"{"class_name":"Camera2D"}"#,
    )))
    .unwrap();
    assert_eq!(v["is_favorite"], true);
    assert!(v["favorites"].as_array().unwrap().iter().any(|f| f == "Camera2D"));

    // Dialog reflects the new favorite.
    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    assert!(v["favorites"].as_array().unwrap().iter().any(|f| f == "Camera2D"));

    // Toggling again removes it.
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog/toggle_favorite",
        r#"{"class_name":"Camera2D"}"#,
    )))
    .unwrap();
    assert_eq!(v["is_favorite"], false);
    assert!(!v["favorites"].as_array().unwrap().iter().any(|f| f == "Camera2D"));
    handle.stop();
}

#[test]
fn favorites_reorder_preserves_order() {
    let (handle, port) = make_server();
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog/favorites",
        r#"{"favorites":["Label","Node2D","Sprite2D"]}"#,
    )))
    .unwrap();
    let favs: Vec<&str> = v["favorites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(favs, vec!["Label", "Node2D", "Sprite2D"]);
    handle.stop();
}

// ---- Recent: created nodes land at the front of the Recent pane ----

#[test]
fn node_add_records_recent_like_godot_confirm() {
    let (handle, port, root) = make_server_with_root();
    // Create under the scene root like the dialog does — the node is
    // auto-named after its class, no prompt.
    let resp = http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{root},"name":"Camera2D","class_name":"Camera2D"}}"#),
    );
    assert!(resp.contains("200"), "node add must succeed: {resp}");

    let v = dialog_response(port, r#"{"search":"","base_class":"Node"}"#);
    let recent = v["recent"].as_array().unwrap();
    assert!(!recent.is_empty(), "recent must not be empty after a create");
    assert_eq!(
        recent[0]["class_name"], "Camera2D",
        "most recent create must be first"
    );
    handle.stop();
}

// ---- Served UI structure (client wiring guard, pat client-wiring-gap) ----

#[test]
fn editor_html_has_godot_dialog_structure() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    // Two-pane Godot layout.
    for id in [
        "add-node-favorites",
        "add-node-recent",
        "add-node-fav-btn",
        "add-node-search-clear",
        "add-node-list",
        "add-node-description",
        "add-node-cancel",
        "add-node-create",
    ] {
        assert!(html.contains(id), "EDITOR_HTML missing {id}");
    }
    for label in ["Favorites:", "Recent:", "Search:", "Matches:", "Description:"] {
        assert!(html.contains(label), "EDITOR_HTML missing pane label {label}");
    }
    assert!(html.contains("Create New Node"));
}

#[test]
fn editor_html_js_is_wired_to_server_dialog() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    // The served JS must actually call the server dialog API and use its
    // results — features can otherwise be server-green but dead in the UI.
    assert!(html.contains("/api/node/create_dialog"));
    assert!(html.contains("best_match"));
    assert!(html.contains("toggle_favorite"));
    assert!(html.contains("fetchAddNodeDialog"));
    assert!(html.contains("uniqueChildName"));
    // Keyboard parity: arrows navigate the tree from the search box.
    assert!(html.contains("moveAddNodeSelection"));
    // Godot has no name prompt on create.
    assert!(
        !html.contains("prompt('Node name:'"),
        "creation must not prompt for a name"
    );
    // Godot's no-results state.
    assert!(html.contains("No results for"));
    // Non-instantiable tooltip parity.
    assert!(html.contains("The selected class can't be instantiated."));
}
