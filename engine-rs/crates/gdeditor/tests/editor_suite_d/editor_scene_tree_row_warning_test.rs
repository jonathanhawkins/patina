//! pat-417cn — the Scene tree dock's **per-row** affordances, enumerated from
//! `upstream/godot/editor/gui/scene_tree_editor.cpp`.
//!
//! `SceneTreeEditor::update_node` hangs a fixed set of row buttons off every
//! item it builds: BUTTON_SCRIPT, BUTTON_VISIBILITY, BUTTON_LOCK, BUTTON_GROUP
//! and — the one Patina was missing — BUTTON_WARNING, added whenever the node
//! reports `_get_configuration_warnings()`, with the tooltip
//! `"Node configuration warning:" + "\n" + warning`.
//!
//! Patina already drew the script / signal / group / unique / lock / visibility
//! badges, and `/api/node/warnings` already existed, but it was only ever called
//! from the right-click menu — so a node with a configuration warning looked
//! identical to a clean one in the dock, which is exactly the kind of gap a
//! side-by-side with Godot shows immediately.
//!
//! Part 1 asserts the served document builds the whole enumerated row set,
//! warning triangle included. Part 2 boots the real editor server and proves the
//! triangle is data-driven: `/api/scene` carries each node's
//! `configuration_warnings`, non-empty for the second `ShaderGlobalsOverride`
//! (Godot's native "only one can be active" warning) and empty for the first and
//! for an ordinary node.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

// ---- Part 1: the served row builder carries every enumerated affordance ----

/// Every per-row affordance `SceneTreeEditor::update_node` always considers has
/// a counterpart in the served row builder — including the warning triangle,
/// whose absence was the gap this bead closes.
#[test]
fn scene_tree_rows_render_every_godot_row_affordance() {
    for needle in [
        // BUTTON_SCRIPT / the signal + group indicators.
        "node.has_script",
        "node.has_signals",
        "node.groups && node.groups.length > 0",
        // Unique-name (%) indicator.
        "node.unique_name",
        // BUTTON_VISIBILITY and BUTTON_LOCK, both of which write back.
        "tree-visibility",
        "tree-lock",
        // BUTTON_WARNING: the triangle, Godot's tooltip wording, and the row
        // actually getting it appended (a badge built but never appended is
        // invisible, which is the same bug in a different place).
        "node.configuration_warnings",
        "Node configuration warning:",
        "row.appendChild(warnBadge)",
    ] {
        assert!(
            HTML.contains(needle),
            "served scene-tree row builder is missing `{needle}` — Godot's \
             update_node builds this affordance for every row"
        );
    }
}

// ---- Part 2: the triangle is driven by real configuration warnings ----

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A tree covering both warning producers Godot folds into one
/// `get_configuration_warnings()`:
///
/// * two `ShaderGlobalsOverride` nodes — only one may be active, so
///   `SceneTree::node_configuration_warnings` warns on the *second*. No script
///   needed, so this does not depend on the interpreter being wired in.
/// * a textureless `Sprite2D` — one of the C++ class overrides that lived in
///   `dock::compute_node_warnings`, fully tested but with no caller outside its
///   own tests, so the served dock never saw it.
fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    tree.add_child(root, Node::new("OverrideFirst", "ShaderGlobalsOverride"))
        .unwrap();
    tree.add_child(root, Node::new("OverrideSecond", "ShaderGlobalsOverride"))
        .unwrap();
    tree.add_child(root, Node::new("NoTexture", "Sprite2D"))
        .unwrap();
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
            Err(e) => panic!("failed to connect to editor server: {e}"),
        }
    }
    unreachable!()
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Depth-first lookup by node name, so the assertions do not depend on where in
/// the window-root's child list a node happens to sit.
fn find_by_name<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    if node.get("name").and_then(|n| n.as_str()) == Some(name) {
        return Some(node);
    }
    for child in node.get("children").and_then(|c| c.as_array())? {
        if let Some(found) = find_by_name(child, name) {
            return Some(found);
        }
    }
    None
}

fn warnings_of(node: &serde_json::Value) -> Vec<String> {
    node["configuration_warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("node {} has no configuration_warnings array", node["name"]))
        .iter()
        .map(|w| w.as_str().unwrap_or_default().to_string())
        .collect()
}

/// 🔑 **THE ACCEPTANCE.** `/api/scene` carries per-node configuration warnings,
/// so the dock can draw the triangle from the tree payload it already fetches
/// instead of one request per row.
#[test]
fn scene_tree_payload_carries_per_node_configuration_warnings() {
    let (_handle, port) = make_server();

    let scene: serde_json::Value =
        serde_json::from_str(body_of(&http_get(port, "/api/scene"))).expect("scene JSON");
    let root = &scene["nodes"];

    let main = find_by_name(root, "Main").expect("Main node in payload");
    let first = find_by_name(root, "OverrideFirst").expect("OverrideFirst in payload");
    let second = find_by_name(root, "OverrideSecond").expect("OverrideSecond in payload");

    // An ordinary node and the *active* override are clean: no triangle.
    assert!(
        warnings_of(main).is_empty(),
        "an ordinary Node2D must report no configuration warning, got {:?}",
        warnings_of(main)
    );
    assert!(
        warnings_of(first).is_empty(),
        "the FIRST ShaderGlobalsOverride is the active one and must not warn, got {:?}",
        warnings_of(first)
    );

    // The second override is inert, and the payload says so with the message
    // Godot's tooltip would show.
    let second_warnings = warnings_of(second);
    assert_eq!(
        second_warnings.len(),
        1,
        "the second ShaderGlobalsOverride must contribute exactly one warning, got {second_warnings:?}"
    );
    assert!(
        second_warnings[0].contains("ShaderGlobalsOverride"),
        "the warning must name the class it is about, got {second_warnings:?}"
    );
}

/// 🔑 The second producer is wired too: `dock::compute_node_warnings` had no
/// caller outside its own tests, so every C++ class warning it implements was
/// invisible in the served dock. A textureless `Sprite2D` is the cheapest proof
/// that its messages now reach the payload.
#[test]
fn scene_tree_payload_carries_dock_computed_class_warnings() {
    let (_handle, port) = make_server();

    let scene: serde_json::Value =
        serde_json::from_str(body_of(&http_get(port, "/api/scene"))).expect("scene JSON");
    let sprite =
        find_by_name(&scene["nodes"], "NoTexture").expect("textureless Sprite2D in payload");

    let warnings = warnings_of(sprite);
    assert!(
        warnings
            .iter()
            .any(|w| w.to_lowercase().contains("texture")),
        "a Sprite2D with no texture must carry dock::compute_node_warnings' \
         message in the scene payload, got {warnings:?}"
    );
}

// ---- Part 3: the dock's Remote/Local toggle and the remaining row buttons ----

/// Godot's `SceneTreeDock` builds a second toolbar below the filter row —
/// `button_hb` with the Remote / Local scene-tree source toggle
/// (upstream/godot/editor/docks/scene_tree_dock.cpp:4899-4921), Local pressed,
/// hidden until the project is being debugged, Remote carrying the stutter
/// warning. Patina served the running clone at `/api/runtime/scene` and forced
/// the dock onto it during play with no way back to the edit tree, so the
/// toggle's whole purpose — "switch back to Local to improve performance" —
/// was unreachable.
#[test]
fn scene_tree_dock_serves_the_remote_local_source_toggle() {
    let (handle, port) = make_server();
    let body = body_of(&http_get(port, "/editor")).to_string();
    handle.stop();

    for needle in [
        r#"id="scene-tree-remote-local""#,
        r#"id="st-remote""#,
        r#"id="st-local""#,
        // Godot's own tooltip on the Remote button.
        "will cause the project to stutter every time it updates",
        "Switch back to the Local scene tree dock to improve performance.",
    ] {
        assert!(
            body.contains(needle),
            "the Scene dock is missing Godot's Remote/Local toggle chrome `{needle}`"
        );
    }

    // Local is the pressed default, exactly as `edit_local->set_pressed(true)`.
    assert!(
        body.contains(r#"<button id="st-local" class="st-tree-source active">Local</button>"#),
        "Local must ship as the pressed button of the pair"
    );

    // Godot places button_hb between the filter toolbar and the tree.
    let toolbar = body
        .find(r#"id="scene-tree-toolbar""#)
        .expect("scene tree toolbar");
    let toggle = body
        .find(r#"id="scene-tree-remote-local""#)
        .expect("remote/local toggle");
    let tree = body.find(r#"id="scene-tree""#).expect("scene tree");
    assert!(
        toolbar < toggle && toggle < tree,
        "the toggle belongs between the filter toolbar and the tree \
         (got {toolbar}/{toggle}/{tree})"
    );

    // It is wired, not decorative: the choice gates which endpoint feeds the
    // tree, and the setup runs at init (a handler defined but never installed
    // is the same bug as a missing button).
    for needle in [
        "function sceneTreeShowsRemote()",
        "sceneTreeSource !== 'local'",
        "'/api/runtime/scene'",
        "setupSceneTreeSourceToggle();",
    ] {
        assert!(
            body.contains(needle),
            "the Remote/Local toggle is not wired: `{needle}` missing"
        );
    }
}

/// The row buttons `SceneTreeEditor::update_node` adds that Patina still had no
/// counterpart for — BUTTON_UNIQUE (which was present but read a payload key
/// `/api/scene` never sends, so it never drew), BUTTON_GROUP and
/// BUTTON_SUBSCENE — plus Godot's wording for BUTTON_VISIBILITY and
/// BUTTON_LOCK.
#[test]
fn scene_tree_rows_render_the_remaining_godot_row_buttons() {
    // BUTTON_UNIQUE read `node.unique_name`; the payload field is
    // `unique_name_in_owner` (see node_to_json_tree_with_warnings), so the `%`
    // badge could never render. Pin the bug: the dead key must not come back.
    assert!(
        !HTML.contains("if (node.unique_name)"),
        "the `%` badge must read the key /api/scene sends, not `node.unique_name`"
    );
    assert!(
        HTML.contains("node.unique_name_in_owner"),
        "the `%` badge must read `unique_name_in_owner`"
    );

    for needle in [
        // BUTTON_UNIQUE, with Godot's tooltip and the collision the server reports.
        "by using the '%' prefix in the node path.",
        "node.unique_name_collision",
        // BUTTON_GROUP: "children are not selectable", from the `grouped` field
        // the payload has always carried and no row ever read.
        "node.grouped",
        "Children are not selectable.",
        "'/api/node/group'",
        // BUTTON_SUBSCENE: "Open in Editor" on an instanced scene row.
        "node.instance_path",
        "Open in Editor",
        "doOpenScene(path)",
        // Godot's wording for the two buttons Patina already drew.
        "Toggle Visibility",
        "Node is locked.",
        // A badge built but never appended is invisible — append both.
        "row.appendChild(groupedBadge)",
        "row.appendChild(subsceneBtn)",
    ] {
        assert!(
            HTML.contains(needle),
            "served scene-tree row builder is missing `{needle}` — \
             SceneTreeEditor::update_node builds this affordance"
        );
    }
}
